# Password Drafts Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans inline, with one independent whole-branch review.

**Goal:** Own and revoke one unsaved generated password in the Rust session/host.

**Architecture:** Add a draft module beside selected reads. Opaque references hold
caller/opened/generation/source plus an Arc identity shared with the active draft;
the session retains the existing `GeneratedPassword`. Reuse idle/snapshot/expiry
rules and drop the draft on lock and confirmed changed-snapshot refresh.

**Tech Stack:** Rust 1.99.1; existing getrandom/zeroize dependencies.

**Spec:** [Password drafts](../specs/2026-11-16-password-drafts-design.md).

## Global Constraints

- One volatile password draft; generation never saves or extends inactivity.
- Correct caller, opened scope, generation and authenticated revision are required.
- Reveal at most 30 seconds, capped by idle; caller copies need host cleanup.
- No dependencies, format, native storage, clipboard, UI or Tauri surface changes.
- Retain prior draft on failed generation/definite save failure; revoke on lock.

## Review Focus

- A stale ref cannot reveal or discard a replacement draft.
- Same numeric metadata from another opened vault is not authority.
- Expiry during generation/reveal must drop new and old session-owned plaintext.
- Unknown saves must not permit secret delivery or imply rollback on discard.
- Confirmed revision changes must release old drafts; definite failures preserve them.

### Task 1: Session-owned draft lifecycle

**Files:** `transaction/session/drafts.rs`, `drafts/tests.rs`, `session.rs`,
`session/refresh.rs`, `transaction.rs`.

**Interfaces:** `generate_password_draft(worker, caller, generation, revision,
PasswordOptions) -> PasswordDraftRef`; `reveal_password_draft(worker, caller,
&PasswordDraftRef) -> PasswordDraftReveal`; `discard_password_draft(worker, caller,
&PasswordDraftRef) -> ()`, all returning controlled `TransactionError` results.
Expose reference generation/revision through getters; do not serialize it.
Also expose `validate_password_draft(worker, caller, &PasswordDraftRef) -> ()`
for state-only queued-delivery checks, with the same controlled errors.

- [x] Write lifecycle, scope, failure/retry, expiry, storage-state and refresh tests.
- [x] Observe missing-interface failures with `cargo test -p codeward-core --lib
  --locked --offline transaction::session::drafts::tests`.
- [x] Implement shared access/reference checks, bounded retention and final expiry
  checks. Private generator injection exists only below the public entry point.
- [x] Verify tests and reference/reveal negative doctests; commit explicit paths.

### Task 2: Host delegates and reviewable delivery

**Files:** `transaction/host.rs`, `host/tests/drafts.rs`, test registration,
`docs/password-drafts.md`, UI/core contract, core README and file map.

- [x] Add failing host tests for ordinary lifecycle, pending save/failure/refresh,
  restricted/locked states and security revocation.
- [x] Add thin idle-gated delegates and verify focused host tests.
- [x] Document API, stale/unknown outcomes, returned-copy obligations and later UI.
- [x] Run default workspace check, all-feature/all-target Clippy, rustfmt,
  documentation/whitespace checks and focused core tests using Rust 1.99.1.
- [x] Review the combined branch independently and fix material findings.
- [ ] Publish the exact tested tree and create a draft PR; record final current-head
  hosted formatting/macOS/Linux/Windows results. If CI is unavailable, run the full
  equivalent local suite and report its platform limits. Do not merge.

Delivery and current-head CI results are recorded in the PR body, preserving the
tested source head without a follow-up source commit solely to copy run metadata.
