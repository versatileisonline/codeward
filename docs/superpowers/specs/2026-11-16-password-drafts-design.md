# Session-owned generated-password drafts

The owner requested the next implementation and a PR without stopping for further
checkpoints on 2026-11-16. Extend the existing Rust generator with one volatile,
session-owned generated-password draft for FR-04/08 and NFR-05/09. This is the next
bounded host foundation, not the full credential editor or desktop integration.

## Approach

Keep one password owner in `Session`; return an opaque scoped reference, with a
separate explicit reveal. This bounds retained plaintext and reuses the existing
Rust revocation, current-snapshot and inactivity gates. Returning unrestricted
plaintext on generation or moving generation to JavaScript would bypass those
boundaries. Multi-record edit drafts and general draft storage are deferred.

## Contract

- Generate only with ordinary master access, correct trusted caller/opened worker,
  current generation, expected committed revision and an idle, current snapshot.
  No enrollment/recovery, pending/uncertain/conflicted or stale-snapshot access.
- Keep at most one generated draft. Successful regeneration replaces the prior
  draft and invalidates its reference. Invalid options/entropy failure leave the
  previous draft available; expiry revokes everything instead.
- Use an opaque reference bound to caller, opened scope, generation, fingerprint
  and a fresh per-draft identity. It carries no password, is not an authorization
  token on its own, and has no automatic diagnostics/copying/serialization.
- Explicit reveal returns only a zeroizing password plus generation, revision and
  a deadline capped at 30 seconds or existing inactivity, whichever is sooner.
  The trusted host retains the original reference/context and rechecks it before
  delivering queued output; returned copies cannot be revoked or proven erased.
- Discard only the referenced current draft. A stale/foreign reference cannot
  remove a newer draft. Manual/idle/OS locks, reload, close/drop and adoption of a
  changed committed snapshot release the retained password and invalidate refs.
- A definite save failure with unchanged committed bytes preserves the draft;
  uncertain writes block access until reconciliation. Reconciliation to unchanged
  bytes permits reuse; reconciliation to a changed base requires reload/refresh
  and invalidates the draft. Discard does not undo a save or prove nonpublication.
- Generation/reveal/discard do not touch storage, refresh inactivity, save a
  record, or return a complete snapshot. Add thin `HeadlessVault` entry points.

Use the existing pinned dependencies, vault format and storage protocol. No Tauri
command, clipboard, credential edit/save-draft API, ordinary-close dialog, UI or
real-vault enablement is added. Default generator controls remain unchanged.

## Verification

Test ordinary generation/reveal/discard, successful replacement, failed generation
preservation, stale/foreign refs, wrong caller/worker/generation/revision, restricted
access, pending/unknown/conflict state, failure/reconciliation/commit-refresh,
security and idle revocation, bounded reveals, unchanged storage/activity and host
delegation. Use synthetic data, controlled deadlines and private entropy injection
for failure/expiry during generation. Negative doctests protect secret ownership.
Review the combined generator/draft branch, run focused/default/lint checks and
submit a PR with current-head three-platform CI evidence when available.
