use super::*;
use crate::{
    PublicationError,
    test_support::{MountFault, MountProbe, TestDir, chmod_acl, clear_test_acl, fixture, stages},
};
use std::{
    fs::{self, OpenOptions, Permissions},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink},
    process::Command,
};

fn prepare(dir: &TestDir) -> VaultPath {
    VaultPath::managed_for_test(&dir.0).unwrap()
}
fn app(dir: &TestDir) -> std::path::PathBuf {
    dir.0.join("Library/Application Support/Codeward")
}
fn write_vault(dir: &TestDir, bytes: &[u8]) {
    use std::io::Write;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dir.managed_vault())
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

#[test]
fn managed_preparation_is_private_idempotent_and_leaves_vault_absent() {
    let dir = TestDir::new();
    let first = prepare(&dir);
    assert_eq!(first.name.as_c_str(), c"vault.cw");
    let paths = [app(&dir), app(&dir).join("Vaults")];
    let identities: Vec<_> = paths
        .iter()
        .map(|p| {
            let file = File::open(p).unwrap();
            assert_eq!(file.metadata().unwrap().mode() & 0o7777, 0o700);
            assert!(native::no_extended_acl(&file).unwrap());
            file.metadata().unwrap().ino()
        })
        .collect();
    let second = prepare(&dir);
    for (path, inode) in paths.iter().zip(identities) {
        assert_eq!(fs::metadata(path).unwrap().ino(), inode);
    }
    assert!(!dir.managed_vault().exists());
    let lease = first.acquire().unwrap();
    assert_eq!(lease.read_current().unwrap(), None);
    assert_eq!(second.acquire().err(), Some(NativeError::WriterUnavailable));
}

#[test]
fn managed_preparation_preserves_existing_encrypted_vault_and_residue() {
    let dir = TestDir::new();
    let path = prepare(&dir);
    let bytes = fixture();
    write_vault(&dir, &bytes);
    let parent = app(&dir).join("Vaults");
    fs::write(parent.join("unrelated"), b"synthetic unrelated").unwrap();
    let lease = path.acquire().unwrap();
    drop(lease.prepare_stage(b"synthetic encrypted residue").unwrap());
    let residue = stages(&parent);
    let inode = fs::metadata(dir.managed_vault()).unwrap().ino();
    assert_eq!(
        prepare(&dir).acquire().err(),
        Some(NativeError::WriterUnavailable)
    );
    drop(lease);
    assert_eq!(
        prepare(&dir).acquire().unwrap().read_current().unwrap(),
        Some(bytes)
    );
    assert_eq!(fs::metadata(dir.managed_vault()).unwrap().ino(), inode);
    assert_eq!(stages(&parent), residue);
    assert_eq!(
        fs::read(&residue[0]).unwrap(),
        b"synthetic encrypted residue"
    );
    assert_eq!(
        fs::read(parent.join("unrelated")).unwrap(),
        b"synthetic unrelated"
    );
}

#[test]
fn managed_fixed_components_refuse_links_files_and_exposed_private_directories() {
    let suffixes = [
        "Library",
        "Library/Application Support",
        "Library/Application Support/Codeward",
        "Library/Application Support/Codeward/Vaults",
    ];
    for index in 0..suffixes.len() {
        for file_collision in [false, true] {
            let dir = TestDir::new();
            for suffix in &suffixes[..index] {
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(dir.0.join(suffix))
                    .unwrap();
            }
            let path = dir.0.join(suffixes[index]);
            let target = dir.0.join("unrelated-target");
            fs::create_dir(&target).unwrap();
            if file_collision {
                fs::write(&path, b"synthetic untouched").unwrap();
            } else {
                symlink(&target, &path).unwrap();
            }
            assert_eq!(
                VaultPath::managed_for_test(&dir.0).err(),
                Some(NativeError::Unsupported)
            );
            assert!(fs::read_dir(&target).unwrap().next().is_none());
            if file_collision {
                assert_eq!(fs::read(&path).unwrap(), b"synthetic untouched");
            } else {
                assert!(
                    fs::symlink_metadata(&path)
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
            }
        }
    }
    for leaf in ["Codeward", "Vaults"] {
        let dir = TestDir::new();
        drop(prepare(&dir));
        let path = if leaf == "Codeward" {
            app(&dir)
        } else {
            app(&dir).join("Vaults")
        };
        fs::set_permissions(&path, Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            VaultPath::managed_for_test(&dir.0).err(),
            Some(NativeError::Permissions)
        );
        assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o755);
    }
}

