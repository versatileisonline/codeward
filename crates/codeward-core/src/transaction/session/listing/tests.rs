use super::*;
use crate::{
    credentials::{Credential, CredentialInput},
    crypto,
    transaction::tests::{
        complete, empty, fixture,
        model::{Fault, Model},
        opened, paper, request,
    },
};

const OWNER: CallerId = CallerId(7);

#[test]
fn credential_listing_is_bounded_and_returns_only_summary_fields_in_id_order() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let records = (0..=100)
        .rev()
        .map(|id| {
            Credential::new(CredentialInput {
                id: &format!("{id:032x}"),
                title: " Synthetic café ",
                username: " synthetic user ",
                password: "synthetic password",
                url: "https://example.test/",
                notes: "synthetic note",
                created_at: 0,
                updated_at: 0,
            })
            .unwrap()
        })
        .collect();
    let snapshot = CredentialSnapshot::new(records).unwrap();
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
    let deadline = session.inactivity_deadline();
    let events = model.events();
    for invalid in [0, 101, usize::MAX] {
        assert!(matches!(
            session.list_credentials(&worker, OWNER, generation, invalid, None),
            Err(TransactionError::InvalidPageSize)
        ));
    }
    let page = session
        .list_credentials(&worker, OWNER, generation, 100, None)
        .unwrap();
    assert_eq!(page.generation, generation);
    assert_eq!(page.revision, 2);
    assert_eq!(page.summaries.len(), 100);
    for (
        index,
        CredentialSummary {
            id,
            title,
            username,
        },
    ) in page.summaries.iter().enumerate()
    {
        assert_eq!(id.as_str(), format!("{index:032x}"));
        assert_eq!(title.as_str(), " Synthetic café ");
        assert_eq!(username.as_str(), " synthetic user ");
    }
    let last = session
        .list_credentials(&worker, OWNER, generation, 100, page.next.as_ref())
        .unwrap();
    assert_eq!(last.revision, 2);
    assert_eq!(last.summaries.len(), 1);
    assert_eq!(last.summaries[0].id.as_str(), format!("{:032x}", 100));
    assert!(last.next.is_none());
    assert_eq!(session.inactivity_deadline(), deadline);
    assert_eq!(model.events(), events);
}

#[test]
fn credential_listing_refuses_wrong_caller_opening_and_stale_generation() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, worker) = opened();
    let (_, mut other, other_worker) = opened();
    let generation = session.generation();
    let first = session
        .list_credentials(&worker, OWNER, generation, 1, None)
        .unwrap();
    let cursor = first.next.as_ref().expect("second synthetic record");
    assert!(matches!(
        session.list_credentials(&worker, CallerId(8), generation, 1, Some(cursor)),
        Err(TransactionError::Unauthorized)
    ));
    assert!(matches!(
        session.list_credentials(&other_worker, OWNER, generation, 1, None),
        Err(TransactionError::Unauthorized)
    ));
    assert!(matches!(
        other.list_credentials(&other_worker, OWNER, generation, 1, Some(cursor)),
        Err(TransactionError::Unauthorized)
    ));

    session.lock();
    assert!(matches!(
        session.list_credentials(&worker, OWNER, session.generation(), 1, None),
        Err(TransactionError::Locked)
    ));
    session
        .reload_master(&worker, OWNER, "synthetic password")
        .unwrap();
    assert!(matches!(
        session.list_credentials(&worker, OWNER, generation, 1, None),
        Err(TransactionError::Locked)
    ));
    assert!(matches!(
        session.list_credentials(&worker, OWNER, session.generation(), 1, Some(cursor)),
        Err(TransactionError::StaleCursor)
    ));
}

#[test]
fn credential_listing_refuses_recovery_and_enrollment_access() {
    let (mut recovery, worker) =
        Committer::open_recovery(Model::new(Some(fixture())), OWNER, paper()).unwrap();
    assert!(matches!(
        recovery.list_credentials(&worker, OWNER, recovery.generation(), 1, None),
        Err(TransactionError::Locked)
    ));
    let (mut enrollment, worker) = Committer::begin_create(Model::new(None), OWNER).unwrap();
    assert!(matches!(
        enrollment.list_credentials(&worker, OWNER, enrollment.generation(), 1, None),
        Err(TransactionError::Locked)
    ));
}

#[test]
fn credential_listing_waits_for_write_resolution_and_invalidates_cursor_on_commit() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    let generation = session.generation();
    let first = session
        .list_credentials(&worker, OWNER, generation, 1, None)
        .unwrap();
    let cursor = first.next.as_ref().expect("second synthetic record");
    let pending = session
        .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
        .unwrap();
    assert!(matches!(
        session.list_credentials(&worker, OWNER, generation, 1, Some(cursor)),
        Err(TransactionError::Busy)
    ));
    model.fault(Fault::UnknownBefore);
    assert_eq!(complete(&mut worker), OperationStatus::OutcomeUnknown);
    assert!(matches!(
        session.list_credentials(&worker, OWNER, generation, 1, Some(cursor)),
        Err(TransactionError::Busy)
    ));
    assert_eq!(
        worker.reconcile_recovery(OWNER, &pending, paper()),
        Ok(OperationStatus::Unchanged)
    );
    let unchanged = session
        .list_credentials(&worker, OWNER, generation, 1, Some(cursor))
        .unwrap();
    assert_eq!(unchanged.revision, 1);
    assert_eq!(unchanged.summaries.len(), 1);

    let saved = session
        .prepare_contents(&mut worker, OWNER, request(2, 1), &empty())
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    assert!(matches!(
        session.list_credentials(&worker, OWNER, generation, 1, Some(cursor)),
        Err(TransactionError::ReloadRequired)
    ));
    session
        .refresh_committed_contents(&worker, OWNER, &saved)
        .unwrap();
    assert!(matches!(
        session.list_credentials(&worker, OWNER, generation, 1, Some(cursor)),
        Err(TransactionError::StaleCursor)
    ));
    let current = session
        .list_credentials(&worker, OWNER, generation, 1, None)
        .unwrap();
    assert_eq!(current.revision, 2);
    assert!(current.summaries.is_empty());
    assert!(current.next.is_none());

    session
        .prepare_contents(&mut worker, OWNER, request(3, 2), &empty())
        .unwrap();
    model.0.lock().unwrap().live = Some(fixture());
    assert_eq!(complete(&mut worker), OperationStatus::Conflict);
    assert!(matches!(
        session.list_credentials(&worker, OWNER, generation, 1, None),
        Err(TransactionError::ReloadRequired)
    ));
}
