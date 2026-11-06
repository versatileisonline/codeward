use super::*;
use crate::{credentials::CredentialSnapshot, crypto, vault};

mod authentication;
pub(super) mod model;
use model::{Event, Fault, Lease, Model};

const OWNER: CallerId = CallerId(7);
const PASSWORD: &str = "synthetic password";
pub(super) fn request(n: u8, revision: u64) -> Request {
    Request {
        id: RequestId([n; 16]),
        expected_revision: revision,
    }
}
pub(super) fn fixture() -> Vec<u8> {
    include_str!("../../tests/fixtures/authenticated-v1.hex")
        .split_ascii_whitespace()
        .flat_map(|line| {
            (0..line.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
        })
        .collect()
}
pub(super) fn paper() -> &'static str {
    include_str!("../../tests/fixtures/authenticated-recovery.txt").trim()
}
pub(super) fn empty() -> CredentialSnapshot {
    CredentialSnapshot::new(Vec::new()).unwrap()
}
pub(super) fn opened() -> (Model, Session, Committer<Lease>) {
    let model = Model::new(Some(fixture()));
    let (session, worker) = Committer::open_master(model.clone(), OWNER, PASSWORD).unwrap();
    (model, session, worker)
}
fn prepare(session: &mut Session, worker: &mut Committer<Lease>, n: u8) -> OperationRef {
    session
        .prepare_contents(worker, OWNER, request(n, 1), &empty())
        .unwrap()
}
pub(super) fn complete(worker: &mut Committer<Lease>) -> OperationStatus {
    loop {
        let status = worker.advance().expect("active operation");
        if !matches!(status, OperationStatus::Pending(_)) {
            return status;
        }
    }
}

#[test]
fn only_final_barriers_advance_revision_and_lost_reply_remains_queryable() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let original = fixture();
    let op = prepare(&mut session, &mut worker, 1);
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
        assert_eq!(worker.committed_revision(), Some(1));
        if phase != Phase::Published {
            assert_eq!(model.live(), Some(original.clone()));
        }
        worker.advance();
    }
    // Simulate lost caller reply: status is already recorded, no second mutation.
    assert_eq!(
        worker.status(OWNER, &op).unwrap(),
        OperationStatus::Committed { revision: 2 }
    );
    assert_eq!(worker.committed_revision(), Some(2));
    let updated = model.live().unwrap();
    assert_eq!(
        vault::unlock_with_master(&updated, PASSWORD)
            .unwrap()
            .snapshot()
            .credentials()
            .len(),
        0
    );
    assert_eq!(
        vault::begin_recovery(&updated, paper()).unwrap().revision(),
        2
    );
    let disk = model.0.lock().unwrap();
    assert_eq!(disk.durable.as_ref(), Some(&updated));
    assert_eq!(
        disk.events,
        vec![
            Event::Read,
            Event::Create,
            Event::Write,
            Event::Close,
            Event::Sync,
            Event::Read,
            Event::Publish(PublishMode::Replace),
            Event::Barrier
        ]
    );
    assert!(
        !disk.stages[0]
            .windows(PASSWORD.len())
            .any(|w| w == PASSWORD.as_bytes())
    );
}

#[test]
fn faults_before_publication_preserve_base_and_new_explicit_retry_encrypts_afresh() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for (fault, error) in [
        (Fault::Create, StorageError::CreateStage),
        (Fault::ShortWrite, StorageError::Write),
        (Fault::Close, StorageError::Close),
        (Fault::Sync, StorageError::SyncStage),
        (Fault::Read, StorageError::Read),
    ] {
        let (model, mut session, mut worker) = opened();
        let op = prepare(&mut session, &mut worker, 1);
        if fault == Fault::Read {
            worker.advance();
        }
        model.fault(fault);
        assert_eq!(complete(&mut worker), OperationStatus::Failed(error));
        assert_eq!(model.live(), Some(fixture()));
        assert_eq!(worker.committed_revision(), Some(1));
        assert_eq!(
            worker.status(OWNER, &op).unwrap(),
            OperationStatus::Failed(error)
        );
        // Known ID does not automatically retry, even after definite failure.
        let same = prepare(&mut session, &mut worker, 1);
        assert_eq!(
            worker.status(OWNER, &same).unwrap(),
            OperationStatus::Failed(error)
        );
        assert!(worker.advance().is_none());
        prepare(&mut session, &mut worker, 2);
        assert_eq!(
            complete(&mut worker),
            OperationStatus::Committed { revision: 2 }
        );
        let disk = model.0.lock().unwrap();
        if disk.stages.len() == 2 && disk.stages[0].len() >= 280 {
            assert_ne!(&disk.stages[0][256..280], &disk.stages[1][256..280]);
        }
    }
}

