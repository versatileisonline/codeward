//! Deadline scheduling and trusted event ordering for the headless host.

use std::{
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

use super::{
    CallerId, HeadlessVault, HostCompletion, HostError, SessionEvent, Storage, TransactionError,
    WriterLease, host::spawn_worker,
};

const WORKER_POLL: Duration = Duration::from_millis(20);

/// The platform event source could not collect observations or wait safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventSourceError;

/// Trusted platform adapter, owned by the control thread.
pub trait HeadlessEventSource {
    /// Collect pending revocation before any command or reply. Return only
    /// revoking events. Coalesce multiple notices into one revocation.
    fn take_revocation(&mut self) -> Result<Option<SessionEvent>, EventSourceError>;

    /// Maximum interval between observations, including on a host-driven loop.
    /// A source that requires no periodic checks can use the default.
    fn max_poll_interval(&self) -> Duration {
        Duration::MAX
    }

    /// Service native events for at most the supplied relative timeout. An
    /// earlier or spurious wake is valid. This call must never refresh activity.
    fn wait(&mut self, timeout: Duration) -> Result<(), EventSourceError>;
}

/// A command was refused or its result was revoked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlError {
    /// A trusted event or inactivity revoked access at the command boundary.
    /// Discard older access and redaction-sensitive state on this signal.
    Revoked,
    /// The event source failed and access was revoked. Discard older access and
    /// redaction-sensitive state and reconstruct the loop with a working source.
    EventSourceUnavailable,
    /// The underlying headless host refused the command.
    Host(HostError),
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Revoked => f.write_str("headless access revoked"),
            Self::EventSourceUnavailable => f.write_str("headless event source unavailable"),
            Self::Host(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ControlError {}

/// A control-loop iteration's observations. Transaction outcomes can arrive
/// even after revocation or source failure. They carry no credentials.
pub struct ControlUpdate {
    /// Access was revoked during this iteration.
    pub revoked: bool,
    /// Whether the source can still be used for commands and waits.
    pub source_available: bool,
    /// Result of opening a vault or starting enrollment. Enrollment remains
    /// volatile. Revoked openings do not report a result.
    pub opening: Option<Result<(), HostError>>,
    /// A current authentication result or independent transaction outcome.
    pub completion: Option<HostCompletion>,
}

/// Which storage state a trusted caller intends to open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultOpenMode {
    /// Read an existing vault and leave it locked.
    Existing,
    /// Start volatile enrollment only if the destination is absent.
    Enrollment,
}

enum VaultState<L: WriterLease + Send + 'static> {
    Opening(Receiver<Result<Box<HeadlessVault<L>>, HostError>>),
    Open(Box<HeadlessVault<L>>),
    Closed,
}

/// Drives a vault and event source together on one control thread. The caller
/// connects the source before opening the vault and repeatedly calls `step`.
/// While a worker is active, result polling is capped at 20 ms intervals. Idle
/// waits use the core's current monotonic deadline and the caller's wait bound.
pub struct HeadlessControlLoop<L: WriterLease + Send + 'static, E: HeadlessEventSource> {
    vault: VaultState<L>,
    source: E,
    source_available: bool,
}

