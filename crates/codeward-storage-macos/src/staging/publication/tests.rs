use std::{
    fs::{self, Permissions},
    io::{BufRead, BufReader, Write},
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::{Command, Stdio},
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use super::*;
use crate::{
    VaultPath,
    test_support::{MASTER_TESTS, MountFault, MountProbe, TestDir, chmod_acl, fixture, stages},
};
use codeward_core::vault::{begin_recovery, unlock_with_master};

const PAPER: &str =
    include_str!("../../../../codeward-core/tests/fixtures/authenticated-recovery.txt");
const NEW_PASSWORD: &str = "replacement synthetic publication password";

#[test]
fn managed_ancestor_change_after_rename_preserves_candidate_and_unknown_outcome() {
    use std::os::unix::fs::OpenOptionsExt;
    for create in [false, true] {
        let dir = TestDir::new();
        let path = VaultPath::managed_for_test(&dir.0).unwrap();
        if !create {
            fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(dir.managed_vault())
                .unwrap()
                .write_all(b"synthetic original")
                .unwrap();
        }
        let lease = path.acquire().unwrap();
        let stage = lease.prepare_stage(b"synthetic candidate").unwrap();
        let result = stage.publish_with(
            (!create).then_some(b"synthetic original".as_slice()),
            |parent, from, to, create| {
                native::publish_at(parent, from, to, create)?;
                fs::set_permissions(dir.0.join("Library"), Permissions::from_mode(0o777)).unwrap();
                Ok(())
            },
        );
        assert_eq!(result, Err(PublicationError::OutcomeUnknown));
        assert_eq!(
            fs::read(dir.managed_vault()).unwrap(),
            b"synthetic candidate"
        );
        assert_eq!(lease.read_current(), Err(NativeError::Permissions));
    }
}

#[test]
fn mount_policy_changes_after_native_publication_preserve_candidate_and_unknown_outcome() {
    for fault in [MountFault::Refused, MountFault::QueryFailed] {
        for create in [false, true] {
            let probe = MountProbe::new();
            let dir = TestDir::new();
            if !create {
                dir.write(b"synthetic original");
            }
            let lease = dir.lease();
            let stage = lease.prepare_stage(b"synthetic candidate").unwrap();
            let result = stage.publish_with(
                (!create).then_some(b"synthetic original".as_slice()),
                |parent, from, to, create| {
                    native::publish_at(parent, from, to, create)?;
                    probe.set(Some(fault));
                    Ok(())
                },
            );
            assert_eq!(result, Err(PublicationError::OutcomeUnknown));
            assert_eq!(fs::read(dir.vault()).unwrap(), b"synthetic candidate");
            assert!(stages(&dir.0).is_empty());
            probe.set(None);
            assert_eq!(
                lease.read_current().unwrap().unwrap(),
                b"synthetic candidate"
            );
        }
    }
}

fn candidate() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let _master = MASTER_TESTS.lock().unwrap();
        let access = begin_recovery(&fixture(), PAPER.trim()).unwrap();
        access
            .prepare_master_password(access.revision(), NEW_PASSWORD, NEW_PASSWORD)
            .unwrap()
            .as_bytes()
            .to_vec()
    })
}

fn stage_path(dir: &TestDir, stage: &PreparedStage<'_>) -> std::path::PathBuf {
    dir.0.join(stage._name.to_str().unwrap())
}

