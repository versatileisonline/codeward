use std::{sync::mpsc, thread, time::Duration};

use super::*;

fn locked() -> (Model, Session, Committer<Lease>) {
    let model = Model::new(Some(fixture()));
    let (session, worker) = Committer::open_locked(model.clone(), OWNER).unwrap();
    (model, session, worker)
}

#[test]
fn master_authentication_installs_only_after_explicit_current_completion() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = locked();
    assert!(session.is_locked());
    let generation = session.generation();
    let job = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap();
    assert!(session.generation() > generation);
    assert!(session.is_locked());
    assert!(matches!(
        session.prepare_contents(&mut worker, OWNER, request(1, 1), &empty()),
        Err(TransactionError::Locked)
    ));
    let result = thread::spawn(move || job.run()).join().unwrap();
    assert!(session.is_locked());
    session
        .complete_authentication(&worker, OWNER, result)
        .unwrap();
    assert!(!session.is_locked());
    assert_eq!(model.events(), vec![Event::Read]);
    prepare(&mut session, &mut worker, 1);
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
}

#[test]
fn lock_during_blocked_authentication_is_immediate_and_late_success_is_rejected() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, worker) = locked();
    let job = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        job.run_after(|| {
            entered_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let generation = session.generation();
    session.lock(); // Neither storage nor the blocked authentication worker is needed.
    assert!(session.is_locked());
    assert!(session.generation() > generation);
    assert!(matches!(
        session.begin_master_authentication(&worker, OWNER, PASSWORD),
        Err(TransactionError::Busy)
    ));
    resume_tx.send(()).unwrap();
    let result = handle.join().unwrap();
    assert_eq!(
        session.complete_authentication(&worker, OWNER, result),
        Err(TransactionError::AuthenticationCanceled)
    );
    assert!(session.is_locked());
    assert_eq!(model.live(), Some(fixture()));
    assert_eq!(model.events(), vec![Event::Read]);
    let retry = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap();
    session
        .complete_authentication(&worker, OWNER, retry.run())
        .unwrap();
}

#[test]
fn old_success_and_failure_cannot_replace_or_lock_a_newer_authenticated_session() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for old_password in [PASSWORD, "wrong synthetic password"] {
        let (_, mut session, mut worker) = locked();
        let old = session
            .begin_master_authentication(&worker, OWNER, old_password)
            .unwrap()
            .run();
        session.lock();
        let new = session
            .begin_master_authentication(&worker, OWNER, PASSWORD)
            .unwrap()
            .run();
        session
            .complete_authentication(&worker, OWNER, new)
            .unwrap();
        let generation = session.generation();
        assert_eq!(
            session.complete_authentication(&worker, OWNER, old),
            Err(TransactionError::AuthenticationCanceled)
        );
        assert_eq!(session.generation(), generation);
        assert!(!session.is_locked());
        prepare(&mut session, &mut worker, 1);
    }
    // Replacement alone invalidates an old completed result, even without lock().
    let (_, mut session, worker) = locked();
    let old = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap()
        .run();
    let new = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap()
        .run();
    assert_eq!(
        session.complete_authentication(&worker, OWNER, old),
        Err(TransactionError::AuthenticationCanceled)
    );
    session
        .complete_authentication(&worker, OWNER, new)
        .unwrap();
}

#[test]
fn queued_job_drop_releases_capacity_and_cancel_skips_authentication() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, worker) = locked();
    let queued = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap();
    assert!(matches!(
        session.begin_recovery_authentication(&worker, OWNER, paper()),
        Err(TransactionError::Busy)
    ));
    drop(queued);
    let canceled = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap();
    session.lock();
    let result = canceled.run_after(|| panic!("canceled job must not authenticate"));
    assert_eq!(
        session.complete_authentication(&worker, OWNER, result),
        Err(TransactionError::AuthenticationCanceled)
    );
    let current = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap();
    session
        .complete_authentication(&worker, OWNER, current.run())
        .unwrap();
}

#[test]
fn dropping_session_or_reopening_worker_never_restores_old_authentication() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, worker) = locked();
    let job = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap();
    drop(session);
    let canceled = job.run_after(|| panic!("dropped session must cancel queued authentication"));
    drop(worker);
    let (mut reopened, worker) = Committer::open_locked(model, OWNER).unwrap();
    assert_eq!(
        reopened.complete_authentication(&worker, OWNER, canceled),
        Err(TransactionError::AuthenticationCanceled)
    );
    assert!(reopened.is_locked());
}

#[test]
fn caller_and_opened_worker_scope_are_checked_before_revoking_or_installing() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (_, mut session, worker) = opened();
    let (_, mut other, other_worker) = locked();
    assert!(matches!(
        session.begin_master_authentication(&worker, CallerId(8), PASSWORD),
        Err(TransactionError::Unauthorized)
    ));
    assert!(matches!(
        session.begin_master_authentication(&other_worker, OWNER, PASSWORD),
        Err(TransactionError::Unauthorized)
    ));
    assert!(!session.is_locked());
    let result = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap()
        .run();
    assert_eq!(
        other.complete_authentication(&other_worker, OWNER, result),
        Err(TransactionError::AuthenticationCanceled)
    );
    assert!(other.is_locked());
    let wrong_caller = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap()
        .run();
    assert_eq!(
        session.complete_authentication(&worker, CallerId(8), wrong_caller),
        Err(TransactionError::Unauthorized)
    );
    let wrong_worker = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap()
        .run();
    assert_eq!(
        session.complete_authentication(&other_worker, OWNER, wrong_worker),
        Err(TransactionError::Unauthorized)
    );
    assert!(session.is_locked());
}

