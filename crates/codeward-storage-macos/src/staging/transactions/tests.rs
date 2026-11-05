use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use codeward_core::{
    credentials::{Credential, CredentialInput, CredentialSnapshot},
    transaction::{
        CallerId, Committer, OperationStatus, Phase, Request, RequestId, TransactionError,
    },
    vault::{begin_recovery, unlock_with_master},
};

use super::*;
use crate::test_support::{MASTER_TESTS, fixture, maximum_revision_fixture, stages};

const OWNER: CallerId = CallerId(7);
const PASSWORD: &str = "synthetic password";
const REPLACEMENT: &str = "synthetic native replacement password";
fn paper() -> &'static str {
    include_str!("../../../../codeward-core/tests/fixtures/authenticated-recovery.txt").trim()
}
fn request(id: u8, revision: u64) -> Request {
    Request {
        id: RequestId([id; 16]),
        expected_revision: revision,
    }
}
fn empty() -> CredentialSnapshot {
    CredentialSnapshot::new(Vec::new()).unwrap()
}
fn complete(worker: &mut Committer<SyntheticLease>) -> OperationStatus {
    // Bound the harness itself: unexpected nonterminal behavior fails instead of
    // looping indefinitely. Production progress scheduling is separate work.
    for _ in 0..5 {
        let status = worker.advance().expect("active synthetic request");
        if !matches!(status, OperationStatus::Pending(_)) {
            return status;
        }
    }
    panic!("synthetic transaction did not finish within its four storage stages");
}
fn opened(
    dir: &TestDir,
) -> (
    codeward_core::transaction::Session,
    Committer<SyntheticLease>,
    Controls,
) {
    let (storage, control) = SyntheticStorage::new(dir);
    let (session, worker) = Committer::open_master(storage, OWNER, PASSWORD).unwrap();
    (session, worker, control)
}

#[test]
fn confirmed_creation_content_delete_and_reusable_recovery_use_actual_native_barriers() {
    let _master = MASTER_TESTS.lock().unwrap();
    let dir = TestDir::new();
    let (storage, control) = SyntheticStorage::new(&dir);
    let (mut session, mut worker) = Committer::begin_create(storage, OWNER).unwrap();
    let enrolled = session.recovery_code(OWNER).unwrap();
    assert!(
        session
            .prepare_creation(
                &mut worker,
                OWNER,
                request(1, 0),
                PASSWORD,
                PASSWORD,
                "invalid synthetic code"
            )
            .is_err()
    );
    assert!(!dir.vault().exists());
    let op = session
        .prepare_creation(
            &mut worker,
            OWNER,
            request(1, 0),
            PASSWORD,
            PASSWORD,
            &enrolled,
        )
        .unwrap();
    for phase in [
        Phase::Prepared,
        Phase::Staged,
        Phase::Admitted,
        Phase::Published,
    ] {
        assert_eq!(
            worker.status(OWNER, &op).unwrap(),
            OperationStatus::Pending(phase)
        );
        assert_eq!(worker.committed_revision(), None);
        assert_eq!(dir.vault().exists(), phase == Phase::Published);
        worker.advance();
    }
    assert_eq!(
        worker.status(OWNER, &op).unwrap(),
        OperationStatus::Committed { revision: 1 }
    );
    let created = fs::read(dir.vault()).unwrap();
    assert_eq!(
        unlock_with_master(&created, PASSWORD).unwrap().revision(),
        1
    );
    assert_eq!(begin_recovery(&created, &enrolled).unwrap().revision(), 1);
    assert_eq!(
        control.lock().unwrap().events,
        [
            Event::Read,
            Event::Stage,
            Event::Read,
            Event::Publish(PublishMode::Create),
            Event::Barrier
        ]
    );

    session.reload_master(&worker, OWNER, PASSWORD).unwrap();
    let record = Credential::new(CredentialInput {
        id: "00000000000000000000000000000001",
        title: "Synthetic native service",
        username: "synthetic user",
        password: "synthetic record password",
        url: "https://example.invalid/",
        notes: "Synthetic native test only",
        created_at: 0,
        updated_at: 0,
    })
    .unwrap();
    let snapshot = CredentialSnapshot::new(vec![record]).unwrap();
    let op = session
        .prepare_contents(&mut worker, OWNER, request(2, 1), &snapshot)
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    assert_eq!(
        worker.status(OWNER, &op).unwrap(),
        OperationStatus::Committed { revision: 2 }
    );
    assert_eq!(
        unlock_with_master(&fs::read(dir.vault()).unwrap(), PASSWORD)
            .unwrap()
            .snapshot()
            .credentials()
            .len(),
        1
    );
    session.reload_master(&worker, OWNER, PASSWORD).unwrap();
    session
        .prepare_contents(&mut worker, OWNER, request(3, 2), &empty())
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 3 }
    );
    session.lock();
    drop(worker);

    let (storage, _) = SyntheticStorage::new(&dir);
    let (mut recovery, mut worker) = Committer::open_recovery(storage, OWNER, &enrolled).unwrap();
    assert!(
        recovery
            .prepare_contents(&mut worker, OWNER, request(4, 3), &empty())
            .is_err()
    );
    recovery
        .prepare_recovery(&mut worker, OWNER, request(4, 3), REPLACEMENT, REPLACEMENT)
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 4 }
    );
    let recovered = fs::read(dir.vault()).unwrap();
    assert!(unlock_with_master(&recovered, PASSWORD).is_err());
    assert_eq!(
        unlock_with_master(&recovered, REPLACEMENT)
            .unwrap()
            .revision(),
        4
    );
    assert_eq!(begin_recovery(&recovered, &enrolled).unwrap().revision(), 4);
    recovery.reload_recovery(&worker, OWNER, &enrolled).unwrap();
    recovery
        .prepare_recovery(&mut worker, OWNER, request(5, 4), PASSWORD, PASSWORD)
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 5 }
    );
    assert_eq!(
        unlock_with_master(&fs::read(dir.vault()).unwrap(), PASSWORD)
            .unwrap()
            .snapshot()
            .credentials()
            .len(),
        0
    );
    assert_eq!(
        begin_recovery(&fs::read(dir.vault()).unwrap(), &enrolled)
            .unwrap()
            .revision(),
        5
    );
}

