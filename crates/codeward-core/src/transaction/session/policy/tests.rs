use std::{sync::mpsc, thread};

use super::*;
use crate::{
    crypto,
    transaction::tests::{
        complete, empty, fixture,
        model::{Event, Fault, Model},
        opened, paper, request,
    },
};

const OWNER: CallerId = CallerId(7);
const PASSWORD: &str = "synthetic password";

fn expire_now(session: &mut Session) {
    // Only a private test observation: drive the production gate without sleeping.
    session.inactivity.deadline = Some(Instant::now());
}

#[test]
fn committed_contents_refresh_at_expiry_revokes_access_without_undoing_save() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, mut worker) = opened();
    let operation = session
        .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
        .unwrap();
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    let generation = session.generation();
    expire_now(&mut session);
    assert_eq!(
        session.refresh_committed_contents(&worker, OWNER, &operation),
        Err(TransactionError::Locked)
    );
    assert!(session.is_locked());
    assert_eq!(session.generation(), generation + 1);
    assert_eq!(session.inactivity_deadline(), None);
    assert_eq!(
        worker.status(OWNER, &operation),
        Ok(OperationStatus::Committed { revision: 2 })
    );
}

#[test]
fn exact_deadline_expires_once_and_each_allowed_activity_refreshes_before_expiry() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    assert_eq!(INACTIVITY_TIMEOUT, Duration::from_secs(300));
    for activity in [
        SessionActivity::Keyboard,
        SessionActivity::Pointer,
        SessionActivity::AssistiveActivation,
    ] {
        let (_, mut session, _) = opened();
        let reference = session.activity_reference(OWNER).unwrap();
        let start = Instant::now();
        session.inactivity = Inactivity::new(start, true).unwrap();
        let deadline = start + INACTIVITY_TIMEOUT;
        assert_eq!(session.inactivity_deadline(), Some(deadline));
        let before = deadline - Duration::from_nanos(1);
        assert!(!session.poll_inactivity_at(before));
        session
            .record_activity_at(OWNER, &reference, activity, before)
            .unwrap();
        let refreshed = before + INACTIVITY_TIMEOUT;
        assert_eq!(session.inactivity_deadline(), Some(refreshed));
        assert!(!session.poll_inactivity_at(deadline));
        let generation = session.generation();
        assert!(session.poll_inactivity_at(refreshed));
        assert!(session.is_locked());
        assert_eq!(session.generation(), generation + 1);
        assert_eq!(session.inactivity_deadline(), None);
        assert!(!session.poll_inactivity_at(refreshed + Duration::from_secs(1)));
        assert_eq!(session.generation(), generation + 1);
    }
}

#[test]
fn activity_at_deadline_or_later_cannot_revive_expired_access() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for late in [Duration::ZERO, Duration::from_secs(1)] {
        let (_, mut session, _) = opened();
        let reference = session.activity_reference(OWNER).unwrap();
        let deadline = session.inactivity_deadline().unwrap();
        assert_eq!(
            session.record_activity_at(
                OWNER,
                &reference,
                SessionActivity::Keyboard,
                deadline + late
            ),
            Err(TransactionError::Locked)
        );
        assert!(session.is_locked());
        assert!(session.inactivity_deadline().is_none());
    }
}

#[test]
fn caller_scope_and_generation_prevent_stale_or_cross_vault_activity() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, worker) = opened();
    let reference = session.activity_reference(OWNER).unwrap();
    let deadline = session.inactivity_deadline();
    assert!(matches!(
        session.activity_reference(CallerId(8)),
        Err(TransactionError::Unauthorized)
    ));
    assert_eq!(
        session.record_activity(CallerId(8), &reference, SessionActivity::Pointer),
        Err(TransactionError::Unauthorized)
    );
    assert_eq!(session.inactivity_deadline(), deadline);
    session.lock();
    session.reload_master(&worker, OWNER, PASSWORD).unwrap();
    let deadline = session.inactivity_deadline();
    assert_eq!(
        session.record_activity(OWNER, &reference, SessionActivity::AssistiveActivation),
        Err(TransactionError::Locked)
    );
    assert!(!session.is_locked());
    assert_eq!(session.inactivity_deadline(), deadline);
    let (_, mut other, _) = opened();
    let other_deadline = other.inactivity_deadline();
    assert_eq!(
        other.record_activity(OWNER, &reference, SessionActivity::Keyboard),
        Err(TransactionError::Unauthorized)
    );
    assert_eq!(other.inactivity_deadline(), other_deadline);
}

