# Windows flush capability implementation plan

> **For agentic workers:** Use superpowers:executing-plans inline and one independent whole-branch review.

**Goal:** Measure native file/directory flush access on disposable NTFS before choosing a Windows namespace barrier.

**Architecture:** Add an explicitly invoked, ignored diagnostic test. Run its
operations under a same-user LUA restricted thread token, compare the existing
retained directory handle with narrowly reopened directory handles, and print
numeric Win32/NT outcomes. Keep the runtime storage API unchanged.

**Tech stack:** Rust 1.99.1, existing windows-sys 0.61.2 features, hosted Windows CI.

**Spec:** [Persistence review](../../windows-persistence-review.md#recommended-next-evidence).
The owner requested continued work through the next PR after this review merged.

## Global constraints

- Disposable synthetic local NTFS only; no real home or account creation.
- No package/feature/format/crypto changes, production writer or saved result.
- Flags zero for NtFlushBuffersFileEx; no volume flush or fallback.
- Restricted effective token must have no elevation or enabled Administrators membership.
- Same-user thread impersonation is a capability proxy, not Windows 11 desktop validation.
- Preserve named vault bytes and abandoned stage; hold companion and directory identities.

## Review focus

- Elevated process state must not be presented as ordinary effective-token evidence.
- Impersonation/query failure must stop the probe before fixture work.
- Reopened directory identity must match its retained parent before native calls.
- A native refusal is a diagnostic outcome, not successful synchronization.
- Test-only rights must not broaden the runtime directory access policy.

## Task 1: Capability matrix and hosted execution

Files: create `crates/codeward-storage-windows/src/tests/flush_probe.rs`;
modify `src/tests.rs`, `.github/workflows/rust.yml` and persistence evidence docs.

Interface: ignored test `tests::flush_probe::ntfs_flush_capability_probe`, explicitly
run with `--ignored --exact --nocapture`. No public interface is produced.

- [ ] Write the diagnostic assertions first: writable file controls succeed;
  create/replacement snapshots and abandoned stage survive; exclusion persists.
- [ ] Check Windows-target tests before helpers exist; expect missing helper diagnostics.
- [ ] Implement scoped restricted-token setup, directory identity checks and
  Win32/Nt/NtEx-zero result collection. Compare retained access, write-attributes,
  add-file and add-subdirectory rights, and read-only/writable file controls.
- [ ] Run Windows-target default build and all-target Clippy; run rustfmt.
- [ ] Add a separate Windows CI step so the ignored diagnostic runs exactly once
  with visible output; retain the complete existing native/workspace checks.
- [ ] Commit, publish a draft PR and inspect native outcomes/current-head CI.
- [ ] Obtain independent review, record actual results and remaining gates, then
  update the PR and report the Milestone 2 estimate and UI timing assessment.

## Execution record

Pending. This plan defines a diagnostic experiment, not a persistence implementation.
