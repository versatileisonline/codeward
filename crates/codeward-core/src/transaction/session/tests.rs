use super::*;
use crate::{
    crypto,
    transaction::tests::{complete, empty, fixture, model::Fault, opened, paper, request},
};

const OWNER: CallerId = CallerId(7);

#[test]
fn committed_contents_refresh_preserves_scope_deadline_and_request_history() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    let deadline = session.inactivity_deadline();
    let operation = session
        .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    let events = model.events();
    for _ in 0..2 {
        session
            .refresh_committed_contents(&worker, OWNER, &operation)
            .unwrap();
    }
    assert_eq!(model.events(), events);
    assert_eq!(session.generation(), generation);
    assert_eq!(session.inactivity_deadline(), deadline);
    let Some(Access::Master(access)) = session.access.as_ref() else {
        panic!("expected ordinary access");
    };
    assert_eq!(access.revision(), 2);
    assert!(access.snapshot().credentials().is_empty());

    let duplicate = session
        .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
        .unwrap();
    assert_eq!(
        worker.status(OWNER, &duplicate).unwrap(),
        OperationStatus::Committed { revision: 2 }
    );
    assert!(worker.advance().is_none());
    assert_eq!(
        session
            .prepare_contents(&mut worker, OWNER, request(1, 2), &empty())
            .err(),
        Some(TransactionError::RequestReused)
    );
    assert_eq!(
        session
            .prepare_contents(&mut worker, OWNER, request(2, 1), &empty())
            .err(),
        Some(VaultError::RevisionConflict.into())
    );
    let next = session
        .prepare_contents(&mut worker, OWNER, request(2, 2), &empty())
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 3 }
    );
    assert_eq!(
        session.refresh_committed_contents(&worker, OWNER, &operation),
        Err(TransactionError::ReloadRequired)
    );
    session
        .refresh_committed_contents(&worker, OWNER, &next)
        .unwrap();
}

#[test]
fn committed_contents_refresh_requires_current_caller_scope_and_generation() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, mut worker) = opened();
    let (_, mut other_session, other_worker) = opened();
    let operation = session
        .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
        .unwrap();
    assert_eq!(
        session.refresh_committed_contents(&worker, CallerId(8), &operation),
        Err(TransactionError::Unauthorized)
    );
    assert_eq!(
        session.refresh_committed_contents(&other_worker, OWNER, &operation),
        Err(TransactionError::Unauthorized)
    );
    assert_eq!(
        other_session.refresh_committed_contents(&other_worker, OWNER, &operation),
        Err(TransactionError::Unauthorized)
    );
    for _ in 0..2 {
        worker.advance();
    }
    session.lock();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    assert_eq!(
        session.refresh_committed_contents(&worker, OWNER, &operation),
        Err(TransactionError::Locked)
    );
    assert!(session.is_locked());
    session
        .reload_master(&worker, OWNER, "synthetic password")
        .unwrap();
    assert_eq!(
        session.refresh_committed_contents(&worker, OWNER, &operation),
        Err(TransactionError::Locked)
    );
    assert!(!session.is_locked());
}

#[test]
fn committed_contents_refresh_waits_for_reconciliation_and_refuses_conflict() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    for (id, fault, outcome) in [
        (1, Fault::UnknownBefore, OperationStatus::Unchanged),
        (
            2,
            Fault::Barrier,
            OperationStatus::Committed { revision: 2 },
        ),
    ] {
        let operation = session
            .prepare_contents(&mut worker, OWNER, request(id, 1), &empty())
            .unwrap();
        assert_eq!(
            session.refresh_committed_contents(&worker, OWNER, &operation),
            Err(TransactionError::Busy)
        );
        model.fault(fault);
        assert_eq!(complete(&mut worker), OperationStatus::OutcomeUnknown);
        assert_eq!(
            session.refresh_committed_contents(&worker, OWNER, &operation),
            Err(TransactionError::Busy)
        );
        assert_eq!(
            worker.reconcile_recovery(OWNER, &operation, paper()),
            Ok(outcome)
        );
        if outcome == OperationStatus::Unchanged {
            assert_eq!(
                session.refresh_committed_contents(&worker, OWNER, &operation),
                Err(TransactionError::ReloadRequired)
            );
        } else {
            session
                .refresh_committed_contents(&worker, OWNER, &operation)
                .unwrap();
            let next = session
                .prepare_contents(&mut worker, OWNER, request(3, 2), &empty())
                .unwrap();
            model.0.lock().unwrap().live = Some(fixture());
            assert_eq!(complete(&mut worker), OperationStatus::Conflict);
            for reference in [&operation, &next] {
                assert_eq!(
                    session.refresh_committed_contents(&worker, OWNER, reference),
                    Err(TransactionError::ReloadRequired)
                );
            }
        }
    }
}