#[test]
fn focus_loss_and_background_progress_do_not_refresh_or_lock_before_deadline() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, _) = opened();
    let deadline = session.inactivity_deadline();
    for event in [SessionEvent::FocusLost, SessionEvent::BackgroundProgress] {
        assert!(!session.observe_event(event));
        assert!(!session.is_locked());
        assert_eq!(session.inactivity_deadline(), deadline);
    }
    expire_now(&mut session);
    assert!(session.observe_event(SessionEvent::BackgroundProgress));
    assert!(session.is_locked());
}

#[test]
fn every_security_event_revokes_including_resume_without_observed_suspend() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let events = [
        SessionEvent::OsSessionLocked,
        SessionEvent::SessionSwitchedAway,
        SessionEvent::Suspend,
        SessionEvent::Resume,
        SessionEvent::Reconnected,
        SessionEvent::DetectedSuspendGap,
    ];
    for event in events {
        // Enrollment avoids a KDF; the same event path owns normal/recovery access.
        let model = Model::new(None);
        let (mut session, worker) = Committer::begin_create(model.clone(), OWNER).unwrap();
        let reference = session.activity_reference(OWNER).unwrap();
        let code = session.recovery_code(OWNER).unwrap();
        let generation = session.generation();
        assert!(session.observe_event(event));
        assert!(session.is_locked());
        assert_eq!(session.generation(), generation + 1);
        assert!(session.recovery_code(OWNER).is_err());
        assert_eq!(
            session.record_activity(OWNER, &reference, SessionActivity::Keyboard),
            Err(TransactionError::Locked)
        );
        session.restart_enrollment(&worker, OWNER).unwrap();
        assert_ne!(&*code, &*session.recovery_code(OWNER).unwrap());
        assert!(session.inactivity_deadline().is_some());
        assert_eq!(model.live(), None);
        assert_eq!(model.events(), vec![Event::Read]);
    }
}

#[test]
fn privileged_mutations_and_enrollment_display_check_expiry_without_host_poll() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    expire_now(&mut session);
    assert!(matches!(
        session.prepare_contents(&mut worker, OWNER, request(1, 1), &empty()),
        Err(TransactionError::Locked)
    ));
    assert!(session.is_locked());
    assert_eq!(model.live(), Some(fixture()));
    let model = Model::new(Some(fixture()));
    let (mut recovery, mut worker) =
        Committer::open_recovery(model.clone(), OWNER, paper()).unwrap();
    expire_now(&mut recovery);
    assert!(matches!(
        recovery.prepare_recovery(&mut worker, OWNER, request(1, 1), PASSWORD, PASSWORD),
        Err(TransactionError::Locked)
    ));
    assert!(recovery.is_locked());
    assert_eq!(model.live(), Some(fixture()));
    let model = Model::new(None);
    let (mut enrollment, mut worker) = Committer::begin_create(model.clone(), OWNER).unwrap();
    let code = enrollment.recovery_code(OWNER).unwrap();
    expire_now(&mut enrollment);
    assert!(matches!(
        enrollment.recovery_code(OWNER),
        Err(TransactionError::Locked)
    ));
    assert!(enrollment.is_locked());
    assert!(matches!(
        enrollment.prepare_creation(&mut worker, OWNER, request(1, 0), PASSWORD, PASSWORD, &code),
        Err(TransactionError::Locked)
    ));
    assert_eq!(model.live(), None);
}

