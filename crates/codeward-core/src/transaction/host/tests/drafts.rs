use super::*;
use crate::password::PasswordOptions;

#[test]
fn headless_drafts_are_scoped_explicit_and_revoked_on_security_lock_or_reopen() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    assert_eq!(
        host.generate_password_draft(host.generation(), 1, PasswordOptions::default())
            .err(),
        Some(TransactionError::Locked.into())
    );
    authenticate(&mut host);
    let generation = host.generation();
    let deadline = host.inactivity_deadline();
    let events = model.events();
    let draft = host
        .generate_password_draft(
            generation,
            1,
            PasswordOptions {
                length: 128,
                ..PasswordOptions::default()
            },
        )
        .unwrap();
    let reveal = host.reveal_password_draft(&draft).unwrap();
    assert_eq!(reveal.password.len(), 128);
    assert_eq!(host.inactivity_deadline(), deadline);
    assert_eq!(model.events(), events);
    assert_eq!(host.validate_password_draft(&draft), Ok(()));
    let replacement = host
        .generate_password_draft(generation, 1, PasswordOptions::default())
        .unwrap();
    assert_eq!(
        host.validate_password_draft(&draft),
        Err(TransactionError::StalePasswordDraft.into())
    );
    assert_eq!(
        host.discard_password_draft(&draft),
        Err(TransactionError::StalePasswordDraft.into())
    );
    assert_eq!(host.validate_password_draft(&replacement), Ok(()));
    drop(reveal); // A queued preview is rejected using its original reference.
    host.discard_password_draft(&replacement).unwrap();
    assert_eq!(
        host.reveal_password_draft(&draft).err(),
        Some(TransactionError::StalePasswordDraft.into())
    );
    let draft = host
        .generate_password_draft(generation, 1, PasswordOptions::default())
        .unwrap();
    host.observe_event(SessionEvent::Suspend);
    assert_eq!(
        host.reveal_password_draft(&draft).err(),
        Some(TransactionError::Locked.into())
    );
    authenticate(&mut host);
    assert_eq!(
        host.discard_password_draft(&draft),
        Err(TransactionError::Locked.into())
    );
    let draft = host
        .generate_password_draft(host.generation(), 1, PasswordOptions::default())
        .unwrap();
    host.close();
    let mut reopened = Host::open(model, OWNER).unwrap();
    authenticate(&mut reopened);
    assert_eq!(
        reopened.reveal_password_draft(&draft).err(),
        Some(TransactionError::Unauthorized.into())
    );
}

#[test]
fn headless_pending_and_failed_saves_preserve_drafts_but_commit_refresh_invalidates_them() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    authenticate(&mut host);
    let generation = host.generation();
    let draft = host
        .generate_password_draft(generation, 1, PasswordOptions::default())
        .unwrap();
    let original = host.reveal_password_draft(&draft).unwrap();
    model.fault(Fault::Create);
    host.save(request(1, 1), empty()).unwrap();
    assert_eq!(
        host.reveal_password_draft(&draft).err(),
        Some(TransactionError::Busy.into())
    );
    assert_eq!(
        host.discard_password_draft(&draft),
        Err(TransactionError::Busy.into())
    );
    assert_eq!(
        host.generate_password_draft(generation, 1, PasswordOptions::default())
            .err(),
        Some(TransactionError::Busy.into())
    );
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Failed(StorageError::CreateStage)),
            ..
        })
    ));
    assert_eq!(
        host.reveal_password_draft(&draft).unwrap().password,
        original.password
    );
    host.save(request(2, 1), empty()).unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 2 }),
            refresh: Some(Ok(())),
            ..
        })
    ));
    assert_eq!(
        host.reveal_password_draft(&draft).err(),
        Some(HostError::Transaction(VaultError::RevisionConflict.into()))
    );
    let current = host
        .generate_password_draft(generation, 2, PasswordOptions::default())
        .unwrap();
    assert!(host.reveal_password_draft(&current).is_ok());
    host.session.set_deadline_for_test(Instant::now());
    assert_eq!(
        host.reveal_password_draft(&current).err(),
        Some(TransactionError::Locked.into())
    );
    assert!(host.is_locked());
}

#[test]
fn headless_drafts_refuse_restricted_access() {
    let mut host = Host::open(Model::new(Some(fixture())), OWNER).unwrap();
    host.begin_authentication(AccessProof::Paper(paper()))
        .unwrap();
    assert_eq!(
        host.generate_password_draft(host.generation(), 1, PasswordOptions::default())
            .err(),
        Some(TransactionError::Busy.into())
    );
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Authentication(Ok(())))
    ));
    assert_eq!(
        host.generate_password_draft(host.generation(), 1, PasswordOptions::default())
            .err(),
        Some(TransactionError::Locked.into())
    );
    host.close();
    let mut host = Host::begin_create(Model::new(None), OWNER).unwrap();
    assert_eq!(
        host.generate_password_draft(host.generation(), 0, PasswordOptions::default())
            .err(),
        Some(TransactionError::Locked.into())
    );
}
