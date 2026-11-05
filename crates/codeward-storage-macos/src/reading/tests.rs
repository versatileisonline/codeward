use std::{
    fs::{self, OpenOptions, Permissions},
    io::{BufRead, BufReader, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    path::Path,
    process::{Child, Command, Stdio},
};

use super::*;
use crate::VaultPath;

use crate::test_support::{TestDir, chmod_acl, fixture, stages};

const ACLS: [&str; 2] = ["everyone allow read", "everyone deny delete"];

fn remove_test_acl(path: &Path) {
    assert!(
        Command::new("/bin/chmod")
            .arg("-N")
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn acl_admission_refuses_parent_before_resolution_or_companion_creation() {
    for acl in ACLS {
        for after_resolve in [false, true] {
            let dir = TestDir::new();
            let resolved = after_resolve.then(|| VaultPath::resolve(&dir.vault()).unwrap());
            chmod_acl(&dir.0, acl);
            assert_eq!(fs::metadata(&dir.0).unwrap().mode() & 0o777, 0o700);
            let result = match resolved {
                Some(path) => path.acquire().err(),
                None => VaultPath::resolve(&dir.vault()).err(),
            };
            assert_eq!(result, Some(NativeError::Permissions));
            assert!(!dir.companion().exists());
            assert!(fs::read_dir(&dir.0).unwrap().next().is_none());
            assert!(!native::no_extended_acl(&File::open(&dir.0).unwrap()).unwrap());
            remove_test_acl(&dir.0);
            dir.lease();
        }
    }
}

#[test]
fn acl_admission_refuses_existing_companion_before_lock_without_changing_it() {
    for acl in ACLS {
        let dir = TestDir::new();
        drop(dir.lease());
        let inode = fs::metadata(dir.companion()).unwrap().ino();
        chmod_acl(&dir.companion(), acl);
        assert_eq!(fs::metadata(dir.companion()).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            VaultPath::resolve(&dir.vault()).unwrap().acquire().err(),
            Some(NativeError::Permissions)
        );
        assert_eq!(fs::metadata(dir.companion()).unwrap().ino(), inode);
        assert!(fs::read(dir.companion()).unwrap().is_empty());
        let companion = File::open(dir.companion()).unwrap();
        assert!(!native::no_extended_acl(&companion).unwrap());
        // A refused attempt must not leave exclusion owned by the process.
        companion.try_lock().unwrap();
        companion.unlock().unwrap();
        remove_test_acl(&dir.companion());
        dir.lease();
    }
}

#[test]
fn acl_admission_refuses_vault_before_reading_even_with_private_mode() {
    for acl in ACLS {
        let dir = TestDir::new();
        dir.write(b"synthetic unchanged vault");
        let lease = dir.lease();
        chmod_acl(&dir.vault(), acl);
        assert_eq!(fs::metadata(dir.vault()).unwrap().mode() & 0o777, 0o600);
        let mut reached_read = false;
        assert_eq!(
            lease.read_with_hook(|| reached_read = true),
            Err(NativeError::Permissions)
        );
        assert!(!reached_read);
        assert_eq!(fs::read(dir.vault()).unwrap(), b"synthetic unchanged vault");
        assert!(!native::no_extended_acl(&File::open(dir.vault()).unwrap()).unwrap());
        remove_test_acl(&dir.vault());
        assert_eq!(
            lease.read_current().unwrap().unwrap(),
            b"synthetic unchanged vault"
        );
    }
}

#[test]
fn acl_admission_rechecks_parent_and_companion_for_present_and_absent_reads() {
    for acl in ACLS {
        for parent in [false, true] {
            for present in [false, true] {
                let dir = TestDir::new();
                if present {
                    dir.write(b"synthetic unchanged vault");
                }
                let lease = dir.lease();
                let target = if parent {
                    dir.0.clone()
                } else {
                    dir.companion()
                };
                chmod_acl(&target, acl);
                assert_eq!(lease.read_current(), Err(NativeError::Permissions));
                assert!(!native::no_extended_acl(&File::open(&target).unwrap()).unwrap());
                assert_eq!(dir.vault().exists(), present);
                if present {
                    assert_eq!(fs::read(dir.vault()).unwrap(), b"synthetic unchanged vault");
                }
                remove_test_acl(&target);
                assert_eq!(lease.read_current().unwrap().is_some(), present);
            }
        }
    }
}

#[test]
fn acl_admission_rechecks_vault_parent_and_companion_after_reading() {
    for acl in ACLS {
        for target in 0..3 {
            let dir = TestDir::new();
            dir.write(b"synthetic unchanged vault");
            let lease = dir.lease();
            let path = match target {
                0 => dir.vault(),
                1 => dir.0.clone(),
                _ => dir.companion(),
            };
            assert_eq!(
                lease.read_with_hook(|| chmod_acl(&path, acl)),
                Err(NativeError::Permissions)
            );
            assert_eq!(fs::read(dir.vault()).unwrap(), b"synthetic unchanged vault");
            assert!(!native::no_extended_acl(&File::open(&path).unwrap()).unwrap());
            remove_test_acl(&path);
            assert_eq!(
                lease.read_current().unwrap().unwrap(),
                b"synthetic unchanged vault"
            );
        }
    }
}

#[test]
fn acl_admission_blocks_staging_publication_and_barriers_on_companion_change() {
    for acl in ACLS {
        for present in [false, true] {
            let dir = TestDir::new();
            if present {
                dir.write(b"synthetic base");
            }
            let lease = dir.lease();
            let stage = lease.prepare_stage(b"synthetic candidate").unwrap();
            let residue = stages(&dir.0);
            assert_eq!(residue.len(), 1);
            chmod_acl(&dir.companion(), acl);
            assert_eq!(lease.read_current(), Err(NativeError::Permissions));
            assert_eq!(
                lease.prepare_stage(b"another candidate").err(),
                Some(NativeError::Permissions)
            );
            let base = present.then_some(b"synthetic base".as_slice());
            let publication = if present {
                stage.publish_replace(base.unwrap())
            } else {
                stage.publish_create()
            };
            assert_eq!(
                publication,
                Err(crate::PublicationError::NotPublished(
                    NativeError::Permissions
                ))
            );
            assert_eq!(
                lease.synchronize_namespace(base),
                Err(NativeError::Permissions)
            );
            assert_eq!(stages(&dir.0), residue);
            assert_eq!(fs::read(&residue[0]).unwrap(), b"synthetic candidate");
            assert_eq!(dir.vault().exists(), present);
            if present {
                assert_eq!(fs::read(dir.vault()).unwrap(), b"synthetic base");
            }
            assert!(!native::no_extended_acl(&File::open(dir.companion()).unwrap()).unwrap());
            remove_test_acl(&dir.companion());
            assert_eq!(lease.read_current().unwrap().as_deref(), base);
            assert_eq!(
                VaultPath::resolve(&dir.vault()).unwrap().acquire().err(),
                Some(NativeError::WriterUnavailable)
            );
            drop(lease);
            dir.lease();
        }
    }
}

#[test]
fn read_preserves_file_and_returns_bytes_requiring_authentication() {
    let dir = TestDir::new();
    let bytes = fixture();
    dir.write(&bytes);
    let lease = dir.lease();
    assert_eq!(lease.read_current().unwrap(), Some(bytes.clone()));
    let access = codeward_core::vault::begin_recovery(
        &bytes,
        include_str!("../../../codeward-core/tests/fixtures/authenticated-recovery.txt").trim(),
    )
    .unwrap();
    assert_eq!(access.revision(), 1);
    assert_eq!(fs::read(dir.vault()).unwrap(), bytes);
    assert_eq!(
        fs::metadata(dir.companion()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::metadata(dir.companion()).unwrap().len(), 0);
    drop(lease);
    assert!(dir.companion().exists());
}

#[test]
fn absence_empty_and_malformed_input_are_distinct_from_authenticated_access() {
    let dir = TestDir::new();
    let lease = dir.lease();
    assert_eq!(lease.read_current().unwrap(), None);
    dir.write(&[]);
    assert_eq!(lease.read_current().unwrap(), Some(Vec::new()));
    assert!(codeward_core::envelope::UnauthenticatedEnvelope::parse(&[]).is_err());
    fs::write(dir.vault(), b"synthetic invalid vault").unwrap();
    assert_eq!(
        lease.read_current().unwrap(),
        Some(b"synthetic invalid vault".to_vec())
    );
}

#[test]
fn reads_exact_cap_and_rejects_oversized_sparse_file_before_buffering() {
    let dir = TestDir::new();
    dir.write(&[]);
    let file = OpenOptions::new().write(true).open(dir.vault()).unwrap();
    file.set_len(MAX_FILE_LENGTH as u64).unwrap();
    let lease = dir.lease();
    assert_eq!(
        lease.read_current().unwrap().unwrap().len(),
        MAX_FILE_LENGTH
    );
    file.set_len(MAX_FILE_LENGTH as u64 + 1).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::TooLarge));
    assert_eq!(
        fs::metadata(dir.vault()).unwrap().len(),
        MAX_FILE_LENGTH as u64 + 1
    );
}

#[test]
fn reads_reject_growth_truncation_and_named_replacement_during_read() {
    for change in 0..3 {
        let dir = TestDir::new();
        dir.write(b"synthetic original");
        let lease = dir.lease();
        let result = lease.read_with_hook(|| match change {
            0 => OpenOptions::new()
                .append(true)
                .open(dir.vault())
                .unwrap()
                .write_all(b"x")
                .unwrap(),
            1 => OpenOptions::new()
                .write(true)
                .open(dir.vault())
                .unwrap()
                .set_len(0)
                .unwrap(),
            _ => {
                fs::rename(dir.vault(), dir.0.join("original.cw")).unwrap();
                dir.write(b"replacement");
            }
        });
        assert_eq!(result, Err(NativeError::Conflict));
    }
}

#[test]
fn growth_at_cap_is_detected_with_stack_probe() {
    let dir = TestDir::new();
    dir.write(&[]);
    let file = OpenOptions::new().write(true).open(dir.vault()).unwrap();
    file.set_len(MAX_FILE_LENGTH as u64).unwrap();
    let lease = dir.lease();
    assert_eq!(
        lease.read_with_hook(|| file.set_len(MAX_FILE_LENGTH as u64 + 1).unwrap()),
        Err(NativeError::TooLarge)
    );
}

#[test]
fn refuses_symlinks_hardlinks_and_special_vault_entries_without_following_them() {
    let dir = TestDir::new();
    let target = dir.0.join("synthetic-target");
    fs::write(&target, b"unchanged").unwrap();
    symlink(&target, dir.vault()).unwrap();
    let lease = dir.lease();
    assert_eq!(lease.read_current(), Err(NativeError::Unsupported));
    fs::remove_file(dir.vault()).unwrap();
    fs::hard_link(&target, dir.vault()).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Unsupported));
    fs::remove_file(dir.vault()).unwrap();
    fs::create_dir(dir.vault()).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Unsupported));
    assert_eq!(fs::read(&target).unwrap(), b"unchanged");
}

