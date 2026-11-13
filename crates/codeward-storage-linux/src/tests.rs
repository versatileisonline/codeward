use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;
use std::path::PathBuf;

const FIXTURE: &str = "CODEWARD_LINUX_EXT4_FIXTURE";
const CHILD: &str = "CODEWARD_LINUX_LEASE_CHILD";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from(env::var_os(FIXTURE).expect("private writable ext4 nodev mount"));
        let path = root.join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }

    fn put(&self, name: &str, bytes: &[u8]) -> File {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.0.join(name))
            .unwrap();
        file.write_all(bytes).unwrap();
        file
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn mount_type_and_options_are_checked_by_mount_id() {
    let ext4 = b"42 1 7:0 / /path\\040with\\040spaces rw,nodev shared:1 - ext4 /dev/loop0 rw";
    assert_eq!(native::mount_allowed(ext4, 42), Some(true));
    assert_eq!(native::mount_allowed(ext4, 41), None);
    for line in [
        &b"42 1 7:0 / /path rw,nodev - ext3 /dev/loop0 rw"[..],
        b"42 1 7:0 / /path rw - ext4 /dev/loop0 rw",
        b"42 1 7:0 / /path ro,nodev - ext4 /dev/loop0 rw",
        b"42 1 7:0 / /path rw,nodev - ext4 /dev/loop0 ro",
    ] {
        assert_eq!(native::mount_allowed(line, 42), Some(false));
    }
}

#[test]
fn invalid_path_spellings_are_rejected() {
    for path in [
        "relative",
        "/",
        "/./example",
        "/../example",
        "//example",
        "/\0",
    ] {
        assert_eq!(
            ReadLease::acquire_synthetic(Path::new(path)).err(),
            Some(NativeError::InvalidPath)
        );
    }
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_reads_and_enforces_file_cap() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    assert_eq!(lease.read_current().unwrap(), None);
    let file = fixture.put("vault.cw", b"synthetic bytes");
    assert_eq!(lease.read_current().unwrap().unwrap(), b"synthetic bytes");
    file.set_len(MAX_FILE_LENGTH as u64).unwrap();
    assert_eq!(
        lease.read_current().unwrap().unwrap().len(),
        MAX_FILE_LENGTH
    );
    file.set_len(MAX_FILE_LENGTH as u64 + 1).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::TooLarge));
    assert_eq!(file.metadata().unwrap().len(), MAX_FILE_LENGTH as u64 + 1);
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_refuses_link_special_and_public_entries() {
    let fixture = Fixture::new();
    let vault = fixture.0.join("vault.cw");
    let other = fixture.0.join("other");
    fixture.put("other", b"preserve");
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    symlink(&other, &vault).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Unsupported));
    fs::remove_file(&vault).unwrap();
    fs::hard_link(&other, &vault).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Unsupported));
    fs::remove_file(&vault).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(&vault)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(lease.read_current(), Err(NativeError::Unsupported));
    fs::remove_file(&vault).unwrap();
    fixture.put("vault.cw", b"public");
    fs::set_permissions(&vault, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Permissions));
    assert_eq!(fs::read(&other).unwrap(), b"preserve");

    let alias = fixture.0.join("alias");
    symlink(&fixture.0, &alias).unwrap();
    assert_eq!(
        ReadLease::acquire_synthetic(&alias).err(),
        Some(NativeError::Unsupported)
    );
    let lock = fixture.0.join(COMPANION.to_str().unwrap());
    fs::rename(&lock, fixture.0.join("former-lock")).unwrap();
    symlink(&other, &lock).unwrap();
    assert_eq!(
        ReadLease::acquire_synthetic(&fixture.0).err(),
        Some(NativeError::Unsupported)
    );
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_rejects_observed_changes_during_read() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    let mut file = fixture.put("vault.cw", b"first");
    assert_eq!(
        lease.read_with(|| file.write_all(b" growth").unwrap()),
        Err(NativeError::Conflict)
    );
    fixture.put("replacement", b"whole second");
    assert_eq!(
        lease.read_with(|| {
            fs::rename(fixture.0.join("replacement"), fixture.0.join("vault.cw")).unwrap();
        }),
        Err(NativeError::Conflict)
    );
    assert_eq!(lease.read_current().unwrap().unwrap(), b"whole second");

    let lock = fixture.0.join(COMPANION.to_str().unwrap());
    assert_eq!(
        lease.read_with(|| {
            fs::rename(&lock, fixture.0.join("old-lock")).unwrap();
            fixture.put(COMPANION.to_str().unwrap(), b"");
        }),
        Err(NativeError::Conflict)
    );
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_refuses_substituted_parent() {
    let fixture = Fixture::new();
    let path = fixture.0.join("parent");
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    let lease = ReadLease::acquire_synthetic(&path).unwrap();
    fs::rename(&path, fixture.0.join("moved")).unwrap();
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Conflict));
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_exclusion_survives_independent_opens() {
    let fixture = Fixture::new();
    let mut child = Command::new(env::current_exe().unwrap());
    child.args(["--exact", "tests::lease_child", "--ignored"]);
    child.env(CHILD, &fixture.0);

    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    assert_eq!(
        ReadLease::acquire_synthetic(&fixture.0).err(),
        Some(NativeError::WriterUnavailable)
    );
    assert!(
        child
            .env("CODEWARD_EXPECT_LOCKED", "1")
            .status()
            .unwrap()
            .success()
    );
    drop(lease);
    assert!(
        child
            .env_remove("CODEWARD_EXPECT_LOCKED")
            .status()
            .unwrap()
            .success()
    );
}