#[test]
fn publishes_complete_creation_and_coherent_recovery_replacement_without_cleanup() {
    let dir = TestDir::new();
    let lease = dir.lease();
    let old = fixture();
    let abandoned = lease.prepare_stage(b"unrelated synthetic residue").unwrap();
    let abandoned_path = stage_path(&dir, &abandoned);
    drop(abandoned);
    lease.prepare_stage(&old).unwrap().publish_create().unwrap();
    assert_eq!(lease.read_current().unwrap().unwrap(), old);
    let old_inode = fs::metadata(dir.vault()).unwrap().ino();
    lease
        .prepare_stage(candidate())
        .unwrap()
        .publish_replace(&old)
        .unwrap();
    let actual = lease.read_current().unwrap().unwrap();
    assert_eq!(actual, candidate());
    assert_ne!(fs::metadata(dir.vault()).unwrap().ino(), old_inode);
    assert_eq!(fs::metadata(dir.vault()).unwrap().mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(dir.vault()).unwrap().nlink(), 1);
    let _master = MASTER_TESTS.lock().unwrap();
    assert_eq!(
        unlock_with_master(&actual, NEW_PASSWORD)
            .unwrap()
            .revision(),
        2
    );
    assert_eq!(begin_recovery(&actual, PAPER.trim()).unwrap().revision(), 2);
    assert!(unlock_with_master(&actual, "synthetic password").is_err());
    assert_eq!(
        fs::read(abandoned_path).unwrap(),
        b"unrelated synthetic residue"
    );
}

#[test]
fn stale_missing_existing_and_oversized_bases_refuse_before_publication() {
    for case in 0..4 {
        let dir = TestDir::new();
        if case != 1 {
            dir.write(b"actual original");
        }
        let lease = dir.lease();
        let stage = lease.prepare_stage(b"candidate").unwrap();
        let path = stage_path(&dir, &stage);
        let original = lease.read_current().unwrap();
        let result = match case {
            0 => stage.publish_replace(b"stale original"),
            1 => stage.publish_replace(b"missing original"),
            2 => stage.publish_create(),
            _ => stage.publish_replace(&vec![0; MAX_FILE_LENGTH + 1]),
        };
        assert!(matches!(result, Err(PublicationError::NotPublished(_))));
        assert_eq!(lease.read_current().unwrap(), original);
        assert_eq!(fs::read(path).unwrap(), b"candidate");
    }
}

#[test]
fn modified_substituted_linked_exposed_and_acl_stages_refuse_with_original_untouched() {
    for case in 0..7 {
        let dir = TestDir::new();
        dir.write(b"original");
        let lease = dir.lease();
        let stage = lease.prepare_stage(b"candidate").unwrap();
        let path = stage_path(&dir, &stage);
        match case {
            0 => fs::write(&path, b"different").unwrap(),
            1 => {
                fs::rename(&path, dir.0.join("retained-stage")).unwrap();
                fs::write(&path, b"candidate").unwrap();
            }
            2 => {
                fs::remove_file(&path).unwrap();
                symlink(dir.vault(), &path).unwrap();
            }
            3 => fs::hard_link(&path, dir.0.join("stage-link")).unwrap(),
            4 => fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap(),
            5 => chmod_acl(&path, "everyone allow read,write"),
            _ => fs::remove_file(&path).unwrap(),
        }
        assert!(matches!(
            stage.publish_replace(b"original"),
            Err(PublicationError::NotPublished(_))
        ));
        assert_eq!(fs::read(dir.vault()).unwrap(), b"original");
    }
}

#[test]
fn observed_destination_parent_companion_and_acl_changes_refuse_before_rename() {
    for case in 0..8 {
        let dir = TestDir::new();
        dir.write(b"original");
        let lease = dir.lease();
        let stage = lease.prepare_stage(b"candidate").unwrap();
        let path = stage_path(&dir, &stage);
        match case {
            0 => chmod_acl(&dir.vault(), "everyone allow read,write"),
            1 => chmod_acl(&dir.0, "everyone deny delete"),
            2 => fs::set_permissions(&dir.0, Permissions::from_mode(0o777)).unwrap(),
            3 => {
                fs::rename(dir.companion(), dir.0.join("old-lock")).unwrap();
                fs::write(dir.companion(), b"").unwrap();
            }
            4 => {
                fs::rename(dir.vault(), dir.0.join("original-vault")).unwrap();
                symlink(dir.0.join("original-vault"), dir.vault()).unwrap();
            }
            5 => fs::hard_link(dir.vault(), dir.0.join("vault-link")).unwrap(),
            6 => fs::set_permissions(dir.vault(), Permissions::from_mode(0o644)).unwrap(),
            _ => {
                fs::rename(&dir.0, dir.0.with_extension("moved")).unwrap();
                fs::create_dir(&dir.0).unwrap();
            }
        }
        let result = stage.publish_replace(b"original");
        assert!(matches!(result, Err(PublicationError::NotPublished(_))));
        if case == 7 {
            fs::remove_dir(&dir.0).unwrap();
            fs::rename(dir.0.with_extension("moved"), &dir.0).unwrap();
        }
        assert_eq!(fs::read(path).unwrap(), b"candidate");
        assert_eq!(fs::read(dir.vault()).unwrap(), b"original");
    }
}

