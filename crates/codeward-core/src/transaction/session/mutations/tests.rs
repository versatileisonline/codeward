use super::*;
use crate::{
    credentials::{Credential, CredentialInput, CredentialSnapshot, MAX_TIMESTAMP},
    crypto,
    password::PasswordOptions,
    transaction::tests::{
        complete, fixture,
        model::{Fault, Model},
        opened, paper, request,
    },
    vault,
};

const OWNER: CallerId = CallerId(7);
const PASSWORD: &str = "synthetic password";
const ID: &str = "00000000000000000000000000000001";
const OTHER: &str = "ffffffffffffffffffffffffffffffff";

#[test]
fn generated_password_save_reopens_exactly_one_changed_record_and_refreshes() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    let draft = session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    let preview = session
        .reveal_password_draft(&worker, OWNER, &draft)
        .unwrap();
    let events = model.events();
    let deadline = session.inactivity_deadline();
    let operation = session
        .prepare_generated_password(&mut worker, OWNER, request(1, 1), ID, &draft)
        .unwrap();
    assert_eq!(
        worker.status(OWNER, &operation).unwrap(),
        OperationStatus::Pending(Phase::Prepared)
    );
    assert_eq!(model.events(), events);
    assert_eq!(model.live(), Some(fixture()));
    assert_eq!(session.inactivity_deadline(), deadline);
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    let bytes = model.live().unwrap();
    let actual = vault::unlock_with_master(&bytes, PASSWORD).unwrap();
    let original = CredentialSnapshot::decode(include_bytes!(
        "../../../../tests/fixtures/credentials-v1.json"
    ))
    .unwrap();
    let expected = CredentialSnapshot::new(vec![
        Credential::new(CredentialInput {
            password: &preview.password,
            ..original.credentials()[0].fields()
        })
        .unwrap(),
        Credential::new(original.credentials()[1].fields()).unwrap(),
    ])
    .unwrap();
    // The original update timestamp is already year 9999; it cannot regress.
    assert_eq!(
        actual.snapshot().encode().unwrap(),
        expected.encode().unwrap()
    );
    assert!(
        !bytes
            .windows(preview.password.len())
            .any(|w| w == preview.password.as_bytes())
    );
    session
        .refresh_committed_contents(&worker, OWNER, &operation)
        .unwrap();
    assert!(session.password_draft.is_none());
    assert_eq!(
        session
            .reveal_password(&worker, OWNER, generation, 2, ID)
            .unwrap()
            .password,
        preview.password
    );
    assert_eq!(session.inactivity_deadline(), deadline);
}

#[test]
fn selected_deletion_removes_only_the_target_and_never_recreates_missing_records() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    let original = CredentialSnapshot::decode(include_bytes!(
        "../../../../tests/fixtures/credentials-v1.json"
    ))
    .unwrap();
    let expected = CredentialSnapshot::new(vec![
        Credential::new(original.credentials()[1].fields()).unwrap(),
    ])
    .unwrap();
    let events = model.events();
    let operation = session
        .prepare_delete_credential(&mut worker, OWNER, generation, request(1, 1), ID)
        .unwrap();
    assert_eq!(model.events(), events);
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    let bytes = model.live().unwrap();
    assert_eq!(
        vault::unlock_with_master(&bytes, PASSWORD)
            .unwrap()
            .snapshot()
            .encode()
            .unwrap(),
        expected.encode().unwrap()
    );
    session
        .refresh_committed_contents(&worker, OWNER, &operation)
        .unwrap();
    let events = model.events();
    assert_eq!(
        session
            .prepare_delete_credential(&mut worker, OWNER, generation, request(2, 2), ID)
            .err(),
        Some(TransactionError::CredentialNotFound)
    );
    assert_eq!(model.events(), events);
    assert_eq!(model.live(), Some(bytes));
}

