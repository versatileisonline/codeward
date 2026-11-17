use super::*;
use crate::credentials::CredentialError;

const ID: &str = "00000000000000000000000000000001";

fn form(password: CredentialPassword<'_>) -> CredentialForm<'_> {
    CredentialForm {
        title: " New credential ",
        username: " résumé ",
        password,
        url: " https://example.invalid/ ",
        notes: "notes\0\r\n🔒",
    }
}

fn committed(host: &mut Host, revision: u64) {
    assert!(matches!(next(host), Ok(HostCompletion::Transaction {
        outcome: Ok(OperationStatus::Committed { revision: actual }), refresh: Some(Ok(())), ..
    }) if actual == revision));
}

#[test]
fn headless_empty_vault_first_record_and_full_editor_round_trip_all_password_modes() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(None);
    let mut host = Host::begin_create(model.clone(), OWNER).unwrap();
    let code = host.recovery_code().unwrap();
    host.create(request(1, 0), PASSWORD, PASSWORD, &code)
        .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 1 }),
            ..
        })
    ));
    drop(code);
    authenticate(&mut host);
    let generation = host.generation();
    let deadline = host.inactivity_deadline();
    let before = model.live();
    host.create_credential(
        generation,
        request(2, 1),
        form(CredentialPassword::Provided(" entered\0password ")),
    )
    .unwrap();
    assert!(host.has_pending_work());
    assert_eq!(model.live(), before);
    committed(&mut host, 2);
    let page = host.list_credentials(generation, 100, None).unwrap();
    assert_eq!(page.summaries.len(), 1);
    let id = page.summaries[0].id.as_str();
    assert!(crate::credentials::valid_id(id));
    let original = host.get_credential_details(generation, 2, id).unwrap();
    let mut edited = form(CredentialPassword::Keep);
    edited.title = "Renamed";
    host.update_credential(generation, request(3, 2), id, edited)
        .unwrap();
    committed(&mut host, 3);
    assert_eq!(
        host.reveal_password(generation, 3, id)
            .unwrap()
            .password
            .as_str(),
        " entered\0password "
    );
    assert_eq!(
        host.get_credential_details(generation, 3, id)
            .unwrap()
            .title
            .as_str(),
        "Renamed"
    );
    host.update_credential(
        generation,
        request(4, 3),
        id,
        form(CredentialPassword::Provided("")),
    )
    .unwrap();
    committed(&mut host, 4);
    assert_eq!(
        host.reveal_password(generation, 4, id)
            .unwrap()
            .password
            .as_str(),
        ""
    );
    let draft = host
        .generate_password_draft(generation, 4, crate::password::PasswordOptions::default())
        .unwrap();
    let preview = host.reveal_password_draft(&draft).unwrap();
    host.update_credential(
        generation,
        request(5, 4),
        id,
        form(CredentialPassword::Generated(&draft)),
    )
    .unwrap();
    committed(&mut host, 5);
    assert_eq!(
        host.reveal_password(generation, 5, id).unwrap().password,
        preview.password
    );
    assert_eq!(
        host.get_credential_details(generation, 5, id)
            .unwrap()
            .created_at,
        original.created_at
    );
    assert_eq!(host.inactivity_deadline(), deadline);
    host.close();
    let reopened = crate::vault::unlock_with_master(&model.live().unwrap(), PASSWORD).unwrap();
    assert_eq!(reopened.snapshot().credentials().len(), 1);
    assert_eq!(reopened.snapshot().credentials()[0].fields().id, id);
    assert_eq!(
        reopened.snapshot().credentials()[0].fields().password,
        preview.password.as_str()
    );
}

#[test]
fn headless_generated_creation_failure_retains_draft_and_deduplicates_before_explicit_retry() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    authenticate(&mut host);
    let generation = host.generation();
    let draft = host
        .generate_password_draft(generation, 1, crate::password::PasswordOptions::default())
        .unwrap();
    model.fault(Fault::Create);
    host.create_credential(
        generation,
        request(1, 1),
        form(CredentialPassword::Generated(&draft)),
    )
    .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Failed(StorageError::CreateStage)),
            ..
        })
    ));
    assert_eq!(host.validate_password_draft(&draft), Ok(()));
    let events = model.events();
    host.create_credential(
        generation,
        request(1, 1),
        form(CredentialPassword::Generated(&draft)),
    )
    .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Failed(StorageError::CreateStage)),
            ..
        })
    ));
    assert_eq!(model.events(), events);
    let mut changed = form(CredentialPassword::Generated(&draft));
    changed.username = "different";
    host.create_credential(generation, request(1, 1), changed)
        .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::PreparationFailed {
            error: TransactionError::RequestReused,
            ..
        })
    ));
    assert_eq!(model.events(), events);
    host.create_credential(
        generation,
        request(2, 1),
        form(CredentialPassword::Generated(&draft)),
    )
    .unwrap();
    committed(&mut host, 2);
    assert_eq!(
        host.list_credentials(generation, 100, None)
            .unwrap()
            .summaries
            .len(),
        3
    );
    assert!(host.validate_password_draft(&draft).is_err());
}

