//! Deadline scheduling and trusted event ordering for the headless host.

use std::time::{Duration, Instant};

use super::{HeadlessVault, HostCompletion, HostError, SessionEvent, WriterLease};

const WORKER_POLL: Duration = Duration::from_millis(20);

/// The platform event source could not collect observations or wait safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventSourceError;

/// Trusted platform adapter, owned by the control thread.
pub trait HeadlessEventSource {
    /// Collect pending revocation before any command or reply. Return only
    /// revoking events. Coalesce multiple notices into one revocation.
    fn take_revocation(&mut self) -> Result<Option<SessionEvent>, EventSourceError>;

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
    /// A current authentication result or independent transaction outcome.
    pub completion: Option<HostCompletion>,
}

/// Drives a vault and event source together on one control thread. The caller
/// connects the source before opening the vault and repeatedly calls `step`.
/// While a worker is active, result polling is capped at 20 ms intervals. Idle
/// waits use the core's current monotonic deadline and the caller's wait bound.
pub struct HeadlessControlLoop<L: WriterLease + Send + 'static, E: HeadlessEventSource> {
    host: HeadlessVault<L>,
    source: E,
    source_available: bool,
}

impl<L: WriterLease + Send + 'static, E: HeadlessEventSource> HeadlessControlLoop<L, E> {
    /// Bind a connected event source to the headless vault.
    pub fn new(host: HeadlessVault<L>, source: E) -> Self {
        Self {
            host,
            source,
            source_available: true,
        }
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
        let result = command(&mut self.host);
        self.check_command()?;
        result.map_err(ControlError::Host)
    }

    /// Collect ready events or wait once, capped by the nearest deadline.
    /// Source failure revokes access and prevents future commands. Pending
    /// transaction outcomes can still be drained with nonblocking iterations.
    pub fn step(&mut self, max_wait: Duration) -> Result<ControlUpdate, HostError> {
        let update = self.collect()?;
        if update.revoked
            || !update.source_available
            || update.completion.is_some()
            || max_wait.is_zero()
        {
            return Ok(update);
        }
        let timeout = next_wait(
            Instant::now(),
            self.host.inactivity_deadline(),
            self.host.has_pending_work(),
            max_wait,
        );
        let failed = self.source.wait(timeout).is_err();
        if failed {
            self.fail_source();
        }
        let mut update = self.collect()?;
        update.revoked |= failed;
        Ok(update)
    }

    /// Revoke and release the control loop. Admitted writes retain their lease
    /// until the worker exits, as with `HeadlessVault::close`.
    pub fn close(self) {
        self.host.close();
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
        let mut completion = self.host.poll()?;
        revoked |= self.synchronize();
        if revoked && matches!(completion, Some(HostCompletion::Authentication(_))) {
            completion = None;
        }
        Ok(ControlUpdate {
            revoked,
            source_available: self.source_available,
            completion,
        })
    }

    fn synchronize(&mut self) -> bool {
        let mut revoked = false;
        if self.source_available {
            match self.source.take_revocation() {
                Ok(Some(event)) => {
                    if !self.host.observe_event(event) {
                        self.fail_source();
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
        self.host.take_inactivity_revocation() || revoked
    }

    fn fail_source(&mut self) {
        self.source_available = false;
        self.host.lock();
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
