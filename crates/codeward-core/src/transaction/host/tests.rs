use std::time::Duration;

use super::*;
use crate::{
    crypto,
    transaction::tests::{
        empty, fixture,
        model::{Fault, Lease, Model},
        paper, request,
    },
};

const OWNER: CallerId = CallerId(7);
const PASSWORD: &str = "synthetic password";
type Host = HeadlessVault<Lease>;

fn next(host: &mut Host) -> Result<HostCompletion, HostError> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(completion) = host.poll()? {
            return Ok(completion);
        }
        assert!(Instant::now() < until, "headless worker did not return");
        thread::sleep(Duration::from_millis(1));
    }
}

fn authenticate(host: &mut Host) {
    host.begin_authentication(AccessProof::Master(PASSWORD))
        .unwrap();
    assert!(matches!(
        next(host),
        Ok(HostCompletion::Authentication(Ok(())))
    ));
}

fn await_storage(host: &mut Host, checkpoint: Receiver<()>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while checkpoint.try_recv() != Ok(()) {
        assert!(Instant::now() < deadline, "storage was never reached");
        assert!(host.poll().unwrap().is_none());
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn headless_host_discards_returned_authentication_on_lock() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let mut host = Host::open(Model::new(Some(fixture())), OWNER).unwrap();
    let (session, worker, caller) = host.idle().unwrap();
    let job = session
        .begin_master_authentication(worker, caller, PASSWORD)
        .unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    assert!(sender.send(job.run()).is_ok());
    host.pending = Some(Pending::Authentication(receiver));
    let generation = host.generation();
    assert!(host.observe_event(SessionEvent::OsSessionLocked));
    assert!(host.pending.is_none());
    assert!(host.poll().unwrap().is_none());
    assert!(host.is_locked());
    assert!(host.generation() > generation);
    assert!(host.inactivity_deadline().is_none());
    authenticate(&mut host);
    assert!(!host.is_locked());
    assert!(host.inactivity_deadline().is_some());
}

#[test]
fn headless_host_lock_cancels_staging_and_preserves_an_admitted_commit() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for admitted in [false, true] {
        let model = Model::new(Some(fixture()));
        let mut host = Host::open(model.clone(), OWNER).unwrap();
        authenticate(&mut host);
        let (entered, checkpoint) = mpsc::channel();
        let (resume, wait) = mpsc::channel();
        if admitted {
            model.0.lock().unwrap().barrier_block = Some((entered, wait));
        } else {
            model.0.lock().unwrap().stage_block = Some((entered, wait));
        }
        host.save(request(1, 1), empty()).unwrap();
        await_storage(&mut host, checkpoint);
        assert!(host.poll().unwrap().is_none());
        host.lock();
        assert!(host.is_locked());
        resume.send(()).unwrap();
        let done = next(&mut host).unwrap();
        if admitted {
            let HostCompletion::Transaction {
                operation,
                outcome: Ok(OperationStatus::Committed { revision: 2 }),
                refresh: Some(Err(TransactionError::Locked)),
            } = done
            else {
                panic!("expected an admitted commit");
            };
            assert_eq!(
                host.status(&operation),
                Ok(OperationStatus::Committed { revision: 2 })
            );
        } else {
            assert!(matches!(
                done,
                HostCompletion::Transaction {
                    outcome: Ok(OperationStatus::Canceled),
                    refresh: None,
                    ..
                }
            ));
            assert_eq!(model.live(), Some(fixture()));
        }
        assert!(host.is_locked());
        authenticate(&mut host);
    }
}

#[test]
fn headless_host_refreshes_committed_saves_and_deduplicates_requests() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    authenticate(&mut host);
    let generation = host.generation();
    let deadline = host.inactivity_deadline();
    host.save(request(1, 1), empty()).unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 2 }),
            refresh: Some(Ok(())),
            ..
        })
    ));
    let events = model.events();
    host.save(request(1, 1), empty()).unwrap();
    let HostCompletion::Transaction {
        operation: duplicate,
        outcome: Ok(OperationStatus::Committed { revision: 2 }),
        refresh: None,
    } = next(&mut host).unwrap()
    else {
        panic!("expected the recorded commit status");
    };
    assert_eq!(
        host.status(&duplicate),
        Ok(OperationStatus::Committed { revision: 2 })
    );
    assert!(host.poll().unwrap().is_none());
    assert_eq!(model.events(), events);
    assert_eq!(host.generation(), generation);
    assert_eq!(host.inactivity_deadline(), deadline);
    let page = host.list_credentials(generation, 10, None).unwrap();
    assert_eq!(page.revision, 2);
    assert!(page.summaries.is_empty());
}

#[test]
fn headless_host_reconciles_unknown_without_restoring_access() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    authenticate(&mut host);
    model.fault(Fault::Barrier);
    host.save(request(1, 1), empty()).unwrap();
    let HostCompletion::Transaction {
        operation,
        outcome: Ok(OperationStatus::OutcomeUnknown),
        refresh: None,
    } = next(&mut host).unwrap()
    else {
        panic!("expected uncertain publication");
    };
    host.save(request(2, 1), empty()).unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::PreparationFailed {
            error: TransactionError::Busy,
            ..
        })
    ));
    host.lock();
    host.reconcile(
        &operation,
        AccessProof::Master("incorrect synthetic password"),
    )
    .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Err(_),
            ..
        })
    ));
    assert_eq!(host.status(&operation), Ok(OperationStatus::OutcomeUnknown));
    host.reconcile(&operation, AccessProof::Paper(paper()))
        .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 2 }),
            refresh: None,
            ..
        })
    ));
    assert!(host.is_locked());
    authenticate(&mut host);
    assert_eq!(
        host.list_credentials(host.generation(), 10, None)
            .unwrap()
            .revision,
        2
    );
}