#[test]
fn uncertain_native_publication_and_barriers_block_replay_until_fresh_authentication() {
    let _master = MASTER_TESTS.lock().unwrap();
    for fault in [Fault::PublishBefore, Fault::PublishAfter, Fault::Barrier] {
        let dir = TestDir::new();
        dir.write(&fixture());
        let (mut session, mut worker, control) = opened(&dir);
        let op = session
            .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
            .unwrap();
        control.lock().unwrap().fault = Some(fault);
        assert_eq!(complete(&mut worker), OperationStatus::OutcomeUnknown);
        assert_eq!(worker.committed_revision(), Some(1));
        assert_eq!(
            session
                .prepare_contents(&mut worker, OWNER, request(2, 1), &empty())
                .err(),
            Some(TransactionError::Busy)
        );
        session.lock();
        assert!(
            worker
                .reconcile_recovery(OWNER, &op, "invalid synthetic paper")
                .is_err()
        );
        assert_eq!(
            worker.status(OWNER, &op).unwrap(),
            OperationStatus::OutcomeUnknown
        );
        control.lock().unwrap().fault = Some(Fault::Read);
        assert!(worker.reconcile_recovery(OWNER, &op, paper()).is_err());
        control.lock().unwrap().fault = Some(Fault::Barrier);
        assert!(worker.reconcile_recovery(OWNER, &op, paper()).is_err());
        assert_eq!(
            worker.status(OWNER, &op).unwrap(),
            OperationStatus::OutcomeUnknown
        );
        let expected = if fault == Fault::PublishBefore {
            OperationStatus::Unchanged
        } else {
            OperationStatus::Committed { revision: 2 }
        };
        assert_eq!(
            worker.reconcile_recovery(OWNER, &op, paper()).unwrap(),
            expected
        );
        assert!(session.is_locked());
        let publish_count = control
            .lock()
            .unwrap()
            .events
            .iter()
            .filter(|event| matches!(event, Event::Publish(_)))
            .count();
        assert_eq!(publish_count, 1);
        let before = control.lock().unwrap().events.len();
        assert_eq!(
            worker.reconcile_recovery(OWNER, &op, paper()).unwrap(),
            expected
        );
        assert_eq!(control.lock().unwrap().events.len(), before);
        assert_eq!(
            stages(&dir.0).len(),
            usize::from(fault == Fault::PublishBefore)
        );
    }
}

#[test]
fn definite_native_failures_preserve_base_deduplicate_and_allow_only_new_explicit_retry() {
    let _master = MASTER_TESTS.lock().unwrap();
    for fault in [Fault::Stage, Fault::Read] {
        let dir = TestDir::new();
        dir.write(&fixture());
        let (mut session, mut worker, control) = opened(&dir);
        let op = session
            .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
            .unwrap();
        control.lock().unwrap().fault = Some(fault);
        let expected = OperationStatus::Failed(if fault == Fault::Stage {
            StorageError::Write
        } else {
            StorageError::Read
        });
        assert_eq!(complete(&mut worker), expected);
        assert_eq!(fs::read(dir.vault()).unwrap(), fixture());
        let count = control.lock().unwrap().events.len();
        let same = session
            .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
            .unwrap();
        assert_eq!(
            worker.status(OWNER, &same).unwrap(),
            worker.status(OWNER, &op).unwrap()
        );
        assert!(worker.advance().is_none());
        assert_eq!(control.lock().unwrap().events.len(), count);
        session
            .prepare_contents(&mut worker, OWNER, request(2, 1), &empty())
            .unwrap();
        assert_eq!(
            complete(&mut worker),
            OperationStatus::Committed { revision: 2 }
        );
        assert_eq!(stages(&dir.0).len(), usize::from(fault == Fault::Read));
    }
}

