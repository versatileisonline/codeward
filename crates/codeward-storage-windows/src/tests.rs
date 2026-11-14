use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

#[test]
fn ntfs_publication_creates_without_overwrite_and_replaces_only_matching_base() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    lease
        .prepare_stage(b"first candidate")
        .unwrap()
        .publish_create()
        .unwrap();
    assert_eq!(lease.read_current().unwrap().unwrap(), b"first candidate");
    assert_eq!(
        lease
            .prepare_stage(b"must not overwrite")
            .unwrap()
            .publish_create(),
        Err(PublicationError::NotPublished(NativeError::Conflict))
    );
    assert_eq!(
        lease
            .prepare_stage(b"must not overwrite")
            .unwrap()
            .publish_replace(b"stale base"),
        Err(PublicationError::NotPublished(NativeError::Conflict))
    );
    assert_eq!(lease.read_current().unwrap().unwrap(), b"first candidate");
    lease
        .prepare_stage(b"second candidate")
        .unwrap()
        .publish_replace(b"first candidate")
        .unwrap();
    assert_eq!(lease.read_current().unwrap().unwrap(), b"second candidate");
}

#[test]
fn ntfs_relative_creation_preserves_collisions_and_rejects_escape_names() {
    use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    let fixture = Fixture::new();
    let parent = open_directory(&fixture.0).unwrap();
    let name = ".codeward.stage-00000000000000000000000000000000";
    let mut file = native::open_child(
        &parent,
        name,
        GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ,
        FILE_CREATE,
        None,
    )
    .unwrap();
    use std::io::Write;
    file.write_all(b"preserve collision").unwrap();
    drop(file);
    assert_eq!(
        native::open_child(&parent, name, GENERIC_WRITE, 0, FILE_CREATE, None)
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(
        fs::read(fixture.0.join(name)).unwrap(),
        b"preserve collision"
    );
    for invalid in [
        "..",
        "../escape",
        r"..\escape",
        r"C:\escape",
        "vault.cw:stream",
        "alias.",
    ] {
        assert_eq!(
            native::open_child(&parent, invalid, GENERIC_WRITE, 0, FILE_CREATE, None)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn ntfs_stage_preserves_live_vault_and_retains_writer_exclusion() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    fs::write(fixture.0.join(VAULT), b"old synthetic vault").unwrap();
    let stage = lease
        .prepare_stage(b"encrypted synthetic candidate")
        .unwrap();
    stage.verify().unwrap();
    assert_eq!(
        lease.read_current().unwrap().unwrap(),
        b"old synthetic vault"
    );
    let stages: Vec<_> = fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".codeward.stage-")
        })
        .collect();
    assert_eq!(stages.len(), 1);
    assert_eq!(
        fs::read(&stages[0]).unwrap(),
        b"encrypted synthetic candidate"
    );
    assert!(OpenOptions::new().write(true).open(&stages[0]).is_err());
    assert!(fs::remove_file(&stages[0]).is_err());
    drop(lease);
    assert_eq!(
        ReadLease::acquire_synthetic(&fixture.0).err(),
        Some(NativeError::WriterUnavailable)
    );
    drop(stage);
    assert!(ReadLease::acquire_synthetic(&fixture.0).is_ok());
    assert_eq!(
        fs::read(&stages[0]).unwrap(),
        b"encrypted synthetic candidate"
    );
    assert_eq!(
        fs::read(fixture.0.join(VAULT)).unwrap(),
        b"old synthetic vault"
    );
}

#[test]
fn windows_leaf_requires_the_normalized_fixed_name() {
    assert!(matches_leaf(Path::new(r"C:\base\VAULT.CW"), VAULT));
    assert!(!matches_leaf(Path::new(r"C:\base\vault long.cw"), VAULT));
    assert!(!matches_leaf(Path::new(r"C:\base\vault.cw:stream"), VAULT));
}

#[test]
fn windows_distinguishes_missing_leaf_from_failed_parent_lookup() {
    use windows_sys::Win32::Foundation::ERROR_PATH_NOT_FOUND;
    assert!(leaf_missing(&io::Error::from_raw_os_error(
        ERROR_FILE_NOT_FOUND as i32
    )));
    assert!(!leaf_missing(&io::Error::from_raw_os_error(
        ERROR_PATH_NOT_FOUND as i32
    )));
}

pub(crate) struct Fixture(pub(crate) PathBuf);

impl Fixture {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "codeward-ntfs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn ntfs_reads_absence_bytes_and_cap_under_exclusion() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    assert_eq!(lease.read_current().unwrap(), None);
    assert_eq!(
        ReadLease::acquire_synthetic(&fixture.0).err(),
        Some(NativeError::WriterUnavailable)
    );
    let vault = fixture.0.join("vault.cw");
    fs::write(&vault, b"synthetic").unwrap();
    assert_eq!(lease.read_current().unwrap().unwrap(), b"synthetic");
    let file = OpenOptions::new().write(true).open(&vault).unwrap();
    file.set_len(MAX_FILE_LENGTH as u64).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Read));
    drop(file);
    assert_eq!(
        lease.read_current().unwrap().unwrap().len(),
        MAX_FILE_LENGTH
    );
    OpenOptions::new()
        .write(true)
        .open(&vault)
        .unwrap()
        .set_len(MAX_FILE_LENGTH as u64 + 1)
        .unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::TooLarge));
    drop(lease);
    assert!(ReadLease::acquire_synthetic(&fixture.0).is_ok());
}

