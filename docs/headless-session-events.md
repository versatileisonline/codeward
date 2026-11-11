# Headless timer and macOS session events

[`HeadlessControlLoop`](../crates/codeward-core/src/transaction/event_loop.rs)
drives the existing [headless host](headless-vault-host.md) under ADR-0005.
Connect the event source before opening the vault. Keep the source and loop on
the same control thread. Route one trusted operation at a time through `command`.

## Ordering and timeouts

- Each iteration collects pending revocation before draining worker results.
  It also checks immediately before and after a command. A queued lock wins
  before the command starts. Revocation at the second check discards the
  command reply, including any credential page. Clear prior access state and
  retained replies on `Revoked`, `EventSourceUnavailable` or the update's
  revocation flag.
- The wait is bounded by the core's five-minute inactivity deadline, the
  caller's wait bound, and a 20 ms worker poll while work is outstanding.
  At the deadline the loop polls the core even when there is no active worker.
  Timer and worker wakeups do not count as activity. If the command itself
  notices expiry, the loop still reports that revocation once.
- Source failure revokes access and prevents more commands on that loop.
  A caller can still drain safe transaction outcomes. An already admitted
  save can commit through lock, expiry, or event-source failure. An obsolete
  authentication completion cannot restore access.

### Opening storage without blocking event delivery

Call `HeadlessControlLoop::begin_open` with the connected source and a trusted
storage factory. The worker resolves storage, acquires exclusion and reads the
baseline. Until `ControlUpdate::opening` reports success, vault commands are
unavailable. Existing vaults open locked. Enrollment is volatile and only a
subsequent committed creation establishes readiness.

Lock, close, or source failure cancels delivery, including a result queued
before the final event check. A blocked storage call can finish later. Its
lease is released when the undeliverable result drops. After cancellation or
failure, create a fresh loop to retry.

Write preparation runs on a worker as well. After receiving a fresh candidate,
the control loop checks for revocation before another poll can start staging.
Keep scheduling while work is pending, even when a poll returns no completion.
After a confirmed save, the loop checks expiry and native events while a
separate worker refreshes the session snapshot. Revocation discards the refresh
result and preserves the committed write result. Refresh worker loss also
reports revocation and leaves the committed status available.

## macOS source

[`MacosEventSource`](../crates/codeward-host-macos/src/lib.rs) registers on the OS
main thread. A small Objective-C bridge uses the shared `NSWorkspace` notification
center for will-sleep, did-wake, session-resign and session-become-active. It pumps
the default Core Foundation run-loop mode in headless mode, including nonblocking
collection at command and reply boundaries. Pending observations coalesce, and the source
fails closed if it cannot complete a bounded collection. The bridge uses the
macOS SDK and system frameworks, with no new third-party packages or permissions.
[Apple NSWorkspace notifications](https://developer.apple.com/documentation/appkit/nsworkspace) ·
[Apple CFRunLoopRunInMode](https://developer.apple.com/documentation/corefoundation/cfrunloopruninmode%28_%3A_%3A_%3A%29)

The distributed `com.apple.screenIsLocked` and `com.apple.screenIsUnlocked`
names are undocumented, unauthenticated hints. Either can only revoke. The
adapter ignores sender and payload. A dropped or spoofed hint is possible, so
screen-lock delivery remains a support limitation and an unlock hint never
restores a vault. [Apple distributed notification constraints](https://developer.apple.com/documentation/foundation/distributednotificationcenter)

The source brackets the sleep-inclusive Mach clock with awake-clock reads.
A rise in the offset outside the overlapping uncertainty intervals is evidence
of a suspend gap. Invalid or regressing readings fail closed. Native idle waits
return after at most one second to resample even if a wake notice was missed.
This bounds the requested wait, with actual delivery still subject to the OS
scheduler and synchronous control work. [Apple Mach clock semantics](https://developer.apple.com/documentation/driverkit/mach_continuous_time)

## Evidence and remaining work

Core tests cover idle expiry, wait bounds, revocation before and after commands,
expiry observed inside a command, stale authentication, and draining an admitted
save after source failure. They also cover cancellation during a blocked open,
late result suppression, lease release and a failed baseline read. macOS tests
cover clock brackets, native wait bounds and mappings. A main-thread smoke test
registers the real bridge and posts workspace notifications within its own
process, then verifies they revoke an enrollment through the control loop.
It does not lock or suspend the machine.

### Driving the control loop from a desktop host

On macOS, construct `MacosEventSource::for_desktop_loop` on the main thread.
The desktop host owns native event delivery. Call `step(Duration::ZERO)` to
collect updates without waiting and reschedule using `next_poll_in` after each
command and poll, with an earlier tick when the host learns of a native event.
The bound accounts for idle expiry, the 20 ms pending-worker interval and
the one-second Mach clock recheck. This is a scheduling bound for the caller,
subject to the outer loop and synchronous work.

Command and reply checks consume observations already delivered to the
adapter and sample the clocks. They do not pump a nested native loop or
dispatch unrelated callbacks while the vault is borrowed. A delivered
revocation discards any sensitive command result at that boundary. An
attempt to wait inside the vault loop in desktop mode fails closed. The host
must also redact on worker failure, handle revocation before publishing other
updates, and recheck reply scope at delivery.

The process-local smoke test exercises this mode with synthetic enrollment.
It checks reply suppression, revocation on a nonwaiting tick, and that a
run-loop observer runs only when the outer driver pumps the loop. This leaves
packaged event delivery, prompt redaction and queued reply handling to the
desktop host tests.

### Live macOS probe

Run each probe separately from a logged-in macOS session with the repository
toolchain. The ordinary test suite runs the process-local smoke test. The live
flags opt into a passive probe of operator-driven OS transitions.

```sh
cargo test -p codeward-host-macos --locked --offline --test native_events -- --live-lock
cargo test -p codeward-host-macos --locked --offline --test native_events -- --live-sleep
```

Each probe prints `ARMED` once volatile synthetic enrollment can retrieve its
paper code. It observes events for up to 120 seconds of awake time without
logging the code, writing a vault, or changing OS power or lock state.

- The lock probe requires `OsSessionLocked` to cause the first revocation and
  then waits for `Reconnected` during the unlock transition. The adapter uses
  that same event for a screen-unlock hint or a workspace session becoming active.
- The sleep probe requires `Suspend`, `Resume` or `DetectedSuspendGap` to
  cause the first revocation. A `Suspend` result also requires a later wake
  observation or clock gap. A screen lock that arrives first cannot prove
  sleep caused the revocation, so the probe reports it and fails the attempt.
- Both probes require the source to stay available and the headless host to
  deny paper-code access after the observed transition. Inactivity, a failed
  event source, and a missing observation cannot pass.

Both operator-driven probes passed on Apple silicon macOS 26.3.1 (25D2128) on
2026-11-08. `OsSessionLocked` revoked enrollment before `Reconnected` and
`Suspend` revoked it before `Resume`. The host denied paper-code access after
both cycles. This exercise covered the volatile enrollment path in one logged-in
session. The clock-gap fallback retains unit-test coverage.

GUI redaction and desktop event-loop integration still require validation.
Synchronous host constructors can delay native event collection. Desktop
hosts can use the worker-backed opening path above.
An event first collected after a command may arrive after a write already
crossed publication admission. The existing commit and reconciliation rules
govern that write. Windows/Linux native events, drafts, IPC and packaging are
separate work.