#[test]
#[ignore = "subprocess of ext4_exclusion_survives_independent_opens"]
fn lease_child() {
    let path = PathBuf::from(env::var_os(CHILD).expect("subprocess fixture path"));
    let lease = ReadLease::acquire_synthetic(&path);
    if env::var_os("CODEWARD_EXPECT_LOCKED").is_some() {
        assert_eq!(lease.err(), Some(NativeError::WriterUnavailable));
    } else {
        assert_eq!(lease.unwrap().read_current().unwrap(), None);
    }
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_refuses_default_acl_without_creating_companion() {
    let fixture = Fixture::new();
    let status = Command::new("python3")
        .arg("-c")
        .arg(
            "import os, struct, sys\n\
             acl = struct.pack('<IHHIHHIHHI', 2, 1, 7, 0xffffffff, 4, 0, 0xffffffff, 32, 0, 0xffffffff)\n\
             os.setxattr(sys.argv[1], 'system.posix_acl_default', acl)",
        )
        .arg(&fixture.0)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        ReadLease::acquire_synthetic(&fixture.0).err(),
        Some(NativeError::Permissions)
    );
    assert!(!fixture.0.join(COMPANION.to_str().unwrap()).exists());
}

#[test]
fn managed_data_home_uses_absolute_xdg_or_account_home() {
    use std::ffi::OsStr;
    let home = || Ok(PathBuf::from("/account"));
    assert_eq!(
        managed::data_root(Some(OsStr::new("/data")), || panic!("account lookup")).unwrap(),
        (PathBuf::from("/data"), false)
    );
    for xdg in [None, Some(OsStr::new("")), Some(OsStr::new("relative"))] {
        assert_eq!(
            managed::data_root(xdg, home).unwrap(),
            (home().unwrap(), true)
        );
    }
    assert_eq!(
        managed::data_root(Some(OsStr::new("/data/../other")), home),
        Err(NativeError::InvalidPath)
    );
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_managed_location_prepares_and_reopens_fixed_names() {
    let fixture = Fixture::new();
    let home = &fixture.0;
    let location = managed::Location::prepare(home, true, test_owner()).unwrap();
    let expected = home.join(".local/share/codeward/vaults");
    assert!(expected.is_dir());
    for path in [expected.as_path(), expected.parent().unwrap()] {
        assert_eq!(fs::metadata(path).unwrap().mode() & 0o7777, 0o700);
    }
    assert!(!expected.join("vault.cw").exists());
    let lease = ReadLease::acquire(location).unwrap();
    assert_eq!(lease.read_current().unwrap(), None);
    let vault = expected.join("vault.cw");
    File::create_new(&vault).unwrap();
    fs::set_permissions(&vault, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&vault, b"existing synthetic bytes").unwrap();
    drop(lease);
    let location = managed::Location::prepare(home, true, test_owner()).unwrap();
    let lease = ReadLease::acquire(location).unwrap();
    assert_eq!(
        lease.read_current().unwrap().unwrap(),
        b"existing synthetic bytes"
    );
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_managed_location_refuses_collision_and_revalidates_ancestors() {
    let fixture = Fixture::new();
    let home = &fixture.0;
    let app = home.join("codeward");
    let outside = home.join("outside");
    fs::DirBuilder::new().mode(0o700).create(&outside).unwrap();
    symlink(&outside, &app).unwrap();
    assert_eq!(
        managed::Location::prepare(home, false, test_owner()).err(),
        Some(NativeError::Unsupported)
    );
    assert!(!outside.join("vaults").exists());
    fs::remove_file(&app).unwrap();
    fs::DirBuilder::new().mode(0o755).create(&app).unwrap();
    fs::set_permissions(&app, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        managed::Location::prepare(home, false, test_owner()).err(),
        Some(NativeError::Permissions)
    );
    assert_eq!(fs::metadata(&app).unwrap().mode() & 0o7777, 0o755);
    assert!(!app.join("vaults").exists());
    fs::set_permissions(&app, fs::Permissions::from_mode(0o700)).unwrap();
    let lease =
        ReadLease::acquire(managed::Location::prepare(home, false, test_owner()).unwrap()).unwrap();
    fs::set_permissions(&app, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Permissions));
    fs::set_permissions(&app, fs::Permissions::from_mode(0o700)).unwrap();
    fs::rename(&app, home.join("former-codeward")).unwrap();
    fs::DirBuilder::new().mode(0o700).create(&app).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Conflict));
}

