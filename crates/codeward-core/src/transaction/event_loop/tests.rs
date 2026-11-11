use std::{cell::Cell, rc::Rc, sync::mpsc, thread};

use super::*;
use crate::{
    crypto,
    transaction::{
        AccessProof, CallerId, OperationStatus, StorageError,
        tests::{
            empty, fixture,
            model::{Event, Fault, Lease, Model},
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
    poll_interval: Option<Duration>,
    notice_after_poll: Option<SessionEvent>,
}

impl HeadlessEventSource for Source {
    fn take_revocation(&mut self) -> Result<Option<SessionEvent>, EventSourceError> {
        if self.failed {
            Err(EventSourceError)
        } else {
            Ok(self.notice.replace(self.notice_after_poll.take()))
        }
    }

    fn max_poll_interval(&self) -> Duration {
        self.poll_interval.unwrap_or(Duration::MAX)
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
fn external_driver_schedules_idle_revocation_without_waiting_in_the_source() {
    let mut host = HeadlessVault::begin_create(Model::new(None), OWNER).unwrap();
    let interval = Duration::from_secs(1);
    let bound = Duration::from_secs(60);
    let source = Source {
        poll_interval: Some(interval),
        ..Source::default()
    };
    host.set_deadline_for_test(Instant::now() + bound);
    let mut control = HeadlessControlLoop::new(host, source);
    assert_eq!(control.next_poll_in(bound), interval);
    expire(&mut control);
    assert_eq!(control.next_poll_in(bound), Duration::ZERO);
    assert!(control.step(Duration::ZERO).unwrap().revoked);
    assert!(control.source.wait_time.is_none());
    assert_eq!(control.next_poll_in(bound), interval);
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
    wait_for_completion_update(control).completion.unwrap()
}

fn wait_for_completion_update(control: &mut HeadlessControlLoop<Lease, Source>) -> ControlUpdate {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let update = control.step(Duration::from_millis(5)).unwrap();
        if update.completion.is_some() {
            return update;
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
        .command(|host| host.save(request(1, 1), empty()))
        .unwrap();
    await_storage(&mut control, checkpoint);
    assert!(control.next_poll_in(Duration::from_secs(1)) <= WORKER_POLL);
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

fn await_storage(control: &mut HeadlessControlLoop<Lease, Source>, checkpoint: Receiver<()>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while checkpoint.try_recv() != Ok(()) {
        assert!(Instant::now() < deadline, "storage was never reached");
        let update = control.step(Duration::ZERO).unwrap();
        assert!(!update.revoked && update.completion.is_none());
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn lost_refresh_worker_reports_revocation_and_keeps_the_commit_and_writer() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let model = Model::new(Some(fixture()));
    let host = HeadlessVault::open(model.clone(), OWNER).unwrap();
    let mut control = HeadlessControlLoop::new(host, Source::default());
    let unlock = |host: &mut HeadlessVault<Lease>| {
        host.begin_authentication(AccessProof::Master("synthetic password"))
    };
    control.command(unlock).unwrap();
    assert!(matches!(
        wait_for_result(&mut control),
        HostCompletion::Authentication(Ok(()))
    ));

    let (checkpoint, resume) = control.command(|host| Ok(host.pause_refresh())).unwrap();
    control
        .command(|host| host.save(request(1, 1), empty()))
        .unwrap();
    await_storage(&mut control, checkpoint);
    // Disconnect the refresh worker after commit, before decoding.
    drop(resume);
    let update = wait_for_completion_update(&mut control);
    assert!(update.revoked && update.source_available);
    let Some(HostCompletion::Transaction {
        operation,
        outcome: Ok(OperationStatus::Committed { revision: 2 }),
        refresh: Some(Err(TransactionError::ReloadRequired)),
    }) = update.completion
    else {
        panic!("expected the proven commit with a refresh failure");
    };
    assert!(control.command(|host| Ok(host.is_locked())).unwrap());
    assert_eq!(
        control.command(|host| host.status(&operation)),
        Ok(OperationStatus::Committed { revision: 2 })
    );
    assert!(matches!(
        model.acquire(),
        Err(StorageError::WriterUnavailable)
    ));

    control.command(unlock).unwrap();
    assert!(matches!(
        wait_for_result(&mut control),
        HostCompletion::Authentication(Ok(()))
    ));
    let page = control
        .command(|host| host.list_credentials(host.generation(), 10, None))
        .unwrap();
    assert_eq!(page.revision, 2);
    assert!(page.summaries.is_empty());
}

#[test]
fn revocation_during_blocked_refresh_reports_the_commit_without_waiting() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    for source_failure in [false, true] {
        let model = Model::new(Some(fixture()));
        let mut host = HeadlessVault::open(model.clone(), OWNER).unwrap();
        let (checkpoint, resume) = host.pause_refresh();
        let mut control = HeadlessControlLoop::new(host, Source::default());
        control
            .command(|host| host.begin_authentication(AccessProof::Master("synthetic password")))
            .unwrap();
        assert!(matches!(
            wait_for_result(&mut control),
            HostCompletion::Authentication(Ok(()))
        ));
        control
            .command(|host| host.save(request(1, 1), empty()))
            .unwrap();
        await_storage(&mut control, checkpoint);
        assert_eq!(control.next_poll_in(Duration::from_secs(1)), WORKER_POLL);
        assert!(!control.command(|host| Ok(host.is_locked())).unwrap());

        if source_failure {
            control.source.failed = true;
        } else {
            expire(&mut control);
        }
        let update = control.step(Duration::ZERO).unwrap();
        assert!(update.revoked);
        assert_eq!(update.source_available, !source_failure);
        assert!(matches!(
            update.completion,
            Some(HostCompletion::Transaction {
                outcome: Ok(OperationStatus::Committed { revision: 2 }),
                refresh: Some(Err(TransactionError::Locked)),
                ..
            })
        ));
        assert_ne!(model.live(), Some(fixture()));
        assert_eq!(model.live(), model.0.lock().unwrap().durable);
        assert!(control.step(Duration::ZERO).unwrap().completion.is_none());

        // The refresh worker never held the writer lease.
        control.close();
        let reopened = HeadlessVault::open(model, OWNER).unwrap();
        resume.send(()).unwrap();
        reopened.close();
    }
}

#[test]
fn revocation_at_the_reply_check_retains_the_commit_and_revokes_refresh() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let host = HeadlessVault::open(Model::new(Some(fixture())), OWNER).unwrap();
    let mut control = HeadlessControlLoop::new(host, Source::default());
    control
        .command(|host| host.begin_authentication(AccessProof::Master("synthetic password")))
        .unwrap();
    assert!(matches!(
        wait_for_result(&mut control),
        HostCompletion::Authentication(Ok(()))
    ));
    let (checkpoint, resume) = control.command(|host| Ok(host.pause_refresh())).unwrap();
    control
        .command(|host| host.save(request(1, 1), empty()))
        .unwrap();
    await_storage(&mut control, checkpoint);
    resume.send(()).unwrap();
    control
        .command(|host| {
            host.await_queued_refresh();
            Ok(())
        })
        .unwrap();
    control.source.notice_after_poll = Some(SessionEvent::Suspend);
    let update = control.step(Duration::ZERO).unwrap();
    assert!(update.revoked);
    assert!(matches!(
        update.completion,
        Some(HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 2 }),
            refresh: Some(Err(TransactionError::Locked)),
            ..
        })
    ));
    assert!(control.command(|host| Ok(host.is_locked())).unwrap());
}

#[test]
fn suspend_cancels_blocked_enrollment_preparation_without_restoring_access() {
    let model = Model::new(None);
    let mut host = HeadlessVault::begin_create(model.clone(), OWNER).unwrap();
    let paper = host.recovery_code().unwrap();
    let (checkpoint, resume) = host.pause_preparation();
    let mut control = HeadlessControlLoop::new(host, Source::default());
    let request = request(1, 0);
    control
        .command(|host| host.create(request, "synthetic", "synthetic", &paper))
        .unwrap();
    checkpoint.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(control.next_poll_in(Duration::from_secs(1)), WORKER_POLL);
    assert!(!control.command(|host| Ok(host.is_locked())).unwrap());
    assert!(control.command(|host| host.activity_reference()).is_ok());
    assert!(matches!(
        control.command(|host| host.recovery_code()),
        Err(ControlError::Host(HostError::Transaction(
            TransactionError::Busy
        )))
    ));
    let events = model.events();
    control.source.notice.set(Some(SessionEvent::Suspend));
    assert!(control.step(Duration::ZERO).unwrap().revoked);
    assert!(control.command(|host| Ok(host.is_locked())).unwrap());
    resume.send(()).unwrap();
    assert!(matches!(
        wait_for_result(&mut control),
        HostCompletion::PreparationFailed { request: id, error: TransactionError::Locked }
            if id == request.id
    ));
    assert!(control.command(|host| Ok(host.is_locked())).unwrap());
    assert_eq!(model.events(), events);
    assert!(model.live().is_none());
    control.command(|host| host.restart_enrollment()).unwrap();
    assert_ne!(
        *control.command(|host| host.recovery_code()).unwrap(),
        *paper
    );
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
        .command(|host| host.save(request(1, 1), empty()))
        .unwrap();
    await_storage(&mut control, checkpoint);
    expire(&mut control);
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

fn expire(control: &mut HeadlessControlLoop<Lease, Source>) {
    let VaultState::Open(host) = &mut control.vault else {
        panic!("expected open vault");
    };
    host.set_deadline_for_test(Instant::now());
}

fn wait_for_opening(control: &mut HeadlessControlLoop<Lease, Source>) -> Result<(), HostError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let update = control.step(WORKER_POLL).unwrap();
        assert!(!update.revoked);
        if let Some(result) = update.opening {
            return result;
        }
        assert!(Instant::now() < deadline);
    }
}

#[test]
fn opening_loads_the_existing_vault_locked_and_retains_exclusion() {
    let model = Model::new(Some(fixture()));
    let storage = model.clone();
    let control_thread = thread::current().id();
    let mut control = HeadlessControlLoop::begin_open(
        Source::default(),
        OWNER,
        VaultOpenMode::Existing,
        move || {
            assert_ne!(thread::current().id(), control_thread);
            Ok(storage)
        },
    )
    .unwrap();
    wait_for_opening(&mut control).unwrap();
    assert!(control.command(|host| Ok(host.is_locked())).unwrap());
    assert!(matches!(
        model.clone().acquire(),
        Err(StorageError::WriterUnavailable)
    ));
    assert_eq!(model.events(), [Event::Read]);
    control.close();
    assert!(model.acquire().is_ok());
}

#[test]
fn enrollment_stays_volatile_and_does_not_overwrite_existing_storage() {
    let model = Model::new(None);
    let mut control =
        HeadlessControlLoop::begin_open(Source::default(), OWNER, VaultOpenMode::Enrollment, {
            let model = model.clone();
            move || Ok(model)
        })
        .unwrap();
    wait_for_opening(&mut control).unwrap();
    assert!(control.command(|host| host.recovery_code()).is_ok());
    assert_eq!(model.events(), [Event::Read]);
    assert_eq!(model.live(), None);
    control.close();

    let mut control = HeadlessControlLoop::begin_open(
        Source::default(),
        OWNER,
        VaultOpenMode::Enrollment,
        || Ok(Model::new(Some(fixture()))),
    )
    .unwrap();
    assert_eq!(
        wait_for_opening(&mut control),
        Err(TransactionError::ReloadRequired.into())
    );
}

#[test]
fn opening_errors_release_the_writer_and_leave_commands_disabled() {
    let model = Model::new(Some(fixture()));
    model.fault(Fault::Read);
    let storage = model.clone();
    let mut control = HeadlessControlLoop::begin_open(
        Source::default(),
        OWNER,
        VaultOpenMode::Existing,
        move || Ok(storage),
    )
    .unwrap();
    assert_eq!(
        wait_for_opening(&mut control),
        Err(TransactionError::from(StorageError::Read).into())
    );
    assert!(matches!(
        control.command::<()>(|_| panic!("failed opening has no vault")),
        Err(ControlError::Host(HostError::Transaction(
            TransactionError::Locked
        )))
    ));
    assert!(control.step(Duration::ZERO).unwrap().opening.is_none());
    assert!(model.acquire().is_ok());
}

#[test]
fn opening_observes_revocation_during_a_blocked_read() {
    for source_failure in [false, true] {
        let model = Model::new(None);
        let (entered, checkpoint) = mpsc::channel();
        let (resume, wait) = mpsc::channel();
        model.0.lock().unwrap().read_block = Some((entered, wait));
        let storage = model.clone();
        let mut control = HeadlessControlLoop::begin_open(
            Source::default(),
            OWNER,
            VaultOpenMode::Enrollment,
            move || Ok(storage),
        )
        .unwrap();
        checkpoint.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(control.next_poll_in(Duration::from_secs(1)), WORKER_POLL);
        assert!(matches!(
            control.command::<()>(|_| panic!("opening has no vault yet")),
            Err(ControlError::Host(HostError::Transaction(
                TransactionError::Busy
            )))
        ));
        if source_failure {
            control.source.failed = true;
        } else {
            control.source.notice.set(Some(SessionEvent::Suspend));
        }
        let update = control.step(Duration::ZERO).unwrap();
        assert!(update.revoked && update.opening.is_none());
        assert_eq!(update.source_available, !source_failure);
        assert!(matches!(
            model.clone().acquire(),
            Err(StorageError::WriterUnavailable)
        ));
        resume.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while model.0.lock().unwrap().held {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        assert!(control.step(Duration::ZERO).unwrap().opening.is_none());
        assert!(model.acquire().is_ok());
    }
}

#[test]
fn revocation_at_the_reply_check_discards_a_completed_opening() {
    let model = Model::new(None);
    let host = HeadlessVault::begin_create(model.clone(), OWNER).unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    sender.send(Ok(Box::new(host))).unwrap();
    let mut control = HeadlessControlLoop {
        vault: VaultState::Opening(receiver),
        source: Source {
            notice_after_poll: Some(SessionEvent::OsSessionLocked),
            ..Source::default()
        },
        source_available: true,
    };
    let update = control.step(Duration::ZERO).unwrap();
    assert!(update.revoked && update.opening.is_none());
    assert!(matches!(control.vault, VaultState::Closed));
    assert!(model.acquire().is_ok());
}

#[test]
fn manual_lock_and_close_drop_queued_openings() {
    for close in [false, true] {
        let model = Model::new(None);
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut control = HeadlessControlLoop {
            vault: VaultState::Opening(receiver),
            source: Source::default(),
            source_available: true,
        };
        let host = HeadlessVault::begin_create(model.clone(), OWNER).unwrap();
        sender.send(Ok(Box::new(host))).unwrap();
        if close {
            control.close();
        } else {
            control.lock();
            assert!(control.step(Duration::ZERO).unwrap().opening.is_none());
        }
        assert!(model.acquire().is_ok());
    }
}
