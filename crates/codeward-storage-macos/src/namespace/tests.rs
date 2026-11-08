use std::{
    fs::{self, OpenOptions, Permissions},
    io::{BufRead, BufReader, Write},
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use super::*;
use crate::{
    PublicationError, VaultPath,
    test_support::{
        MountFault, MountProbe, TestDir, chmod_acl, fixture, maximum_revision_fixture, stages,
    },
};

// (directory descriptor, full flush rather than fsync).
type Step = (bool, bool);

#[test]
fn managed_ancestor_change_at_last_barrier_refuses_present_or_absent_success() {
    use std::os::unix::fs::OpenOptionsExt;
    for present in [false, true] {
        let dir = TestDir::new();
        let path = VaultPath::managed_for_test(&dir.0).unwrap();
        if present {
            use std::io::Write;
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(dir.managed_vault())
                .unwrap()
                .write_all(b"synthetic original")
                .unwrap();
        }
        let lease = path.acquire().unwrap();
        let count = if present { 12 } else { 10 };
        let mut probe = Probe {
            hook: Some(Box::new(|index| {
                if index == count {
                    chmod_acl(&dir.0.join("Library"), "everyone allow read");
                }
            })),
            ..Probe::default()
        };
        let expected = present.then_some(b"synthetic original".as_slice());
        assert_eq!(
            lease.namespace_with(expected, &mut probe),
            Err(NativeError::Permissions)
        );
        assert_eq!(probe.calls.len(), count);
        assert_eq!(fs::read(dir.managed_vault()).ok().as_deref(), expected);
    }
}
#[derive(Default)]
struct Probe<'a> {
    calls: Vec<Step>,
    inodes: Vec<u64>,
    fail: Option<(usize, i32)>,
    hook: Option<Box<dyn FnMut(usize) + 'a>>,
}
impl Probe<'_> {
    fn run(&mut self, file: &File, full: bool) -> io::Result<()> {
        let metadata = file.metadata()?;
        self.calls.push((metadata.is_dir(), full));
        self.inodes.push(metadata.ino());
        if let Some((index, code)) = self.fail
            && self.calls.len() == index
        {
            return Err(io::Error::from_raw_os_error(code));
        }
        if full {
            native::full_sync(file)?;
        } else {
            native::fsync_once(file)?;
        }
        if let Some(hook) = &mut self.hook {
            hook(self.calls.len());
        }
        Ok(())
    }
}

#[test]
fn managed_namespace_flushes_each_retained_directory_leaf_first_even_after_reopen() {
    let dir = TestDir::new();
    // A previous process can create part or all of the tree without committing.
    drop(VaultPath::managed_for_test(&dir.0).unwrap());
    let vault = dir.managed_vault();
    let directories: Vec<_> = vault
        .ancestors()
        .skip(1)
        .take(5)
        .map(|path| fs::metadata(path).unwrap().ino())
        .collect();
    assert_eq!(
        directories.last(),
        Some(&fs::metadata(&dir.0).unwrap().ino())
    );

    let lease = VaultPath::managed_for_test(&dir.0)
        .unwrap()
        .acquire()
        .unwrap();
    let mut absent = Probe::default();
    lease.namespace_with(None, &mut absent).unwrap();
    let paired: Vec<_> = directories
        .iter()
        .flat_map(|&inode| [inode, inode])
        .collect();
    assert_eq!(absent.inodes, paired);
    assert_eq!(absent.calls, [(true, false), (true, true)].repeat(5));

    let bytes = fixture();
    lease
        .prepare_stage(&bytes)
        .unwrap()
        .publish_create()
        .unwrap();
    let file_inode = fs::metadata(vault).unwrap().ino();
    let mut expected = vec![file_inode; 2];
    expected.extend(paired);
    for (index, error) in [(7, libc::ENOSPC), (12, libc::EIO)] {
        let mut refused = Probe {
            fail: Some((index, error)),
            ..Probe::default()
        };
        assert_eq!(
            lease.namespace_with(Some(&bytes), &mut refused),
            Err(NativeError::SyncNamespace)
        );
        assert_eq!(refused.inodes, expected[..index]);
        assert_eq!(
            lease.read_current().unwrap().as_deref(),
            Some(bytes.as_slice())
        );
    }
    let mut present = Probe::default();
    lease.namespace_with(Some(&bytes), &mut present).unwrap();
    assert_eq!(present.inodes, expected);
    assert_eq!(present.calls[..2], [(false, false), (false, true)]);
    assert_eq!(present.calls[2..], absent.calls);
}
impl NamespaceIo for Probe<'_> {
    fn fsync(&mut self, file: &File) -> io::Result<()> {
        self.run(file, false)
    }
    fn full_sync(&mut self, file: &File) -> io::Result<()> {
        self.run(file, true)
    }
}

