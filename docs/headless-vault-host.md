# Headless vault lifecycle

[`HeadlessVault`](../crates/codeward-core/src/transaction/host.rs) connects
the Rust session and transaction worker for one trusted caller. It covers
FR-01/02 and the control-side parts of ADR-0003/0005. The macOS test uses the
shared writer and a disposable managed APFS home under the
[accepted macOS policy](adr/0006-managed-working-location.md).

## Control and worker ownership

- Open acquires the writer and loads the encrypted baseline locked. Creation
  starts explicit volatile enrollment. A verified paper code and password can
  prepare creation. Only the worker's committed outcome establishes readiness.
- Unlock and paper authentication run on a worker. The control side owns the
  session, checks expiry and installs only the current result. A security lock
  drops the authentication receiver immediately. A running derivation can finish
  on its worker, which drops any undeliverable result. Paper authentication
  remains restricted.
- Explicit content, creation and recovery candidates use the same core request
  and publication policy as the synchronous transaction API. A preparation
  worker owns the supplied inputs, the original Rust access state and the
  writer lease while it validates, derives and encrypts. The control side
  retains revocation authority and restores access only from the current
  preparation. A worker that observes cancellation drops its access state.
- A fresh candidate waits for another control poll before storage work starts,
  giving the host an event and expiry check at the handoff. The storage worker
  receives encrypted transaction state. Lock cancels before publication
  admission. An admitted write can commit while access stays locked.
- After an ordinary confirmed save, a separate worker authenticates and decodes
  the exact committed snapshot. It borrows no session and holds no writer
  lease. The control side adopts the result only in the same generation and
  retains the committed status if refresh is canceled or fails. A lost refresh
  worker revokes access and requires reauthentication while keeping the writer
  and its request history.
- Known duplicate requests retain their status without replay. Fresh proof can
  reconcile an uncertain operation. A changed base after reconciliation requires
  new authentication. Unknown or failed results never acknowledge a save.
- Closing revokes access without joining workers. A worker that owns the lease
  retains exclusion until it returns or exits. A derivation or decode may
  finish after lock. Its result cannot restore revoked access. Reopen checks
  the actual named vault and starts a new request history. A lost storage
  worker fails closed and its old status may be unavailable on reopen.

## Write replies

`create`, `save` and `replace_master_password` return once work is queued.
`save` takes ownership of the validated snapshot. Poll while work is pending,
including after a poll that produced no completion. Preparation can report a
controlled `PreparationFailed` with the request ID. A transaction completion
carries the operation reference for status lookup and reconciliation. A duplicate
reports its existing status and does not replay the write. Only a committed
outcome establishes Saved, Ready or RecoveryComplete.
An ordinary save completion includes a separate refresh result. Revocation
can discard that work and report the proven commit while the refresh worker
finishes. Refresh never extends the inactivity deadline.

## Event-loop boundary

The [headless control loop](headless-session-events.md) polls for worker results
and at `inactivity_deadline`, including during a blocked commit or while idle.
The macOS source supplies platform observations. Reading state or draining
results does not extend the deadline. The caller retains the original scope of
any queued credential page and rechecks it before delivery.

The control loop can open storage on a worker, including managed-path preparation,
writer acquisition, and the baseline read. Its synchronous host constructors
remain available for headless tests. Draft ownership, close dialogs, packaged
desktop event integration, IPC and UI remain pending. The macOS writer remains
opt-in for synthetic development.

## Evidence

Model tests exercise queued authentication revocation, lock before and after
admission, post-save refresh and deduplication, uncertain reconciliation, close
during a blocked barrier and loss of the writer. Preparation tests cover suspend
during blocked enrollment, cancellation before staging, retry after
reauthentication and preservation of enrollment after invalid confirmation.
Refresh tests exercise expiry and source failure while decoding is blocked,
revocation at the reply check, and worker loss after commit. They verify that
the committed result survives and stale access stays revoked.
The managed APFS integration test uses the host for creation, master unlock,
save and refresh, close and reopen, restricted paper recovery, password
replacement and a fresh master unlock. The focused host and managed APFS tests
passed locally with the pinned toolchain.
