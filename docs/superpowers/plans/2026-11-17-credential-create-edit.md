# Credential Create/Edit Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans inline; request one independent whole-branch review.

**Goal:** Complete scoped credential creation and general editing for UI integration.

**Architecture:** One borrowed form plus password choice becomes bounded zeroizing
owned input after session checks. Existing worker preparation hashes intent before
generating IDs/time or rebuilding snapshots, then reuses commit/refresh machinery.

**Tech Stack:** Rust 1.99.1 and existing schema/getrandom/zeroize dependencies.

**Spec:** [Create/edit design](../specs/2026-11-17-credential-create-edit-design.md).

## Global Constraints

- Preserve schema/format/crypto/storage contracts and exact text bytes.
- Rust owns new IDs and timestamps; edits preserve ID and creation time.
- Keep/provided/generated password modes; Keep forbidden on creation.
- No bulk UI snapshot, implicit upsert, automatic retry or premature Saved.
- All complete-snapshot work stays in revocable background preparation.

## Review Focus

- Duplicate creation must not insert twice or generate a new identity before dedup.
- Keep and explicit empty replacement passwords must remain distinct intentions.
- Editing must preserve identity/creation time and all unrelated exact records.
- Collision/capacity/entropy/clock/schema failures must leave authority unchanged.
- Locks/uncertainty and authenticated refresh retain their existing guarantees.

### Task 1: Scoped form and worker-side create/edit

**Files:** `credentials.rs`, `transaction/session/editing.rs`, its tests,
`session/mutations.rs`, `drafts.rs`, `write.rs`, session/transaction exports.

**Interfaces:** `CredentialForm { title, username, url, notes, password }`,
`CredentialPassword::{Keep, Provided, Generated}`; Session
`prepare_create_credential(worker, caller, generation, request, form)` and
`prepare_update_credential(worker, caller, generation, request, id, form)` return
`Result<OperationRef, TransactionError>`.

- [x] Write failing creation/editing/password/scope/bounds/failure/identity tests.
- [x] Observe missing interfaces with focused session editing tests.
- [x] Add bounded owned forms, operation-specific intent, ID/time generation and
  cancellation-aware shared snapshot rebuilding. Do not change existing write intent.
- [x] Verify session/schema and existing targeted-mutation regressions; commit.

### Task 2: Headless operations and UI handoff

**Files:** `transaction/host.rs`, `host/tests/editing.rs`, test registration;
`docs/credential-editing.md`, related API/readme/map/roadmap documentation.

- [x] Add failing host tests for empty-vault first record, full editing and modes,
  preparation/storage failure, duplicate/retry/uncertainty and security locks.
- [x] Add bounded idle-gated host delegates and normal post-commit refresh.
- [x] Verify focused host tests and document exact source/status/ownership limits.
- [x] Run default check, Clippy, rustfmt, whitespace/docs and appropriate regressions.
- [x] Independently review and fix material findings with failing regressions.
- [ ] Open a draft PR; record current head/base/test-merge and final CI in its body.
  Do not merge. Next implementation phase is the real macOS Tauri/React UI bridge.

Review and exact-head CI delivery evidence is recorded in PR #59's description,
avoiding a follow-up source commit solely to copy CI metadata.