#[test]
fn invalid_inputs_and_authentication_failures_leave_access_locked_and_file_untouched() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, worker) = opened();
    for password in ["", &"x".repeat(vault::MAX_MASTER_PASSWORD_BYTES + 1)] {
        assert!(matches!(
            session.begin_master_authentication(&worker, OWNER, password),
            Err(TransactionError::Vault(VaultError::InvalidPassword))
        ));
        assert!(session.is_locked());
    }
    assert!(matches!(
        session.begin_recovery_authentication(&worker, OWNER, &"x".repeat(257)),
        Err(TransactionError::Vault(_))
    ));
    let wrong = session
        .begin_master_authentication(&worker, OWNER, "incorrect synthetic password")
        .unwrap()
        .run();
    assert!(matches!(
        session.complete_authentication(&worker, OWNER, wrong),
        Err(TransactionError::Vault(_))
    ));
    let wrong = session
        .begin_recovery_authentication(&worker, OWNER, "invalid paper")
        .unwrap()
        .run();
    assert!(matches!(
        session.complete_authentication(&worker, OWNER, wrong),
        Err(TransactionError::Vault(_))
    ));
    assert!(session.is_locked());
    assert_eq!(model.live(), Some(fixture()));
    assert_eq!(model.events(), vec![Event::Read]);

    let mut tampered = fixture();
    *tampered.last_mut().unwrap() ^= 1;
    let model = Model::new(Some(tampered.clone()));
    let (mut session, worker) = Committer::open_locked(model.clone(), OWNER).unwrap();
    let result = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap()
        .run();
    assert!(matches!(
        session.complete_authentication(&worker, OWNER, result),
        Err(TransactionError::Vault(_))
    ));
    assert!(session.is_locked());
    assert_eq!(model.live(), Some(tampered));
}

#[test]
fn recovery_completion_remains_restricted_and_only_a_commit_changes_access_metadata() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = locked();
    let result = session
        .begin_recovery_authentication(&worker, OWNER, paper())
        .unwrap()
        .run();
    session
        .complete_authentication(&worker, OWNER, result)
        .unwrap();
    assert!(matches!(
        session.prepare_contents(&mut worker, OWNER, request(1, 1), &empty()),
        Err(TransactionError::Locked)
    ));
    assert!(matches!(
        session.recovery_code(OWNER),
        Err(TransactionError::Locked)
    ));
    session
        .prepare_recovery(
            &mut worker,
            OWNER,
            request(2, 1),
            "replacement synthetic",
            "replacement synthetic",
        )
        .unwrap();
    assert_eq!(model.live(), Some(fixture()));
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision: 2 }
    );
    let result = session
        .begin_master_authentication(&worker, OWNER, "replacement synthetic")
        .unwrap()
        .run();
    session
        .complete_authentication(&worker, OWNER, result)
        .unwrap();
    session
        .prepare_contents(&mut worker, OWNER, request(3, 2), &empty())
        .unwrap();
}

#[test]
fn active_uncertain_conflicted_and_absent_workers_do_not_bypass_transaction_policy() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = opened();
    prepare(&mut session, &mut worker, 1);
    assert!(matches!(
        session.begin_master_authentication(&worker, OWNER, PASSWORD),
        Err(TransactionError::Busy)
    ));
    worker.advance();
    worker.advance(); // Publication admitted before lock.
    session.lock();
    model.fault(Fault::UnknownAfter);
    assert_eq!(complete(&mut worker), OperationStatus::OutcomeUnknown);
    assert!(matches!(
        session.begin_recovery_authentication(&worker, OWNER, paper()),
        Err(TransactionError::Busy)
    ));
    assert!(session.is_locked());
    assert_eq!(worker.committed_revision(), Some(1));

    let (model, mut session, mut worker) = opened();
    prepare(&mut session, &mut worker, 1);
    model.0.lock().unwrap().live = None;
    assert_eq!(complete(&mut worker), OperationStatus::Conflict);
    session.lock();
    assert!(matches!(
        session.begin_master_authentication(&worker, OWNER, PASSWORD),
        Err(TransactionError::ReloadRequired)
    ));

    let absent = Model::new(None);
    assert!(matches!(
        Committer::open_locked(absent.clone(), OWNER),
        Err(TransactionError::ReloadRequired)
    ));
    assert_eq!(absent.live(), None);
    assert!(!absent.0.lock().unwrap().held);
}

#[test]
fn completion_requires_the_exact_full_file_fingerprint_not_only_vault_and_revision() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let (model, mut session, mut worker) = locked();
    let result = session
        .begin_master_authentication(&worker, OWNER, PASSWORD)
        .unwrap()
        .run();
    // White-box defense check: preserve vault ID/revision but change the worker's
    // known source. A completion authenticated against old bytes must not install.
    worker.fingerprint.as_mut().unwrap().sha256[0] ^= 1;
    assert_eq!(
        session.complete_authentication(&worker, OWNER, result),
        Err(TransactionError::ReloadRequired)
    );
    assert!(session.is_locked());
    assert_eq!(model.live(), Some(fixture()));
    assert_eq!(model.events(), vec![Event::Read]);
}