#[test]
fn rejects_relative_dot_parent_empty_nul_and_reserved_companion_paths() {
    for path in [
        "relative.cw",
        "/private/tmp/../vault",
        "/private/tmp/./vault",
        "/private//tmp/vault",
        "/private/tmp/",
        "/private/tmp/\0vault",
        "/private/tmp/.CODEWARD.WRITER-LOCK",
    ] {
        assert_eq!(
            VaultPath::resolve(Path::new(path)).err(),
            Some(NativeError::InvalidPath)
        );
    }
    let dir = TestDir::new();
    symlink(&dir.0, dir.0.join("alias")).unwrap();
    assert_eq!(
        VaultPath::resolve(&dir.0.join("alias/vault.cw")).err(),
        Some(NativeError::Unsupported)
    );
}

#[test]
fn posix_mode_checks_refuse_exposed_parent_vault_and_companion_without_chmod() {
    let dir = TestDir::new();
    fs::set_permissions(&dir.0, Permissions::from_mode(0o777)).unwrap();
    assert_eq!(
        VaultPath::resolve(&dir.vault()).err(),
        Some(NativeError::Permissions)
    );
    fs::set_permissions(&dir.0, Permissions::from_mode(0o700)).unwrap();
    dir.write(b"synthetic");
    let lease = dir.lease();
    fs::set_permissions(dir.vault(), Permissions::from_mode(0o644)).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Permissions));
    fs::set_permissions(dir.companion(), Permissions::from_mode(0o644)).unwrap();
    assert!(lease.read_current().is_err());
    drop(lease);
    assert_eq!(
        VaultPath::resolve(&dir.vault()).unwrap().acquire().err(),
        Some(NativeError::Permissions)
    );
    assert_eq!(
        fs::metadata(dir.companion()).unwrap().permissions().mode() & 0o777,
        0o644
    );
}

