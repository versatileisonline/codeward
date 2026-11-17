use super::*;
use crate::{
    credentials::{
        Credential, CredentialError, CredentialInput, CredentialSnapshot, MAX_CREDENTIALS, valid_id,
    },
    crypto::{self, CryptoError},
    password::PasswordOptions,
    transaction::tests::{
        complete, fixture,
        model::{Fault, Model},
        opened, paper, request,
    },
    vault,
};

const OWNER: CallerId = CallerId(7);
const ID: &str = "00000000000000000000000000000001";
const PASSWORD: &str = "synthetic password";

fn form(password: CredentialPassword<'_>) -> CredentialForm<'_> {
    CredentialForm {
        title: " New résumé service ",
        username: " user\0name ",
        password,
        url: " https://example.invalid/ ",
        notes: "line\r\n🗝\t[]{}",
    }
}

fn invalid() -> TransactionError {
    VaultError::Credentials(CredentialError::InvalidCredential).into()
}

fn owned() -> OwnedCredentialFields {
    OwnedCredentialFields {
        title: copy_text("new").unwrap(),
        username: copy_text("").unwrap(),
        url: copy_text("").unwrap(),
        notes: copy_text("").unwrap(),
        password: Some(copy_text("synthetic").unwrap()),
    }
}

#[test]
fn creation_generates_identity_and_timestamps_and_preserves_every_existing_record() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    let deadline = session.inactivity_deadline();
    let events = model.events();
    let operation = session
        .prepare_create_credential(
            &mut worker,
            OWNER,
            generation,
            request(1, 1),
            form(CredentialPassword::Provided(" a\0\"🔒é ")),
        )
        .unwrap();
    assert_eq!(model.events(), events);
    assert_eq!(
        worker.status(OWNER, &operation).unwrap(),
        OperationStatus::Pending(Phase::Prepared)
    );
    assert_eq!(model.live(), Some(fixture()));
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    let actual = vault::unlock_with_master(&model.live().unwrap(), PASSWORD).unwrap();
    let records = actual.snapshot().credentials();
    assert_eq!(records.len(), 3);
    let new = records
        .iter()
        .find(|r| r.fields().title == " New résumé service ")
        .unwrap()
        .fields();
    assert!(valid_id(new.id));
    assert_ne!(new.id, ID);
    assert_ne!(new.id, "ffffffffffffffffffffffffffffffff");
    assert_eq!(new.username, " user\0name ");
    assert_eq!(new.password, " a\0\"🔒é ");
    assert_eq!(new.url, " https://example.invalid/ ");
    assert_eq!(new.notes, "line\r\n🗝\t[]{}");
    assert!(new.created_at > 0);
    assert_eq!(new.created_at, new.updated_at);
    let old = CredentialSnapshot::new(
        records
            .iter()
            .filter(|r| r.fields().id != new.id)
            .map(|r| Credential::new(r.fields()).unwrap())
            .collect(),
    )
    .unwrap();
    let expected = CredentialSnapshot::decode(include_bytes!(
        "../../../../tests/fixtures/credentials-v1.json"
    ))
    .unwrap();
    assert_eq!(old.encode().unwrap(), expected.encode().unwrap());
    session
        .refresh_committed_contents(&worker, OWNER, &operation)
        .unwrap();
    let listed = session
        .list_credentials(&worker, OWNER, generation, 100, None)
        .unwrap();
    assert_eq!(listed.summaries.len(), 3);
    assert_eq!(session.inactivity_deadline(), deadline);
}