#[test]
fn publication_ambiguity_and_barrier_failure_block_mutations_until_reconciled() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for fault in [Fault::UnknownBefore, Fault::UnknownAfter, Fault::Barrier] {
        let (model, mut session, mut worker) = opened();
        let op = prepare(&mut session, &mut worker, 1);
        model.fault(fault);
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
                .reconcile_master(OWNER, &op, "wrong synthetic")
                .is_err()
        );
        assert_eq!(
            worker.status(OWNER, &op).unwrap(),
            OperationStatus::OutcomeUnknown
        );
        for _ in 0..2 {
            model.fault(Fault::Barrier);
            assert_eq!(
                worker.reconcile_recovery(OWNER, &op, paper()),
                Err(TransactionError::Storage(StorageError::Barrier))
            );
            assert_eq!(worker.committed_revision(), Some(1));
        }
        let expected = if fault == Fault::UnknownBefore {
            OperationStatus::Unchanged
        } else {
            OperationStatus::Committed { revision: 2 }
        };
        assert_eq!(
            worker.reconcile_recovery(OWNER, &op, paper()).unwrap(),
            expected
        );
        assert!(session.is_locked());
    }
}

#[test]
fn proven_nonpublication_is_failure_without_unknown_or_revision_advance() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    prepare(&mut session, &mut worker, 1);
    model.fault(Fault::PublishBefore);
    assert_eq!(complete(&mut worker), OperationStatus::NotPublished);
    assert_eq!(model.live(), Some(fixture()));
    prepare(&mut session, &mut worker, 2);
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
}

#[test]
fn exact_file_digest_detects_changed_bytes_even_at_same_vault_id_and_revision() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for current in [
        None,
        Some(Vec::new()),
        Some({
            let mut b = fixture();
            *b.last_mut().unwrap() ^= 1;
            b
        }),
    ] {
        let (model, mut session, mut worker) = opened();
        prepare(&mut session, &mut worker, 1);
        model.0.lock().unwrap().live = current.clone();
        assert_eq!(complete(&mut worker), OperationStatus::Conflict);
        assert_eq!(model.live(), current);
        assert!(
            !model
                .events()
                .iter()
                .any(|e| matches!(e, Event::Publish(_)))
        );
        assert_eq!(
            session
                .prepare_contents(&mut worker, OWNER, request(2, 1), &empty())
                .err(),
            Some(TransactionError::ReloadRequired)
        );
    }
}

#[test]
fn lock_wins_before_admission_and_admitted_transactions_finish_without_access() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for advances in 0..=3 {
        let (model, mut session, mut worker) = opened();
        let op = prepare(&mut session, &mut worker, 1);
        for _ in 0..advances {
            worker.advance();
        }
        let generation = session.generation();
        session.lock();
        assert!(session.is_locked());
        assert!(session.generation() > generation);
        assert_eq!(
            session
                .prepare_contents(&mut worker, OWNER, request(2, 1), &empty())
                .err(),
            Some(TransactionError::Locked)
        );
        let outcome = complete(&mut worker);
        assert_eq!(
            outcome,
            if advances < 2 {
                OperationStatus::Canceled
            } else {
                OperationStatus::Committed { revision: 2 }
            }
        );
        assert_eq!(worker.status(OWNER, &op).unwrap(), outcome);
        if advances < 2 {
            assert_eq!(model.live(), Some(fixture()));
        }
    }
}

