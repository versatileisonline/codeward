# Scoped Credential Writes Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans inline, with one independent whole-branch review.

**Goal:** Save generated passwords to existing credentials and explicitly delete
selected credentials without exposing complete snapshots.

**Architecture:** Extend `WriteInput` with targeted mutations. Scope/copy bounded
inputs in the session, then reuse the revocable background preparation, keyed
intent history, commit/status and refresh paths. Copy snapshots on the worker.

**Tech Stack:** Rust 1.99.1; existing crypto, schema and native storage contracts.

**Spec:** [Scoped credential writes](../specs/2026-11-17-scoped-credential-writes-design.md).

## Global Constraints

- Existing records only; no new records, general edit form, UI or Tauri commands.
- Ordinary current access and revision/draft scope are required.
- No whole-snapshot UI input/output or new dependencies/format/storage behavior.
- No Saved before complete commit; definite failure preserves the draft.
- No automatic retry, inactivity refresh or rollback inference from discard/lock.

## Review Focus

- Same request ID with changed operation, record or password must not replay.
- Replacement/deletion must preserve all unrelated exact record fields.
- Snapshot copies and encryption stay off the host control thread and revoke on lock.
- Unknown/admitted outcomes must remain distinct from canceled/uncommitted saves.
- Refresh must clear the old draft only after a proven changed committed snapshot.

### Task 1: Shared targeted mutation preparation

**Files:** `transaction/session/mutations.rs`, its tests, `session/write.rs`,
`session/drafts.rs`, module registration and shared access guards.

**Interfaces:** `prepare_generated_password(worker, caller, request, id, &draft)`
and `prepare_delete_credential(worker, caller, generation, request, id)` return
`Result<OperationRef, TransactionError>`. Internal bounded owned inputs feed the
existing `WritePreparation`; mutation intent is keyed before applying snapshots.

- [x] Write failing session tests for exact replacement/deletion and reopen,
  scope/bounds, failure/retry/dedup, uncertainty, expiry and lock cancellation.
- [x] Observe missing-interface failures with focused session mutation tests.
- [x] Implement targeted input validation/intent and worker-side schema copying.
  Use checked clock conversion and nonregressing update timestamps.
- [x] Verify focused tests/default builds and commit explicit core paths.

### Task 2: Headless commands and PR delivery

**Files:** `transaction/host.rs`, `host/tests/mutations.rs`, test registration,
`docs/scoped-credential-writes.md`, draft/UI contract docs and core README/map.

- [x] Add failing host tests for actual saves/deletion, duplicate failed requests,
  pending/uncertain saves, preparation errors and queued/admitted lock behavior.
- [x] Add idle-gated delegates and request refresh for both targeted operations.
- [x] Verify focused host tests and relevant existing draft/write regressions.
- [x] Document exact behavior, controlled errors, status correlation and limits.
- [x] Run default check, all-target/all-feature Clippy, rustfmt and docs/whitespace.
- [ ] Independently review; fix material findings with regressions.
- [ ] Push a draft PR and record exact-head/base/merge CI in its description.
  Do not merge. Preserve user work; no full local suite duplication if CI passes.
