use super::*;
use crate::{
    crypto,
    password::{GenerationError, PasswordOptions, generate_password},
    transaction::tests::{
        complete, empty, fixture,
        model::{Fault, Model},
        opened, paper, request,
    },
};
use std::time::{Duration, Instant};

const OWNER: CallerId = CallerId(7);

#[test]
fn draft_generation_is_unsaved_reveal_is_explicit_and_discard_is_scoped() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, worker) = opened();
    let generation = session.generation();
    let events = model.events();
    let deadline = session.inactivity_deadline();
    let draft = session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    assert_eq!(draft.generation(), generation);
    assert_eq!(draft.revision(), 1);
    let start = Instant::now();
    let reveal = session
        .reveal_password_draft(&worker, OWNER, &draft)
        .unwrap();
    assert_eq!(reveal.generation, generation);
    assert_eq!(reveal.revision, 1);
    assert_eq!(reveal.password.len(), 24);
    assert!(reveal.expires_at > start);
    assert!(reveal.expires_at <= Instant::now() + Duration::from_secs(30));
    assert!(reveal.expires_at <= deadline.unwrap());
    let again = session
        .reveal_password_draft(&worker, OWNER, &draft)
        .unwrap();
    assert_eq!(reveal.password, again.password);
    session
        .discard_password_draft(&worker, OWNER, &draft)
        .unwrap();
    assert!(session.password_draft.is_none());
    assert_eq!(
        session.reveal_password_draft(&worker, OWNER, &draft).err(),
        Some(TransactionError::StalePasswordDraft)
    );
    assert_eq!(session.inactivity_deadline(), deadline);
    assert_eq!(model.events(), events);
    assert_eq!(model.live(), Some(fixture()));
}

#[test]
fn regeneration_invalidates_only_after_success_and_stale_discard_cannot_remove_new_draft() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, worker) = opened();
    let generation = session.generation();
    let first = session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    let original = session
        .reveal_password_draft(&worker, OWNER, &first)
        .unwrap();
    for error in [
        GenerationError::RandomnessUnavailable,
        GenerationError::AllocationFailed,
    ] {
        assert_eq!(
            session
                .generate_draft_with(&worker, OWNER, generation, 1, || Err(error))
                .err(),
            Some(TransactionError::PasswordGeneration(error))
        );
        assert_eq!(
            session
                .reveal_password_draft(&worker, OWNER, &first)
                .unwrap()
                .password,
            original.password
        );
    }
    assert_eq!(
        session
            .generate_password_draft(
                &worker,
                OWNER,
                generation,
                1,
                PasswordOptions {
                    length: 11,
                    ..PasswordOptions::default()
                }
            )
            .err(),
        Some(TransactionError::PasswordGeneration(
            GenerationError::InvalidOptions
        ))
    );
    assert!(
        session
            .reveal_password_draft(&worker, OWNER, &first)
            .is_ok()
    );
    let second = session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    assert_eq!(
        session.reveal_password_draft(&worker, OWNER, &first).err(),
        Some(TransactionError::StalePasswordDraft)
    );
    assert_eq!(
        session.discard_password_draft(&worker, OWNER, &first),
        Err(TransactionError::StalePasswordDraft)
    );
    assert!(
        session
            .reveal_password_draft(&worker, OWNER, &second)
            .is_ok()
    );
}