#[test]
fn managed_ancestor_owner_mode_and_deny_only_acl_policy_is_conservative() {
    for owner in [0, 7] {
        assert!(ancestor_owner_mode(owner, 0o755, 7));
    }
    assert!(!ancestor_owner_mode(8, 0o700, 7));
    for mode in [0o775, 0o777, 0o1777] {
        assert!(!ancestor_owner_mode(7, mode, 7));
    }
    let dir = TestDir::new();
    chmod_acl(&dir.0, "everyone deny delete");
    let path = prepare(&dir);
    let lease = path.acquire().unwrap();
    assert_eq!(lease.read_current().unwrap(), None);
    let library = dir.0.join("Library");
    chmod_acl(&library, "everyone allow read");
    assert_eq!(lease.read_current(), Err(NativeError::Permissions));
    assert!(!dir.managed_vault().exists());
    clear_test_acl(&library);
    clear_test_acl(&dir.0);
    assert_eq!(lease.read_current().unwrap(), None);
}

#[test]
fn managed_preparation_refuses_bad_home_or_mount_without_creating_a_fallback() {
    for home in ["", "relative", "/", "/private//tmp", "/private/../tmp"] {
        assert_eq!(
            VaultPath::managed_for_test(Path::new(home)).err(),
            Some(NativeError::InvalidPath)
        );
    }
    for (fault, error) in [
        (MountFault::Refused, NativeError::Unsupported),
        (MountFault::QueryFailed, NativeError::Io),
    ] {
        let probe = MountProbe::new();
        let dir = TestDir::new();
        probe.set(Some(fault));
        assert_eq!(VaultPath::managed_for_test(&dir.0).err(), Some(error));
        assert!(!dir.0.join("Library").exists());
    }
    let dir = TestDir::new();
    assert!(VaultPath::managed_for_test(&dir.0.join("missing-account-home")).is_err());
    assert!(fs::read_dir(&dir.0).unwrap().next().is_none());
}

#[test]
fn managed_substitution_after_open_is_refused_without_removing_either_directory() {
    for target_private in [false, true] {
        let dir = TestDir::new();
        let count = components(&dir.0.join(SUFFIX)).unwrap().0.len();
        let target_index = if target_private { count - 1 } else { count - 2 };
        let target = if target_private {
            app(&dir).join("Vaults")
        } else {
            app(&dir)
        };
        let displaced = target.with_extension("displaced");
        assert_eq!(
            VaultPath::prepare_managed(&dir.0, |index| {
                if index == target_index {
                    fs::rename(&target, &displaced).unwrap();
                    fs::DirBuilder::new().mode(0o700).create(&target).unwrap();
                }
            })
            .err(),
            Some(NativeError::Conflict)
        );
        assert!(target.is_dir());
        assert!(displaced.is_dir());
        assert!(!dir.managed_vault().exists());
    }
}

#[test]
fn managed_competing_preparations_share_the_same_private_directories_and_lease() {
    let dir = TestDir::new();
    let (first, second) = std::thread::scope(|scope| {
        let one = scope.spawn(|| prepare(&dir));
        let two = scope.spawn(|| prepare(&dir));
        (one.join().unwrap(), two.join().unwrap())
    });
    assert_eq!(
        first.parent().metadata().unwrap().ino(),
        second.parent().metadata().unwrap().ino()
    );
    let lease = first.acquire().unwrap();
    assert_eq!(second.acquire().err(), Some(NativeError::WriterUnavailable));
    drop(lease);
    assert_eq!(
        prepare(&dir).acquire().unwrap().read_current().unwrap(),
        None
    );
}