#[test]
fn racing_creation_never_overwrites_regular_symlink_directory_or_case_alias() {
    for case in 0..4 {
        let dir = TestDir::new();
        let lease = dir.lease();
        let stage = lease.prepare_stage(b"candidate").unwrap();
        let path = stage_path(&dir, &stage);
        let result = stage.publish_with(None, |parent, from, to, create| {
            assert!(create);
            match case {
                0 => dir.write(b"racing external file"),
                1 => {
                    fs::write(dir.0.join("target"), b"untouched target").unwrap();
                    symlink(dir.0.join("target"), dir.vault()).unwrap();
                }
                2 => fs::create_dir(dir.vault()).unwrap(),
                _ => fs::write(dir.0.join("VAULT.cw"), b"case alias").unwrap(),
            }
            let error = native::publish_at(parent, from, to, create).unwrap_err();
            assert_eq!(error.raw_os_error(), Some(libc::EEXIST));
            Err(error)
        });
        assert_eq!(result, Err(PublicationError::OutcomeUnknown));
        assert_eq!(fs::read(path).unwrap(), b"candidate");
        match case {
            0 => assert_eq!(fs::read(dir.vault()).unwrap(), b"racing external file"),
            1 => {
                assert!(fs::symlink_metadata(dir.vault()).unwrap().is_symlink());
                assert_eq!(fs::read(dir.vault()).unwrap(), b"untouched target");
            }
            2 => assert!(dir.vault().is_dir()),
            _ => assert_eq!(fs::read(dir.0.join("VAULT.cw")).unwrap(), b"case alias"),
        }
    }
}

#[test]
fn native_errors_and_lost_replies_remain_unknown_without_retry_rollback_or_cleanup() {
    for published in [false, true] {
        for code in [libc::EIO, libc::EINTR, libc::ENOTSUP] {
            let dir = TestDir::new();
            dir.write(b"original");
            let lease = dir.lease();
            let stage = lease.prepare_stage(b"candidate").unwrap();
            let path = stage_path(&dir, &stage);
            let mut calls = 0;
            let result = stage.publish_with(Some(b"original"), |parent, from, to, create| {
                calls += 1;
                if published {
                    native::publish_at(parent, from, to, create)?;
                }
                Err(io::Error::from_raw_os_error(code))
            });
            assert_eq!(result, Err(PublicationError::OutcomeUnknown));
            assert_eq!(calls, 1);
            assert_eq!(
                fs::read(dir.vault()).unwrap(),
                if published {
                    b"candidate".as_slice()
                } else {
                    b"original"
                }
            );
            assert_eq!(path.exists(), !published);
        }
    }
}

#[test]
fn changes_observed_after_success_are_unknown_and_never_restore_the_original() {
    for case in 0..4 {
        let dir = TestDir::new();
        dir.write(b"original");
        let lease = dir.lease();
        let stage = lease.prepare_stage(b"candidate").unwrap();
        let result = stage.publish_with(Some(b"original"), |parent, from, to, create| {
            native::publish_at(parent, from, to, create)?;
            match case {
                0 => fs::set_permissions(&dir.0, Permissions::from_mode(0o777)).unwrap(),
                1 => {
                    fs::rename(dir.companion(), dir.0.join("old-lock")).unwrap();
                    fs::write(dir.companion(), b"").unwrap();
                }
                2 => chmod_acl(&dir.vault(), "everyone allow read"),
                _ => {
                    fs::rename(dir.vault(), dir.0.join("published-candidate")).unwrap();
                    dir.write(b"external replacement");
                }
            }
            Ok(())
        });
        assert_eq!(result, Err(PublicationError::OutcomeUnknown));
        assert_eq!(
            fs::read(dir.vault()).unwrap(),
            if case == 3 {
                b"external replacement".as_slice()
            } else {
                b"candidate"
            }
        );
    }
}

