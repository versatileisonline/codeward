# Windows atomic publication implementation plan

> **For agentic workers:** Use superpowers:executing-plans inline, followed by
> an independent whole-branch review.

**Goal:** Publish a complete synthetic encrypted stage on NTFS without
overwriting a racing creator or an observed stale base.

**Architecture:** Continue the accepted ADR-0003 protocol after private staging.
Use handle-based `NtSetInformationFile(FileRenameInformationEx)` with the retained
destination directory and only the replace flag when requested. A lease-local
nonblocking guard serializes publication. Pre-call refusals are definite;
attempted rename failures and failed postchecks remain unknown.

**Tech stack:** Rust 1.99.1 and the existing windows-sys 0.61.2 bindings.

**Spec:** [ADR-0003](../../adr/0003-committed-writes.md),
[Windows staging](../../windows-staging.md), and
[platform validation](../../platform-validation.md).

## Global constraints

- Synthetic local NTFS only; no core writer or save acknowledgement.
- No dependency/profile/format changes and no copy/delete fallback.
- Initial creation uses atomic no-replace; replacement requires exact current bytes.
- Preserve residue and the visible vault on pre-publication refusal; no rollback.
- Publication is visibility only; namespace/storage barriers remain separate.
- Bound encrypted snapshots by the existing 16 MiB cap and allocate fallibly.

## Review focus

- A racing creator must survive no-replace publication.
- Source reopen for DELETE access must keep the original file identity pinned.
- Mutation/substitution during that sharing transition must refuse publication.
- Rename may have happened even if a reply/postcheck/close fails.
- Termination before/after rename must expose only a complete old/new candidate.

## Task 1: Native rename and consuming publication

Files: `src/native/publication.rs`, `src/staging/publication.rs`, `src/lib.rs`,
native/staging exports, and `src/staging/publication/tests.rs` in the Windows crate.

Interfaces: `PreparedStage::publish_create(self)` and
`PreparedStage::publish_replace(self, expected: &[u8])` return
`Result<(), PublicationError>`. Errors distinguish `NotPublished(NativeError)`
and `OutcomeUnknown`. Private `PublicationIo` supplies checked close and rename
operations for real syscall delegation/fault cases.

- [x] Add tests for absent creation, existing/stale refusal and replacement.
- [x] Observe missing publication APIs with Windows-target test compilation.
- [x] Implement the aligned bounded native rename buffer and fixed relative
  destination; use only flags 0 or FILE_RENAME_REPLACE_IF_EXISTS.
- [x] Add shared publication exclusion, bounded candidate snapshot, identity
  guard, checked source-handle transition, exact base recheck and postchecks.
- [x] Add real racing creator, transition substitution/mutation, native error,
  lost reply, postcheck/close refusal and same-lease contention tests.
- [x] Verify Windows-target default build, test compilation/Clippy and formatting.
- [ ] Commit the implementation and native tests.

## Task 2: Integrated evidence and review

Files: publication tests, `docs/windows-publication.md`, roadmap/progress/file map.

- [x] Add authentication checks for synthetic creation/content/password replacements through both
  access routes and compare the complete expected snapshot.
- [x] Add dedicated child kills just before/after actual creation/replacement;
  check exclusion, coherent visible bytes and preserved residue on reopen.
- [x] Document API review, temporary encrypted-memory bounds and the remaining
  Windows persistence-barrier/core-composition gates.
- [ ] Obtain independent review and current-head three-platform Rust CI.
- [ ] Open a focused PR, record the tested head/base/run, and report milestone
  completion estimates without claiming formal acceptance or merging.

## Execution notes

The initial Windows-target test check failed on missing publication methods
and error type before implementation. Local default-build and all-target,
all-feature Clippy checks passed with Rust 1.99.1 and warnings denied.
Independent read-only review found no concrete implementation defects; native
Windows execution remains the required evidence gate for the FFI and sharing
transition. The test additions above await that hosted execution.

Debugging evidence: native Win32 rename returned error 87 in the boundary
probe. Testing the documented NT entry point with identical rename semantics;
current-head native CI must pass before this is treated as resolved.
