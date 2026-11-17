# Scoped credential creation and general editing

The Rust session/headless host now accepts one bounded credential form for creation
or full editing. Together with [listing and selected reads](scoped-credential-reads.md),
[password drafts](password-drafts.md) and [targeted writes/deletion](scoped-credential-writes.md),
this completes the scoped record operations needed for the first desktop editor.

## Entry points and fields

| Layer | Create | Edit |
| --- | --- | --- |
| Session | `prepare_create_credential(worker, caller, generation, request, form)` | `prepare_update_credential(worker, caller, generation, request, id, form)` |
| HeadlessVault | `create_credential(generation, request, form)` | `update_credential(generation, request, id, form)` |

`CredentialForm` borrows exact title, username, URL, notes and a password choice:

- `CredentialPassword::Keep` preserves an existing password without revealing it
  to an edit form. It is invalid for creation.
- `Provided(&str)` replaces the password with exact entered UTF-8. An explicit
  empty credential password is valid and differs from Keep.
- `Generated(&PasswordDraftRef)` uses the current scoped generated draft, with no
  plaintext password round trip through the caller's save arguments.

Forms carry no ID or timestamp fields. Editing takes a separate selected canonical
ID and never inserts missing records. The form supplies all non-password fields;
this is not a partial patch operation. Schema bounds remain title 1–512 bytes,
username 0–1,024, password/URL 0–4,096 and notes 0–65,536. UTF-8, whitespace, NULs
and permitted empty fields are preserved exactly, without normalization or URL fetches.

Ordinary current access, trusted caller/opened scope, generation and expected
revision are checked before validating/copying form data. Generated references
also require their exact current source and identity. Locked, restricted,
enrollment, expired, pending/uncertain, conflicted and stale-source states cannot
save. No full-vault snapshot is accepted from or returned to a renderer.

## Identity, timestamps and results

After request deduplication, the worker generates 128 independent OS-random bits
and encodes a canonical 32-character lowercase hex ID. Collisions are retried at
most eight times, then fail without overwriting anything. The existing 10,000-record
limit is checked before entropy/time sampling or snapshot construction. ID entropy
failure, schema/capacity/clock errors and canceled preparation never imply a save.

Rust assigns bounded Unix milliseconds to both timestamps on creation. Editing
preserves ID and creation time, and keeps the maximum of prior update time and
current wall time. All unrelated records remain exact copies of authority.

Session preparation returns the existing pending operation reference. The host
queues background work and reports normal `HostCompletion` results: a controlled
`PreparationFailed`, or transaction outcome plus separate authenticated refresh.
Only a complete committed revision acknowledges saving. After successful refresh,
list credentials to obtain Rust-assigned IDs; no pending ID is treated as saved.
If refresh fails after a proven commit, the save remains committed and the session
requires reload. Use the original operation reference for status correlation.

## Retry, lifetime and locking

Keyed request intent binds operation, expected revision, target when editing,
every exact form field and the password keep/replacement choice. Provided and
generated passwords with identical resolved bytes have identical replacement
semantics. Keep and an explicit empty replacement have distinct intent.

Known requests deduplicate before new IDs/time, complete-snapshot copying and
encryption. Reusing an ID with changed intent is refused. A definite failed
creation cannot replay as a second record; an explicit new request is required
to retry a recorded failure. Uncertainty blocks mutations until existing fresh
proof establishes unchanged/committed bytes. Editing never becomes an upsert.
Current access/ref checks precede handling, including duplicate commands.

The control side copies only bounded single-record input into zeroizing buffers.
Complete-snapshot rebuilding, schema validation and encryption remain in
revocable worker preparation, with cancellation checks while copying records.
Locks revoke access immediately; queued/unadmitted work cancels, while admitted
publication may finish locked and reports status only. Confirmed changed-snapshot
refresh releases a used generated draft. Definite failure preserves that draft
while access remains valid, and no mutation refreshes inactivity.

Entered forms remain caller-owned volatile state; this API is not a persistent or
session-owned form-draft store. The future interface must clear its own input and
display copies on lock and enforce ordinary-close Save/Discard/Cancel behavior.
No diagnostics, cloning or automatic serialization are supplied for borrowed forms;
these types are not a completed Tauri wire schema. Zeroizing Rust ownership cannot
guarantee erasure of caller/compiler/allocator/OS copies.

## Verification and UI handoff

Eight session and five host scenarios cover exact creation/editing, the first record
in a newly created empty vault, all password modes, schema limits, authorization,
ID collisions/entropy/capacity/cancellation, failed-request dedup/changed intent,
uncertainty/reconciliation and queued/admitted security locks. Three compile-fail
doctests protect the form/password input boundary. Existing schema and targeted
mutation regressions cover the reused validator and shared snapshot rebuilding.

```sh
cargo test -p codeward-core --lib --locked --offline transaction::session::editing::tests
cargo test -p codeward-core --lib --locked --offline transaction::host::tests::editing
cargo test -p codeward-core --doc --locked --offline transaction::session::editing
```

Use Rust 1.99.1 and synthetic data. Final current-head CI and independent review
are recorded in the PR. Dependencies, format, crypto profiles and native adapters
are unchanged. The next implementation phase is real macOS managed-vault/Tauri
integration and React screens; search, form-close behavior and installed UI
validation follow in that phase. No milestone acceptance or real-vault admission
is inferred from completing these Rust operations.

Implementation and final exact-head verification are tracked in
[PR #59](https://github.com/versatileisonline/codeward/pull/59). Independent review
of `3984f43..e5fe769` found no Critical or Important issues. Its two minor follow-ups
were addressed by clarifying the full-form UI contract and exercising generated
versus identical provided-password request equivalence; the focused regression passes.