#[test]
fn companion_acl_added_after_publication_preserves_candidate_and_unknown_outcome() {
    for create in [false, true] {
        let dir = TestDir::new();
        if !create {
            dir.write(b"original");
        }
        let lease = dir.lease();
        let stage = lease.prepare_stage(b"candidate").unwrap();
        let result = stage.publish_with(
            (!create).then_some(b"original".as_slice()),
            |parent, from, to, create| {
                native::publish_at(parent, from, to, create)?;
                chmod_acl(&dir.companion(), "everyone allow read");
                Ok(())
            },
        );
        assert_eq!(result, Err(PublicationError::OutcomeUnknown));
        assert_eq!(fs::read(dir.vault()).unwrap(), b"candidate");
        assert_eq!(lease.read_current(), Err(NativeError::Permissions));
        assert!(!native::no_extended_acl(&File::open(dir.companion()).unwrap()).unwrap());
        drop(lease);
        assert_eq!(
            VaultPath::resolve(&dir.vault()).unwrap().acquire().err(),
            Some(NativeError::Permissions)
        );
    }
}

#[test]
fn shared_lease_serializes_base_check_through_publication_and_refuses_stale_followup() {
    let dir = TestDir::new();
    dir.write(b"original");
    let lease = dir.lease();
    let first = lease.prepare_stage(b"first candidate").unwrap();
    let second = lease.prepare_stage(b"second candidate").unwrap();
    let second_path = stage_path(&dir, &second);
    let (entered, preflight_done) = mpsc::channel();
    let (release, proceed) = mpsc::channel();
    std::thread::scope(|scope| {
        let publisher = scope.spawn(move || {
            first.publish_with(Some(b"original"), |parent, from, to, create| {
                entered.send(()).unwrap();
                proceed.recv_timeout(Duration::from_secs(15)).unwrap();
                native::publish_at(parent, from, to, create)
            })
        });
        preflight_done
            .recv_timeout(Duration::from_secs(15))
            .unwrap();
        let contending = second.publish_replace(b"original");
        let while_paused = fs::read(dir.vault()).unwrap();
        release.send(()).unwrap();
        assert_eq!(publisher.join().unwrap(), Ok(()));
        assert_eq!(
            contending,
            Err(PublicationError::NotPublished(
                NativeError::WriterUnavailable
            ))
        );
        assert_eq!(while_paused, b"original");
    });
    assert_eq!(fs::read(dir.vault()).unwrap(), b"first candidate");
    assert_eq!(fs::read(second_path).unwrap(), b"second candidate");
    assert_eq!(
        lease
            .prepare_stage(b"stale candidate")
            .unwrap()
            .publish_replace(b"original"),
        Err(PublicationError::NotPublished(NativeError::Conflict))
    );
}

#[test]
fn concurrent_reader_sees_only_whole_old_or_new_snapshots_during_replacements() {
    let dir = TestDir::new();
    let old = vec![0x31; 64 * 1024];
    let new = vec![0x72; 64 * 1024];
    dir.write(&old);
    let lease = dir.lease();
    let stop = AtomicBool::new(false);
    let reads = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    struct StopReader<'a>(&'a AtomicBool);
    impl Drop for StopReader<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    std::thread::scope(|scope| {
        // Drop during writer/receiver unwind before scope waits for the reader.
        let _stop_on_unwind = StopReader(&stop);
        let reader = scope.spawn(|| {
            let first = fs::read(dir.vault()).unwrap();
            assert_eq!(first, old);
            reads.fetch_add(1, Ordering::Relaxed);
            sender.send(()).unwrap();
            while !stop.load(Ordering::Relaxed) {
                let actual = fs::read(dir.vault()).unwrap();
                assert!(actual == old || actual == new);
                reads.fetch_add(1, Ordering::Relaxed);
            }
        });
        receiver.recv_timeout(Duration::from_secs(15)).unwrap();
        for index in 0..32 {
            let (expected, next) = if index % 2 == 0 {
                (&old, &new)
            } else {
                (&new, &old)
            };
            lease
                .prepare_stage(next)
                .unwrap()
                .publish_replace(expected)
                .unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        reader.join().unwrap();
    });
    assert!(reads.load(Ordering::Relaxed) > 1);
    assert_eq!(lease.read_current().unwrap(), Some(old));
}

