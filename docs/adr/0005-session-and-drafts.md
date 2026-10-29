# ADR-0005: Sessions, locking and drafts

Status: Accepted — concrete session/draft contract, 2026-10-29.
Acceptance PR: pending publication. Target: Milestone 1. No implemented session behavior.

## Context and alternatives

FR-04/05/08 and NFR-05/07/09 require understandable save states and prompt access
revocation. Retaining drafts across lock would retain sensitive plaintext or require
a separate encrypted draft store. Autosaving changes would conflict with explicit
save intent. Recommend volatile drafts and security locking that does not wait for
a user decision; make potential draft loss clear before it happens.

## Session policy

- Lock after five minutes without interaction in the Codeward window. Rust owns a
  monotonic deadline; scoped keyboard/pointer/assistive activation activity may
  refresh it. Background progress, notifications, animations and storage work do not.
  Input events are usability hints, not proof of trusted human presence.
- Lock on manual request, OS session lock, session switch away and suspend/sleep.
  On resume/reconnection, remain locked. Treat a detected suspend gap as a lock
  even if the expected OS notification was missed. Validate events per platform;
  lack of an adapter is a support limitation, not assumed protection.
- Losing window focus alone does not lock; users may switch to a website while
  referring to a credential. The inactivity deadline still applies.
- Lock revokes normal access, increments the core's session generation, cancels
  secret-returning requests, invalidates draft/recovery/enrollment references and
  promptly redacts UI input/display. New privileged requests are denied.
- Every secret-bearing response is scoped to caller/window, vault and current
  session generation. Discard stale responses after lock, switch, close or unlock.
  Notifications and post-lock commit status contain no credential data.
- The same inactivity/security-lock policy cancels volatile enrollment or restricted
  recovery. Uncommitted enrollment must restart; no incomplete vault is marked ready.

## Draft, close and commit policy

| Situation | Accepted behavior |
| --- | --- |
| Generate/edit without Save | Volatile unsaved draft; no disk/browser persistence |
| Manual/automatic/OS security lock with draft | Lock immediately and discard draft; do not delay for a confirmation or silently save |
| Ordinary close with draft | Offer Save, Discard, Cancel; Save closes only after proven commit; Cancel keeps editing |
| Definite failed save while unlocked | Keep draft available for explicit retry/discard; report failure |
| Uncertain save | Display uncertainty, block further mutations and reconcile under ADR-0003; Discard does not undo a possible commit |
| Security lock during ordinary close dialog | Lock wins; redact/discard volatile input |

On lock, revoke UI access immediately. A commit not yet admitted to publication
may be canceled while preserving the live vault. Once publication is admitted,
resolve the file transaction under ADR-0003, without returning secrets or
reopening the session. Retain only the minimum transaction context until resolution;
release key material as soon as encryption no longer needs it. A stuck OS operation
must not keep the UI unlocked. Never publish a previously canceled candidate later.

No timeout or cancellation can assert rollback of an already published file.
An unresolved commit remains unresolved across locking/closing; restart validates
the actual file. Final status is targeted and generation-aware. Do not make the
Save/Discard dialog appear to reverse a possibly committed operation.

## Display, generation and memory

Passwords are masked by default. Explicit reveal is limited to the selected record,
at most 30 seconds, and ends immediately on lock, navigation or loss of focus.
Authentication secrets are cleared from forms after use; never retain a wrapping
key merely for convenient future password changes. No app clipboard command is
included initially. OS-level copying/history or a compromised endpoint is not
controlled by this policy.

Recommend a 24-character generated-password default, configurable from 12 to 128,
uniformly sampled without modulo bias from the chosen nonempty character set.
Offer ASCII lowercase, uppercase, digits and `!@#$%^&*()-_=+[]{}:,.?`; require at
least two distinct available characters. Do not force one character from each
class or promise compatibility with every website. Generation never saves.

Accept master passwords as exact UTF-8, 1–4,096 bytes; offer long-passphrase guidance
without silently trimming/normalizing input or inventing a format migration.
Require confirmation when setting a new master password. Recovery has its separate
strict transcription rules. UI form bounds must agree with Rust validation.

Minimize plaintext copies; clear forms and owned Rust secret buffers on completion,
cancel and lock. Do not persist sensitive state in logs, browser storage or crash
reporting. Review allocator/KDF/hash temporaries, swap and OS dumps. Clearing UI
references or using a zeroization helper does not prove complete erasure or prevent
observation by a compromised unlocked UI/OS.

## Validation before relying on the policy

Test inactivity deadlines, permitted resets, OS events and resume on every declared
platform. Exercise every row above, including locks while deriving, encrypting,
publishing, showing errors or waiting on close. Inject late replies after lock and
reunlock; verify no secret reappears. Verify keyboard and screen-reader access to
save states and dialogs, with focus moved safely after lock. Save states are
`Unsaved`, `Saving`, `Saved`, `Failed` and `OutcomeUnknown`; show Saved only after
the core reports a committed revision. Inspect logs/temp/UI persistence with
synthetic secrets. These are future tests, not results.