#[test]
fn mount_policy_changes_at_final_barrier_refuse_present_and_absent_results() {
    for (fault, error) in [
        (MountFault::Refused, NativeError::Unsupported),
        (MountFault::QueryFailed, NativeError::Io),
    ] {
        for present in [false, true] {
            let mount = MountProbe::new();
            let dir = TestDir::new();
            if present {
                dir.write(b"synthetic original");
            }
            let lease = dir.lease();
            let count = if present { 4 } else { 2 };
            let mut probe = Probe {
                hook: Some(Box::new(|index| {
                    if index == count {
                        mount.set(Some(fault));
                    }
                })),
                ..Probe::default()
            };
            let expected = present.then_some(b"synthetic original".as_slice());
            assert_eq!(lease.namespace_with(expected, &mut probe), Err(error));
            assert_eq!(probe.calls.len(), count);
            assert_eq!(fs::read(dir.vault()).ok().as_deref(), expected);
            mount.set(None);
            lease.synchronize_namespace(expected).unwrap();
        }
    }
}

#[test]
fn synchronizes_actual_published_file_then_parent_and_repeats_on_reconciliation() {
    let dir = TestDir::new();
    let old = fixture();
    let new = maximum_revision_fixture();
    let lease = dir.lease();
    drop(lease.prepare_stage(b"unrelated synthetic residue").unwrap());
    lease.prepare_stage(&old).unwrap().publish_create().unwrap();
    lease.synchronize_namespace(Some(&old)).unwrap();
    lease
        .prepare_stage(&new)
        .unwrap()
        .publish_replace(&old)
        .unwrap();
    let inode = fs::metadata(dir.vault()).unwrap().ino();
    for _ in 0..2 {
        let mut probe = Probe::default();
        lease.namespace_with(Some(&new), &mut probe).unwrap();
        assert_eq!(
            probe.calls,
            [(false, false), (false, true), (true, false), (true, true)]
        );
        assert_eq!(fs::read(dir.vault()).unwrap(), new);
        assert_eq!(fs::metadata(dir.vault()).unwrap().ino(), inode);
    }
    let access = codeward_core::vault::begin_recovery(
        &new,
        include_str!("../../../codeward-core/tests/fixtures/authenticated-recovery.txt").trim(),
    )
    .unwrap();
    assert_eq!(access.revision(), u64::MAX);
    assert_eq!(stages(&dir.0).len(), 1);
}

#[test]
fn absence_requires_both_parent_calls_and_never_promotes_a_complete_stage() {
    let dir = TestDir::new();
    let lease = dir.lease();
    drop(lease.prepare_stage(&fixture()).unwrap());
    let mut probe = Probe::default();
    lease.namespace_with(None, &mut probe).unwrap();
    assert_eq!(probe.calls, [(true, false), (true, true)]);
    assert!(!dir.vault().exists());
    assert_eq!(fs::read(stages(&dir.0).pop().unwrap()).unwrap(), fixture());
}

#[test]
fn wrong_missing_oversized_and_changed_bases_refuse_before_any_barrier() {
    for case in 0..5 {
        let dir = TestDir::new();
        let lease = dir.lease();
        if case != 1 {
            dir.write(b"actual");
        }
        if case == 4 {
            OpenOptions::new()
                .write(true)
                .open(dir.vault())
                .unwrap()
                .set_len(MAX_FILE_LENGTH as u64 + 1)
                .unwrap();
        }
        let mut probe = Probe::default();
        let oversized = vec![0; MAX_FILE_LENGTH + 1];
        let expected = match case {
            2 => None,
            3 => Some(oversized.as_slice()),
            _ => Some(b"other".as_slice()),
        };
        assert!(lease.namespace_with(expected, &mut probe).is_err());
        assert!(probe.calls.is_empty());
        if case == 4 {
            assert_eq!(
                fs::metadata(dir.vault()).unwrap().len(),
                MAX_FILE_LENGTH as u64 + 1
            );
        } else if case != 1 {
            assert_eq!(fs::read(dir.vault()).unwrap(), b"actual");
        }
    }
}

#[test]
fn exact_cap_observation_is_bounded_and_preserved() {
    let dir = TestDir::new();
    let bytes = vec![0x73; MAX_FILE_LENGTH];
    dir.write(&bytes);
    let lease = dir.lease();
    lease.synchronize_namespace(Some(&bytes)).unwrap();
    assert_eq!(lease.read_current().unwrap().unwrap(), bytes);
}