#[test]
fn separate_opens_and_vault_names_share_one_directory_lease() {
    let dir = TestDir::new();
    let lease = dir.lease();
    for path in [
        dir.vault(),
        dir.0.join("another.cw"),
        dir.0.join("VAULT.cw"),
    ] {
        assert_eq!(
            VaultPath::resolve(&path).unwrap().acquire().err(),
            Some(NativeError::WriterUnavailable)
        );
    }
    drop(lease);
    VaultPath::resolve(&dir.vault()).unwrap().acquire().unwrap();
}

#[test]
fn companion_symlink_hardlink_content_and_substitution_are_refused() {
    let dir = TestDir::new();
    dir.write(b"synthetic vault untouched");
    symlink(dir.vault(), dir.companion()).unwrap();
    assert_eq!(
        VaultPath::resolve(&dir.vault()).unwrap().acquire().err(),
        Some(NativeError::Unsupported)
    );
    fs::remove_file(dir.companion()).unwrap();
    fs::hard_link(dir.vault(), dir.companion()).unwrap();
    assert_eq!(
        VaultPath::resolve(&dir.vault()).unwrap().acquire().err(),
        Some(NativeError::Unsupported)
    );
    fs::remove_file(dir.companion()).unwrap();
    let mut unrelated = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dir.companion())
        .unwrap();
    unrelated.write_all(b"synthetic unrelated file").unwrap();
    assert_eq!(
        VaultPath::resolve(&dir.vault()).unwrap().acquire().err(),
        Some(NativeError::Permissions)
    );
    assert_eq!(
        fs::read(dir.companion()).unwrap(),
        b"synthetic unrelated file"
    );
    drop(unrelated);
    fs::remove_file(dir.companion()).unwrap();
    let lease = dir.lease();
    fs::rename(dir.companion(), dir.0.join("original-lock")).unwrap();
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dir.companion())
        .unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Conflict));
    assert_eq!(fs::read(dir.vault()).unwrap(), b"synthetic vault untouched");
}

