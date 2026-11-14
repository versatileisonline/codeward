# Windows encrypted staging implementation plan

> **For agentic workers:** Use superpowers:executing-plans to implement this
> plan inline, followed by an independent whole-branch review.

**Goal:** Prepare complete encrypted synthetic candidates on NTFS without
changing the named vault or acknowledging a save.

**Architecture:** Continue ADR-0003's accepted whole-file protocol. Use
`NtCreateFile` with a retained directory handle and fixed single-component
names. Apply the existing per-user protected DACL at creation. Completed
stages retain writer exclusion and a read handle that denies writes/deletes.

**Tech stack:** Rust 1.99.1; existing windows-sys 0.61.2/windows-link 0.2.1.

**Spec:** [ADR-0003](../../adr/0003-committed-writes.md),
[Windows foundation](../../windows-storage-foundation.md) and
[platform validation](../../platform-validation.md).

## Global constraints

- Synthetic NTFS use only; no runtime writer, publication or save acknowledgement.
- Keep vault bytes, crypto profiles and locked dependency versions unchanged.
- Enforce the core 16 MiB cap before creating a stage.
- Preserve partial/completed residue on every failure; no cleanup or promotion.
- Generate 128-bit stage names from the OS; at most eight exclusive attempts.

## Review focus

- A path alias or reparse entry must not redirect file creation.
- Existing collisions must never be truncated or repaired.
- Partial writes, write/flush/close failures must return no completed stage.
- The stage must keep the companion held after the caller drops its lease.
- Closing the write handle may finalize timestamps: retain a read identity guard
  across close, reopen without write/delete sharing, and compare the full
  encrypted bytes before selecting the final observation.

## Task 1: Native relative file boundary

Files: `crates/codeward-storage-windows/src/native/relative.rs`,
`src/native.rs`, `src/managed.rs`, `src/lib.rs`, `Cargo.toml`, `src/tests.rs`.

Interfaces: private `native::open_child(parent, name, access, share,
disposition, security) -> io::Result<File>`; retain all directory handles.

- [x] Add tests for exclusive collision preservation, rejected multi-component
  names and creation under the retained parent.
- [x] Run Windows-target `cargo check --tests` and observe missing APIs.
- [x] Implement the bounded `UNICODE_STRING`/`OBJECT_ATTRIBUTES` wrapper,
  synchronous native open, status conversion and native randomness/close.
- [x] Use the same relative boundary for companions and vault reads; preserve
  existing admission and missing-leaf behavior.
- [x] Run Windows-target checks and Clippy; commit the native boundary.

## Task 2: Staging and evidence

Files: `src/staging.rs`, `src/staging/tests.rs`, `src/lib.rs`, `src/native.rs`,
`docs/windows-staging.md`, roadmap/progress/file-map documentation.

Interfaces: public `ReadLease::prepare_stage(&self, encrypted: &[u8]) ->
Result<PreparedStage, NativeError>` and opaque `PreparedStage::verify()`.

- [x] Add tests requiring byte-exact stages, cap-before-create, bounded collisions,
  random failure, partial writes and flush/close refusal without vault changes.
- [x] Observe the missing staging API in the Windows-target check.
- [x] Implement shared lease ownership, exclusive private file creation,
  checked write/flush/close, retained identity and bounded encrypted readback.
- [ ] Cover ACL refusal, independent-process exclusion, stage lifetime and
  preservation of abandoned stages. Native tests run on Windows CI.
- [ ] Run focused checks, independent review and current-head three-platform
  Rust CI; record exact head/base/run and unresolved acceptance gates in the PR.
- [ ] Open a focused PR against reviewed main. Do not merge or claim milestone
  acceptance without explicit review.

## Execution notes

The native and staging code are committed together because the shared lease
and public stage type form one compiling boundary. The first Windows-target
test check failed on missing `open_child` and `prepare_stage` as expected.
All-feature/all-target Windows-target Clippy and the default build pass
locally; native execution is pending hosted Windows CI. Independent review
identified a close/reopen race coverage gap; added real filesystem probes for
the identity guard and both interfering-writer outcomes, plus child termination.
