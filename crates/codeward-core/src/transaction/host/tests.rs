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
        let operation = host.save(request(1, 1), &empty()).unwrap();
        checkpoint.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(host.poll().unwrap().is_none());
        assert_eq!(host.status(&operation), Err(TransactionError::Busy.into()));
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
    host.save(request(1, 1), &empty()).unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 2 }),
            refresh: Some(Ok(())),
            ..
        })
    ));
    let events = model.events();
    let duplicate = host.save(request(1, 1), &empty()).unwrap();
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
    let operation = host.save(request(1, 1), &empty()).unwrap();
    assert!(matches!(
        next(&mut host),
        Ok(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::OutcomeUnknown),
            refresh: None,
            ..
        })
    ));
    assert!(matches!(
        host.save(request(2, 1), &empty()),
        Err(HostError::Transaction(TransactionError::Busy))
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
    let operation = host.save(request(1, 1), &empty()).unwrap();
    checkpoint.recv_timeout(Duration::from_secs(5)).unwrap();
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
    assert_eq!(
        reopened.status(&operation),
        Ok(OperationStatus::Unavailable)
    );
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
    let operation = host.save(request(1, 1), &empty()).unwrap();
    checkpoint.recv_timeout(Duration::from_secs(5)).unwrap();
    // A dropped test gate panics the worker after publication, before barriers.
    drop(resume);
    assert!(matches!(next(&mut host), Err(HostError::WorkerUnavailable)));
    assert!(host.is_locked());
    assert_eq!(host.status(&operation), Err(HostError::WorkerUnavailable));
    assert_eq!(model.0.lock().unwrap().durable, Some(fixture()));
    let mut reopened = Host::open(model, OWNER).unwrap();
    assert_eq!(
        reopened.status(&operation),
        Ok(OperationStatus::Unavailable)
    );
}