#[test]
fn headless_host_close_releases_control_before_admitted_writer_finishes() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    authenticate(&mut host);
    let (entered, checkpoint) = mpsc::channel();
    let (resume, wait) = mpsc::channel();
    model.0.lock().unwrap().barrier_block = Some((entered, wait));
    host.save(request(1, 1), empty()).unwrap();
    await_storage(&mut host, checkpoint);
    host.close();
    assert!(matches!(
        Host::open(model.clone(), OWNER),
        Err(HostError::Transaction(TransactionError::Storage(
            StorageError::WriterUnavailable
        )))
    ));
    resume.send(()).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    let mut reopened = loop {
        match Host::open(model.clone(), OWNER) {
            Ok(host) => break host,
            Err(HostError::Transaction(TransactionError::Storage(
                StorageError::WriterUnavailable,
            ))) => {
                assert!(Instant::now() < until);
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("{error}"),
        }
    };
    authenticate(&mut reopened);
    assert_eq!(
        reopened
            .list_credentials(reopened.generation(), 10, None)
            .unwrap()
            .revision,
        2
    );
}

#[test]
fn headless_host_lost_writer_revokes_and_requires_reopen() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    authenticate(&mut host);
    let (entered, checkpoint) = mpsc::channel();
    let (resume, wait) = mpsc::channel();
    model.0.lock().unwrap().barrier_block = Some((entered, wait));
    host.save(request(1, 1), empty()).unwrap();
    await_storage(&mut host, checkpoint);
    // A dropped test gate panics the worker after publication, before barriers.
    drop(resume);
    assert!(matches!(next(&mut host), Err(HostError::WorkerUnavailable)));
    assert!(host.is_locked());
    assert_eq!(
        host.begin_authentication(AccessProof::Master(PASSWORD)),
        Err(HostError::WorkerUnavailable)
    );
    assert_eq!(model.0.lock().unwrap().durable, Some(fixture()));
    assert!(Host::open(model, OWNER).unwrap().is_locked());
}

#[test]
fn prepared_candidate_canceled_before_storage_does_not_block_a_fresh_save() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    authenticate(&mut host);
    let events = model.events();
    host.save(request(1, 1), empty()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(host.pending, Some(Pending::Prepared { .. })) {
        assert!(Instant::now() < deadline);
        assert!(host.poll().unwrap().is_none());
        thread::sleep(Duration::from_millis(1));
    }
    host.lock();
    let HostCompletion::Transaction {
        operation,
        outcome: Ok(OperationStatus::Canceled),
        refresh: None,
    } = next(&mut host).unwrap()
    else {
        panic!("expected canceled preparation");
    };
    assert_eq!(host.status(&operation), Ok(OperationStatus::Canceled));
    assert_eq!(model.events(), events);
    assert!(host.is_locked());

    authenticate(&mut host);
    host.save(request(1, 1), empty()).unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::PreparationFailed {
            error: TransactionError::RequestReused,
            ..
        })
    ));
    host.save(request(2, 1), empty()).unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 2 }),
            ..
        })
    ));
}

#[test]
fn queued_recovery_candidate_cannot_restore_access_after_lock() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let mut host = Host::open(model.clone(), OWNER).unwrap();
    host.begin_authentication(AccessProof::Paper(paper()))
        .unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Authentication(Ok(())))
    ));
    let events = model.events();

    host.replace_master_password(
        request(1, 1),
        "synthetic replacement",
        "synthetic replacement",
    )
    .unwrap();
    let Some(Pending::Preparation(receiver)) = host.pending.take() else {
        panic!("expected preparation worker");
    };
    let done = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(done.result, Ok(PreparedWrite::New(_))));
    assert!(done.authorization.is_some());
    let (sender, receiver) = mpsc::sync_channel(1);
    assert!(sender.send(done).is_ok());
    host.pending = Some(Pending::Preparation(receiver));

    host.lock();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Canceled),
            ..
        })
    ));
    assert!(host.is_locked());
    assert_eq!(host.inactivity_deadline(), None);
    assert_eq!(model.events(), events);
    assert_eq!(model.live(), Some(fixture()));
    authenticate(&mut host);
}

#[test]
fn creation_failure_preserves_enrollment_and_known_retry_does_not_rewrite() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(None);
    let mut host = Host::begin_create(model.clone(), OWNER).unwrap();
    let paper = host.recovery_code().unwrap();
    let request = request(1, 0);
    host.create(request, PASSWORD, "different", &paper).unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::PreparationFailed {
            request: id,
            error: TransactionError::Vault(VaultError::PasswordConfirmationMismatch),
        }) if id == request.id
    ));
    assert_eq!(*host.recovery_code().unwrap(), *paper);
    assert!(model.live().is_none());

    host.create(request, PASSWORD, PASSWORD, &paper).unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 1 }),
            ..
        })
    ));
    assert_eq!(
        host.recovery_code(),
        Err(TransactionError::Vault(VaultError::EnrollmentFinished).into())
    );
    let events = model.events();
    host.create(request, PASSWORD, PASSWORD, &paper).unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 1 }),
            refresh: None,
            ..
        })
    ));
    assert_eq!(model.events(), events);
}
