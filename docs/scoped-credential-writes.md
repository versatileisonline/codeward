# Scoped generated-password saves and credential deletion

The Rust session and headless host can apply a generated password to an existing
credential or explicitly delete one selected credential. These operations extend
the [draft lifecycle](password-drafts.md) under FR-03/04/05/08 and reuse the
existing [committed-write contract](adr/0003-committed-writes.md).

## Entry points

| Layer | Password replacement | Deletion |
| --- | --- | --- |
| Session | `prepare_generated_password(worker, caller, request, id, &draft)` | `prepare_delete_credential(worker, caller, generation, request, id)` |
| HeadlessVault | `save_generated_password(request, id, &draft)` | `delete_credential(generation, request, id)` |

`Request` carries the request ID and expected revision. Password saving requires
the current opaque `PasswordDraftRef`; the caller does not send the password or a
complete snapshot. Deletion requires current session generation and a canonical
selected record ID. Calling the named delete operation expresses explicit intent;
the future interface must provide its confirmation interaction.

The synchronous Session APIs return a pending `OperationRef`. The host methods
return once background preparation is queued. Poll the existing `HostCompletion`
surface: `PreparationFailed` is a controlled pre-submission failure; a transaction
completion carries operation status and separate authenticated-snapshot refresh.
Neither queuing nor preparing means Saved. Only a proven `Committed` revision
acknowledges a save/deletion, and an error refreshing the session cannot undo it.

## Authorization and exact changes

Require correct trusted caller/opened scope, ordinary master access, current
generation, current authenticated source and expected revision. A generated draft
also requires its current unique identity and exact source. No enrollment/recovery,
locked/expired, pending/uncertain, conflicted or unrefreshed access may mutate records.
Validate canonical IDs after those authorization/state gates. Missing IDs never
insert or recreate records; the worker reports `CredentialNotFound` without writes.

Password replacement preserves the exact ID, title, username, URL, notes, creation
time and every unrelated record. Rust chooses updated Unix milliseconds, bounded
by the existing format profile. If wall time moved backward, the prior update time
is retained; pre-epoch, unrepresentable or out-of-profile clock values return
`ClockUnavailable` without a write. Deletion removes exactly the selected record
and makes no undelete promise.

The host copies only the 32-byte canonical ID and at most 128 generated-password
bytes into zeroizing command inputs. The revocable preparation worker copies and
validates the full replacement snapshot, checks cancellation while copying, and
encrypts through the existing vault layer. No bulk decrypted data crosses a UI
boundary, and no snapshot copying/encryption is added to the host control thread.
Commands do not extend inactivity or touch storage before normal commit admission.

## Failures, duplicate requests and locking

The existing per-generation keyed intent digest separates whole-snapshot writes,
generated-password replacement and deletion. It includes expected revision and
exact record/password inputs. Known requests deduplicate before clock sampling,
snapshot rebuilding or encryption; changed intent with the same ID is `RequestReused`.
No IDs are evicted and no failed request automatically replays.

An unchanged, definitely failed save leaves the session draft available. Repeating
its known request reports the recorded failure; an explicit new request ID is
required to retry. Pending/unknown outcomes block further mutations and draft
access until existing fresh-proof reconciliation establishes actual bytes.
Reconciliation to unchanged bytes permits explicit retry; changed bytes require
refresh/reload. No failure, timeout, discard or lock implies rollback.

Current reference/state authorization always precedes command handling. A successful
changed-snapshot refresh invalidates the old draft. After that, use the original
operation reference/status to correlate a completed save; resending a stale draft
command cannot grant access or publish again. The same current-generation/revision
rules apply to deletion. No new durable request ledger is introduced.

Lock immediately releases session-owned drafts and revokes queued/unadmitted
preparation. Admitted publication may finish while locked and returns status only;
it never reopens the session or returns the saved password. The existing writer
still owns publication, barriers, uncertainty and lease lifetime.

## Verification and retained scope

Eight session and five host scenarios exercise exact field/record preservation,
reopening committed bytes, selected deletion/missing records, scoped references and
restricted/expired access, definite failure/explicit retry, request-intent reuse,
uncertain before/after publication, reconciliation and queued/admitted locks.
Timestamp tests cover profile endpoints, backward time and cancellation while
constructing snapshots. Native storage and whole-snapshot regression evidence
remain under their existing contracts.

```sh
cargo test -p codeward-core --lib --locked --offline transaction::session::mutations::tests
cargo test -p codeward-core --lib --locked --offline transaction::host::tests::mutations
```

Use Rust 1.99.1 and synthetic data. Record current-head/base/test-merge CI and
independent review in the PR. New-record creation, general field editing, ordinary
close dialogs, Tauri commands and rendered UI remain later integration. No
dependency, vault-format, cryptographic profile, native adapter, clipboard or
real-vault admission change is included; no milestone acceptance is implied.