#[test]
fn each_sync_error_is_terminal_without_retry_downgrade_rollback_or_cleanup() {
    for present in [false, true] {
        let limit = if present { 4 } else { 2 };
        for index in 1..=limit {
            for code in [
                libc::EIO,
                libc::EINTR,
                libc::EINVAL,
                libc::ENOTSUP,
                libc::ENOTTY,
            ] {
                let dir = TestDir::new();
                let lease = dir.lease();
                let bytes = fixture();
                drop(lease.prepare_stage(b"preserved residue").unwrap());
                if present {
                    lease
                        .prepare_stage(&bytes)
                        .unwrap()
                        .publish_create()
                        .unwrap();
                }
                let mut probe = Probe {
                    fail: Some((index, code)),
                    ..Probe::default()
                };
                assert_eq!(
                    lease.namespace_with(present.then_some(bytes.as_slice()), &mut probe),
                    Err(NativeError::SyncNamespace)
                );
                assert_eq!(probe.calls.len(), index);
                assert_eq!(lease.read_current().unwrap(), present.then_some(bytes));
                assert_eq!(
                    fs::read(stages(&dir.0).pop().unwrap()).unwrap(),
                    b"preserved residue"
                );
            }
        }
    }
}

#[test]
fn permissions_acl_links_and_parent_companion_substitution_refuse_before_barriers() {
    for case in 0..7 {
        let dir = TestDir::new();
        dir.write(b"original");
        let lease = dir.lease();
        match case {
            0 => chmod_acl(&dir.vault(), "everyone allow read"),
            1 => chmod_acl(&dir.0, "everyone allow read"),
            2 => fs::set_permissions(dir.vault(), Permissions::from_mode(0o644)).unwrap(),
            3 => fs::hard_link(dir.vault(), dir.0.join("alias")).unwrap(),
            4 => {
                fs::rename(dir.vault(), dir.0.join("original")).unwrap();
                symlink(dir.0.join("original"), dir.vault()).unwrap();
            }
            5 => {
                fs::rename(dir.companion(), dir.0.join("old-lock")).unwrap();
                fs::write(dir.companion(), b"").unwrap();
            }
            _ => {
                fs::rename(&dir.0, dir.0.with_extension("moved")).unwrap();
                fs::create_dir(&dir.0).unwrap();
            }
        }
        let mut probe = Probe::default();
        assert!(lease.namespace_with(Some(b"original"), &mut probe).is_err());
        assert!(probe.calls.is_empty());
        if case == 6 {
            fs::remove_dir(&dir.0).unwrap();
            fs::rename(dir.0.with_extension("moved"), &dir.0).unwrap();
        }
        assert_eq!(fs::read(dir.vault()).unwrap(), b"original");
    }
}

#[test]
fn companion_acl_added_by_last_barrier_refuses_present_or_absent_result() {
    for present in [false, true] {
        let dir = TestDir::new();
        if present {
            dir.write(b"original");
        }
        let lease = dir.lease();
        let count = if present { 4 } else { 2 };
        let mut probe = Probe {
            hook: Some(Box::new(|index| {
                if index == count {
                    chmod_acl(&dir.companion(), "everyone allow read");
                }
            })),
            ..Probe::default()
        };
        assert_eq!(
            lease.namespace_with(present.then_some(b"original".as_slice()), &mut probe),
            Err(NativeError::Permissions)
        );
        assert_eq!(probe.calls.len(), count);
        assert_eq!(
            fs::read(dir.vault()).ok().as_deref(),
            present.then_some(b"original".as_slice())
        );
        assert!(!native::no_extended_acl(&File::open(dir.companion()).unwrap()).unwrap());
    }
}

#[test]
fn changes_during_barriers_do_not_establish_the_requested_observation() {
    for case in 0..6 {
        let dir = TestDir::new();
        let lease = dir.lease();
        let present = case != 5;
        if present {
            dir.write(b"original");
        }
        let mut probe = Probe {
            hook: Some(Box::new(|index| {
                if index != if present { 4 } else { 2 } {
                    return;
                }
                match case {
                    0 => fs::write(dir.vault(), b"modified").unwrap(),
                    1 => {
                        fs::rename(dir.vault(), dir.0.join("old")).unwrap();
                        dir.write(b"original");
                    }
                    2 => chmod_acl(&dir.vault(), "everyone allow read"),
                    3 => fs::set_permissions(&dir.0, Permissions::from_mode(0o777)).unwrap(),
                    4 => {
                        fs::rename(dir.companion(), dir.0.join("old-lock")).unwrap();
                        fs::write(dir.companion(), b"").unwrap();
                    }
                    _ => dir.write(b"racing creation"),
                }
            })),
            ..Probe::default()
        };
        assert!(
            lease
                .namespace_with(present.then_some(b"original".as_slice()), &mut probe)
                .is_err()
        );
        assert_eq!(
            fs::read(dir.vault()).unwrap(),
            match case {
                0 => b"modified".as_slice(),
                5 => b"racing creation",
                _ => b"original",
            }
        );
    }
}