#[test]
fn managed_inherited_deny_acls_refuse_private_creation_without_stripping_them() {
    let dir = TestDir::new();
    chmod_acl(
        &dir.0,
        "everyone deny delete,file_inherit,directory_inherit",
    );
    assert_eq!(
        VaultPath::managed_for_test(&dir.0).err(),
        Some(NativeError::Permissions)
    );
    assert!(app(&dir).is_dir());
    assert!(!app(&dir).join("Vaults").exists());
    assert!(!native::no_extended_acl(&File::open(app(&dir)).unwrap()).unwrap());
    for suffix in [
        "Library/Application Support/Codeward",
        "Library/Application Support",
        "Library",
        "",
    ] {
        clear_test_acl(&dir.0.join(suffix));
    }
}

#[test]
fn managed_ancestor_changes_block_reads_staging_publication_and_barriers() {
    for acl in [false, true] {
        let dir = TestDir::new();
        let path = prepare(&dir);
        write_vault(&dir, b"synthetic original");
        let lease = path.acquire().unwrap();
        let stage = lease.prepare_stage(b"synthetic candidate").unwrap();
        let parent = app(&dir).join("Vaults");
        let residue = stages(&parent);
        let ancestor = dir.0.join("Library");
        if acl {
            chmod_acl(&ancestor, "everyone allow read");
        } else {
            fs::set_permissions(&ancestor, Permissions::from_mode(0o777)).unwrap();
        }
        assert_eq!(lease.read_current(), Err(NativeError::Permissions));
        assert_eq!(
            lease.prepare_stage(b"another candidate").err(),
            Some(NativeError::Permissions)
        );
        assert_eq!(
            stage.publish_replace(b"synthetic original"),
            Err(PublicationError::NotPublished(NativeError::Permissions))
        );
        assert_eq!(
            lease.synchronize_namespace(Some(b"synthetic original")),
            Err(NativeError::Permissions)
        );
        assert_eq!(
            fs::read(dir.managed_vault()).unwrap(),
            b"synthetic original"
        );
        assert_eq!(stages(&parent), residue);
        assert_eq!(fs::read(&residue[0]).unwrap(), b"synthetic candidate");
        if acl {
            clear_test_acl(&ancestor);
        } else {
            fs::set_permissions(&ancestor, Permissions::from_mode(0o700)).unwrap();
        }
        assert_eq!(
            prepare(&dir).acquire().err(),
            Some(NativeError::WriterUnavailable)
        );
        drop(lease);
        assert_eq!(
            prepare(&dir)
                .acquire()
                .unwrap()
                .read_current()
                .unwrap()
                .unwrap(),
            b"synthetic original"
        );
    }
}

#[test]
fn managed_umask_helper() {
    let Some(home) = std::env::var_os("CODEWARD_MANAGED_TEST_HOME") else {
        return;
    };
    let restrictive = std::env::var_os("CODEWARD_MANAGED_TEST_RESTRICTIVE").is_some();
    // SAFETY: only this dedicated synthetic child process changes its umask.
    unsafe {
        libc::umask(if restrictive { 0o777 } else { 0o077 });
    }
    let result = VaultPath::managed_for_test(Path::new(&home));
    if restrictive {
        assert_eq!(result.err(), Some(NativeError::Permissions));
    } else {
        assert!(result.is_ok());
    }
}

#[test]
fn managed_restrictive_child_umask_is_not_repaired() {
    for restrictive in [false, true] {
        let dir = TestDir::new();
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", "path::managed::tests::managed_umask_helper"])
            .env("CODEWARD_MANAGED_TEST_HOME", &dir.0);
        if restrictive {
            child.env("CODEWARD_MANAGED_TEST_RESTRICTIVE", "1");
        } else {
            child.env_remove("CODEWARD_MANAGED_TEST_RESTRICTIVE");
        }
        assert!(child.status().unwrap().success());
        if restrictive {
            assert_eq!(
                fs::metadata(dir.0.join("Library")).unwrap().mode() & 0o777,
                0
            );
            assert!(!app(&dir).exists());
            fs::set_permissions(dir.0.join("Library"), Permissions::from_mode(0o700)).unwrap();
        } else {
            assert_eq!(fs::metadata(app(&dir)).unwrap().mode() & 0o777, 0o700);
        }
    }
}
