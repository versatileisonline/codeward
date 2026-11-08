use std::{cell::Cell, rc::Rc, thread};

use super::*;
use crate::{
    crypto,
    transaction::{
        AccessProof, CallerId, OperationStatus,
        tests::{
            empty, fixture,
            model::{Lease, Model},
            request,
        },
    },
};

const OWNER: CallerId = CallerId(7);

#[derive(Default)]
struct Source {
    notice: Rc<Cell<Option<SessionEvent>>>,
    wait_event: Option<SessionEvent>,
    wait_time: Option<Duration>,
    failed: bool,
}

impl HeadlessEventSource for Source {
    fn take_revocation(&mut self) -> Result<Option<SessionEvent>, EventSourceError> {
        if self.failed {
            Err(EventSourceError)
        } else {
            Ok(self.notice.take())
        }
    }

    fn wait(&mut self, timeout: Duration) -> Result<(), EventSourceError> {
        self.wait_time = Some(timeout);
        if let Some(event) = self.wait_event.take() {
            self.notice.set(Some(event));
        } else {
            thread::sleep(timeout);
        }
        Ok(())
    }
}

#[test]
fn headless_loop_wait_is_bounded_by_deadline_and_active_worker_poll() {
    let now = Instant::now();
    let max_wait = Duration::from_secs(1);
    let idle = Duration::from_millis(120);
    assert_eq!(next_wait(now, None, false, max_wait), max_wait);
    assert_eq!(next_wait(now, Some(now + idle), false, max_wait), idle);
    assert_eq!(
        next_wait(now, Some(now + idle), true, max_wait),
        WORKER_POLL
    );
    assert_eq!(next_wait(now, Some(now), true, max_wait), Duration::ZERO);
}

#[test]
fn headless_loop_waits_for_idle_expiry_without_commands_or_worker() {
    let mut host = HeadlessVault::begin_create(Model::new(None), OWNER).unwrap();
    host.set_deadline_for_test(Instant::now() + Duration::from_millis(35));
    let mut control = HeadlessControlLoop::new(host, Source::default());
    let update = control.step(Duration::from_secs(1)).unwrap();
    assert!(update.revoked);
    assert!(update.completion.is_none());
    assert!(control.source.wait_time.unwrap_or_default() <= Duration::from_millis(35));
    assert!(control.command(|host| Ok(host.is_locked())).unwrap());
}

#[test]
fn headless_loop_reports_expiry_even_when_a_command_observed_it_first() {
    let host = HeadlessVault::begin_create(Model::new(None), OWNER).unwrap();
    let mut control = HeadlessControlLoop::new(host, Source::default());
    assert!(matches!(
        control.command(|host| {
            host.set_deadline_for_test(Instant::now());
            assert!(host.is_locked());
            Ok(())
        }),
        Err(ControlError::Revoked)
    ));
    assert!(!control.step(Duration::ZERO).unwrap().revoked);
}

#[test]
fn headless_loop_revoked_command_cannot_leave_new_enrollment_armed() {
    let host = HeadlessVault::begin_create(Model::new(None), OWNER).unwrap();
    let mut control = HeadlessControlLoop::new(host, Source::default());
    assert!(matches!(
        control.command(|host| {
            host.set_deadline_for_test(Instant::now());
            host.restart_enrollment()
        }),
        Err(ControlError::Revoked)
    ));
    assert!(control.command(|host| Ok(host.is_locked())).unwrap());
    assert!(matches!(
        control.command(|host| host.recovery_code()),
        Err(ControlError::Host(_))
    ));
}

#[test]
fn headless_loop_processes_wake_and_revokes_before_or_after_commands() {
    let host = HeadlessVault::begin_create(Model::new(None), OWNER).unwrap();
    let source = Source {
        wait_event: Some(SessionEvent::Resume),
        ..Source::default()
    };
    let mut control = HeadlessControlLoop::new(host, source);
    assert!(control.step(Duration::from_secs(1)).unwrap().revoked);
    control.command(|host| host.restart_enrollment()).unwrap();
    control
        .source
        .notice
        .set(Some(SessionEvent::OsSessionLocked));
    assert!(matches!(
        control.command::<()>(|_| panic!("revocation precedes commands")),
        Err(ControlError::Revoked)
    ));
    control.command(|host| host.restart_enrollment()).unwrap();
    let notice = control.source.notice.clone();
    assert!(matches!(
        control.command(|host| {
            let code = host.recovery_code()?;
            notice.set(Some(SessionEvent::Suspend));
            Ok(code)
        }),
        Err(ControlError::Revoked)
    ));
    assert!(control.command(|host| Ok(host.is_locked())).unwrap());
}

