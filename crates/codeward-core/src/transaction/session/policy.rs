use std::time::{Duration, Instant};

use super::*;

/// Accepted fixed inactivity interval; not a UI-configurable timeout.
pub const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(300);

/// Only these scoped window interactions may refresh the inactivity deadline.
/// They are usability hints, not proof of trusted human presence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionActivity {
    /// Keyboard interaction in the Codeward window.
    Keyboard,
    /// Pointer interaction in the Codeward window.
    Pointer,
    /// Assistive-technology activation in the Codeward window.
    AssistiveActivation,
}

/// Nonsecret observations supplied by the trusted Rust host, not UI claims.
/// OS events and suspend-gap detection still need validated platform adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionEvent {
    /// Losing focus alone neither locks nor refreshes the inactivity deadline.
    FocusLost,
    /// Storage work, notifications and other progress never refresh inactivity.
    BackgroundProgress,
    /// The operating-system session locked.
    OsSessionLocked,
    /// The operating-system session switched away.
    SessionSwitchedAway,
    /// Suspend/sleep was reported.
    Suspend,
    /// Resume must leave the vault locked even if suspend was not observed.
    Resume,
    /// Reconnection must not restore access.
    Reconnected,
    /// A trusted adapter detected a suspend gap; no threshold is inferred here.
    DetectedSuspendGap,
}

/// Opaque activity context bound to one caller, opened vault and generation.
/// An identifier alone does not grant activity or vault access.
#[derive(Clone)]
pub struct ActivityRef {
    scope: Arc<()>,
    caller: CallerId,
    generation: u64,
}

pub(super) struct Inactivity {
    observed: Instant,
    deadline: Option<Instant>,
}

impl Inactivity {
    pub(super) fn new(now: Instant, active: bool) -> Result<Self, TransactionError> {
        let mut policy = Self {
            observed: now,
            deadline: None,
        };
        if active {
            policy.start(now)?;
        }
        Ok(policy)
    }

    fn start(&mut self, now: Instant) -> Result<(), TransactionError> {
        if now < self.observed {
            return Err(TransactionError::ClockUnavailable);
        }
        self.observed = now;
        self.deadline = Some(
            now.checked_add(INACTIVITY_TIMEOUT)
                .ok_or(TransactionError::ClockUnavailable)?,
        );
        Ok(())
    }

    pub(super) fn disarm(&mut self) {
        self.deadline = None;
    }

    fn expired(&mut self, now: Instant) -> bool {
        let regressed = now < self.observed;
        self.observed = self.observed.max(now);
        self.deadline
            .is_some_and(|deadline| regressed || now >= deadline)
    }
}

impl Session {
    /// Snapshot of the core-owned deadline for a trusted host to schedule a wakeup.
    /// `None` means no active access/authentication deadline, not saved/ready status.
    pub fn inactivity_deadline(&self) -> Option<Instant> {
        self.inactivity.deadline
    }

    /// Check the monotonic clock and revoke expired access/authentication without
    /// touching storage. Returns true only when this observation caused revocation.
    /// Hosts must schedule this while idle; this method starts no background timer.
    pub fn poll_inactivity(&mut self) -> bool {
        self.poll_inactivity_at(Instant::now())
    }

    fn poll_inactivity_at(&mut self, now: Instant) -> bool {
        if self.inactivity.expired(now) {
            self.lock();
            true
        } else {
            false
        }
    }

    pub(super) fn start_inactivity_at(&mut self, now: Instant) -> Result<(), TransactionError> {
        if let Err(error) = self.inactivity.start(now) {
            self.lock();
            return Err(error);
        }
        Ok(())
    }

    /// Obtain a scoped activity context for current access or pending authentication.
    /// Lock, a new attempt and close/reopen invalidate previously issued contexts.
    pub fn activity_reference(
        &mut self,
        caller: CallerId,
    ) -> Result<ActivityRef, TransactionError> {
        if caller != self.caller {
            return Err(TransactionError::Unauthorized);
        }
        self.poll_inactivity();
        if self.access.is_none() && self.authentication.is_none() {
            return Err(TransactionError::Locked);
        }
        Ok(ActivityRef {
            scope: self.scope.clone(),
            caller,
            generation: self.generation,
        })
    }

    /// Refresh only a current scoped interaction, checking expiry first. A late
    /// interaction cannot revive access. Rust samples time; no UI timestamp is used.
    pub fn record_activity(
        &mut self,
        caller: CallerId,
        reference: &ActivityRef,
        activity: SessionActivity,
    ) -> Result<(), TransactionError> {
        self.record_activity_at(caller, reference, activity, Instant::now())
    }

    fn record_activity_at(
        &mut self,
        caller: CallerId,
        reference: &ActivityRef,
        _activity: SessionActivity,
        now: Instant,
    ) -> Result<(), TransactionError> {
        if caller != self.caller
            || reference.caller != caller
            || !Arc::ptr_eq(&reference.scope, &self.scope)
        {
            return Err(TransactionError::Unauthorized);
        }
        self.poll_inactivity_at(now);
        if reference.generation != self.generation
            || (self.access.is_none() && self.authentication.is_none())
        {
            return Err(TransactionError::Locked);
        }
        self.start_inactivity_at(now)
    }

    /// Apply a trusted host event. Security events revoke immediately; focus loss
    /// and background progress only check expiry. Resume/reconnection never unlock.
    /// Return true when this event revoked the current generation.
    pub fn observe_event(&mut self, event: SessionEvent) -> bool {
        match event {
            SessionEvent::FocusLost | SessionEvent::BackgroundProgress => self.poll_inactivity(),
            SessionEvent::OsSessionLocked
            | SessionEvent::SessionSwitchedAway
            | SessionEvent::Suspend
            | SessionEvent::Resume
            | SessionEvent::Reconnected
            | SessionEvent::DetectedSuspendGap => {
                self.lock();
                true
            }
        }
    }
}

#[cfg(test)]
mod tests;