fn test_owner() -> u32 {
    // SAFETY: geteuid has no arguments or preconditions.
    unsafe { libc::geteuid() }
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_managed_public_entry_uses_xdg_in_separate_process() {
    let fixture = Fixture::new();
    let result = Command::new(env::current_exe().unwrap())
        .args(["--exact", "tests::managed_xdg_child", "--ignored"])
        .env("XDG_DATA_HOME", &fixture.0)
        .env("HOME", "/unused-account-home")
        .status()
        .unwrap();
    assert!(result.success());
    let vaults = fixture.0.join("codeward/vaults");
    assert!(vaults.join(COMPANION.to_str().unwrap()).is_file());
    assert!(!vaults.join("vault.cw").exists());
}

#[test]
#[ignore = "subprocess of ext4_managed_public_entry_uses_xdg_in_separate_process"]
fn managed_xdg_child() {
    let lease = ReadLease::acquire_managed_synthetic().unwrap();
    assert_eq!(lease.read_current().unwrap(), None);
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_competing_managed_preparations_preserve_the_same_directory() {
    let fixture = Fixture::new();
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| managed::Location::prepare(&fixture.0, true, test_owner()));
        let second = managed::Location::prepare(&fixture.0, true, test_owner());
        (first.join().unwrap().unwrap(), second.unwrap())
    });
    let first = ReadLease::acquire(first).unwrap();
    assert_eq!(
        ReadLease::acquire(second).err(),
        Some(NativeError::WriterUnavailable)
    );
    assert_eq!(first.read_current().unwrap(), None);
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_stage_preserves_live_bytes_and_residue() {
    let fixture = Fixture::new();
    fixture.put("vault.cw", b"live");
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    let marker = lease.prepare_stage(b"synthetic candidate").unwrap();
    marker.verify().unwrap();
    let path = stage_path(&fixture);
    assert_eq!(fs::read(&path).unwrap(), b"synthetic candidate");
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o7777, 0o600);
    assert_eq!(lease.read_current().unwrap().unwrap(), b"live");
    assert_eq!(
        lease.prepare_stage(&vec![0; MAX_FILE_LENGTH + 1]).err(),
        Some(NativeError::TooLarge)
    );
    assert_eq!(stage_path(&fixture), path);
    fs::rename(&path, fixture.0.join("former-stage")).unwrap();
    fixture.put(path.file_name().unwrap().to_str().unwrap(), b"substitute");
    assert_eq!(marker.verify(), Err(NativeError::Conflict));
    assert_eq!(
        fs::read(fixture.0.join("former-stage")).unwrap(),
        b"synthetic candidate"
    );
    assert_eq!(fs::read(&path).unwrap(), b"substitute");
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_partial_stage_failure_preserves_live_bytes() {
    let fixture = Fixture::new();
    fixture.put("vault.cw", b"live");
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    let result = lease.prepare_stage_with(b"candidate", |file, bytes| {
        file.write_all(&bytes[..3])?;
        Err(io::Error::other("synthetic short-write failure"))
    });
    assert_eq!(result.err(), Some(NativeError::WriteStage));
    assert_eq!(fs::read(stage_path(&fixture)).unwrap(), b"can");
    assert_eq!(lease.read_current().unwrap().unwrap(), b"live");
}

