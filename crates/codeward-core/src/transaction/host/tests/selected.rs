use super::*;

const ID: &str = "00000000000000000000000000000001";

#[test]
fn headless_selected_reads_are_explicit_scoped_and_expire_with_session() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    assert_eq!(
        host.get_credential_details(host.generation(), 1, ID).err(),
        Some(TransactionError::Locked.into())
    );
    authenticate(&mut host);
    let generation = host.generation();
    let deadline = host.inactivity_deadline();
    let events = model.events();
    let details = host.get_credential_details(generation, 1, ID).unwrap();
    assert_eq!(details.id.as_str(), ID);
    assert_eq!(details.title.as_str(), "Synthetic service");
    assert_eq!(details.username.as_str(), "  résumé  ");
    let reveal = host.reveal_password(generation, 1, ID).unwrap();
    assert_eq!(reveal.password.as_str(), " A\0\"\\🔒é ");
    assert!(reveal.expires_at <= deadline.unwrap());
    assert_eq!(host.inactivity_deadline(), deadline);
    assert_eq!(model.events(), events);
    assert_eq!(
        host.reveal_password(generation + 1, 1, ID).err(),
        Some(TransactionError::Locked.into())
    );
    drop(details);
    drop(reveal);
    host.session.set_deadline_for_test(Instant::now());
    assert_eq!(
        host.get_credential_details(generation, 1, ID).err(),
        Some(TransactionError::Locked.into())
    );
    assert_eq!(
        host.reveal_password(generation, 1, ID).err(),
        Some(TransactionError::Locked.into())
    );
    assert!(host.is_locked());
}

#[test]
fn headless_selected_reads_refuse_recovery_and_enrollment() {
    let mut host = Host::open(Model::new(Some(fixture())), OWNER).unwrap();
    host.begin_authentication(AccessProof::Paper(paper()))
        .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Authentication(Ok(())))
    ));
    assert_eq!(
        host.get_credential_details(host.generation(), 1, ID).err(),
        Some(TransactionError::Locked.into())
    );
    assert_eq!(
        host.reveal_password(host.generation(), 1, ID).err(),
        Some(TransactionError::Locked.into())
    );
    host.close();
    let mut host = Host::begin_create(Model::new(None), OWNER).unwrap();
    assert_eq!(
        host.get_credential_details(host.generation(), 0, ID).err(),
        Some(TransactionError::Locked.into())
    );
    assert_eq!(
        host.reveal_password(host.generation(), 0, ID).err(),
        Some(TransactionError::Locked.into())
    );
}

#[test]
fn headless_selected_reads_wait_for_save_refresh_and_reject_old_selection() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let mut host = Host::open(Model::new(Some(fixture())), OWNER).unwrap();
    authenticate(&mut host);
    let generation = host.generation();
    host.save(request(1, 1), empty()).unwrap();
    assert_eq!(
        host.get_credential_details(generation, 1, ID).err(),
        Some(TransactionError::Busy.into())
    );
    assert_eq!(
        host.reveal_password(generation, 1, ID).err(),
        Some(TransactionError::Busy.into())
    );
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 2 }),
            refresh: Some(Ok(())),
            ..
        })
    ));
    for old in [0, 1] {
        let expected = HostError::Transaction(VaultError::RevisionConflict.into());
        assert_eq!(
            host.get_credential_details(generation, old, ID).err(),
            Some(expected)
        );
        assert_eq!(
            host.reveal_password(generation, old, ID).err(),
            Some(expected)
        );
    }
    assert_eq!(
        host.get_credential_details(generation, 2, ID).err(),
        Some(TransactionError::CredentialNotFound.into())
    );
    assert_eq!(
        host.reveal_password(generation, 2, ID).err(),
        Some(TransactionError::CredentialNotFound.into())
    );
}
