use super::*;
use crate::{password::PasswordOptions, vault};

const ID: &str = "00000000000000000000000000000001";
const OTHER: &str = "ffffffffffffffffffffffffffffffff";
const MISSING: &str = "00000000000000000000000000000000";

#[test]
fn headless_generated_save_and_delete_commit_refresh_and_reopen_actual_bytes() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    authenticate(&mut host);
    let generation = host.generation();
    let deadline = host.inactivity_deadline();
    let draft = host
        .generate_password_draft(generation, 1, PasswordOptions::default())
        .unwrap();
    let preview = host.reveal_password_draft(&draft).unwrap();
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    host.save_generated_password(request(1, 1), OTHER, &draft)
        .unwrap();
    assert!(host.has_pending_work());
    assert_eq!(model.live(), Some(fixture()));
    assert_eq!(
        host.validate_password_draft(&draft),
        Err(TransactionError::Busy.into())
    );
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 2 }),
            refresh: Some(Ok(())),
            ..
        })
    ));
    assert_eq!(
        host.reveal_password(generation, 2, OTHER).unwrap().password,
        preview.password
    );
    let detail = host.get_credential_details(generation, 2, OTHER).unwrap();
    assert_eq!(detail.created_at, 1);
    assert!(detail.updated_at >= before);
    assert_eq!(
        host.validate_password_draft(&draft),
        Err(HostError::Transaction(VaultError::RevisionConflict.into()))
    );
    host.delete_credential(generation, request(2, 2), ID)
        .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 3 }),
            refresh: Some(Ok(())),
            ..
        })
    ));
    assert_eq!(
        host.get_credential_details(generation, 3, ID).err(),
        Some(TransactionError::CredentialNotFound.into())
    );
    let reopened = vault::unlock_with_master(&model.live().unwrap(), PASSWORD).unwrap();
    assert_eq!(reopened.snapshot().credentials().len(), 1);
    let fields = reopened.snapshot().credentials()[0].fields();
    assert_eq!(fields.id, OTHER);
    assert_eq!(fields.password, preview.password.as_str());
    assert_eq!(host.inactivity_deadline(), deadline);
}

#[test]
fn headless_missing_targets_and_restricted_commands_fail_without_writes_or_lost_drafts() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    assert_eq!(
        host.delete_credential(host.generation(), request(1, 1), "bad")
            .err(),
        Some(TransactionError::Locked.into())
    );
    authenticate(&mut host);
    let generation = host.generation();
    let draft = host
        .generate_password_draft(generation, 1, PasswordOptions::default())
        .unwrap();
    let events = model.events();
    assert_eq!(
        host.save_generated_password(request(1, 1), "bad", &draft)
            .err(),
        Some(TransactionError::InvalidCredentialId.into())
    );
    assert_eq!(
        host.delete_credential(generation, request(1, 1), "bad")
            .err(),
        Some(TransactionError::InvalidCredentialId.into())
    );
    host.save_generated_password(request(1, 1), MISSING, &draft)
        .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::PreparationFailed {
            error: TransactionError::CredentialNotFound,
            ..
        })
    ));
    assert_eq!(host.validate_password_draft(&draft), Ok(()));
    host.delete_credential(generation, request(2, 1), MISSING)
        .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::PreparationFailed {
            error: TransactionError::CredentialNotFound,
            ..
        })
    ));
    assert_eq!(model.events(), events);
    assert_eq!(model.live(), Some(fixture()));
    host.begin_authentication(AccessProof::Paper(paper()))
        .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Authentication(Ok(())))
    ));
    assert_eq!(
        host.save_generated_password(request(3, 1), ID, &draft)
            .err(),
        Some(TransactionError::Locked.into())
    );
    assert_eq!(
        host.delete_credential(host.generation(), request(3, 1), ID)
            .err(),
        Some(TransactionError::Locked.into())
    );
}