#[test]
fn blocked_storage_worker_does_not_hold_session_secrets_or_delay_lock() {
    use std::{sync::mpsc, thread, time::Duration};
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let op = prepare(&mut session, &mut worker, 1);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    model.0.lock().unwrap().stage_block = Some((entered_tx, resume_rx));
    let task = thread::spawn(move || {
        worker.advance();
        worker
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    session.lock();
    assert!(session.is_locked());
    assert_eq!(
        session.recovery_code(OWNER).err(),
        Some(TransactionError::Locked)
    );
    resume_tx.send(()).unwrap();
    let worker = task.join().unwrap();
    assert_eq!(
        worker.status(OWNER, &op).unwrap(),
        OperationStatus::Canceled
    );
    assert!(
        !model
            .events()
            .iter()
            .any(|e| matches!(e, Event::Publish(_)))
    );
}

#[test]
fn duplicate_intent_is_bound_to_arguments_caller_scope_and_generation() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, mut worker) = opened();
    let op = prepare(&mut session, &mut worker, 1);
    let same = prepare(&mut session, &mut worker, 1);
    assert_eq!(
        worker.status(OWNER, &same).unwrap(),
        worker.status(OWNER, &op).unwrap()
    );
    let changed = vault::unlock_with_master(&fixture(), PASSWORD).unwrap();
    assert_eq!(
        session
            .prepare_contents(&mut worker, OWNER, request(1, 1), changed.snapshot())
            .err(),
        Some(TransactionError::RequestReused)
    );
    assert_eq!(
        session
            .prepare_contents(&mut worker, OWNER, request(1, 2), &empty())
            .err(),
        Some(TransactionError::RequestReused)
    );
    assert_eq!(
        worker.status(CallerId(8), &op),
        Err(TransactionError::Unauthorized)
    );
    assert_eq!(
        session
            .prepare_contents(&mut worker, CallerId(8), request(2, 1), &empty())
            .err(),
        Some(TransactionError::Unauthorized)
    );
    let (_, mut other_session, mut other_worker) = opened();
    assert_eq!(
        session
            .prepare_contents(&mut other_worker, OWNER, request(2, 1), &empty())
            .err(),
        Some(TransactionError::Unauthorized)
    );
    assert_eq!(
        other_worker.status(OWNER, &op).unwrap(),
        OperationStatus::Unavailable
    );
    other_session.lock();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    assert_eq!(
        session
            .prepare_contents(&mut worker, OWNER, request(2, 2), &empty())
            .err(),
        Some(TransactionError::ReloadRequired)
    );
    let generation = session.generation();
    session.reload_master(&worker, OWNER, PASSWORD).unwrap();
    assert!(session.generation() > generation);
    assert_eq!(
        session
            .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
            .err(),
        Some(TransactionError::RequestReused)
    );
    let next = session
        .prepare_contents(&mut worker, OWNER, request(2, 2), &empty())
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 3 }
    );
    assert_eq!(
        worker.status(OWNER, &next).unwrap(),
        OperationStatus::Committed { revision: 3 }
    );
}

#[test]
fn creation_requires_confirmation_and_atomic_no_replace_even_after_admission() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(None);
    let (mut session, mut worker) = Committer::begin_create(model.clone(), OWNER).unwrap();
    assert_eq!(worker.committed_revision(), None);
    assert_eq!(
        session.recovery_code(CallerId(8)).err(),
        Some(TransactionError::Unauthorized)
    );
    let code = session.recovery_code(OWNER).unwrap();
    assert!(
        session
            .prepare_creation(&mut worker, OWNER, request(1, 0), PASSWORD, "wrong", &code)
            .is_err()
    );
    assert!(worker.advance().is_none());
    let op = session
        .prepare_creation(&mut worker, OWNER, request(1, 0), PASSWORD, PASSWORD, &code)
        .unwrap();
    assert_eq!(
        session.recovery_code(OWNER).err(),
        Some(TransactionError::Vault(VaultError::EnrollmentFinished))
    );
    // Same known intent returns existing status even after code redisplay is disabled.
    assert!(
        session
            .prepare_creation(&mut worker, OWNER, request(1, 0), PASSWORD, PASSWORD, &code)
            .is_ok()
    );
    worker.advance();
    worker.advance();
    // Bypassing writer exclusion after the core's check must still not overwrite
    // an existing destination during initial atomic creation.
    model.0.lock().unwrap().live = Some(fixture());
    assert_eq!(complete(&mut worker), OperationStatus::NotPublished);
    assert_eq!(model.live(), Some(fixture()));
    assert_eq!(
        worker.status(OWNER, &op).unwrap(),
        OperationStatus::NotPublished
    );
    assert_eq!(worker.committed_revision(), None);
}