#[test]
fn general_edit_preserves_identity_creation_and_unrelated_records_with_password_keep_or_replace() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    for (number, password) in [
        (1, CredentialPassword::Provided("explicit replacement")),
        (2, CredentialPassword::Keep),
        (3, CredentialPassword::Provided("")),
    ] {
        let operation = session
            .prepare_update_credential(
                &mut worker,
                OWNER,
                generation,
                request(number, u64::from(number)),
                ID,
                form(password),
            )
            .unwrap();
        assert_eq!(
            complete(&mut worker),
            OperationStatus::Committed {
                revision: u64::from(number) + 1
            }
        );
        session
            .refresh_committed_contents(&worker, OWNER, &operation)
            .unwrap();
        let actual = vault::unlock_with_master(&model.live().unwrap(), PASSWORD).unwrap();
        let selected = actual.snapshot().credentials()[0].fields();
        assert_eq!(selected.id, ID);
        assert_eq!(selected.created_at, 0);
        assert_eq!(selected.updated_at, crate::credentials::MAX_TIMESTAMP);
        assert_eq!(selected.title, " New résumé service ");
        assert_eq!(selected.username, " user\0name ");
        assert_eq!(
            selected.password,
            if number == 3 {
                ""
            } else {
                "explicit replacement"
            }
        );
        assert_eq!(selected.notes, "line\r\n🗝\t[]{}");
        let other = actual.snapshot().credentials()[1].fields();
        assert_eq!(other.id, "ffffffffffffffffffffffffffffffff");
        assert_eq!(other.title, "Second synthetic service");
        assert_eq!(other.password, "");
        assert_eq!(other.created_at, 1);
        assert_eq!(other.updated_at, 1);
    }
}

#[test]
fn forms_accept_generated_passwords_and_revoke_drafts_only_after_confirmed_refresh() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, mut worker) = opened();
    let generation = session.generation();
    for creation in [true, false] {
        let revision = if creation { 1 } else { 2 };
        let draft = session
            .generate_password_draft(
                &worker,
                OWNER,
                generation,
                revision,
                PasswordOptions::default(),
            )
            .unwrap();
        let preview = session
            .reveal_password_draft(&worker, OWNER, &draft)
            .unwrap();
        let operation = if creation {
            session.prepare_create_credential(
                &mut worker,
                OWNER,
                generation,
                request(1, revision),
                form(CredentialPassword::Generated(&draft)),
            )
        } else {
            session.prepare_update_credential(
                &mut worker,
                OWNER,
                generation,
                request(2, revision),
                ID,
                form(CredentialPassword::Generated(&draft)),
            )
        }
        .unwrap();
        assert_eq!(
            complete(&mut worker),
            OperationStatus::Committed {
                revision: revision + 1
            }
        );
        assert!(session.password_draft.is_some());
        session
            .refresh_committed_contents(&worker, OWNER, &operation)
            .unwrap();
        assert!(session.password_draft.is_none());
        let actual = session
            .list_credentials(&worker, OWNER, generation, 100, None)
            .unwrap();
        let id = if creation {
            actual
                .summaries
                .iter()
                .find(|r| r.title.as_str() == " New résumé service ")
                .unwrap()
                .id
                .as_str()
        } else {
            ID
        };
        assert_eq!(
            session
                .reveal_password(&worker, OWNER, generation, revision + 1, id)
                .unwrap()
                .password,
            preview.password
        );
    }
}

#[test]
fn invalid_form_bounds_are_rejected_before_work_and_schema_maxima_preserve_exact_bytes() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    let events = model.events();
    assert_eq!(
        session
            .prepare_create_credential(
                &mut worker,
                OWNER,
                generation,
                request(1, 1),
                form(CredentialPassword::Keep)
            )
            .err(),
        Some(invalid())
    );
    for (field, length) in [
        (0, 0),
        (0, 513),
        (1, 1025),
        (2, 4097),
        (3, 65537),
        (4, 4097),
    ] {
        let long = "x".repeat(length);
        let mut input = form(CredentialPassword::Provided("synthetic"));
        match field {
            0 => input.title = &long,
            1 => input.username = &long,
            2 => input.url = &long,
            3 => input.notes = &long,
            _ => input.password = CredentialPassword::Provided(&long),
        }
        assert_eq!(
            session
                .prepare_create_credential(&mut worker, OWNER, generation, request(1, 1), input)
                .err(),
            Some(invalid())
        );
    }
    assert_eq!(model.events(), events);
    let title = "é".repeat(256);
    let username = "u".repeat(1024);
    let password = "p".repeat(4096);
    let url = "x".repeat(4096);
    let notes = "n".repeat(65536);
    let input = CredentialForm {
        title: &title,
        username: &username,
        password: CredentialPassword::Provided(&password),
        url: &url,
        notes: &notes,
    };
    let operation = session
        .prepare_update_credential(&mut worker, OWNER, generation, request(2, 1), ID, input)
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    session
        .refresh_committed_contents(&worker, OWNER, &operation)
        .unwrap();
    let detail = session
        .get_credential_details(&worker, OWNER, generation, 2, ID)
        .unwrap();
    assert_eq!(detail.title.as_str(), title);
    assert_eq!(detail.username.as_str(), username);
    assert_eq!(detail.url.as_str(), url);
    assert_eq!(detail.notes.as_str(), notes);
    assert_eq!(
        session
            .reveal_password(&worker, OWNER, generation, 2, ID)
            .unwrap()
            .password
            .as_str(),
        password
    );
}

