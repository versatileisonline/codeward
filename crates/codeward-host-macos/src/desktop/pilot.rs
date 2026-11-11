use std::time::{Duration, Instant};

use codeward_core::transaction::{
    CallerId, ControlError, HeadlessControlLoop, HeadlessEventSource, PublishError, PublishMode,
    Storage, StorageError, VaultOpenMode, WriterLease,
};
use serde::Serialize;

const PILOT_CALLER: CallerId = CallerId(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum State {
    Closed,
    Opening,
    Enrollment,
    Locked,
    Unavailable,
}

pub(super) struct Pilot<E: HeadlessEventSource> {
    control: Option<HeadlessControlLoop<Absent, E>>,
    state: State,
}

impl<E: HeadlessEventSource> Pilot<E> {
    pub(super) fn new() -> Self {
        Self {
            control: None,
            state: State::Closed,
        }
    }

    pub(super) fn open(&mut self, source: impl FnOnce() -> Option<E>) -> State {
        if self.control.is_some() {
            return self.tick();
        }
        let Some(source) = source() else {
            self.state = State::Unavailable;
            return self.state;
        };
        match HeadlessControlLoop::begin_open(
            source,
            PILOT_CALLER,
            VaultOpenMode::Enrollment,
            || Ok(Absent),
        ) {
            Ok(control) => {
                self.control = Some(control);
                self.state = State::Opening;
            }
            Err(ControlError::Revoked) => self.state = State::Locked,
            Err(_) => self.state = State::Unavailable,
        }
        self.tick()
    }

    pub(super) fn lock(&mut self) -> State {
        if self.control.is_some() {
            self.close(State::Locked);
        }
        self.state
    }

    pub(super) fn state(&self) -> State {
        self.state
    }

    pub(super) fn tick(&mut self) -> State {
        if let Some(control) = &mut self.control {
            match control.step(Duration::ZERO) {
                Ok(update) if update.source_available => {
                    if update.revoked {
                        self.close(State::Locked);
                    } else if let Some(opening) = update.opening {
                        if opening.is_ok() {
                            self.state = State::Enrollment;
                        } else {
                            self.close(State::Unavailable);
                        }
                    }
                }
                _ => self.close(State::Unavailable),
            }
        }
        self.state
    }

    pub(super) fn next_tick(&self) -> Option<Instant> {
        self.control
            .as_ref()
            .map(|control| Instant::now() + control.next_poll_in(Duration::from_secs(1)))
    }

    fn close(&mut self, state: State) {
        if let Some(control) = self.control.take() {
            control.close();
        }
        self.state = state;
    }
}

// Volatile enrollment has no filesystem adapter, so even an accidental write
// invocation cannot stage, publish or persist a candidate in this pilot.
struct Absent;

impl Storage for Absent {
    type Lease = Self;

    fn acquire(self) -> Result<Self, StorageError> {
        Ok(self)
    }
}

impl WriterLease for Absent {
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(None)
    }

    fn stage(&mut self, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn publish(&mut self, _: PublishMode) -> Result<(), PublishError> {
        Err(PublishError::NotPublished)
    }

    fn persist(&mut self) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc, thread};

    use codeward_core::transaction::{EventSourceError, SessionEvent};

    use super::*;

    #[derive(Clone, Default)]
    struct Source {
        event: Rc<Cell<Option<SessionEvent>>>,
        failed: Rc<Cell<bool>>,
    }

    impl HeadlessEventSource for Source {
        fn take_revocation(&mut self) -> Result<Option<SessionEvent>, EventSourceError> {
            if self.failed.get() {
                Err(EventSourceError)
            } else {
                Ok(self.event.take())
            }
        }

        fn wait(&mut self, _: Duration) -> Result<(), EventSourceError> {
            panic!("a desktop tick must never wait or pump the source");
        }
    }

    fn wait_for_enrollment(pilot: &mut Pilot<Source>) {
        let timeout = Instant::now() + Duration::from_secs(5);
        loop {
            match pilot.tick() {
                State::Enrollment => return,
                State::Opening => (),
                state => panic!("unexpected pilot state: {state:?}"),
            }
            assert!(Instant::now() < timeout);
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn queued_revocation_wins_against_an_open_request() {
        let source = Source::default();
        let mut pilot = Pilot::new();
        pilot.open(|| Some(source.clone()));
        wait_for_enrollment(&mut pilot);
        assert!(pilot.next_tick().is_some());
        source.event.set(Some(SessionEvent::Suspend));
        assert_eq!(
            pilot.open(|| panic!("revoked command must not reopen")),
            State::Locked
        );
        assert!(pilot.next_tick().is_none());
        assert_eq!(pilot.open(|| None), State::Unavailable);
        pilot.open(|| Some(source.clone()));
        wait_for_enrollment(&mut pilot);
        assert_eq!(pilot.lock(), State::Locked);
        assert_eq!(pilot.tick(), State::Locked);
    }

    #[test]
    fn late_open_cannot_override_lock_or_source_failure() {
        let source = Source::default();
        let mut pilot = Pilot::new();
        pilot.open(|| Some(source.clone()));
        assert_eq!(pilot.lock(), State::Locked);
        assert_eq!(pilot.tick(), State::Locked);
        pilot.open(|| Some(source.clone()));
        wait_for_enrollment(&mut pilot);
        source.failed.set(true);
        assert_eq!(pilot.tick(), State::Unavailable);
        assert!(pilot.next_tick().is_none());
    }
}