#[test]
fn creation_unknown_absence_or_candidate_requires_barriers_and_stays_locked() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for fault in [Fault::UnknownBefore, Fault::UnknownAfter, Fault::Barrier] {
        let model = Model::new(None);
        let (mut session, mut worker) = Committer::begin_create(model.clone(), OWNER).unwrap();
        let code = session.recovery_code(OWNER).unwrap();
        let op = session
            .prepare_creation(&mut worker, OWNER, request(1, 0), PASSWORD, PASSWORD, &code)
            .unwrap();
        model.fault(fault);
        assert_eq!(complete(&mut worker), OperationStatus::OutcomeUnknown);
        assert_eq!(worker.committed_revision(), None);
        session.lock();
        assert!(session.recovery_code(OWNER).is_err());
        model.fault(Fault::Barrier);
        assert!(worker.reconcile_recovery(OWNER, &op, &code).is_err());
        assert_eq!(
            worker.status(OWNER, &op).unwrap(),
            OperationStatus::OutcomeUnknown
        );
        let expected = if fault == Fault::UnknownBefore {
            OperationStatus::Unchanged
        } else {
            OperationStatus::Committed { revision: 1 }
        };
        assert_eq!(
            worker.reconcile_recovery(OWNER, &op, &code).unwrap(),
            expected
        );
        assert!(session.is_locked());
        if fault == Fault::UnknownBefore {
            session.restart_enrollment(&worker, OWNER).unwrap();
            assert_ne!(&*session.recovery_code(OWNER).unwrap(), &*code);
        } else {
            let bytes = model.live().unwrap();
            assert_eq!(
                vault::unlock_with_master(&bytes, PASSWORD)
                    .unwrap()
                    .revision(),
                1
            );
            assert_eq!(vault::begin_recovery(&bytes, &code).unwrap().revision(), 1);
        }
    }
}

#[test]
fn recovery_is_restricted_and_commits_master_and_payload_with_reusable_paper_slot() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for fault in [
        Fault::Close,
        Fault::UnknownBefore,
        Fault::UnknownAfter,
        Fault::Barrier,
    ] {
        let model = Model::new(Some(fixture()));
        let (mut session, mut worker) =
            Committer::open_recovery(model.clone(), OWNER, paper()).unwrap();
        assert_eq!(
            session
                .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
                .err(),
            Some(TransactionError::Locked)
        );
        let op = session
            .prepare_recovery(
                &mut worker,
                OWNER,
                request(1, 1),
                "new synthetic",
                "new synthetic",
            )
            .unwrap();
        assert_eq!(
            session
                .prepare_recovery(&mut worker, OWNER, request(1, 1), "changed", "changed")
                .err(),
            Some(TransactionError::RequestReused)
        );
        model.fault(fault);
        let status = complete(&mut worker);
        if fault == Fault::Close {
            assert_eq!(status, OperationStatus::Failed(StorageError::Close));
            session
                .prepare_recovery(
                    &mut worker,
                    OWNER,
                    request(2, 1),
                    "new synthetic",
                    "new synthetic",
                )
                .unwrap();
            assert_eq!(
                complete(&mut worker),
                OperationStatus::Committed { revision: 2 }
            );
        } else {
            assert_eq!(status, OperationStatus::OutcomeUnknown);
            session.lock();
            let expected = if fault == Fault::UnknownBefore {
                OperationStatus::Unchanged
            } else {
                OperationStatus::Committed { revision: 2 }
            };
            assert_eq!(
                worker.reconcile_recovery(OWNER, &op, paper()).unwrap(),
                expected
            );
        }
        let bytes = model.live().unwrap();
        assert_eq!(&bytes[148..256], &fixture()[148..256]);
        let revision = vault::begin_recovery(&bytes, paper()).unwrap().revision();
        let password = if revision == 1 {
            PASSWORD
        } else {
            "new synthetic"
        };
        assert_eq!(
            vault::unlock_with_master(&bytes, password)
                .unwrap()
                .revision(),
            revision
        );
        if revision == 2 {
            assert!(vault::unlock_with_master(&bytes, PASSWORD).is_err());
        }
    }
}