#[test]
fn create_edit_authorize_context_revision_and_generated_refs_without_implicit_upsert() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let (_, mut foreign, mut foreign_worker) = opened();
    let generation = session.generation();
    let reference = foreign
        .generate_password_draft(
            &foreign_worker,
            OWNER,
            generation,
            1,
            PasswordOptions::default(),
        )
        .unwrap();
    let events = model.events();
    assert_eq!(
        session
            .prepare_create_credential(
                &mut worker,
                CallerId(8),
                generation,
                request(1, 1),
                form(CredentialPassword::Provided("x"))
            )
            .err(),
        Some(TransactionError::Unauthorized)
    );
    assert_eq!(
        session
            .prepare_create_credential(
                &mut foreign_worker,
                OWNER,
                generation,
                request(1, 1),
                form(CredentialPassword::Provided("x"))
            )
            .err(),
        Some(TransactionError::Unauthorized)
    );
    assert_eq!(
        session
            .prepare_create_credential(
                &mut worker,
                OWNER,
                generation + 1,
                request(1, 1),
                form(CredentialPassword::Provided("x"))
            )
            .err(),
        Some(TransactionError::Locked)
    );
    assert_eq!(
        session
            .prepare_update_credential(
                &mut worker,
                OWNER,
                generation,
                request(1, 0),
                ID,
                form(CredentialPassword::Keep)
            )
            .err(),
        Some(VaultError::RevisionConflict.into())
    );
    assert_eq!(
        session
            .prepare_create_credential(
                &mut worker,
                OWNER,
                generation,
                request(1, 1),
                form(CredentialPassword::Generated(&reference))
            )
            .err(),
        Some(TransactionError::Unauthorized)
    );
    assert_eq!(
        session
            .prepare_update_credential(
                &mut worker,
                OWNER,
                generation,
                request(1, 1),
                "bad",
                form(CredentialPassword::Keep)
            )
            .err(),
        Some(TransactionError::InvalidCredentialId)
    );
    assert_eq!(
        session
            .prepare_update_credential(
                &mut worker,
                OWNER,
                generation,
                request(1, 1),
                "00000000000000000000000000000000",
                form(CredentialPassword::Keep)
            )
            .err(),
        Some(TransactionError::CredentialNotFound)
    );
    let stale = session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    session
        .generate_password_draft(&worker, OWNER, generation, 1, PasswordOptions::default())
        .unwrap();
    assert_eq!(
        session
            .prepare_create_credential(
                &mut worker,
                OWNER,
                generation,
                request(1, 1),
                form(CredentialPassword::Generated(&stale))
            )
            .err(),
        Some(TransactionError::StalePasswordDraft)
    );
    assert_eq!(model.events(), events);
    session.set_deadline_for_test(std::time::Instant::now());
    assert_eq!(
        session
            .prepare_create_credential(
                &mut worker,
                OWNER,
                generation,
                request(1, 1),
                form(CredentialPassword::Provided("x"))
            )
            .err(),
        Some(TransactionError::Locked)
    );
    assert!(session.password_draft.is_none());
    session.lock();
    assert_eq!(
        session
            .prepare_create_credential(
                &mut worker,
                OWNER,
                generation,
                request(1, 1),
                form(CredentialPassword::Keep)
            )
            .err(),
        Some(TransactionError::Locked)
    );
    let (mut recovery, mut recovery_worker) =
        Committer::open_recovery(Model::new(Some(fixture())), OWNER, paper()).unwrap();
    assert_eq!(
        recovery
            .prepare_create_credential(
                &mut recovery_worker,
                OWNER,
                recovery.generation(),
                request(1, 1),
                form(CredentialPassword::Provided("x"))
            )
            .err(),
        Some(TransactionError::Locked)
    );
    let (mut enrollment, mut enrollment_worker) =
        Committer::begin_create(Model::new(None), OWNER).unwrap();
    assert_eq!(
        enrollment
            .prepare_create_credential(
                &mut enrollment_worker,
                OWNER,
                enrollment.generation(),
                request(1, 0),
                form(CredentialPassword::Provided("x"))
            )
            .err(),
        Some(TransactionError::Locked)
    );
}