#[test]
fn headless_form_errors_and_missing_targets_do_not_write_or_revoke_valid_access() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    assert_eq!(
        host.create_credential(
            host.generation(),
            request(1, 1),
            form(CredentialPassword::Keep)
        )
        .err(),
        Some(TransactionError::Locked.into())
    );
    authenticate(&mut host);
    let generation = host.generation();
    let events = model.events();
    assert_eq!(
        host.create_credential(generation, request(1, 1), form(CredentialPassword::Keep))
            .err(),
        Some(HostError::Transaction(
            VaultError::Credentials(CredentialError::InvalidCredential).into()
        ))
    );
    assert_eq!(
        host.update_credential(
            generation,
            request(1, 1),
            "bad",
            form(CredentialPassword::Keep)
        )
        .err(),
        Some(TransactionError::InvalidCredentialId.into())
    );
    host.update_credential(
        generation,
        request(1, 1),
        "00000000000000000000000000000000",
        form(CredentialPassword::Keep),
    )
    .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::PreparationFailed {
            error: TransactionError::CredentialNotFound,
            ..
        })
    ));
    assert_eq!(model.events(), events);
    host.begin_authentication(AccessProof::Paper(paper()))
        .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Authentication(Ok(())))
    ));
    assert_eq!(
        host.create_credential(
            host.generation(),
            request(2, 1),
            form(CredentialPassword::Provided("x"))
        )
        .err(),
        Some(TransactionError::Locked.into())
    );
    assert_eq!(
        host.update_credential(
            host.generation(),
            request(2, 1),
            ID,
            form(CredentialPassword::Keep)
        )
        .err(),
        Some(TransactionError::Locked.into())
    );
}

#[test]
fn headless_create_and_edit_uncertainty_require_proof_and_never_duplicate_records() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for creation in [false, true] {
        for (fault, changed) in [(Fault::UnknownBefore, false), (Fault::UnknownAfter, true)] {
            let model = Model::new(Some(fixture()));
            let mut host = Host::open(model.clone(), OWNER).unwrap();
            authenticate(&mut host);
            let generation = host.generation();
            model.fault(fault);
            if creation {
                host.create_credential(
                    generation,
                    request(1, 1),
                    form(CredentialPassword::Provided("replacement")),
                )
                .unwrap();
            } else {
                host.update_credential(
                    generation,
                    request(1, 1),
                    ID,
                    form(CredentialPassword::Provided("replacement")),
                )
                .unwrap();
            }
            let HostCompletion::Transaction {
                operation,
                outcome: Ok(OperationStatus::OutcomeUnknown),
                ..
            } = next(&mut host).unwrap()
            else {
                panic!("expected uncertainty");
            };
            assert_eq!(
                host.create_credential(
                    generation,
                    request(2, 1),
                    form(CredentialPassword::Provided("x"))
                )
                .err(),
                Some(TransactionError::Busy.into())
            );
            assert_eq!(
                host.update_credential(
                    generation,
                    request(2, 1),
                    ID,
                    form(CredentialPassword::Keep)
                )
                .err(),
                Some(TransactionError::Busy.into())
            );
            host.reconcile(&operation, AccessProof::Paper(paper()))
                .unwrap();
            let expected = if changed {
                OperationStatus::Committed { revision: 2 }
            } else {
                OperationStatus::Unchanged
            };
            assert!(
                matches!(next(&mut host), Ok(HostCompletion::Transaction { outcome: Ok(status), refresh: None, .. }) if status == expected)
            );
            if changed {
                assert_eq!(
                    host.create_credential(
                        generation,
                        request(2, 1),
                        form(CredentialPassword::Provided("x"))
                    )
                    .err(),
                    Some(TransactionError::ReloadRequired.into())
                );
                authenticate(&mut host);
            } else {
                if creation {
                    host.create_credential(
                        generation,
                        request(2, 1),
                        form(CredentialPassword::Provided("replacement")),
                    )
                    .unwrap();
                } else {
                    host.update_credential(
                        generation,
                        request(2, 1),
                        ID,
                        form(CredentialPassword::Provided("replacement")),
                    )
                    .unwrap();
                }
                committed(&mut host, 2);
            }
            assert_eq!(
                host.list_credentials(host.generation(), 100, None)
                    .unwrap()
                    .summaries
                    .len(),
                if creation { 3 } else { 2 }
            );
        }
    }
}

#[test]
fn headless_create_edit_workers_cancel_queued_work_and_finish_admitted_commits_locked() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for creation in [false, true] {
        for admitted in [false, true] {
            let model = Model::new(Some(fixture()));
            let mut host = Host::open(model.clone(), OWNER).unwrap();
            authenticate(&mut host);
            let generation = host.generation();
            let (checkpoint, resume) = if admitted {
                let (entered, checkpoint) = mpsc::channel();
                let (resume, wait) = mpsc::channel();
                model.0.lock().unwrap().barrier_block = Some((entered, wait));
                (checkpoint, resume)
            } else {
                host.pause_preparation()
            };
            if creation {
                host.create_credential(
                    generation,
                    request(1, 1),
                    form(CredentialPassword::Provided("synthetic")),
                )
                .unwrap();
            } else {
                host.update_credential(
                    generation,
                    request(1, 1),
                    ID,
                    form(CredentialPassword::Keep),
                )
                .unwrap();
            }
            await_storage(&mut host, checkpoint);
            host.lock();
            assert!(host.is_locked());
            resume.send(()).unwrap();
            let done = next(&mut host).unwrap();
            if admitted {
                assert!(matches!(
                    done,
                    HostCompletion::Transaction {
                        outcome: Ok(OperationStatus::Committed { revision: 2 }),
                        refresh: Some(Err(TransactionError::Locked)),
                        ..
                    }
                ));
            } else {
                assert!(matches!(
                    done,
                    HostCompletion::PreparationFailed {
                        error: TransactionError::Locked,
                        ..
                    }
                ));
                assert_eq!(model.live(), Some(fixture()));
            }
            assert!(host.is_locked());
        }
    }
}
