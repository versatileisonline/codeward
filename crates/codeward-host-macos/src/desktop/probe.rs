use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use codeward_core::transaction::{EventSourceError, HeadlessEventSource, SessionEvent};
use codeward_host_macos::MacosEventSource;

use super::pilot::State;

const WINDOW: Duration = Duration::from_secs(120);
pub(super) const PROBE_PAGE: &str = "probe.html";

#[derive(Clone, Copy, Debug)]
pub(super) enum Scenario {
    Lock,
    Sleep,
}

impl Scenario {
    pub(super) fn parse(flag: &str) -> Option<Self> {
        match flag {
            "--live-lock" => Some(Self::Lock),
            "--live-sleep" => Some(Self::Sleep),
            _ => None,
        }
    }

    fn events(self) -> (SessionEvent, SessionEvent) {
        match self {
            Self::Lock => (SessionEvent::OsSessionLocked, SessionEvent::Reconnected),
            Self::Sleep => (SessionEvent::Suspend, SessionEvent::Resume),
        }
    }
}

pub(super) enum DesktopSource {
    Normal(MacosEventSource),
    Probe(RecordedSource),
}

impl DesktopSource {
    pub(super) fn normal() -> Option<Self> {
        MacosEventSource::for_desktop_loop().ok().map(Self::Normal)
    }
}

impl HeadlessEventSource for DesktopSource {
    fn take_revocation(&mut self) -> Result<Option<SessionEvent>, EventSourceError> {
        match self {
            Self::Normal(source) => source.take_revocation(),
            Self::Probe(source) => source.collect(),
        }
    }

    fn max_poll_interval(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn wait(&mut self, _: Duration) -> Result<(), EventSourceError> {
        Err(EventSourceError)
    }
}

struct Observations {
    first: Option<SessionEvent>,
    returned: bool,
    coalesced: bool,
}

struct Recording {
    native: MacosEventSource,
    expected_return: SessionEvent,
    observations: Observations,
}

#[derive(Clone)]
pub(super) struct RecordedSource(Rc<RefCell<Recording>>);

impl RecordedSource {
    fn collect(&self) -> Result<Option<SessionEvent>, EventSourceError> {
        let mut recording = self.0.borrow_mut();
        let batch = recording.native.take_observation()?;
        let first_batch = recording.observations.first.is_none();
        if first_batch {
            recording.observations.first = batch.revocation;
        }
        if !recording.observations.returned && batch.includes(recording.expected_return) {
            recording.observations.returned = true;
            recording.observations.coalesced = first_batch;
        }
        Ok(batch.revocation)
    }

    fn first(&self) -> Option<SessionEvent> {
        self.0.borrow().observations.first
    }
}

pub(super) struct Probe {
    scenario: Scenario,
    source: RecordedSource,
    expires: Instant,
    armed: bool,
    revoked: bool,
    cleared: bool,
}

impl Probe {
    pub(super) fn new(scenario: Scenario) -> Result<Self, &'static str> {
        let native =
            MacosEventSource::for_desktop_loop().map_err(|_| "native source unavailable")?;
        Ok(Self {
            scenario,
            source: RecordedSource(Rc::new(RefCell::new(Recording {
                native,
                expected_return: scenario.events().1,
                observations: Observations {
                    first: None,
                    returned: false,
                    coalesced: false,
                },
            }))),
            expires: Instant::now() + WINDOW,
            armed: false,
            revoked: false,
            cleared: false,
        })
    }

    pub(super) fn source(&self) -> DesktopSource {
        DesktopSource::Probe(self.source.clone())
    }

    pub(super) fn next_tick(&self) -> Instant {
        self.expires.min(Instant::now() + Duration::from_secs(1))
    }

    pub(super) fn arm(&mut self, state: State) -> bool {
        if self.armed || state != State::Enrollment || self.source.first().is_some() {
            return false;
        }
        self.armed = true;
        self.expires = Instant::now() + WINDOW;
        println!(
            "ARMED {:?} for {} awake seconds",
            self.scenario,
            WINDOW.as_secs()
        );
        true
    }

    pub(super) fn acknowledge_clear(&mut self, state: State) -> bool {
        if !self.revoked || state != State::Locked {
            return false;
        }
        self.cleared = true;
        true
    }

    /// Finish only after the desktop loop locked on the expected native event,
    /// its corresponding return event arrived, and the renderer cleared both fields.
    pub(super) fn advance(&mut self, state: State) -> Result<bool, &'static str> {
        if Instant::now() >= self.expires {
            return Err("timed out");
        }
        if state == State::Unavailable {
            return Err("desktop source unavailable");
        }
        let (expected, returned) = self.scenario.events();
        if let Some(first) = self.source.first() {
            if !self.armed || first != expected || state != State::Locked {
                eprintln!("OBSERVED {first:?}");
                return Err("unexpected first revocation");
            }
            if !self.revoked {
                self.revoked = true;
                println!("REVOKED {first:?}");
            }
            // Pilot has closed the access context. Continue sampling the same
            // source solely to record the return transition.
            self.source
                .collect()
                .map_err(|_| "return source unavailable")?;
            let recording = self.source.0.borrow();
            if self.cleared && recording.observations.returned {
                let coalesced = recording.observations.coalesced;
                println!(
                    "PASS {:?}: {returned:?}; coalesced={coalesced}; synthetic fields cleared; session locked",
                    self.scenario
                );
                return Ok(true);
            }
        } else if state == State::Locked {
            return Err("lock without the required native observation");
        }
        Ok(false)
    }
}