#[test]
fn drafts_reject_foreign_caller_worker_reference_generation_and_revision() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, worker) = opened();
    let (_, mut other, other_worker) = opened();
    let generation = session.generation();
    let draft = session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    let foreign = other
        .generate_password_draft(
            &other_worker,
            OWNER,
            generation,
            1,
            PasswordOptions::default(),
        )
        .unwrap();
    for (caller, opened) in [(CallerId(8), &worker), (OWNER, &other_worker)] {
        assert_eq!(
            session
                .generate_draft_with(opened, caller, generation, 1, || panic!(
                    "unauthorized entropy"
                ))
                .err(),
            Some(TransactionError::Unauthorized)
        );
        assert_eq!(
            session.reveal_password_draft(opened, caller, &draft).err(),
            Some(TransactionError::Unauthorized)
        );
        assert_eq!(
            session.discard_password_draft(opened, caller, &draft),
            Err(TransactionError::Unauthorized)
        );
    }
    assert_eq!(
        session
            .reveal_password_draft(&worker, OWNER, &foreign)
            .err(),
        Some(TransactionError::Unauthorized)
    );
    assert_eq!(
        session.discard_password_draft(&worker, OWNER, &foreign),
        Err(TransactionError::Unauthorized)
    );
    assert_eq!(
        session
            .generate_draft_with(&worker, OWNER, generation + 1, 1, || panic!(
                "stale generation entropy"
            ))
            .err(),
        Some(TransactionError::Locked)
    );
    assert_eq!(
        session
            .generate_draft_with(&worker, OWNER, generation, 0, || panic!(
                "stale revision entropy"
            ))
            .err(),
        Some(VaultError::RevisionConflict.into())
    );
    assert!(
        session
            .reveal_password_draft(&worker, OWNER, &draft)
            .is_ok()
    );
    session.lock();
    assert!(session.password_draft.is_none());
    session
        .reload_master(&worker, OWNER, "synthetic password")
        .unwrap();
    assert_eq!(
        session.reveal_password_draft(&worker, OWNER, &draft).err(),
        Some(TransactionError::Locked)
    );
}

#[test]
fn enrollment_recovery_and_locked_sessions_cannot_generate() {
    let (mut session, worker) =
        Committer::open_recovery(Model::new(Some(fixture())), OWNER, paper()).unwrap();
    let generation = session.generation();
    assert_eq!(
        session
            .generate_draft_with(&worker, OWNER, generation, 1, || panic!("recovery entropy"))
            .err(),
        Some(TransactionError::Locked)
    );
    let (mut session, worker) = Committer::begin_create(Model::new(None), OWNER).unwrap();
    let generation = session.generation();
    assert_eq!(
        session
            .generate_draft_with(&worker, OWNER, generation, 0, || panic!(
                "enrollment entropy"
            ))
            .err(),
        Some(TransactionError::Locked)
    );
    session.lock();
    assert_eq!(
        session
            .generate_draft_with(&worker, OWNER, session.generation(), 0, || panic!(
                "locked entropy"
            ))
            .err(),
        Some(TransactionError::Locked)
    );
}

#[test]
fn definite_failure_retains_draft_but_unknown_conflict_and_changed_commits_deny_access() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    let draft = session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    let original = session
        .reveal_password_draft(&worker, OWNER, &draft)
        .unwrap();
    session
        .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
        .unwrap();
    assert_eq!(
        session.reveal_password_draft(&worker, OWNER, &draft).err(),
        Some(TransactionError::Busy)
    );
    assert_eq!(
        session.discard_password_draft(&worker, OWNER, &draft),
        Err(TransactionError::Busy)
    );
    assert_eq!(
        session
            .generate_draft_with(&worker, OWNER, generation, 1, || panic!("busy entropy"))
            .err(),
        Some(TransactionError::Busy)
    );
    model.fault(Fault::Create);
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Failed(StorageError::CreateStage)
    );
    assert_eq!(
        session
            .reveal_password_draft(&worker, OWNER, &draft)
            .unwrap()
            .password,
        original.password
    );
    let uncertain = session
        .prepare_contents(&mut worker, OWNER, request(2, 1), &empty())
        .unwrap();
    model.fault(Fault::UnknownBefore);
    assert_eq!(complete(&mut worker), OperationStatus::OutcomeUnknown);
    assert_eq!(
        session.reveal_password_draft(&worker, OWNER, &draft).err(),
        Some(TransactionError::Busy)
    );
    assert_eq!(
        session.discard_password_draft(&worker, OWNER, &draft),
        Err(TransactionError::Busy)
    );
    assert_eq!(
        worker.reconcile_recovery(OWNER, &uncertain, paper()),
        Ok(OperationStatus::Unchanged)
    );
    assert_eq!(
        session
            .reveal_password_draft(&worker, OWNER, &draft)
            .unwrap()
            .password,
        original.password
    );
    let saved = session
        .prepare_contents(&mut worker, OWNER, request(3, 1), &empty())
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    assert_eq!(
        session.reveal_password_draft(&worker, OWNER, &draft).err(),
        Some(TransactionError::ReloadRequired)
    );
    session
        .refresh_committed_contents(&worker, OWNER, &saved)
        .unwrap();
    assert!(session.password_draft.is_none());
    assert!(
        session
            .reveal_password_draft(&worker, OWNER, &draft)
            .is_err()
    );
    let current = session
        .generate_password_draft(&worker, OWNER, generation, 2, PasswordOptions::default())
        .unwrap();
    session
        .prepare_contents(&mut worker, OWNER, request(4, 2), &empty())
        .unwrap();
    model.0.lock().unwrap().live = Some(fixture());
    assert_eq!(complete(&mut worker), OperationStatus::Conflict);
    assert_eq!(
        session
            .reveal_password_draft(&worker, OWNER, &current)
            .err(),
        Some(TransactionError::ReloadRequired)
    );
}