#[test]
fn failed_creation_deduplicates_and_changed_form_or_keep_choice_never_replays() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    session
        .prepare_create_credential(
            &mut worker,
            OWNER,
            generation,
            request(1, 1),
            form(CredentialPassword::Provided("")),
        )
        .unwrap();
    model.fault(Fault::Create);
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Failed(StorageError::CreateStage)
    );
    let events = model.events();
    let duplicate = session
        .prepare_create_credential(
            &mut worker,
            OWNER,
            generation,
            request(1, 1),
            form(CredentialPassword::Provided("")),
        )
        .unwrap();
    assert_eq!(
        worker.status(OWNER, &duplicate).unwrap(),
        OperationStatus::Failed(StorageError::CreateStage)
    );
    assert!(worker.advance().is_none());
    assert_eq!(model.events(), events);
    let mut changed = form(CredentialPassword::Provided(""));
    changed.notes = "different";
    assert_eq!(
        session
            .prepare_create_credential(&mut worker, OWNER, generation, request(1, 1), changed)
            .err(),
        Some(TransactionError::RequestReused)
    );
    assert_eq!(
        session
            .prepare_update_credential(
                &mut worker,
                OWNER,
                generation,
                request(1, 1),
                ID,
                form(CredentialPassword::Provided(""))
            )
            .err(),
        Some(TransactionError::RequestReused)
    );
    session
        .prepare_update_credential(
            &mut worker,
            OWNER,
            generation,
            request(2, 1),
            ID,
            form(CredentialPassword::Keep),
        )
        .unwrap();
    model.fault(Fault::Create);
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Failed(StorageError::CreateStage)
    );
    assert_eq!(
        session
            .prepare_update_credential(
                &mut worker,
                OWNER,
                generation,
                request(2, 1),
                ID,
                form(CredentialPassword::Provided(""))
            )
            .err(),
        Some(TransactionError::RequestReused)
    );
    session
        .prepare_create_credential(
            &mut worker,
            OWNER,
            generation,
            request(3, 1),
            form(CredentialPassword::Provided("")),
        )
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    assert_eq!(
        vault::unlock_with_master(&model.live().unwrap(), PASSWORD)
            .unwrap()
            .snapshot()
            .credentials()
            .len(),
        3
    );
}