#[test]
fn targeted_mutations_check_scope_revision_ids_and_draft_identity_before_work() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let (_, mut other_session, mut other_worker) = opened();
    let generation = session.generation();
    let draft = session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    let foreign = other_session
        .generate_password_draft(
            &other_worker,
            OWNER,
            generation,
            1,
            PasswordOptions::default(),
        )
        .unwrap();
    let events = model.events();
    assert_eq!(
        session
            .prepare_generated_password(&mut worker, CallerId(8), request(1, 1), ID, &draft)
            .err(),
        Some(TransactionError::Unauthorized)
    );
    assert_eq!(
        session
            .prepare_generated_password(&mut other_worker, OWNER, request(1, 1), ID, &draft)
            .err(),
        Some(TransactionError::Unauthorized)
    );
    assert_eq!(
        session
            .prepare_generated_password(&mut worker, OWNER, request(1, 1), ID, &foreign)
            .err(),
        Some(TransactionError::Unauthorized)
    );
    assert_eq!(
        session
            .prepare_generated_password(&mut worker, OWNER, request(1, 0), ID, &draft)
            .err(),
        Some(VaultError::RevisionConflict.into())
    );
    assert_eq!(
        session
            .prepare_delete_credential(&mut worker, OWNER, generation + 1, request(1, 1), ID)
            .err(),
        Some(TransactionError::Locked)
    );
    assert_eq!(
        session
            .prepare_delete_credential(&mut worker, CallerId(8), generation, request(1, 1), ID)
            .err(),
        Some(TransactionError::Unauthorized)
    );
    assert_eq!(
        session
            .prepare_delete_credential(&mut other_worker, OWNER, generation, request(1, 1), ID)
            .err(),
        Some(TransactionError::Unauthorized)
    );
    assert_eq!(
        session
            .prepare_delete_credential(&mut worker, OWNER, generation, request(1, 0), ID)
            .err(),
        Some(VaultError::RevisionConflict.into())
    );
    for id in ["", "0", "0000000000000000000000000000000A", "🔒"] {
        assert_eq!(
            session
                .prepare_generated_password(&mut worker, OWNER, request(1, 1), id, &draft)
                .err(),
            Some(TransactionError::InvalidCredentialId)
        );
        assert_eq!(
            session
                .prepare_delete_credential(&mut worker, OWNER, generation, request(1, 1), id)
                .err(),
            Some(TransactionError::InvalidCredentialId)
        );
    }
    assert_eq!(
        session
            .prepare_generated_password(
                &mut worker,
                OWNER,
                request(1, 1),
                "00000000000000000000000000000000",
                &draft
            )
            .err(),
        Some(TransactionError::CredentialNotFound)
    );
    session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    assert_eq!(
        session
            .prepare_generated_password(&mut worker, OWNER, request(1, 1), ID, &draft)
            .err(),
        Some(TransactionError::StalePasswordDraft)
    );
    assert_eq!(model.events(), events);
    session.lock();
    assert_eq!(
        session
            .prepare_delete_credential(&mut worker, OWNER, generation, request(1, 1), "bad")
            .err(),
        Some(TransactionError::Locked)
    );
    assert_eq!(
        session
            .prepare_generated_password(&mut worker, OWNER, request(1, 1), ID, &draft)
            .err(),
        Some(TransactionError::Locked)
    );
}

#[test]
fn failed_saves_preserve_drafts_and_known_ids_never_replay_changed_intent() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    let draft = session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    let operation = session
        .prepare_generated_password(&mut worker, OWNER, request(1, 1), ID, &draft)
        .unwrap();
    model.fault(Fault::Create);
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Failed(StorageError::CreateStage)
    );
    assert!(
        session
            .validate_password_draft(&worker, OWNER, &draft)
            .is_ok()
    );
    let events = model.events();
    let duplicate = session
        .prepare_generated_password(&mut worker, OWNER, request(1, 1), ID, &draft)
        .unwrap();
    assert_eq!(
        worker.status(OWNER, &duplicate).unwrap(),
        OperationStatus::Failed(StorageError::CreateStage)
    );
    assert_eq!(model.events(), events);
    assert!(worker.advance().is_none());
    assert_eq!(
        session
            .prepare_generated_password(&mut worker, OWNER, request(1, 1), OTHER, &draft)
            .err(),
        Some(TransactionError::RequestReused)
    );
    assert_eq!(
        session
            .prepare_delete_credential(&mut worker, OWNER, generation, request(1, 1), ID)
            .err(),
        Some(TransactionError::RequestReused)
    );
    let next_draft = session
        .generate_password_draft(
            &worker,
            OWNER,
            generation,
            1,
            PasswordOptions {
                lowercase: false,
                uppercase: false,
                digits: true,
                symbols: false,
                length: 128,
            },
        )
        .unwrap();
    assert_eq!(
        session
            .prepare_generated_password(&mut worker, OWNER, request(1, 1), ID, &next_draft)
            .err(),
        Some(TransactionError::RequestReused)
    );
    session
        .prepare_generated_password(&mut worker, OWNER, request(2, 1), ID, &next_draft)
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    assert_eq!(
        worker.status(OWNER, &operation).unwrap(),
        OperationStatus::Failed(StorageError::CreateStage)
    );
}