fn checkpoint() -> ! {
    println!("publication-checkpoint");
    io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn publication_subprocess_helper() {
    let Ok(path) = std::env::var("CODEWARD_PUBLICATION_PATH") else {
        return;
    };
    let path = Path::new(&path);
    let lease = VaultPath::resolve(path).unwrap().acquire().unwrap();
    let bytes = fs::read(path.parent().unwrap().join("candidate-source")).unwrap();
    let stage = lease.prepare_stage(&bytes).unwrap();
    let phase = std::env::var("CODEWARD_PUBLICATION_PHASE").unwrap();
    let old = fixture();
    let expected = (std::env::var("CODEWARD_PUBLICATION_CREATE").unwrap() == "false")
        .then_some(old.as_slice());
    stage
        .publish_with(expected, |parent, from, to, create| {
            if phase == "before" {
                checkpoint();
            }
            if phase == "race" {
                println!("publication-checkpoint");
                io::stdout().flush().unwrap();
            }
            native::publish_at(parent, from, to, create)?;
            if phase == "after" {
                checkpoint();
            }
            Ok(())
        })
        .unwrap();
    checkpoint();
}

#[test]
fn killed_creation_and_replacement_reopen_complete_states_and_release_exclusion() {
    for create in [false, true] {
        for phase in [
            "before", "after", "returned", "race", "race", "race", "race",
        ] {
            let dir = TestDir::new();
            let old = fixture();
            if !create {
                dir.write(&old);
            }
            fs::write(dir.0.join("candidate-source"), candidate()).unwrap();
            fs::set_permissions(
                dir.0.join("candidate-source"),
                Permissions::from_mode(0o600),
            )
            .unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "staging::publication::tests::publication_subprocess_helper",
                    "--nocapture",
                ])
                .env("CODEWARD_PUBLICATION_PATH", dir.vault())
                .env("CODEWARD_PUBLICATION_PHASE", phase)
                .env("CODEWARD_PUBLICATION_CREATE", create.to_string())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let stdout = child.stdout.take().unwrap();
            let (sender, receiver) = mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if line.unwrap() == "publication-checkpoint" {
                        let _ = sender.send(());
                        break;
                    }
                }
            });
            let ready = receiver.recv_timeout(Duration::from_secs(15));
            child.kill().unwrap();
            child.wait().unwrap();
            reader.join().unwrap();
            assert!(ready.is_ok(), "child did not reach {phase}");
            let lease = dir.lease();
            let actual = lease.read_current().unwrap();
            if phase == "race" {
                assert!(
                    actual.as_deref() == Some(candidate()) || actual == (!create).then_some(old)
                );
                if let Some(bytes) = actual {
                    assert!(begin_recovery(&bytes, PAPER.trim()).is_ok());
                }
            } else if phase == "before" {
                assert_eq!(actual, (!create).then_some(old));
                assert_eq!(stages(&dir.0).len(), 1);
            } else {
                assert_eq!(actual.as_deref(), Some(candidate()));
                assert_eq!(
                    begin_recovery(actual.as_ref().unwrap(), PAPER.trim())
                        .unwrap()
                        .revision(),
                    2
                );
                assert!(stages(&dir.0).is_empty());
            }
            assert!(dir.companion().exists());
        }
    }
}