#[test]
fn admitted_creation_refuses_a_racing_destination_and_stale_updates_preserve_external_state() {
    let _master = MASTER_TESTS.lock().unwrap();
    let dir = TestDir::new();
    let (storage, _) = SyntheticStorage::new(&dir);
    let (mut session, mut worker) = Committer::begin_create(storage, OWNER).unwrap();
    let code = session.recovery_code(OWNER).unwrap();
    session
        .prepare_creation(&mut worker, OWNER, request(1, 0), PASSWORD, PASSWORD, &code)
        .unwrap();
    assert_eq!(
        worker.advance(),
        Some(OperationStatus::Pending(Phase::Staged))
    );
    assert_eq!(
        worker.advance(),
        Some(OperationStatus::Pending(Phase::Admitted))
    );
    dir.write(&fixture());
    assert_eq!(worker.advance(), Some(OperationStatus::NotPublished));
    assert_eq!(worker.committed_revision(), None);
    assert_eq!(fs::read(dir.vault()).unwrap(), fixture());
    assert_eq!(stages(&dir.0).len(), 1);
    drop(worker);

    let (mut session, mut worker, _) = opened(&dir);
    session
        .prepare_contents(&mut worker, OWNER, request(2, 1), &empty())
        .unwrap();
    fs::write(dir.vault(), b"synthetic external replacement").unwrap();
    assert_eq!(complete(&mut worker), OperationStatus::Conflict);
    assert_eq!(
        fs::read(dir.vault()).unwrap(),
        b"synthetic external replacement"
    );
    assert_eq!(
        session
            .prepare_contents(&mut worker, OWNER, request(3, 1), &empty())
            .err(),
        Some(TransactionError::ReloadRequired)
    );
}

#[test]
fn security_lock_cancels_unadmitted_native_work_but_admitted_commits_finish_without_unlocking() {
    let _master = MASTER_TESTS.lock().unwrap();
    for admitted in [false, true] {
        let dir = TestDir::new();
        dir.write(&fixture());
        let (mut session, mut worker, _) = opened(&dir);
        let op = session
            .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
            .unwrap();
        worker.advance();
        if admitted {
            worker.advance();
        }
        session.lock();
        assert_eq!(
            VaultPath::resolve(&dir.vault()).unwrap().acquire().err(),
            Some(NativeError::WriterUnavailable)
        );
        let expected = if admitted {
            OperationStatus::Committed { revision: 2 }
        } else {
            OperationStatus::Canceled
        };
        assert_eq!(complete(&mut worker), expected);
        assert_eq!(worker.status(OWNER, &op).unwrap(), expected);
        assert!(session.is_locked());
        assert!(worker.advance().is_none());
        if !admitted {
            assert_eq!(fs::read(dir.vault()).unwrap(), fixture());
        }
        assert_eq!(stages(&dir.0).len(), usize::from(!admitted));
        drop(worker);
        drop(dir.lease());
    }
}

#[test]
fn startup_uses_only_named_destination_and_preserves_complete_partial_and_unrelated_residue() {
    let _master = MASTER_TESTS.lock().unwrap();
    let dir = TestDir::new();
    let complete_stage = {
        let lease = dir.lease();
        drop(lease.prepare_stage(&maximum_revision_fixture()).unwrap());
        stages(&dir.0).pop().unwrap()
    };
    let partial = dir
        .0
        .join(".codeward.stage-00000000000000000000000000000000");
    fs::write(&partial, b"partial synthetic stage").unwrap();
    let unrelated = dir.0.join("unrelated-file");
    fs::write(&unrelated, b"unrelated").unwrap();
    let (storage, _) = SyntheticStorage::new(&dir);
    assert!(Committer::open_master(storage, OWNER, PASSWORD).is_err());
    assert!(!dir.vault().exists());
    let (storage, _) = SyntheticStorage::new(&dir);
    let (_, worker) = Committer::begin_create(storage, OWNER).unwrap();
    assert_eq!(worker.committed_revision(), None);
    drop(worker);
    dir.write(b"synthetic corrupt destination");
    let (storage, _) = SyntheticStorage::new(&dir);
    assert!(Committer::open_master(storage, OWNER, PASSWORD).is_err());
    assert_eq!(
        fs::read(dir.vault()).unwrap(),
        b"synthetic corrupt destination"
    );
    assert_eq!(
        fs::read(complete_stage).unwrap(),
        maximum_revision_fixture()
    );
    assert_eq!(fs::read(partial).unwrap(), b"partial synthetic stage");
    assert_eq!(fs::read(unrelated).unwrap(), b"unrelated");
}