#[test]
fn idle_and_security_revocation_drop_drafts_and_reveals_have_bounded_deadlines() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, worker) = opened();
    let generation = session.generation();
    let draft = session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    session.set_deadline_for_test(deadline);
    assert_eq!(
        session
            .reveal_password_draft(&worker, OWNER, &draft)
            .unwrap()
            .expires_at,
        deadline
    );
    assert_eq!(session.inactivity_deadline(), Some(deadline));
    session.set_deadline_for_test(Instant::now());
    assert_eq!(
        session.reveal_password_draft(&worker, OWNER, &draft).err(),
        Some(TransactionError::Locked)
    );
    assert!(session.password_draft.is_none());
    session
        .reload_master(&worker, OWNER, "synthetic password")
        .unwrap();
    let generation = session.generation();
    let draft = session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    session.observe_event(SessionEvent::OsSessionLocked);
    assert!(session.password_draft.is_none());
    assert_eq!(
        session.discard_password_draft(&worker, OWNER, &draft),
        Err(TransactionError::Locked)
    );
}

#[test]
fn expiry_during_generation_drops_old_draft_and_never_installs_new_output() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for succeeds in [false, true] {
        let (_, mut session, worker) = opened();
        let generation = session.generation();
        session
            .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
            .unwrap();
        let deadline = Instant::now() + Duration::from_millis(100);
        session.set_deadline_for_test(deadline);
        let mut invoked = false;
        let result = session.generate_draft_with(&worker, OWNER, generation, 1, || {
            invoked = true;
            let value = if succeeds {
                generate_password(PasswordOptions::default())
            } else {
                Err(GenerationError::RandomnessUnavailable)
            };
            std::thread::sleep(
                (deadline + Duration::from_millis(1)).saturating_duration_since(Instant::now()),
            );
            value
        });
        assert!(invoked);
        assert_eq!(result.err(), Some(TransactionError::Locked));
        assert!(session.password_draft.is_none());
        assert!(session.is_locked());
    }
}

#[test]
fn reconciliation_to_changed_bytes_requires_reload_and_never_restores_a_draft() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    let draft = session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    let operation = session
        .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
        .unwrap();
    model.fault(Fault::UnknownAfter);
    assert_eq!(complete(&mut worker), OperationStatus::OutcomeUnknown);
    assert_eq!(
        session.validate_password_draft(&worker, OWNER, &draft),
        Err(TransactionError::Busy)
    );
    assert_eq!(
        worker.reconcile_recovery(OWNER, &operation, paper()),
        Ok(OperationStatus::Committed { revision: 2 })
    );
    assert_eq!(
        session.validate_password_draft(&worker, OWNER, &draft),
        Err(TransactionError::ReloadRequired)
    );
    session
        .reload_master(&worker, OWNER, "synthetic password")
        .unwrap();
    assert!(session.password_draft.is_none());
    assert_eq!(
        session.validate_password_draft(&worker, OWNER, &draft),
        Err(TransactionError::Locked)
    );
}
