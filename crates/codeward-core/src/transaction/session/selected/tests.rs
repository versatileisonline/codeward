use std::time::{Duration, Instant};

use super::super::*;
use crate::{
    credentials::{Credential, CredentialInput, CredentialSnapshot},
    crypto,
    transaction::tests::{
        complete, empty, fixture,
        model::{Fault, Lease, Model},
        opened, paper, request,
    },
};

const OWNER: CallerId = CallerId(7);
const ID: &str = "00000000000000000000000000000001";

fn denied(
    session: &mut Session,
    worker: &Committer<Lease>,
    caller: CallerId,
    generation: u64,
    revision: u64,
    id: &str,
    expected: TransactionError,
) {
    assert_eq!(
        session
            .get_credential_details(worker, caller, generation, revision, id)
            .err(),
        Some(expected)
    );
    assert_eq!(
        session
            .reveal_password(worker, caller, generation, revision, id)
            .err(),
        Some(expected)
    );
}

#[test]
fn selected_details_and_explicit_reveal_preserve_requested_fields() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, worker) = opened();
    let expected = CredentialSnapshot::decode(include_bytes!(
        "../../../../tests/fixtures/credentials-v1.json"
    ))
    .unwrap();
    let generation = session.generation();
    let deadline = session.inactivity_deadline();
    let events = model.events();
    assert_eq!(super::PASSWORD_REVEAL_DURATION, Duration::from_secs(30));
    for record in expected.credentials() {
        let fields = record.fields();
        let detail = session
            .get_credential_details(&worker, OWNER, generation, 1, fields.id)
            .unwrap();
        assert_eq!(detail.generation, generation);
        assert_eq!(detail.revision, 1);
        assert_eq!(detail.id.as_str(), fields.id);
        assert_eq!(detail.title.as_str(), fields.title);
        assert_eq!(detail.username.as_str(), fields.username);
        assert_eq!(detail.url.as_str(), fields.url);
        assert_eq!(detail.notes.as_str(), fields.notes);
        assert_eq!(detail.created_at, fields.created_at);
        assert_eq!(detail.updated_at, fields.updated_at);
        let before = Instant::now();
        let reveal = session
            .reveal_password(&worker, OWNER, generation, 1, fields.id)
            .unwrap();
        let after = Instant::now();
        assert_eq!(reveal.generation, generation);
        assert_eq!(reveal.revision, 1);
        assert_eq!(reveal.id.as_str(), fields.id);
        assert_eq!(reveal.password.as_str(), fields.password);
        assert!(reveal.expires_at > before);
        assert!(reveal.expires_at <= after + Duration::from_secs(30));
        assert!(reveal.expires_at <= deadline.unwrap());
    }
    assert_eq!(session.inactivity_deadline(), deadline);
    assert_eq!(model.events(), events);
}

#[test]
fn selected_reads_validate_ids_only_after_authorization() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, worker) = opened();
    let generation = session.generation();
    for id in [
        "",
        "1",
        "0000000000000000000000000000000A",
        "0000000000000000000000000000000g",
        "🔒",
    ] {
        denied(
            &mut session,
            &worker,
            OWNER,
            generation,
            1,
            id,
            TransactionError::InvalidCredentialId,
        );
    }
    for length in [31, 33, 65536] {
        denied(
            &mut session,
            &worker,
            OWNER,
            generation,
            1,
            &"0".repeat(length),
            TransactionError::InvalidCredentialId,
        );
    }
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        1,
        "00000000000000000000000000000000",
        TransactionError::CredentialNotFound,
    );
    session.lock();
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        1,
        "invalid",
        TransactionError::Locked,
    );
}

#[test]
fn selected_reads_reject_wrong_caller_worker_generation_and_revision() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, worker) = opened();
    let (_, _, other_worker) = opened();
    let generation = session.generation();
    denied(
        &mut session,
        &worker,
        CallerId(8),
        generation,
        1,
        ID,
        TransactionError::Unauthorized,
    );
    denied(
        &mut session,
        &other_worker,
        OWNER,
        generation,
        1,
        ID,
        TransactionError::Unauthorized,
    );
    denied(
        &mut session,
        &worker,
        OWNER,
        generation + 1,
        1,
        ID,
        TransactionError::Locked,
    );
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        0,
        ID,
        VaultError::RevisionConflict.into(),
    );
    session.lock();
    session
        .reload_master(&worker, OWNER, "synthetic password")
        .unwrap();
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        1,
        ID,
        TransactionError::Locked,
    );
}

#[test]
fn selected_reads_refuse_recovery_and_enrollment() {
    let (mut session, worker) =
        Committer::open_recovery(Model::new(Some(fixture())), OWNER, paper()).unwrap();
    let generation = session.generation();
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        1,
        ID,
        TransactionError::Locked,
    );
    let (mut session, worker) = Committer::begin_create(Model::new(None), OWNER).unwrap();
    let generation = session.generation();
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        0,
        ID,
        TransactionError::Locked,
    );
}