fn checkpoint() -> ! {
    println!("native-transaction-checkpoint");
    io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn native_transaction_subprocess_helper() {
    let Ok(path) = std::env::var("CODEWARD_NATIVE_TRANSACTION_PATH") else {
        return;
    };
    let mode = std::env::var("CODEWARD_NATIVE_TRANSACTION_MODE").unwrap();
    let phase: usize = std::env::var("CODEWARD_NATIVE_TRANSACTION_PHASE")
        .unwrap()
        .parse()
        .unwrap();
    let storage = SyntheticStorage {
        path: VaultPath::resolve(Path::new(&path)).unwrap(),
        control: Arc::new(Mutex::new(Control::default())),
    };
    let (mut session, mut worker) = match mode.as_str() {
        "create" => Committer::begin_create(storage, OWNER).unwrap(),
        "recover" => Committer::open_recovery(storage, OWNER, paper()).unwrap(),
        _ => Committer::open_master(storage, OWNER, PASSWORD).unwrap(),
    };
    match mode.as_str() {
        "create" => {
            let code = session.recovery_code(OWNER).unwrap();
            session
                .prepare_creation(&mut worker, OWNER, request(1, 0), PASSWORD, PASSWORD, &code)
                .unwrap();
        }
        "recover" => {
            session
                .prepare_recovery(&mut worker, OWNER, request(1, 1), REPLACEMENT, REPLACEMENT)
                .unwrap();
        }
        _ => {
            session
                .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
                .unwrap();
        }
    }
    for _ in 0..phase {
        worker.advance().unwrap();
    }
    checkpoint();
}

#[test]
fn real_process_kills_at_core_native_boundaries_reopen_coherent_creation_update_and_recovery() {
    let _master = MASTER_TESTS.lock().unwrap();
    for mode in ["create", "update", "recover"] {
        for phase in 0..=4 {
            let dir = TestDir::new();
            if mode != "create" {
                dir.write(&fixture());
            }
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "staging::transactions::tests::native_transaction_subprocess_helper",
                    "--nocapture",
                ])
                .env("CODEWARD_NATIVE_TRANSACTION_PATH", dir.vault())
                .env("CODEWARD_NATIVE_TRANSACTION_MODE", mode)
                .env("CODEWARD_NATIVE_TRANSACTION_PHASE", phase.to_string())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let stdout = child.stdout.take().unwrap();
            let (sender, receiver) = mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if line.unwrap() == "native-transaction-checkpoint" {
                        let _ = sender.send(());
                        break;
                    }
                }
            });
            let ready = receiver.recv_timeout(Duration::from_secs(30));
            child.kill().unwrap();
            child.wait().unwrap();
            reader.join().unwrap();
            assert!(ready.is_ok(), "child did not reach {mode}/{phase}");
            let (storage, _) = SyntheticStorage::new(&dir);
            if mode == "create" && phase < 3 {
                assert!(!dir.vault().exists());
                let (_, worker) = Committer::begin_create(storage, OWNER).unwrap();
                assert_eq!(worker.committed_revision(), None);
            } else {
                let password = if mode == "recover" && phase >= 3 {
                    REPLACEMENT
                } else {
                    PASSWORD
                };
                let (session, worker) = Committer::open_master(storage, OWNER, password).unwrap();
                assert_eq!(
                    worker.committed_revision(),
                    Some(if phase < 3 || mode == "create" { 1 } else { 2 })
                );
                let actual = fs::read(dir.vault()).unwrap();
                if phase < 3 {
                    assert_eq!(actual, fixture());
                }
                if mode != "create" {
                    assert_eq!(
                        begin_recovery(&actual, paper()).unwrap().revision(),
                        if phase < 3 { 1 } else { 2 }
                    );
                }
                if mode == "update" && phase >= 3 {
                    assert_eq!(
                        unlock_with_master(&actual, PASSWORD)
                            .unwrap()
                            .snapshot()
                            .credentials()
                            .len(),
                        0
                    );
                }
                drop((session, worker));
            }
            assert_eq!(stages(&dir.0).len(), usize::from(phase == 1 || phase == 2));
        }
    }
}
