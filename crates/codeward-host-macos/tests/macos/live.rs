use std::{cell::RefCell, rc::Rc, time::Instant};

use codeward_core::transaction::{EventSourceError, HostError, TransactionError};

use super::*;

const WINDOW: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug)]
pub(crate) enum Scenario {
    Lock,
    Sleep,
}

impl Scenario {
    fn starts_with(self, event: SessionEvent) -> bool {
        match self {
            Self::Lock => event == SessionEvent::OsSessionLocked,
            Self::Sleep => event == SessionEvent::Suspend || self.completed_by(event),
        }
    }

    fn completed_by(self, event: SessionEvent) -> bool {
        match self {
            Self::Lock => event == SessionEvent::Reconnected,
            Self::Sleep => matches!(
                event,
                SessionEvent::Resume | SessionEvent::DetectedSuspendGap
            ),
        }
    }
}

#[derive(Default)]
struct Observations {
    first: Option<SessionEvent>,
    completion: Option<SessionEvent>,
}

struct RecordingSource {
    native: MacosEventSource,
    scenario: Scenario,
    observations: Rc<RefCell<Observations>>,
}

impl HeadlessEventSource for RecordingSource {
    fn take_revocation(&mut self) -> Result<Option<SessionEvent>, EventSourceError> {
        let event = self.native.take_revocation()?;
        if let Some(event) = event {
            let mut observations = self.observations.borrow_mut();
            observations.first.get_or_insert(event);
            if self.scenario.completed_by(event) {
                observations.completion.get_or_insert(event);
            }
        }
        Ok(event)
    }

    fn wait(&mut self, timeout: Duration) -> Result<(), EventSourceError> {
        self.native.wait(timeout)
    }
}

pub(crate) fn run(scenario: Scenario) {
    let observations = Rc::new(RefCell::new(Observations::default()));
    let source = RecordingSource {
        native: MacosEventSource::new().expect("register native observers on the main thread"),
        scenario,
        observations: observations.clone(),
    };
    let host =
        HeadlessVault::begin_create(Absent, CallerId(1)).expect("start synthetic enrollment");
    let mut control = HeadlessControlLoop::new(host, source);
    control
        .command(|host| host.recovery_code().map(drop))
        .expect("synthetic enrollment must have access before arming");
    let deadline = Instant::now() + WINDOW;
    println!("ARMED {scenario:?} for {} awake seconds", WINDOW.as_secs());

    let mut initial = None;
    let completion = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out waiting for {scenario:?}; first revocation: {initial:?}"
        );
        let update = control.step(remaining).expect("drive headless host");
        assert!(update.source_available, "native event source failed");
        let events = observations.take();
        if !update.revoked {
            assert!(events.first.is_none(), "native observation did not revoke");
            continue;
        }
        let first = events
            .first
            .expect("revocation must come from a native observation");
        if initial.is_none() {
            assert!(
                scenario.starts_with(first),
                "unexpected first revocation: {first:?}"
            );
            initial = Some(first);
            println!("REVOKED {first:?}");
        }
        if let Some(completion) = events.completion {
            break completion;
        }
    };

    // A queued notice may win the command boundary. Keep checking until the
    // host itself denies access, without accepting a generic control error.
    loop {
        match control.command(|host| host.recovery_code().map(drop)) {
            Err(ControlError::Host(HostError::Transaction(TransactionError::Locked))) => break,
            Err(ControlError::Revoked) if Instant::now() < deadline => continue,
            result => {
                panic!("expected locked synthetic enrollment after {completion:?}: {result:?}")
            }
        }
    }
    control.close();
    println!("PASS {scenario:?}: {completion:?}; synthetic enrollment remains locked");
}
