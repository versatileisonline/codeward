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

- [x] Test expected presence/absence/stale/cap, directory order/anchor coverage,
  refusal/error/close paths, identity/ACL changes and same-lease contention.
- [x] Implement flags-zero native file/directory flush under the publication guard.
- [x] Verify cross-target default build, tests, Clippy and formatting; commit.

## Task 2: Owned core adapter and lifecycle evidence

Files: Windows `writer.rs`, `writer/tests.rs`, feature/export registration and docs.
Interfaces: opt-in `ExperimentalStorage::managed()` plus owned
`ExperimentalWriterLease` implementing core `Storage`/`WriterLease`.

- [x] Write adapter/core workflow assertions; observe missing APIs before code.
- [x] Implement capped fallible encrypted copies, selected-observation invalidation,
  owned staging and exact mapping of definite versus uncertain publication errors.
- [x] Exercise create, content/deletion, password-slot replacement, lock/reopen,
  wrong authentication, malformed/tampered named input, failed barriers/reconcile,
  stale stages and staged/acknowledged-commit process termination.
- [x] Integrate namespace commit, cross-check default/feature builds and Clippy.
- [x] Obtain independent review, publish PR #53, inspect native regression
  results and record remaining admission limits. Final combined-head three-platform
  CI must pass before merge; the PR records that external check result.

Milestone acceptance and merges remain explicit owner decisions after the concrete
evidence packet is reviewable. UI and OS-crash/power-loss support are separate gates.

## Execution record

Writer assertions first failed to compile on missing exports. Both native
components subsequently passed Windows-target default/feature checks and Clippy
with warnings denied. Namespace tests execute under a verified restricted thread
token; core writer tests share the crate's KDF exclusion mutex.

Independent Windows review found one Important final-close identity gap; the
real native regression failed at head `b14b7f4` in run `43652628091` before the
identity-guard/final-reader correction. The corrected native suite and explicit
probe steps passed at head `dfd1f9a` in run `43653030714`; full joint-head evidence
is recorded in PR #53. A separate ACL mutation test-control handle was corrected
to include descriptor-read access without broadening runtime or token rights.

The independently reviewed Linux closeout branch is merged into the final
acceptance candidate so the final PR validates both adapters together. The
acceptance packet and PR distinguish earlier native evidence from current-head
full-suite checks. Merge and formal milestone acceptance remain owner decisions.