#[test]
fn parent_substitution_is_detected_from_retained_ancestor_handles() {
    let dir = TestDir::new();
    let parent = dir.0.join("parent");
    fs::create_dir(&parent).unwrap();
    let lease = VaultPath::resolve(&parent.join("vault.cw"))
        .unwrap()
        .acquire()
        .unwrap();
    fs::rename(&parent, dir.0.join("old-parent")).unwrap();
    fs::create_dir(&parent).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Conflict));
}

// The same test executable is its own subprocess helper, so no shipped CLI or
// test-only production backdoor is needed. Child process gets only a synthetic path.
#[test]
fn subprocess_lock_helper() {
    let Some(path) = std::env::var_os("CODEWARD_SYNTHETIC_LOCK_PATH") else {
        return;
    };
    let result = VaultPath::resolve(Path::new(&path)).and_then(VaultPath::acquire);
    match result {
        Ok(_lease) => {
            println!("LEASE_ACQUIRED");
            io::stdout().flush().unwrap();
            let mut command = String::new();
            io::stdin().read_line(&mut command).unwrap();
        }
        Err(NativeError::WriterUnavailable) => println!("LEASE_BUSY"),
        Err(error) => panic!("{error}"),
    }
}

fn child(dir: &TestDir) -> (Child, String) {
    let mut process = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "reading::tests::subprocess_lock_helper",
            "--nocapture",
        ])
        .env("CODEWARD_SYNTHETIC_LOCK_PATH", dir.vault())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(process.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        if line.contains("LEASE_ACQUIRED") || line.contains("LEASE_BUSY") {
            break;
        }
    }
    process.stdout = Some(output.into_inner());
    (process, line)
}

#[test]
fn real_subprocess_contends_and_termination_releases_without_unlinking() {
    let dir = TestDir::new();
    let (mut holder, status) = child(&dir);
    assert!(status.contains("LEASE_ACQUIRED"));
    let (mut competitor, status) = child(&dir);
    assert!(status.contains("LEASE_BUSY"));
    assert!(competitor.wait().unwrap().success());
    assert_eq!(
        VaultPath::resolve(&dir.vault()).unwrap().acquire().err(),
        Some(NativeError::WriterUnavailable)
    );
    let inode = fs::metadata(dir.companion()).unwrap().ino();
    holder.kill().unwrap();
    assert!(!holder.wait().unwrap().success());
    let lease = dir.lease();
    assert_eq!(fs::metadata(dir.companion()).unwrap().ino(), inode);
    drop(lease);
    let (mut graceful, status) = child(&dir);
    assert!(status.contains("LEASE_ACQUIRED"));
    graceful.stdin.take().unwrap().write_all(b"exit\n").unwrap();
    assert!(graceful.wait().unwrap().success());
    assert!(VaultPath::resolve(&dir.vault()).unwrap().acquire().is_ok());
}
