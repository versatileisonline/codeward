# Core inactivity and security-event policy

This bounded Milestone 2 slice implements part of FR-02/08, NFR-05/09 and
[ADR-0005](adr/0005-session-and-drafts.md). The Rust `Session` owns a fixed
300-second `Instant` deadline for normal/restricted access, enrollment and pending
[authentication](session-authentication.md). It starts no background timer and
does not supply OS event adapters or complete application dispatch.

## Core behavior

- Privileged mutation preparation, enrollment-code display, activity admission and
  authentication completion check expiry. The exact deadline expires; activity
  arriving at or after it cannot restore access. `recovery_code` now takes mutable
  session access so it can revoke expired enrollment before returning a code.
- `activity_reference` supplies an opaque context bound to trusted caller, actual
  opened-worker scope and generation. `record_activity` accepts only typed
  keyboard, pointer or assistive activation for that current context. Lock, a new
  attempt and close/reopen invalidate old references. UI-provided timestamps are
  not accepted; Rust samples its monotonic clock. Input is still a usability hint,
  not evidence of a trusted human gesture.
- Only accepted activity refreshes an existing deadline. Merely obtaining a
  reference, completing authentication, saving, receiving notifications, losing
  focus or doing background work does not. Fresh authentication/enrollment starts
  a deadline; authentication completion keeps it rather than granting another
  five minutes. A locked session cannot be revived by activity.
- Typed host observations for OS session lock, switch away, suspend, resume,
  reconnection and a detected suspend gap call the existing immediate `lock` path.
  Resume/reconnection lock even if a prior suspend event was missed. Focus loss and
  background progress only poll expiry; focus loss alone does not lock.
- A regressing monotonic observation revokes an active deadline. New deadline
  creation uses checked arithmetic; regression or an unrepresentable deadline
  returns `ClockUnavailable` and leaves access revoked. Generation exhaustion
  remains fail-closed.

Expiry/security lock increments generation, releases session-owned access and
keys, invalidates enrollment and cancels pending authentication/publication
admission. A blocked authentication/storage worker is not needed to lock. Admitted
publication still resolves and may commit; uncertainty still blocks further
mutations under [ADR-0003](adr/0003-committed-writes.md). Timeout never proves rollback,
clears an unknown outcome or turns an unsaved candidate into Saved/Ready.

## Host obligations and remaining scope

The trusted Rust host must schedule `poll_inactivity` using
`inactivity_deadline`, deliver platform events, and promptly redact UI/input and
drain stale authentication results. `is_locked`/`generation` are state snapshots;
poll before composing a session-state response. Pending authentication has a
deadline even though `is_locked` is true until access installs. Polls do not touch
storage and repeated polls after revocation do not advance generation again.

Entry-point checks deny new expired privileged requests; idle revocation and
revocation during already executing work still require the control-side host to
run scheduled polls/events. No platform-independent suspend detector or gap
threshold is inferred from `Instant`: clock behavior across sleep and event
delivery require native evidence on every supported target. These tests do not
establish prompt OS notifications or UI redaction.

Draft ownership/close dialogs, timed reveal, host scheduling/dispatch, OS adapters
and packaged behavior remain pending. An executing KDF cannot be forcibly stopped
and a previously returned opaque result can retain access until delivery/drop.
Clearing owned buffers is not complete erasure. The slice retains the existing
dependencies, vault bytes/schema, crypto profile and native writer feature.
Synthetic use and native storage acceptance gates still apply.

## Evidence

Ten [policy tests](../crates/codeward-core/src/transaction/session/policy/tests.rs)
use private controlled observations without sleeps. They cover exact expiry,
refresh for all permitted activity kinds, late/stale/cross-vault activity,
focus/background behavior, all security events, normal/recovery/enrollment gates,
blocked/late/failed authentication, before/after publication admission, unknown outcomes,
clock regression and generation exhaustion. Overflow is guarded by checked
`Instant` arithmetic; no platform-specific maximum instant is fabricated in tests.
Current check results are recorded in [progress evidence](milestone-2-progress.md).