#[test]
fn ntfs_rejects_hard_links_and_nonempty_companion() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    let other = fixture.0.join("other");
    fs::write(&other, b"preserve").unwrap();
    fs::hard_link(&other, fixture.0.join("vault.cw")).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Unsupported));
    drop(lease);
    let companion = fixture.0.join(".codeward.writer-lock");
    fs::write(&companion, b"preserve").unwrap();
    assert_eq!(
        ReadLease::acquire_synthetic(&fixture.0).err(),
        Some(NativeError::Unsupported)
    );
    assert_eq!(fs::read(companion).unwrap(), b"preserve");
}

#[test]
fn windows_rejects_non_disk_and_ambiguous_paths() {
    for path in [
        r"relative",
        r"C:relative",
        r"C:\safe\..\other",
        r"C:\safe\alias.",
        r"C:\safe\COM¹.txt",
        r"C:\safe\NUL",
        r"C:\safe\entry:stream",
        r"\\server\share\fixture",
        r"\\?\C:\fixture",
    ] {
        assert_eq!(
            ReadLease::acquire_synthetic(Path::new(path)).err(),
            Some(NativeError::InvalidPath)
        );
    }
}

#[test]
fn ntfs_excludes_an_independent_process() {
    let fixture = Fixture::new();
    let child = |expected| {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::companion_child", "--ignored"])
            .env("CODEWARD_NTFS_FIXTURE", &fixture.0)
            .env("CODEWARD_NTFS_EXPECT_LOCKED", expected)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    };
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    child("yes");
    drop(lease);
    child("no");
}

#[test]
#[ignore = "subprocess of ntfs_excludes_an_independent_process"]
fn companion_child() {
    let path = PathBuf::from(std::env::var_os("CODEWARD_NTFS_FIXTURE").unwrap());
    let result = ReadLease::acquire_synthetic(&path);
    match std::env::var("CODEWARD_NTFS_EXPECT_LOCKED")
        .unwrap()
        .as_str()
    {
        "yes" => assert_eq!(result.err(), Some(NativeError::WriterUnavailable)),
        "no" => assert_eq!(result.unwrap().read_current().unwrap(), None),
        _ => panic!("missing expected companion state"),
    }
}

#[test]
fn ntfs_refuses_junction_as_directory_or_vault() {
    let fixture = Fixture::new();
    let target = fixture.0.join("target");
    let junction = fixture.0.join("vault.cw");
    fs::create_dir(&target).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J", "vault.cw", "target"])
        .current_dir(&fixture.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        ReadLease::acquire_synthetic(&junction).err(),
        Some(NativeError::Unsupported)
    );
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Unsupported));
}

#[test]
fn ntfs_managed_prepares_fixed_private_tree_and_reopens() {
    let fixture = Fixture::new();
    managed::protect_fixture(&fixture.0);
    let lease = ReadLease::acquire_managed_at(&fixture.0).unwrap();
    let parent = fixture.0.join("Codeward").join("Vaults");
    assert!(parent.join(COMPANION).is_file());
    assert_eq!(lease.read_current().unwrap(), None);
    drop(lease);
    assert_eq!(
        ReadLease::acquire_managed_at(&fixture.0)
            .unwrap()
            .read_current()
            .unwrap(),
        None
    );
}

#[test]
fn ntfs_managed_refuses_acl_grants_without_repair() {
    let fixture = Fixture::new();
    managed::protect_fixture(&fixture.0);
    let app = fixture.0.join("Codeward");
    managed::create_broad_fixture(&app);
    assert_eq!(
        ReadLease::acquire_managed_at(&fixture.0).err(),
        Some(NativeError::Permissions)
    );
    assert!(app.is_dir());
    assert!(!app.join("Vaults").exists());
}

#[test]
fn windows_resolves_os_local_app_data_without_creating_entries() {
    let path = managed::local_app_data().unwrap();
    let (_, components) = split_path(&path).unwrap();
    assert!(!components.is_empty());
    assert!(path.is_dir());
}

#[test]
fn ntfs_managed_refuses_existing_vault_with_broad_acl() {
    let fixture = Fixture::new();
    managed::protect_fixture(&fixture.0);
    let lease = ReadLease::acquire_managed_at(&fixture.0).unwrap();
    let vault = fixture.0.join("Codeward").join("Vaults").join(VAULT);
    // The protected app directories use non-inheritable grants. This ordinary
    // creation uses the token default DACL, which need not be broad, so supply
    // an explicit test descriptor to make the refusal deterministic.
    managed::create_broad_vault(&vault);
    assert_eq!(lease.read_current(), Err(NativeError::Permissions));
    assert!(vault.is_file());
}