fn wait_for_result(control: &mut HeadlessControlLoop<Lease, Source>) -> HostCompletion {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(completion) = control.step(Duration::from_millis(5)).unwrap().completion {
            return completion;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn headless_loop_discards_authentication_queued_before_security_event() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let host = HeadlessVault::open(Model::new(Some(fixture())), OWNER).unwrap();
    let mut control = HeadlessControlLoop::new(host, Source::default());
    control
        .command(|host| host.begin_authentication(AccessProof::Master("synthetic password")))
        .unwrap();
    control
        .source
        .notice
        .set(Some(SessionEvent::OsSessionLocked));
    let update = control.step(Duration::ZERO).unwrap();
    assert!(update.revoked && update.completion.is_none());
    assert!(control.command(|host| Ok(host.is_locked())).unwrap());
    // The global derivation guard is shared by the synthetic core tests. Wait
    // for the revoked worker to release it before releasing this test's guard.
    let until = Instant::now() + Duration::from_secs(10);
    while matches!(
        control
            .command(|host| host.begin_authentication(AccessProof::Master("synthetic password"))),
        Err(ControlError::Host(HostError::Transaction(
            super::super::TransactionError::Busy
        )))
    ) {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        wait_for_result(&mut control),
        HostCompletion::Authentication(Ok(()))
    ));
}

#[test]
fn headless_loop_source_failure_locks_but_drains_an_admitted_commit() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let host = HeadlessVault::open(model.clone(), OWNER).unwrap();
    let mut control = HeadlessControlLoop::new(host, Source::default());
    control
        .command(|host| host.begin_authentication(AccessProof::Master("synthetic password")))
        .unwrap();
    assert!(matches!(
        wait_for_result(&mut control),
        HostCompletion::Authentication(Ok(()))
    ));
    let (entered, checkpoint) = std::sync::mpsc::channel();
    let (resume, wait) = std::sync::mpsc::channel();
    model.0.lock().unwrap().barrier_block = Some((entered, wait));
    control
        .command(|host| host.save(request(1, 1), &empty()))
        .unwrap();
    checkpoint.recv_timeout(Duration::from_secs(5)).unwrap();
    control.source.failed = true;
    let update = control.step(Duration::ZERO).unwrap();
    assert!(update.revoked && !update.source_available);
    assert!(matches!(
        control.command::<()>(|_| panic!("source failure prevents commands")),
        Err(ControlError::EventSourceUnavailable)
    ));
    resume.send(()).unwrap();
    assert!(matches!(
        wait_for_result(&mut control),
        HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 2 }),
            refresh: Some(Err(super::super::TransactionError::Locked)),
            ..
        }
    ));
}

#[test]
fn headless_loop_expiry_revokes_while_staging_is_blocked() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let host = HeadlessVault::open(model.clone(), OWNER).unwrap();
    let mut control = HeadlessControlLoop::new(host, Source::default());
    control
        .command(|host| host.begin_authentication(AccessProof::Master("synthetic password")))
        .unwrap();
    assert!(matches!(
        wait_for_result(&mut control),
        HostCompletion::Authentication(Ok(()))
    ));
    let (entered, checkpoint) = std::sync::mpsc::channel();
    let (resume, wait) = std::sync::mpsc::channel();
    model.0.lock().unwrap().stage_block = Some((entered, wait));
    control
        .command(|host| host.save(request(1, 1), &empty()))
        .unwrap();
    checkpoint.recv_timeout(Duration::from_secs(5)).unwrap();
    control.host.set_deadline_for_test(Instant::now());
    let update = control.step(Duration::ZERO).unwrap();
    assert!(update.revoked && update.completion.is_none());
    resume.send(()).unwrap();
    assert!(matches!(
        wait_for_result(&mut control),
        HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Canceled),
            refresh: None,
            ..
        }
    ));
    assert_eq!(model.live(), Some(fixture()));
}
