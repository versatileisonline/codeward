# Scoped credential creation and editing

The owner approved finishing the recommended create/edit work on 2026-11-17,
continuing through PR delivery. Complete the remaining scoped record operations
before the first useful desktop integration under FR-03/04/05 and ADR-0003/0005.

## Approach

Extend shared revocable write preparation with creation and complete selected-record
editing. Keep the established session/host authorization, keyed request history,
commit/status and refresh boundaries. Accept one bounded form; the renderer never
constructs a complete decrypted snapshot. Rust generates record IDs and timestamps.

## Form and operations

- `CredentialForm` borrows title, username, URL, notes and a `CredentialPassword`.
  Password choices are `Keep`, `Provided(&str)` and `Generated(&PasswordDraftRef)`.
  `Keep` is valid only when editing. Explicit empty provided passwords remain valid.
- `Session::prepare_create_credential` / `HeadlessVault::create_credential` require
  current generation, request ID/expected revision and a form. Creation returns
  the existing pending operation/status surface; after a committed refresh, list
  credentials to obtain Rust-assigned IDs. No ID or timestamp is accepted from UI.
- `Session::prepare_update_credential` / `HeadlessVault::update_credential` also
  require the selected canonical ID. Missing records never become implicit inserts.
- Reuse the unchanged schema byte limits and exact UTF-8/whitespace preservation.
  Validate all bounds after authorization and before copying owned inputs. Owned
  form data is zeroizing; no diagnostics/cloning/serialization are added to forms.
- Generated references require the existing current caller/opened scope, generation,
  source revision and draft identity. Resolve only a bounded owned password for
  revocable worker preparation; do not return it to a UI save operation.

## Identity, mutation and durability

- Generate independent 128-bit OS-random IDs, encoded as 32 lowercase hex bytes,
  on the worker after keyed request deduplication. Retry collisions at most eight
  times, then fail without overwriting a record. Reject the existing 10,000-record
  capacity before entropy use or snapshot construction.
- Rust sets creation/update time to bounded Unix milliseconds for new records.
  Editing preserves ID/creation time and uses the maximum of current wall time and
  prior update time. `Keep` preserves the password without exposing it to the form.
  All other records remain exactly unchanged; schema/snapshot limits still apply.
- Bind command domain, revision, target ID, every exact form field and password
  keep/replacement choice into existing keyed request intent. Resolved generated
  and provided passwords with identical bytes have the same replacement semantics.
  Known IDs deduplicate before ID/time generation, copying snapshots or encryption.
- Preserve existing failure/explicit retry and uncertainty/reconciliation behavior.
  A definite failed creation cannot replay as a second record. Editing never upserts.
  Current access/reference authorization precedes command processing; completed
  stale requests are correlated through original operation references/status.
- Queue only bounded single-record inputs on the host thread. The worker owns full
  snapshot copying/schema validation/encryption and checks cancellation. Lock wins
  over unadmitted preparation; admitted publication may finish while remaining locked.
  Only complete commits acknowledge success and trigger authenticated refresh.

No dependencies, vault format, crypto profile, native adapter or Tauri command
changes. No search, general form-draft store, close dialogs or rendered UI in this
PR. This finishes the scoped create/edit prerequisite; desktop integration is next.

## Verification

Exercise empty-vault first-record creation and reopen, exact full-field editing,
password keep/provided/generated modes, schema maxima/negative bounds, caller and
reference restrictions, bounded ID collision/entropy/capacity handling, duplicate
failed requests/changed intent, uncertainty, and queued/admitted locks. Run focused
schema/new/existing mutation tests, default/lint/docs checks, independent branch
review, and final exact-head three-platform CI. Record delivery in the PR body.
