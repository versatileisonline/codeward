# Windows core writer and namespace implementation plan

> **For agentic workers:** Execute inline with the namespace implementation delegated
> in an isolated worktree, then obtain an independent whole-branch review.

**Goal:** Compose the measured NTFS barrier candidate with the existing core commit
protocol, retaining exact-state checks and honest unknown outcomes.

**Architecture:** Keep retained directories unchanged. A guarded namespace operation
flushes a writable, identity-pinned expected vault and narrowly reopened directories
from the managed leaf through its existing LocalAppData anchor. A default-off
experimental writer owns the read lease, staged marker and selected encrypted
observation across publication, barriers and fresh reconciliation.

**Tech stack:** Rust 1.99.1, existing windows-sys 0.61.2 features.

**Spec:** [ADR-0003](../../adr/0003-committed-writes.md),
[measured flush access](../../windows-persistence-review.md#observed-native-results),
and the authorized Milestone 2 synthetic implementation scope.

## Global constraints

- Synthetic local NTFS; no real-vault enablement or changed crypto/format/packages.
- No volume/root flush, copy/delete fallback, rollback or residue promotion.
- Managed barriers include Vaults, Codeward and the pre-existing LocalAppData anchor.
- Shared exclusion outlives every stage and uncertain outcome.
- Failed barriers invalidate selected observations; fresh authenticated read is
  required for reconciliation. Publication is not acknowledgement.

## Review focus

- Staging failure must revoke an earlier pending marker.
- Invalid/stale input and failed authentication preserve named data and residue.
- Lost/failed barriers never advance the core revision or unlock a locked session.
- Directory access/sharing transitions preserve retained identity and ACL admission.
- Process termination evidence must distinguish before publication and after ack.

## Task 1: Namespace barriers

Files: Windows `namespace.rs`, native helpers, managed anchor accessor and tests.
Interface: `ReadLease::synchronize_namespace(Option<&[u8]>) -> Result<(), NativeError>`.

- [ ] Test expected presence/absence/stale/cap, directory order/anchor coverage,
  refusal/error/close paths, identity/ACL changes and same-lease contention.
- [ ] Implement flags-zero native file/directory flush under the publication guard.
- [ ] Verify cross-target default build, tests, Clippy and formatting; commit.

## Task 2: Owned core adapter and lifecycle evidence

Files: Windows `writer.rs`, `writer/tests.rs`, feature/export registration and docs.
Interfaces: opt-in `ExperimentalStorage::managed()` plus owned
`ExperimentalWriterLease` implementing core `Storage`/`WriterLease`.

- [ ] Write adapter/core workflow assertions; observe missing APIs before code.
- [ ] Implement capped fallible encrypted copies, selected-observation invalidation,
  owned staging and exact mapping of definite versus uncertain publication errors.
- [ ] Exercise create, content/deletion, password-slot replacement, lock/reopen,
  wrong authentication, malformed/tampered named input, failed barriers/reconcile,
  stale stages and staged/acknowledged-commit process termination.
- [ ] Integrate namespace commit, cross-check default/feature builds and Clippy.
- [ ] Obtain independent review, publish a focused PR and inspect current-head
  native Windows and full three-platform CI; record remaining admission limits.

Milestone acceptance and merges remain explicit owner decisions after the concrete
evidence packet is reviewable. UI and OS-crash/power-loss support are separate gates.
