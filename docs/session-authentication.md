# Cancellable core authentication

This Milestone 2 slice implements part of [FR-02/NFR-09](requirements.md),
[ADR-0005](adr/0005-session-and-drafts.md) and the [UI/core contract](ui-core-contract.md).
It separates expensive authentication from the secret-owning session so a host can
revoke access while verification is running. It is a Rust API, not an IPC command,
application dispatcher or complete session lifecycle.

## Ownership and use

1. On a storage thread, `Committer::open_locked` acquires the trusted adapter's
   exclusion and bounded encrypted base. Parsing grants no access. An absent file
   returns `ReloadRequired`; explicit enrollment remains a separate workflow.
2. Keep `Session` on the control side. Supply trusted `CallerId` and call
   `begin_master_authentication` or `begin_recovery_authentication` against the
   idle worker. This revokes previous access and advances generation before
   preparing a job. Wrong caller/worker and busy/conflicted workers are refused
   before changing access.
3. Move the opaque `AuthenticationJob` to a worker thread and call consuming
   `run`. It owns a fallibly allocated encrypted-base copy and zeroizing bounded
   input, without borrowing session or storage. Master input preserves exact UTF-8
   within 1–4,096 bytes; paper input retains its 256-byte transcription cap.
4. The control side may call `lock` or drop the session at any time. Revocation
   cancels the attempt without acquiring a storage/authentication-worker mutex.
   The job checks cancellation before and after the existing authentication and
   strict schema gate. An already executing KDF runs to completion; it is not
   forcibly interrupted. The process-wide master-derivation guard is unchanged.
5. Promptly deliver the opaque `AuthenticationResult` to
   `complete_authentication`, or drop it after cancellation/close. Installation
   requires the same trusted caller, opened-worker scope, current attempt and
   generation, exact vault ID/revision/full-file SHA-256, and idle worker. A stale
   success or failure cannot unlock, replace or revoke a newer session. Errors
   release the result and leave the current failed attempt locked.

At most one queued/running job per session is allowed. Lock never waits for it,
but starting another job returns `Busy` until the old job completes or is dropped.
Completed results do not occupy that slot: a new attempt cancels the previous
result's authority even if it has not been delivered. Results are single-use Rust
secret owners, with no public keys/credentials or Clone/Debug implementations.

`reload_master` and `reload_recovery` are synchronous convenience wrappers over
the same job/completion path. Hosts requiring prompt lock must use the split-phase
API. Initial synchronous `open_master`/`open_recovery` remain convenience APIs;
`open_locked` provides the separated entry. Storage acquisition/read may itself
block, so it must also be scheduled away from the control/UI thread.

## Preserved boundaries and limits

Recovery completion installs restricted recovery access, never normal browsing or
`RecoveryComplete`. A replacement password still needs an explicit coherent commit.
Pending/uncertain/conflicted transactions cannot be cleared by authentication;
the existing worker owns reconciliation and acknowledgement. Enrollment cannot be
recovered from an absent vault by this API. Generation overflow fails closed.

Lock releases session-owned access. A running authentication still owns its input
and temporary key/plaintext buffers until it exits. A result already returned to
another Rust owner can retain authenticated access until delivery/drop; a host
must promptly drain stale completions. This is no erasure guarantee. Caller/UI
input copies need their own cleanup; neither job nor result may be serialized,
logged or exposed to a WebView. Keep error/status messages free of secret input.

No dependency, vault bytes/schema, primitive, KDF cost or native writer feature
changes. No background executor is started. The [core inactivity policy](session-inactivity.md)
now gates authentication completion and revokes timed-out attempts. Host timer
scheduling, OS lock/resume adapters, enrollment cancellation orchestration, draft
management, selected-record replies and UI redaction remain pending.
Native storage acceptance and Milestone 2 completion remain separate gates.

## Verification

Synthetic tests in [authentication.rs](../crates/codeward-core/src/transaction/tests/authentication.rs)
cover explicit master installation, channel-blocked authentication with immediate
lock, late successes/failures across lock/reunlock and replacement, queued-job
drop/cancellation, session drop/reopen, caller/worker isolation, input bounds,
invalid credentials/tampering, restricted recovery and commit gating, full-file
fingerprint changes, and active/uncertain/conflicted/absent worker policy.
The blocked test pauses a private test seam before real authentication; it does
not inject interruption inside Argon2 or establish OS event latency. Compile-fail
examples check consumed jobs and absence of result diagnostic formatting.
Recorded check results belong in [progress evidence](milestone-2-progress.md).