fn stage_path(fixture: &Fixture) -> PathBuf {
    let mut stages = fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .as_encoded_bytes()
                .starts_with(b".codeward.stage-")
        });
    let path = stages.next().unwrap();
    assert!(stages.next().is_none());
    path
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_publish_checks_expected_state_and_preserves_residue() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    lease
        .prepare_stage(b"initial")
        .unwrap()
        .publish_create()
        .unwrap();
    assert_eq!(lease.read_current().unwrap().unwrap(), b"initial");
    assert_eq!(
        lease.prepare_stage(b"collision").unwrap().publish_create(),
        Err(PublicationError::NotPublished(NativeError::Conflict))
    );
    assert_eq!(fs::read(stage_path(&fixture)).unwrap(), b"collision");
    assert_eq!(
        lease
            .prepare_stage(b"replacement")
            .unwrap()
            .publish_replace(b"stale"),
        Err(PublicationError::NotPublished(NativeError::Conflict))
    );
    assert_eq!(lease.read_current().unwrap().unwrap(), b"initial");
    lease
        .prepare_stage(b"replacement")
        .unwrap()
        .publish_replace(b"initial")
        .unwrap();
    lease.synchronize_namespace(Some(b"replacement")).unwrap();
    assert_eq!(lease.read_current().unwrap().unwrap(), b"replacement");
    assert_eq!(
        lease.synchronize_namespace(Some(b"initial")),
        Err(NativeError::Conflict)
    );
    assert_eq!(
        lease.synchronize_namespace(None),
        Err(NativeError::Conflict)
    );
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_namespace_sync_checks_absence_and_managed_chain_order() {
    let fixture = Fixture::new();
    let location = managed::Location::prepare(&fixture.0, true, test_owner()).unwrap();
    let lease = ReadLease::acquire(location).unwrap();
    lease.synchronize_namespace(None).unwrap();
    lease
        .prepare_stage(b"candidate")
        .unwrap()
        .publish_create()
        .unwrap();

    let mut seen = Vec::new();
    lease
        .synchronize_namespace_with(Some(b"candidate"), |file| {
            seen.push(file.metadata()?.ino());
            file.sync_all()
        })
        .unwrap();
    let root = &fixture.0;
    let expected: Vec<_> = [
        ".local/share/codeward/vaults/vault.cw",
        ".local/share/codeward/vaults",
        ".local/share/codeward",
        ".local/share",
        ".local",
        "",
    ]
    .map(|path| fs::metadata(root.join(path)).unwrap().ino())
    .into();
    assert_eq!(seen, expected);
    assert_eq!(
        lease.synchronize_namespace_with(Some(b"candidate"), |file| {
            if file.metadata()?.is_dir() {
                return Err(io::Error::other("synthetic directory sync error"));
            }
            file.sync_all()
        }),
        Err(NativeError::SyncNamespace)
    );
    assert_eq!(lease.read_current().unwrap().unwrap(), b"candidate");
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_publish_refuses_racing_create_and_marks_lost_reply_unknown() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    let stage = lease.prepare_stage(b"candidate").unwrap();
    assert_eq!(
        stage.publish_with(None, |parent, from, to, create| {
            fixture.put("vault.cw", b"racing");
            native::publish_at(parent, from, to, create)
        }),
        Err(PublicationError::OutcomeUnknown)
    );
    assert_eq!(lease.read_current().unwrap().unwrap(), b"racing");
    assert_eq!(fs::read(stage_path(&fixture)).unwrap(), b"candidate");

    let stage = lease.prepare_stage(b"replacement").unwrap();
    assert_eq!(
        stage.publish_with(Some(b"racing"), |parent, from, to, create| {
            let contender = lease.prepare_stage(b"competing").unwrap();
            assert_eq!(
                contender.publish_replace(b"racing"),
                Err(PublicationError::NotPublished(
                    NativeError::WriterUnavailable
                ))
            );
            assert_eq!(
                lease.synchronize_namespace(Some(b"racing")),
                Err(NativeError::WriterUnavailable)
            );
            native::publish_at(parent, from, to, create)?;
            Err(io::Error::other("synthetic lost reply"))
        }),
        Err(PublicationError::OutcomeUnknown)
    );
    assert_eq!(lease.read_current().unwrap().unwrap(), b"replacement");
    lease.synchronize_namespace(Some(b"replacement")).unwrap();
}
