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

    fn accepts_first(self, first: SessionEvent) -> bool {
        first == self.events().0
            || (matches!(self, Self::Sleep) && first == SessionEvent::OsSessionLocked)
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

#[derive(Default)]
struct Observations {
    first: Option<SessionEvent>,
    started: bool,
    returned: bool,
    coalesced: bool,
}

impl Observations {
    fn record(&mut self, revocation: Option<SessionEvent>, start: bool, returned: bool) {
        if self.first.is_none() {
            self.first = revocation;
        }
        if !self.returned && returned && (self.started || start) {
            self.returned = true;
            self.coalesced = !self.started && start;
        }
        self.started |= start;
    }
}

struct Recording {
    native: MacosEventSource,
    scenario: Scenario,
    observations: Observations,
}

#[derive(Clone)]
pub(super) struct RecordedSource(Rc<RefCell<Recording>>);

impl RecordedSource {
    fn collect(&self) -> Result<Option<SessionEvent>, EventSourceError> {
        let mut recording = self.0.borrow_mut();
        let batch = recording.native.take_observation()?;
        let (start, returned) = recording.scenario.events();
        recording.observations.record(
            batch.revocation,
            batch.includes(start),
            batch.includes(returned),
        );
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
                scenario,
                observations: Observations::default(),
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
        let (start, returned) = self.scenario.events();
        if let Some(first) = self.source.first() {
            if !self.armed || !self.scenario.accepts_first(first) || state != State::Locked {
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
                    "PASS {:?}: {start:?} and {returned:?}; first={first:?}; coalesced={coalesced}; synthetic fields cleared; session locked",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_after_screen_lock_still_requires_both_sleep_notices() {
        let mut seen = Observations::default();
        seen.record(Some(SessionEvent::OsSessionLocked), false, false);
        assert!(Scenario::Sleep.accepts_first(seen.first.unwrap()));
        seen.record(Some(SessionEvent::Resume), false, true);
        assert!(!seen.returned);
        seen.record(Some(SessionEvent::Suspend), true, false);
        assert!(!seen.returned);
        seen.record(Some(SessionEvent::Resume), false, true);
        assert!(seen.returned);
        assert!(!seen.coalesced);
        assert_eq!(seen.first, Some(SessionEvent::OsSessionLocked));
    }

    #[test]
    fn coalesced_sleep_pair_does_not_infer_arrival_order() {
        let mut seen = Observations::default();
        seen.record(Some(SessionEvent::Suspend), true, true);
        assert!(seen.returned && seen.coalesced);
        assert!(!Scenario::Lock.accepts_first(SessionEvent::Suspend));
        assert!(!Scenario::Sleep.accepts_first(SessionEvent::DetectedSuspendGap));
    }
}
