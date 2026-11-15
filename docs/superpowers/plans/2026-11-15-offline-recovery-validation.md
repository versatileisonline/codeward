# Offline recovery validation implementation plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Produce reviewable macOS headless evidence and recovery guidance for FR-06/07/10.

**Architecture:** Exercise the existing `HeadlessVault<SyntheticLease>` through
the test-only wrapper around the shared managed APFS writer. Keep networking
denial in a separate explicitly selected native probe and CI invocation.

**Tech stack:** Rust 1.99.1, existing RustCrypto pins, macOS APFS and sandbox-exec.

**Spec:** [Offline recovery validation](../specs/2026-11-15-offline-recovery-validation-design.md).

## Global constraints

- Synthetic data and exclusively created disposable homes only.
- No dependency, Argon2 profile, vault-format or runtime writer-feature changes.
- The credential UI and installed-app offline validation remain in M4.
- M3 acceptance is an explicit owner decision after evidence review.

## Review focus

- A valid code belonging to a different vault must fail without writes.
- Code confirmation after enrollment restart must not accept the previous code.
- Lock before storage admission must prevent a queued replacement from publishing.
- A missing vault plus complete residue must not become an empty or promoted vault.
- A lost acknowledgement must not report recovery complete or unlock browsing.

### Task 1: Native recovery acceptance scenarios

**Files:** Add `crates/codeward-storage-macos/src/staging/transactions/tests/recovery.rs`;
register it in the sibling `tests.rs`.

**Interfaces:** Consume existing `HeadlessVault`, `AccessProof`, `HostCompletion`,
`OperationStatus` and test-only `SyntheticStorage::from_path`/`Controls`.
Produce five focused native tests for enrollment/reuse, invalid input, missing or
tampered authority, cancellation, and failed/uncertain recovery saves.

- [ ] Verify the existing managed headless lifecycle baseline.
- [ ] Add acceptance tests asserting exact committed revisions, complete snapshot
  preservation, both access routes, rejected old passwords and unchanged residue.
- [ ] Run the new module with `cargo test -p codeward-storage-macos --all-features --locked --offline staging::transactions::tests::recovery:: -- --test-threads=1`.
  These characterize existing contracts; any defect requires a failing regression
  before a runtime correction, rather than inventing a failure for existing behavior.
- [ ] Fix any discovered contract defect and rerun focused checks; commit the tests.

### Task 2: Verify recovery with networking denied

**Files:** The recovery test module and `.github/workflows/rust.yml`.

**Interfaces:** Add an ignored `offline_network_denial_probe` test, explicitly
selected under sandbox-exec. Keep ordinary native tests runnable without a sandbox.

- [ ] Write the probe requiring `EPERM` from a loopback TCP connection attempt.
- [ ] Run the probe without sandboxing and observe its expected failure.
- [ ] Run the same probe with `(version 1) (allow default) (deny network*)` and
  require success; run the five recovery scenarios with that same policy.
- [ ] Add those explicit invocations to macOS CI; check default builds, native
  Clippy and formatting. Commit the verified offline harness and CI step.

### Task 3: Recovery guidance and evidence

**Files:** `docs/offline-recovery.md`, `docs/milestone-3-progress.md`, affected
requirements/ADR/roadmap summaries, README navigation and file map.

- [ ] Document exact enrollment/recovery states and messages for later UI use.
- [ ] Map FR-06/07/10 to tests, network-denied execution and retained limitations.
- [ ] Record current local commands and results without reusing older runs as
  current evidence; leave formal M3 acceptance pending.
- [ ] Verify Markdown links/anchors, whitespace and relevant Rust checks.
- [ ] Review the full branch independently, address material findings and publish
  the reviewable branch/PR without merging it or starting M4.