#[test]
fn reconciliation_never_promotes_unrelated_or_corrupt_bytes_and_preserves_artifacts() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let op = prepare(&mut session, &mut worker, 1);
    model.fault(Fault::UnknownAfter);
    assert_eq!(complete(&mut worker), OperationStatus::OutcomeUnknown);
    let candidate = model.live().unwrap();
    let mut damaged = candidate.clone();
    *damaged.last_mut().unwrap() ^= 1;
    model.0.lock().unwrap().live = Some(damaged.clone());
    let events = model.events().len();
    assert_eq!(
        worker.reconcile_recovery(OWNER, &op, paper()),
        Err(TransactionError::OutcomeUnknown)
    );
    assert_eq!(model.live(), Some(damaged));
    assert_eq!(&model.events()[events..], &[Event::Read]);
    assert_eq!(model.0.lock().unwrap().stages.len(), 1);
    model.fault(Fault::Read);
    assert_eq!(
        worker.reconcile_master(OWNER, &op, PASSWORD),
        Err(TransactionError::Storage(StorageError::Read))
    );
    model.0.lock().unwrap().live = Some(candidate);
    assert_eq!(
        worker.reconcile_master(OWNER, &op, PASSWORD).unwrap(),
        OperationStatus::Committed { revision: 2 }
    );
}

#[test]
fn writer_exclusion_survives_session_lock_and_is_released_on_drop_or_auth_failure() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, worker) = opened();
    session.lock();
    assert_eq!(
        Committer::open_master(model.clone(), OWNER, PASSWORD).err(),
        Some(TransactionError::Storage(StorageError::WriterUnavailable))
    );
    drop(worker);
    assert!(Committer::open_master(model.clone(), OWNER, "wrong synthetic").is_err());
    assert!(!model.0.lock().unwrap().held);
    assert!(Committer::open_master(model, OWNER, PASSWORD).is_ok());
}

#[test]
fn bounded_history_never_evicts_known_ids_or_grants_blind_replay() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    for n in 0..MAX_REQUESTS as u8 {
        prepare(&mut session, &mut worker, n);
        model.fault(Fault::Create);
        assert_eq!(
            complete(&mut worker),
            OperationStatus::Failed(StorageError::CreateStage)
        );
    }
    let known = prepare(&mut session, &mut worker, 0);
    assert_eq!(
        worker.status(OWNER, &known).unwrap(),
        OperationStatus::Failed(StorageError::CreateStage)
    );
    assert_eq!(
        session
            .prepare_contents(&mut worker, OWNER, request(99, 1), &empty())
            .err(),
        Some(TransactionError::HistoryFull)
    );
    assert!(worker.advance().is_none());
    assert_eq!(model.live(), Some(fixture()));
}

#[test]
fn modeled_process_termination_at_each_boundary_reopens_only_complete_old_or_new() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for advances in 0..=4 {
        let (model, mut session, mut worker) = opened();
        let reference = prepare(&mut session, &mut worker, 1);
        for _ in 0..advances {
            worker.advance();
        }
        drop(session);
        drop(worker);
        // In-memory model termination, not a native process-kill/storage test.
        let (mut reopened_session, reopened) =
            Committer::open_master(model.clone(), OWNER, PASSWORD).unwrap();
        assert_eq!(
            reopened.committed_revision(),
            Some(if advances < 3 { 1 } else { 2 })
        );
        assert_eq!(
            reopened.status(OWNER, &reference).unwrap(),
            OperationStatus::Unavailable
        );
        reopened_session.lock();
        assert_eq!(
            vault::begin_recovery(&model.live().unwrap(), paper())
                .unwrap()
                .revision(),
            reopened.committed_revision().unwrap()
        );
    }
}

#[test]
fn dropping_session_cancels_unadmitted_work_and_invalid_password_reload_stays_locked() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, mut worker) = opened();
    prepare(&mut session, &mut worker, 1);
    drop(session);
    assert_eq!(complete(&mut worker), OperationStatus::Canceled);
    // A different opened writer cannot reuse the source session's scope.
    let (_, mut session, worker) = opened();
    let generation = session.generation();
    assert!(
        session
            .reload_master(&worker, OWNER, "wrong synthetic")
            .is_err()
    );
    assert!(session.is_locked());
    assert!(session.generation() > generation);
}