#[test]
fn deadline_cancels_blocked_authentication_and_completed_results_never_refresh_it() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let (mut session, worker) = Committer::open_locked(model.clone(), OWNER).unwrap();
    assert!(session.inactivity_deadline().is_none());
    let job = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap();
    let reference = session.activity_reference(OWNER).unwrap();
    session
        .record_activity(OWNER, &reference, SessionActivity::Keyboard)
        .unwrap();
    let deadline = session.inactivity_deadline().unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        job.run_after(|| {
            entered_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(session.poll_inactivity_at(deadline));
    assert!(session.is_locked());
    resume_tx.send(()).unwrap();
    assert_eq!(
        session.complete_authentication(&worker, OWNER, handle.join().unwrap()),
        Err(TransactionError::AuthenticationCanceled)
    );
    assert_eq!(model.events(), vec![Event::Read]);

    let model = Model::new(Some(fixture()));
    let (mut session, worker) = Committer::open_locked(model, OWNER).unwrap();
    let job = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap();
    let deadline = session.inactivity_deadline();
    session
        .complete_authentication(&worker, OWNER, job.run())
        .unwrap();
    assert_eq!(session.inactivity_deadline(), deadline);
    let job = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap();
    let result = job.run();
    expire_now(&mut session);
    assert_eq!(
        session.complete_authentication(&worker, OWNER, result),
        Err(TransactionError::AuthenticationCanceled)
    );
    assert!(session.is_locked());
}

#[test]
fn inactivity_cancels_before_admission_but_never_rolls_back_an_admitted_commit() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for admitted in [false, true] {
        let (model, mut session, mut worker) = opened();
        let op = session
            .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
            .unwrap();
        if admitted {
            worker.advance();
            worker.advance();
        }
        expire_now(&mut session);
        assert!(session.poll_inactivity());
        let status = complete(&mut worker);
        assert_eq!(
            status,
            if admitted {
                OperationStatus::Committed { revision: 2 }
            } else {
                OperationStatus::Canceled
            }
        );
        assert_eq!(worker.status(OWNER, &op).unwrap(), status);
        assert!(session.is_locked());
        if !admitted {
            assert_eq!(model.live(), Some(fixture()));
        }
    }
    let (model, mut session, mut worker) = opened();
    session
        .prepare_contents(&mut worker, OWNER, request(1, 1), &empty())
        .unwrap();
    worker.advance();
    worker.advance();
    model.fault(Fault::UnknownAfter);
    assert_eq!(complete(&mut worker), OperationStatus::OutcomeUnknown);
    expire_now(&mut session);
    assert!(session.poll_inactivity());
    assert!(matches!(
        session.begin_master_authentication(&worker, OWNER, PASSWORD),
        Err(TransactionError::Busy)
    ));
    assert!(session.is_locked());
    assert_eq!(worker.committed_revision(), Some(1));
}

#[test]
fn clock_regression_and_generation_exhaustion_fail_closed() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, worker) = opened();
    let before = session.inactivity.observed;
    session.inactivity.observed += Duration::from_secs(1);
    assert!(session.poll_inactivity_at(before));
    assert!(session.is_locked());
    assert!(session.inactivity_deadline().is_none());
    // A regressing observation also cannot arm a new attempt.
    assert_eq!(
        session.start_inactivity_at(before),
        Err(TransactionError::ClockUnavailable)
    );
    assert!(session.is_locked());
    session.inactivity.observed = Instant::now();
    session.generation = u64::MAX;
    assert!(matches!(
        session.begin_master_authentication(&worker, OWNER, PASSWORD),
        Err(TransactionError::GenerationExhausted)
    ));
    assert!(session.is_locked());
    assert!(session.inactivity_deadline().is_none());
}

#[test]
fn failed_current_authentication_disarms_without_preserving_activity_authority() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let (mut session, worker) = Committer::open_locked(model, OWNER).unwrap();
    let job = session
        .begin_master_authentication(&worker, OWNER, "wrong synthetic password")
        .unwrap();
    let reference = session.activity_reference(OWNER).unwrap();
    assert!(session.inactivity_deadline().is_some());
    assert!(matches!(
        session.complete_authentication(&worker, OWNER, job.run()),
        Err(TransactionError::Vault(_))
    ));
    assert!(session.is_locked());
    assert!(session.inactivity_deadline().is_none());
    let generation = session.generation();
    assert!(!session.poll_inactivity());
    assert_eq!(session.generation(), generation);
    assert_eq!(
        session.record_activity(OWNER, &reference, SessionActivity::Keyboard),
        Err(TransactionError::Locked)
    );
}