#[test]
fn selected_reads_wait_for_resolution_refresh_and_deleted_record_state() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    let uncertain = session
        .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
        .unwrap();
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        1,
        ID,
        TransactionError::Busy,
    );
    model.fault(Fault::UnknownBefore);
    assert_eq!(complete(&mut worker), OperationStatus::OutcomeUnknown);
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        1,
        ID,
        TransactionError::Busy,
    );
    assert_eq!(
        worker.reconcile_recovery(OWNER, &uncertain, paper()),
        Ok(OperationStatus::Unchanged)
    );
    assert!(
        session
            .get_credential_details(&worker, OWNER, generation, 1, ID)
            .is_ok()
    );
    let saved = session
        .prepare_contents(&mut worker, OWNER, request(2, 1), &empty())
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        1,
        ID,
        TransactionError::ReloadRequired,
    );
    session
        .refresh_committed_contents(&worker, OWNER, &saved)
        .unwrap();
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        1,
        ID,
        VaultError::RevisionConflict.into(),
    );
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        2,
        ID,
        TransactionError::CredentialNotFound,
    );
    session
        .prepare_contents(&mut worker, OWNER, request(3, 2), &empty())
        .unwrap();
    model.0.lock().unwrap().live = Some(fixture());
    assert_eq!(complete(&mut worker), OperationStatus::Conflict);
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        2,
        ID,
        TransactionError::ReloadRequired,
    );
}

#[test]
fn selected_reads_require_reload_after_reconciled_candidate() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    let operation = session
        .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
        .unwrap();
    model.fault(Fault::UnknownAfter);
    assert_eq!(complete(&mut worker), OperationStatus::OutcomeUnknown);
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        1,
        ID,
        TransactionError::Busy,
    );
    assert_eq!(
        worker.reconcile_recovery(OWNER, &operation, paper()),
        Ok(OperationStatus::Committed { revision: 2 })
    );
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        2,
        ID,
        TransactionError::ReloadRequired,
    );
    session
        .reload_master(&worker, OWNER, "synthetic password")
        .unwrap();
    let current = session.generation();
    denied(
        &mut session,
        &worker,
        OWNER,
        current,
        2,
        ID,
        TransactionError::CredentialNotFound,
    );
}

#[test]
fn reveal_deadline_is_capped_by_idle_and_expired_reads_revoke_access() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, worker) = opened();
    let generation = session.generation();
    let deadline = Instant::now() + Duration::from_secs(10);
    session.set_deadline_for_test(deadline);
    let events = model.events();
    let reveal = session
        .reveal_password(&worker, OWNER, generation, 1, ID)
        .unwrap();
    assert_eq!(reveal.expires_at, deadline);
    assert_eq!(session.inactivity_deadline(), Some(deadline));
    assert_eq!(model.events(), events);
    drop(reveal);
    session.set_deadline_for_test(Instant::now());
    denied(
        &mut session,
        &worker,
        OWNER,
        generation,
        1,
        ID,
        TransactionError::Locked,
    );
    assert!(session.is_locked());
    assert_eq!(session.generation(), generation + 1);
    assert!(session.inactivity_deadline().is_none());
    session
        .reload_master(&worker, OWNER, "synthetic password")
        .unwrap();
    let current = session.generation();
    session.set_deadline_for_test(Instant::now());
    assert_eq!(
        session
            .reveal_password(&worker, OWNER, current, 1, ID)
            .err(),
        Some(TransactionError::Locked)
    );
    assert!(session.is_locked());
}

#[test]
fn selected_payloads_preserve_maximum_schema_fields() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, mut worker) = opened();
    let title = "é".repeat(256);
    let username = "u".repeat(1024);
    let password = "p".repeat(4096);
    let url = "x".repeat(4096);
    let notes = "n".repeat(65536);
    let record = Credential::new(CredentialInput {
        id: ID,
        title: &title,
        username: &username,
        password: &password,
        url: &url,
        notes: &notes,
        created_at: 0,
        updated_at: 253_402_300_799_999,
    })
    .unwrap();
    let snapshot = CredentialSnapshot::new(vec![record]).unwrap();
    let operation = session
        .prepare_contents(&mut worker, OWNER, request(1, 1), &snapshot)
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    session
        .refresh_committed_contents(&worker, OWNER, &operation)
        .unwrap();
    let generation = session.generation();
    let detail = session
        .get_credential_details(&worker, OWNER, generation, 2, ID)
        .unwrap();
    assert_eq!(detail.title.as_str(), title);
    assert_eq!(detail.username.as_str(), username);
    assert_eq!(detail.url.as_str(), url);
    assert_eq!(detail.notes.as_str(), notes);
    assert_eq!(detail.updated_at, 253_402_300_799_999);
    assert_eq!(
        session
            .reveal_password(&worker, OWNER, generation, 2, ID)
            .unwrap()
            .password
            .as_str(),
        password
    );
}