#[test]
fn namespace_observation_excludes_same_lease_publication_and_other_barriers() {
    let dir = TestDir::new();
    dir.write(b"original");
    let lease = dir.lease();
    let stage = lease.prepare_stage(b"candidate").unwrap();
    let (entered, ready) = mpsc::channel();
    let (release, proceed) = mpsc::channel();
    std::thread::scope(|scope| {
        let observation = &lease;
        let worker = scope.spawn(move || {
            let mut probe = Probe {
                hook: Some(Box::new(move |index| {
                    if index == 1 {
                        entered.send(()).unwrap();
                        proceed.recv_timeout(Duration::from_secs(15)).unwrap();
                    }
                })),
                ..Probe::default()
            };
            observation.namespace_with(Some(b"original"), &mut probe)
        });
        ready.recv_timeout(Duration::from_secs(15)).unwrap();
        let publish = stage.publish_replace(b"original");
        let sync = lease.synchronize_namespace(Some(b"original"));
        release.send(()).unwrap();
        assert_eq!(worker.join().unwrap(), Ok(()));
        assert_eq!(
            publish,
            Err(PublicationError::NotPublished(
                NativeError::WriterUnavailable
            ))
        );
        assert_eq!(sync, Err(NativeError::WriterUnavailable));
    });
    assert_eq!(fs::read(dir.vault()).unwrap(), b"original");
    lease.synchronize_namespace(Some(b"original")).unwrap();
}

fn checkpoint() -> ! {
    println!("namespace-checkpoint");
    io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}
#[test]
fn namespace_subprocess_helper() {
    let Ok(path) = std::env::var("CODEWARD_NAMESPACE_PATH") else {
        return;
    };
    let lease = VaultPath::resolve(Path::new(&path))
        .unwrap()
        .acquire()
        .unwrap();
    let mode = std::env::var("CODEWARD_NAMESPACE_MODE").unwrap();
    let phase: usize = std::env::var("CODEWARD_NAMESPACE_PHASE")
        .unwrap()
        .parse()
        .unwrap();
    let bytes = maximum_revision_fixture();
    let expected = if mode == "absent" {
        None
    } else {
        Some(bytes.as_slice())
    };
    drop(lease.prepare_stage(b"preserved residue").unwrap());
    if mode == "create" {
        lease
            .prepare_stage(&bytes)
            .unwrap()
            .publish_create()
            .unwrap();
    }
    if mode == "replace" {
        lease
            .prepare_stage(&bytes)
            .unwrap()
            .publish_replace(&fixture())
            .unwrap();
    }
    if phase == 0 {
        checkpoint();
    }
    let mut probe = Probe {
        hook: Some(Box::new(|index| {
            if index == phase {
                checkpoint();
            }
        })),
        ..Probe::default()
    };
    lease.namespace_with(expected, &mut probe).unwrap();
    checkpoint();
}

#[test]
fn process_kills_around_each_barrier_preserve_state_residue_and_allow_reconciliation() {
    for mode in ["create", "replace", "absent"] {
        let count = if mode == "absent" { 2 } else { 4 };
        for phase in 0..=count + 1 {
            let dir = TestDir::new();
            if mode == "replace" {
                dir.write(&fixture());
            }
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "namespace::tests::namespace_subprocess_helper",
                    "--nocapture",
                ])
                .env("CODEWARD_NAMESPACE_PATH", dir.vault())
                .env("CODEWARD_NAMESPACE_MODE", mode)
                .env("CODEWARD_NAMESPACE_PHASE", phase.to_string())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let stdout = child.stdout.take().unwrap();
            let (sender, receiver) = mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if line.unwrap() == "namespace-checkpoint" {
                        let _ = sender.send(());
                        break;
                    }
                }
            });
            let ready = receiver.recv_timeout(Duration::from_secs(15));
            child.kill().unwrap();
            child.wait().unwrap();
            reader.join().unwrap();
            assert!(ready.is_ok(), "child did not reach {mode}/{phase}");
            let lease = dir.lease();
            let bytes = maximum_revision_fixture();
            let expected = (mode != "absent").then_some(bytes.as_slice());
            assert_eq!(lease.read_current().unwrap().as_deref(), expected);
            lease.synchronize_namespace(expected).unwrap();
            assert_eq!(
                fs::read(stages(&dir.0).pop().unwrap()).unwrap(),
                b"preserved residue"
            );
        }
    }
}
