# Scoped credential reads implementation plan

> **For agentic workers:** Use superpowers:executing-plans task-by-task.

**Goal:** Give the forthcoming interface minimal, authorized selected-record reads.

**Architecture:** Add a selected-read session module and thin headless-host delegates,
using existing committed-snapshot and inactivity contracts. Keep passwords out of
detail payloads and require an explicit separate reveal request.

**Tech stack:** Rust 1.99.1, existing zeroize buffers and transaction test model.

**Spec:** [Scoped credential reads](../specs/2026-11-15-scoped-credential-reads-design.md).

## Global constraints

- No dependency, vault-format, crypto-profile, storage or UI-feature changes.
- No secret diagnostics, automatic serialization, clipboard or real-vault admission.
- Reveal lifetime is at most 30 seconds, capped by the existing idle deadline;
  callers must clear their own already-returned copies.

## Review focus

- A stale list selection cannot reveal a newer revision's password accidentally.
- Wrong caller/opened worker and restricted recovery must not enumerate record IDs.
- Reading/revealing must not refresh inactivity or touch storage.
- Deletion/refresh and unknown saves must never expose an obsolete snapshot.
- Returned copies cannot be represented as automatically revoked or erased.

### Task 1: Session-selected reads

**Files:** `transaction/session/selected.rs`, its tests, `session.rs`,
`transaction.rs` and the shared credential-ID validation in `credentials.rs`.

**Interfaces:** `Session::get_credential_details(worker, caller, generation,
expected_revision, id) -> Result<CredentialDetails, TransactionError>` and
`Session::reveal_password` with the same arguments returning `PasswordReveal`.
Export the result types and `PASSWORD_REVEAL_DURATION` through `transaction`.

- [x] Baseline existing scoped-list tests.
- [x] Write tests for field selection/bounds, stale and unauthorized contexts,
  invalid/missing IDs, pending/uncertain/conflicted writes, refresh/deletion,
  expiry and reveal lifetime. Observe failure before implementing the methods.
- [x] Implement one shared selected-record authorization/lookup path, fallible
  bounded payload copies and final expiry checks. Reuse ID validation in schema.
- [x] Run focused tests and negative API doctests; commit.

### Task 2: Headless entry points

**Files:** `transaction/host.rs`, `transaction/host/tests/selected.rs`, test registration.

- [x] Write host tests before adding delegates; observe missing-method failures.
- [x] Add `get_credential_details(generation, expected_revision, id)` and
  `reveal_password(generation, expected_revision, id)` through the existing idle gate.
- [x] Verify normal reads, restricted access, queued work, expiry and old revision
  refusal after a saved deletion. Run core Clippy/default builds/formatting; commit.

### Task 3: Reviewable integration

**Files:** `docs/scoped-credential-reads.md`, core README, UI/core contract and file map.

- [x] Document payload fields, exact guards, deadline and delivery/erasure limits.
- [ ] Verify docs and relevant Rust checks; independently review the full branch.
- [ ] Address material findings with failing regressions and passing checks.
- [ ] Push a draft PR and record final current-head/base three-platform CI.
