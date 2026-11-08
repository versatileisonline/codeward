# Headless timer and macOS session events

[`HeadlessControlLoop`](../crates/codeward-core/src/transaction/event_loop.rs)
drives the existing [headless host](headless-vault-host.md) under ADR-0005.
Connect the event source before opening the vault. Keep the source and loop on
the same control thread. Call `step` repeatedly and route one trusted operation
at a time through `command`.

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

## macOS source

[`MacosEventSource`](../crates/codeward-host-macos/src/lib.rs) registers on the OS
main thread. A small Objective-C bridge uses the shared `NSWorkspace` notification
center for will-sleep, did-wake, session-resign and session-become-active. It pumps
the default Core Foundation run-loop mode, including nonblocking collection at
command and reply boundaries. Pending observations coalesce, and the source
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
save after source failure. macOS tests cover clock brackets, native wait bounds
and mappings. A main-thread smoke test registers the real bridge and posts
workspace notifications within its own process, then verifies they revoke an
enrollment through the control loop. It does not lock or suspend the machine.

Actual OS lock/sleep delivery, GUI redaction and integration with the desktop
event loop remain to be exercised. Synchronous open and candidate preparation
can delay native event collection. An event first collected after a command may
arrive after a write already crossed publication admission. The existing commit
and reconciliation rules govern that write. Windows/Linux native events, drafts,
IPC and packaging are separate work.