#[test]
fn uncertain_creation_requires_reconciliation_before_retry_or_reload() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for (fault, committed) in [(Fault::UnknownBefore, false), (Fault::UnknownAfter, true)] {
        let (model, mut session, mut worker) = opened();
        let generation = session.generation();
        let operation = session
            .prepare_create_credential(
                &mut worker,
                OWNER,
                generation,
                request(1, 1),
                form(CredentialPassword::Provided("synthetic")),
            )
            .unwrap();
        model.fault(fault);
        assert_eq!(complete(&mut worker), OperationStatus::OutcomeUnknown);
        assert_eq!(
            session
                .prepare_create_credential(
                    &mut worker,
                    OWNER,
                    generation,
                    request(2, 1),
                    form(CredentialPassword::Provided("synthetic"))
                )
                .err(),
            Some(TransactionError::Busy)
        );
        let status = if committed {
            OperationStatus::Committed { revision: 2 }
        } else {
            OperationStatus::Unchanged
        };
        assert_eq!(
            worker.reconcile_recovery(OWNER, &operation, paper()),
            Ok(status)
        );
        if committed {
            assert_eq!(
                session
                    .prepare_create_credential(
                        &mut worker,
                        OWNER,
                        generation,
                        request(2, 1),
                        form(CredentialPassword::Provided("synthetic"))
                    )
                    .err(),
                Some(TransactionError::ReloadRequired)
            );
            session.reload_master(&worker, OWNER, PASSWORD).unwrap();
        } else {
            session
                .prepare_create_credential(
                    &mut worker,
                    OWNER,
                    generation,
                    request(2, 1),
                    form(CredentialPassword::Provided("synthetic")),
                )
                .unwrap();
            assert_eq!(
                complete(&mut worker),
                OperationStatus::Committed { revision: 2 }
            );
        }
        assert_eq!(
            vault::unlock_with_master(&model.live().unwrap(), PASSWORD)
                .unwrap()
                .snapshot()
                .credentials()
                .len(),
            3
        );
    }
}

#[test]
fn creation_id_collisions_entropy_capacity_and_cancellation_fail_without_overwrite() {
    let snapshot = CredentialSnapshot::decode(include_bytes!(
        "../../../../tests/fixtures/credentials-v1.json"
    ))
    .unwrap();
    let fields = owned();
    let permit = Permit::new();
    let mut calls = 0;
    let created = created_snapshot_with(
        &snapshot,
        &fields,
        &permit,
        || {
            calls += 1;
            let mut bytes = [0; 16];
            if calls == 1 {
                bytes[15] = 1;
            }
            Ok(bytes)
        },
        || Ok(7),
    )
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(created.credentials().len(), 3);
    let new = created.credentials()[0].fields();
    assert_eq!(new.id, "00000000000000000000000000000000");
    assert_eq!(new.created_at, 7);
    assert_eq!(new.updated_at, 7);
    let mut collisions = 0;
    assert_eq!(
        created_snapshot_with(
            &snapshot,
            &fields,
            &permit,
            || {
                collisions += 1;
                let mut id = [0; 16];
                id[15] = 1;
                Ok(id)
            },
            || panic!("collision sampled clock")
        )
        .err(),
        Some(VaultError::Credentials(CredentialError::DuplicateId).into())
    );
    assert_eq!(collisions, 8);
    let unavailable =
        TransactionError::Vault(VaultError::Crypto(CryptoError::RandomnessUnavailable));
    assert_eq!(
        created_snapshot_with(
            &snapshot,
            &fields,
            &permit,
            || Err(unavailable),
            || panic!("failed entropy sampled clock")
        )
        .err(),
        Some(unavailable)
    );
    let full = CredentialSnapshot::new(
        (0..MAX_CREDENTIALS)
            .map(|i| {
                Credential::new(CredentialInput {
                    id: &format!("{i:032x}"),
                    title: "x",
                    username: "",
                    password: "",
                    url: "",
                    notes: "",
                    created_at: 0,
                    updated_at: 0,
                })
                .unwrap()
            })
            .collect(),
    )
    .unwrap();
    assert_eq!(
        created_snapshot_with(
            &full,
            &fields,
            &permit,
            || panic!("full vault sampled entropy"),
            || panic!("full vault sampled clock")
        )
        .err(),
        Some(VaultError::Credentials(CredentialError::LimitExceeded).into())
    );
    permit.cancel();
    assert_eq!(
        created_snapshot_with(
            &snapshot,
            &fields,
            &permit,
            || panic!("canceled creation entropy"),
            || panic!("canceled creation clock")
        )
        .err(),
        Some(TransactionError::Locked)
    );
}
