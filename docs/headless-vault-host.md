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
- Explicit content, creation and recovery candidates use the existing core
  request and publication policy. The encrypted writer moves off the control
  side until it reports a terminal or uncertain status. Lock cancels work before
  publication admission. An admitted write can commit while access stays locked.
- On an ordinary confirmed save, the host attempts snapshot refresh in the
  same generation. Refresh has its own result and cannot change the commit
  outcome. Known duplicate requests retain their status without replay. Fresh
  proof can reconcile an uncertain operation. A changed base after reconciliation
  requires new authentication. Unknown or failed results never acknowledge a save.
- Closing revokes access without joining an in-flight worker. The worker retains
  exclusion until it returns or exits. Reopen checks the actual named vault and
  starts a new request history. A lost worker fails closed and its old status may
  be unavailable on reopen.

## Event-loop boundary

Poll for worker results when `has_pending_work` is true and at
`inactivity_deadline`, including during a blocked commit or while idle. The
caller supplies wakeups and trusted activity/security events. Reading state or
draining results does not extend the deadline. The caller retains the original
scope of any queued credential page and rechecks it before delivery.

Opening storage and candidate preparation are synchronous. Creation and password
replacement still perform their preparation KDF on the invoking thread. Before
dispatch, expiry can cancel the prepared candidate without skipping worker cleanup.
Draft ownership, close dialogs, native event and timer wiring, IPC and UI remain
separate. The macOS writer remains opt-in for synthetic development.

## Evidence

Model tests exercise queued authentication revocation, lock before and after
admission, post-save refresh and deduplication, uncertain reconciliation, close
during a blocked barrier and loss of the writer. The managed APFS integration
test uses the host for creation, master unlock, save and refresh, close and reopen,
restricted paper recovery, password replacement and a fresh master unlock.
Local locked/offline checks passed all six host model tests, the managed macOS
host test and the five core compile-fail doctests. Workspace formatting and
all-feature/all-target Clippy with warnings denied also passed.
