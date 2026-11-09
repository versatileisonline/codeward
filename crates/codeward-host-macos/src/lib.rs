//! Main-thread macOS event source for the headless Codeward control loop.

#![cfg(target_os = "macos")]
#![deny(missing_docs, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc, time::Duration};

use codeward_core::transaction::{EventSourceError, HeadlessEventSource, SessionEvent};

const GAP_RECHECK: Duration = Duration::from_secs(1);

unsafe extern "C" {
    fn cw_macos_events_open() -> *mut c_void;
    fn cw_macos_events_close(source: *mut c_void);
    fn cw_macos_events_collect(source: *mut c_void, pump: bool, events: *mut u32) -> bool;
    fn cw_macos_events_wait(seconds: f64) -> bool;
    fn mach_absolute_time() -> u64;
    fn mach_continuous_time() -> u64;
}

/// macOS workspace and screen-lock observations. Construct and drive it on the
/// OS main thread of an interactive session. The bridge creates no windows.
/// Distributed screen-lock notices are best-effort revocation hints.
pub struct MacosEventSource {
    source: NonNull<c_void>,
    gap: SleepGap,
    pump_run_loop: bool,
    _main_thread: PhantomData<Rc<()>>,
}

impl MacosEventSource {
    /// Register observers for a headless driver that pumps the main run loop.
    pub fn new() -> Result<Self, EventSourceError> {
        Self::open(true)
    }

    /// Register observers before opening the vault on a desktop-owned main
    /// loop. The caller must drive native event delivery and schedule polls
    /// without waiting inside the vault control loop.
    pub fn for_desktop_loop() -> Result<Self, EventSourceError> {
        Self::open(false)
    }

    fn open(pump_run_loop: bool) -> Result<Self, EventSourceError> {
        let gap = SleepGap::new(sample())?;
        // SAFETY: the bridge rejects non-main threads and returns an owned
        // opaque Objective-C object only after successful registration.
        let source = NonNull::new(unsafe { cw_macos_events_open() }).ok_or(EventSourceError)?;
        Ok(Self {
            source,
            gap,
            pump_run_loop,
            _main_thread: PhantomData,
        })
    }
}

impl HeadlessEventSource for MacosEventSource {
    fn take_revocation(&mut self) -> Result<Option<SessionEvent>, EventSourceError> {
        let mut events = 0;
        // SAFETY: self owns a live source on its registered thread. The bridge
        // writes at most one u32 to a valid stack out pointer.
        if !unsafe {
            cw_macos_events_collect(self.source.as_ptr(), self.pump_run_loop, &mut events)
        } {
            return Err(EventSourceError);
        }
        let gap = self.gap.observe(sample())?;
        Ok(observation(events).or(gap.then_some(SessionEvent::DetectedSuspendGap)))
    }

    fn max_poll_interval(&self) -> Duration {
        GAP_RECHECK
    }

    fn wait(&mut self, timeout: Duration) -> Result<(), EventSourceError> {
        if !self.pump_run_loop {
            return Err(EventSourceError);
        }
        // SAFETY: this non-Send source is driven on the thread that opened it.
        // Duration yields a finite, nonnegative interval.
        if unsafe { cw_macos_events_wait(wait_seconds(timeout)) } {
            Ok(())
        } else {
            Err(EventSourceError)
        }
    }
}

fn wait_seconds(timeout: Duration) -> f64 {
    // Resample the clocks even if a wake notification was lost while idle.
    timeout.min(GAP_RECHECK).as_secs_f64()
}

impl Drop for MacosEventSource {
    fn drop(&mut self) {
        // SAFETY: uniquely owned bridge object from open, released once on the
        // same main thread after the final collect/wait.
        unsafe { cw_macos_events_close(self.source.as_ptr()) };
    }
}

fn observation(bits: u32) -> Option<SessionEvent> {
    // Keep the bit assignments in sync with enum CWEvent in events.m.
    [
        SessionEvent::OsSessionLocked,
        SessionEvent::SessionSwitchedAway,
        SessionEvent::Suspend,
        SessionEvent::Resume,
        SessionEvent::Reconnected,
    ]
    .into_iter()
    .enumerate()
    .find(|(bit, _)| bits & (1 << bit) != 0)
    .map(|(_, event)| event)
}

fn sample() -> [u64; 3] {
    // SAFETY: these libSystem functions take no arguments or pointers. The
    // awake readings bracket the continuous clock in the same Mach tick units.
    unsafe {
        [
            mach_absolute_time(),
            mach_continuous_time(),
            mach_absolute_time(),
        ]
    }
}

struct SleepGap {
    lower: i128,
    upper: i128,
    last: [u64; 3],
}

impl SleepGap {
    fn new(last: [u64; 3]) -> Result<Self, EventSourceError> {
        let [before, continuous, after] = last;
        if after < before {
            return Err(EventSourceError);
        }
        let continuous = i128::from(continuous);
        Ok(Self {
            lower: continuous - i128::from(after),
            upper: continuous - i128::from(before),
            last,
        })
    }

    fn observe(&mut self, sample: [u64; 3]) -> Result<bool, EventSourceError> {
        let next = Self::new(sample)?;
        if sample[0] < self.last[2] || sample[1] < self.last[1] || next.upper < self.lower {
            return Err(EventSourceError);
        }
        if next.lower > self.upper {
            *self = next;
            return Ok(true);
        }
        self.lower = self.lower.max(next.lower);
        self.upper = self.upper.min(next.upper);
        self.last = sample;
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