#[test]
fn headless_failed_password_saves_deduplicate_without_replay_and_allow_explicit_retry() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    authenticate(&mut host);
    let draft = host
        .generate_password_draft(host.generation(), 1, PasswordOptions::default())
        .unwrap();
    model.fault(Fault::Create);
    host.save_generated_password(request(1, 1), ID, &draft)
        .unwrap();
    let HostCompletion::Transaction {
        operation,
        outcome: Ok(OperationStatus::Failed(StorageError::CreateStage)),
        ..
    } = next(&mut host).unwrap()
    else {
        panic!("expected definite failure");
    };
    assert_eq!(host.validate_password_draft(&draft), Ok(()));
    let events = model.events();
    host.save_generated_password(request(1, 1), ID, &draft)
        .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Failed(StorageError::CreateStage)),
            ..
        })
    ));
    assert_eq!(model.events(), events);
    host.delete_credential(host.generation(), request(1, 1), ID)
        .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::PreparationFailed {
            error: TransactionError::RequestReused,
            ..
        })
    ));
    assert_eq!(model.events(), events);
    host.save_generated_password(request(2, 1), ID, &draft)
        .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 2 }),
            refresh: Some(Ok(())),
            ..
        })
    ));
    assert_eq!(
        host.status(&operation),
        Ok(OperationStatus::Failed(StorageError::CreateStage))
    );
}

#[test]
fn headless_uncertainty_requires_reconciliation_without_replay_or_automatic_unlock() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for (fault, committed) in [(Fault::UnknownBefore, false), (Fault::UnknownAfter, true)] {
        let model = Model::new(Some(fixture()));
        let mut host = Host::open(model.clone(), OWNER).unwrap();
        authenticate(&mut host);
        let draft = host
            .generate_password_draft(host.generation(), 1, PasswordOptions::default())
            .unwrap();
        let preview = host.reveal_password_draft(&draft).unwrap();
        model.fault(fault);
        host.save_generated_password(request(1, 1), ID, &draft)
            .unwrap();
        let HostCompletion::Transaction {
            operation,
            outcome: Ok(OperationStatus::OutcomeUnknown),
            ..
        } = next(&mut host).unwrap()
        else {
            panic!("expected uncertainty");
        };
        assert_eq!(
            host.save_generated_password(request(2, 1), ID, &draft)
                .err(),
            Some(TransactionError::Busy.into())
        );
        assert_eq!(
            host.delete_credential(host.generation(), request(2, 1), ID)
                .err(),
            Some(TransactionError::Busy.into())
        );
        host.reconcile(&operation, AccessProof::Paper(paper()))
            .unwrap();
        let expected = if committed {
            OperationStatus::Committed { revision: 2 }
        } else {
            OperationStatus::Unchanged
        };
        assert!(
            matches!(next(&mut host), Ok(HostCompletion::Transaction { outcome: Ok(status), refresh: None, .. }) if status == expected)
        );
        if committed {
            assert_eq!(
                host.validate_password_draft(&draft),
                Err(TransactionError::ReloadRequired.into())
            );
            authenticate(&mut host);
            assert_eq!(
                host.reveal_password(host.generation(), 2, ID)
                    .unwrap()
                    .password,
                preview.password
            );
        } else {
            assert_eq!(host.validate_password_draft(&draft), Ok(()));
            host.save_generated_password(request(2, 1), ID, &draft)
                .unwrap();
            assert!(matches!(
                next(&mut host),
                Ok(HostCompletion::Transaction {
                    outcome: Ok(OperationStatus::Committed { revision: 2 }),
                    refresh: Some(Ok(())),
                    ..
                })
            ));
        }
    }
}

#[test]
fn headless_targeted_preparation_is_revocable_and_admitted_saves_finish_locked() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for deletion in [false, true] {
        for admitted in [false, true] {
            let model = Model::new(Some(fixture()));
            let mut host = Host::open(model.clone(), OWNER).unwrap();
            authenticate(&mut host);
            let draft = host
                .generate_password_draft(host.generation(), 1, PasswordOptions::default())
                .unwrap();
            let (checkpoint, resume) = if admitted {
                let (entered, checkpoint) = mpsc::channel();
                let (resume, wait) = mpsc::channel();
                model.0.lock().unwrap().barrier_block = Some((entered, wait));
                (checkpoint, resume)
            } else {
                host.pause_preparation()
            };
            if deletion {
                host.delete_credential(host.generation(), request(1, 1), ID)
                    .unwrap();
            } else {
                host.save_generated_password(request(1, 1), ID, &draft)
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
            assert_eq!(
                host.reveal_password_draft(&draft).err(),
                Some(TransactionError::Locked.into())
            );
        }
    }
}