impl<L: WriterLease + Send + 'static, E: HeadlessEventSource> HeadlessControlLoop<L, E> {
    /// Bind a connected event source to the headless vault.
    pub fn new(host: HeadlessVault<L>, source: E) -> Self {
        Self {
            vault: VaultState::Open(Box::new(host)),
            source,
            source_available: true,
        }
    }

    /// Start opening on a worker. The factory resolves storage on that worker,
    /// and may return a controlled error before acquisition. Drive this loop
    /// immediately to keep checking native events and receive the opening result.
    /// Close or revocation discards late results and releases their leases when
    /// the worker returns. To retry an opening, construct a new loop.
    pub fn begin_open<S: Storage<Lease = L>>(
        source: E,
        caller: CallerId,
        mode: VaultOpenMode,
        storage: impl FnOnce() -> Result<S, TransactionError> + Send + 'static,
    ) -> Result<Self, ControlError> {
        let mut control = Self {
            vault: VaultState::Closed,
            source,
            source_available: true,
        };
        control.check_command()?;
        let receiver = spawn_worker(move || {
            let storage = storage()?;
            match mode {
                VaultOpenMode::Existing => HeadlessVault::open(storage, caller),
                VaultOpenMode::Enrollment => HeadlessVault::begin_create(storage, caller),
            }
            .map(Box::new)
        })
        .map_err(ControlError::Host)?;
        control.vault = VaultState::Opening(receiver);
        Ok(control)
    }

    /// Run one trusted host command with native events and expiry checked before
    /// and after it. A revocation at the second check discards the return value.
    /// A write already admitted can still finish and report through `step`.
    /// The closure must return its output rather than delivering it itself.
    pub fn command<T>(
        &mut self,
        command: impl FnOnce(&mut HeadlessVault<L>) -> Result<T, HostError>,
    ) -> Result<T, ControlError> {
        self.check_command()?;
        let result = match &mut self.vault {
            VaultState::Open(host) => command(host),
            VaultState::Opening(_) => Err(TransactionError::Busy.into()),
            VaultState::Closed => Err(TransactionError::Locked.into()),
        };
        self.check_command()?;
        result.map_err(ControlError::Host)
    }

    /// Collect ready events or wait once, capped by the nearest deadline.
    /// An external driver can pass zero to skip waiting and schedule its next
    /// tick using `next_poll_in`.
    /// Source failure revokes access and prevents future commands. Pending
    /// transaction outcomes can still be drained with nonblocking iterations.
    pub fn step(&mut self, max_wait: Duration) -> Result<ControlUpdate, HostError> {
        let update = self.collect()?;
        if update.revoked
            || !update.source_available
            || update.opening.is_some()
            || update.completion.is_some()
            || max_wait.is_zero()
        {
            return Ok(update);
        }
        let failed = self.source.wait(self.next_poll_in(max_wait)).is_err();
        if failed {
            self.fail_source();
        }
        let mut update = self.collect()?;
        update.revoked |= failed;
        Ok(update)
    }

    /// Bound the delay until the next poll by the inactivity deadline, pending
    /// worker work, and any periodic source checks. An external driver must
    /// reschedule after commands and polls, and may poll earlier on native events.
    pub fn next_poll_in(&self, max_wait: Duration) -> Duration {
        let max_wait = if self.source_available {
            max_wait.min(self.source.max_poll_interval())
        } else {
            max_wait
        };
        let (deadline, pending) = match &self.vault {
            VaultState::Open(host) => (host.inactivity_deadline(), host.has_pending_work()),
            VaultState::Opening(_) => (None, true),
            VaultState::Closed => (None, false),
        };
        next_wait(Instant::now(), deadline, pending, max_wait)
    }

    /// Revoke and release the control loop without waiting for workers.
    /// An opening or admitted write can hold its lease until the worker exits.
    pub fn close(self) {
        if let VaultState::Open(host) = self.vault {
            (*host).close();
        }
    }

    /// Revoke access or cancel an in-flight opening immediately.
    pub fn lock(&mut self) {
        match &mut self.vault {
            VaultState::Open(host) => host.lock(),
            VaultState::Opening(_) => self.vault = VaultState::Closed,
            VaultState::Closed => (),
        }
    }

    fn check_command(&mut self) -> Result<(), ControlError> {
        let revoked = self.synchronize();
        if !self.source_available {
            return Err(ControlError::EventSourceUnavailable);
        }
        if revoked {
            return Err(ControlError::Revoked);
        }
        Ok(())
    }

    fn collect(&mut self) -> Result<ControlUpdate, HostError> {
        let mut revoked = self.synchronize();
        let (mut completion, opened) = match &mut self.vault {
            VaultState::Open(host) => (host.poll()?, None),
            VaultState::Opening(receiver) => (
                None,
                match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(TryRecvError::Disconnected) => Some(Err(HostError::WorkerUnavailable)),
                    Err(TryRecvError::Empty) => None,
                },
            ),
            VaultState::Closed => (None, None),
        };
        revoked |= self.synchronize();
        if revoked && matches!(completion, Some(HostCompletion::Authentication(_))) {
            completion = None;
        }
        let mut opening = None;
        if let Some(result) = opened {
            self.vault = VaultState::Closed;
            if !revoked {
                match result {
                    Ok(mut host) => {
                        revoked = host.take_inactivity_revocation();
                        if !revoked {
                            self.vault = VaultState::Open(host);
                            opening = Some(Ok(()));
                        }
                    }
                    Err(error) => opening = Some(Err(error)),
                }
            }
        }
        Ok(ControlUpdate {
            revoked,
            source_available: self.source_available,
            opening,
            completion,
        })
    }

    fn synchronize(&mut self) -> bool {
        let mut revoked = false;
        if self.source_available {
            match self.source.take_revocation() {
                Ok(Some(event)) => {
                    if matches!(
                        event,
                        SessionEvent::FocusLost | SessionEvent::BackgroundProgress
                    ) {
                        self.fail_source();
                    } else {
                        self.lock();
                    }
                    revoked = true;
                }
                Ok(None) => (),
                Err(_) => {
                    self.fail_source();
                    revoked = true;
                }
            }
        }
        if let VaultState::Open(host) = &mut self.vault {
            revoked |= host.take_inactivity_revocation();
        }
        revoked
    }

    fn fail_source(&mut self) {
        self.source_available = false;
        self.lock();
    }
}

fn next_wait(
    now: Instant,
    deadline: Option<Instant>,
    worker_pending: bool,
    max_wait: Duration,
) -> Duration {
    let wait = deadline.map_or(max_wait, |deadline| {
        max_wait.min(deadline.saturating_duration_since(now))
    });
    if worker_pending {
        wait.min(WORKER_POLL)
    } else {
        wait
    }
}

#[cfg(test)]
mod tests;