#[test]
fn uncertain_password_saves_block_mutations_until_actual_bytes_are_reconciled() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for (fault, committed) in [(Fault::UnknownBefore, false), (Fault::UnknownAfter, true)] {
        let (model, mut session, mut worker) = opened();
        let generation = session.generation();
        let draft = session
            .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
            .unwrap();
        let operation = session
            .prepare_generated_password(&mut worker, OWNER, request(1, 1), ID, &draft)
            .unwrap();
        model.fault(fault);
        assert_eq!(complete(&mut worker), OperationStatus::OutcomeUnknown);
        assert_eq!(
            session
                .prepare_generated_password(&mut worker, OWNER, request(2, 1), ID, &draft)
                .err(),
            Some(TransactionError::Busy)
        );
        assert_eq!(
            session
                .prepare_delete_credential(&mut worker, OWNER, generation, request(2, 1), ID)
                .err(),
            Some(TransactionError::Busy)
        );
        let expected = if committed {
            OperationStatus::Committed { revision: 2 }
        } else {
            OperationStatus::Unchanged
        };
        assert_eq!(
            worker.reconcile_recovery(OWNER, &operation, paper()),
            Ok(expected)
        );
        if committed {
            assert_eq!(
                session
                    .prepare_generated_password(&mut worker, OWNER, request(2, 1), ID, &draft)
                    .err(),
                Some(TransactionError::ReloadRequired)
            );
            session.reload_master(&worker, OWNER, PASSWORD).unwrap();
            assert!(session.password_draft.is_none());
        } else {
            assert!(
                session
                    .validate_password_draft(&worker, OWNER, &draft)
                    .is_ok()
            );
            session
                .prepare_generated_password(&mut worker, OWNER, request(2, 1), ID, &draft)
                .unwrap();
            assert_eq!(
                complete(&mut worker),
                OperationStatus::Committed { revision: 2 }
            );
        }
    }
}

#[test]
fn security_lock_cancels_unadmitted_password_saves_but_never_rolls_back_admitted_work() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for admitted in [false, true] {
        let (model, mut session, mut worker) = opened();
        let generation = session.generation();
        let draft = session
            .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
            .unwrap();
        session
            .prepare_generated_password(&mut worker, OWNER, request(1, 1), ID, &draft)
            .unwrap();
        if admitted {
            worker.advance();
            worker.advance();
        }
        session.lock();
        assert!(session.password_draft.is_none());
        assert!(session.is_locked());
        assert_eq!(
            complete(&mut worker),
            if admitted {
                OperationStatus::Committed { revision: 2 }
            } else {
                OperationStatus::Canceled
            }
        );
        if !admitted {
            assert_eq!(model.live(), Some(fixture()));
        }
    }
}

#[test]
fn restricted_and_expired_sessions_cannot_delete_or_save_generated_passwords() {
    let (mut session, mut worker) =
        Committer::open_recovery(Model::new(Some(fixture())), OWNER, paper()).unwrap();
    let generation = session.generation();
    assert_eq!(
        session
            .prepare_delete_credential(&mut worker, OWNER, generation, request(1, 1), "bad")
            .err(),
        Some(TransactionError::Locked)
    );
    let (mut session, mut worker) = Committer::begin_create(Model::new(None), OWNER).unwrap();
    let generation = session.generation();
    assert_eq!(
        session
            .prepare_delete_credential(&mut worker, OWNER, generation, request(1, 0), "bad")
            .err(),
        Some(TransactionError::Locked)
    );
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    let draft = session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    let events = model.events();
    session.set_deadline_for_test(std::time::Instant::now());
    assert_eq!(
        session
            .prepare_generated_password(&mut worker, OWNER, request(1, 1), ID, &draft)
            .err(),
        Some(TransactionError::Locked)
    );
    assert_eq!(
        session
            .prepare_delete_credential(&mut worker, OWNER, generation, request(1, 1), ID)
            .err(),
        Some(TransactionError::Locked)
    );
    assert!(session.password_draft.is_none());
    assert_eq!(model.events(), events);
}

#[test]
fn mutation_clock_checks_profile_bounds_and_snapshot_copy_obeys_cancellation() {
    use std::time::{Duration, UNIX_EPOCH};
    assert_eq!(unix_millis(UNIX_EPOCH), Ok(0));
    assert_eq!(unix_millis(UNIX_EPOCH + Duration::from_millis(9)), Ok(9));
    assert_eq!(
        unix_millis(UNIX_EPOCH + Duration::from_millis(MAX_TIMESTAMP)),
        Ok(MAX_TIMESTAMP)
    );
    assert_eq!(
        unix_millis(UNIX_EPOCH + Duration::from_millis(MAX_TIMESTAMP + 1)),
        Err(TransactionError::ClockUnavailable)
    );
    assert_eq!(
        unix_millis(UNIX_EPOCH - Duration::from_millis(1)),
        Err(TransactionError::ClockUnavailable)
    );
    let snapshot = CredentialSnapshot::decode(include_bytes!(
        "../../../../tests/fixtures/credentials-v1.json"
    ))
    .unwrap();
    let permit = Permit::new();
    let updated = changed_snapshot(
        &snapshot,
        OTHER,
        Some(("synthetic replacement", 0)),
        &permit,
    )
    .unwrap();
    assert_eq!(updated.credentials()[1].fields().updated_at, 1);
    permit.cancel();
    assert_eq!(
        changed_snapshot(&snapshot, OTHER, None, &permit).err(),
        Some(TransactionError::Locked)
    );
}
