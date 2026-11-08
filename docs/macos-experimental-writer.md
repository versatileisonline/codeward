# macOS experimental managed writer

Scope: one shared implementation of the core `Storage`/`WriterLease` contracts
for synthetic development under ADR-0003. This replaces the separate test bridge's
adapter logic; it does not accept real-vault storage or complete Milestone 2.

## Build and public boundary

The local Cargo feature `experimental-writer` is empty and disabled by default.
Only explicit opt-in exports `ExperimentalStorage`/`ExperimentalWriterLease` and
the parameterless `ExperimentalStorage::managed()` constructor. Default runtime
builds omit this adapter. Arbitrary `from_path` construction is private to tests;
fault/event controls remain in the test-only wrapper. No registry package/version,
dependency feature, unsafe call, vault byte or cryptographic profile changes.

The feature permits synthetic integration work. It does not technically prevent
someone from supplying real data after opting in, and it must not be enabled in a
shipping application until location/persistence acceptance passes. Existing
managed-location checks and their cloud/OS/storage limits still apply.

## Ownership and failure behavior

The adapter retains one real native lease, at most one actual owned stage and
complete encrypted bytes for its selected observation. The lease stays held
through publication, barriers and reconciliation, including while the core locks
its session. Session authentication, request history, generation checks and
unknown-outcome blocking remain in the core, not duplicated here.

Retained byte copies check the 16 MiB cap before fallible reservation/copy. A new
stage revokes the previous marker before any fallible work, preserving its residue.
Copy failure precedes native stage creation. Native write/sync/close errors retain
their fixed core categories; no paths, credentials or OS diagnostics are returned.

An unselected state is distinct from a successful read of actual absence. Reads
invalidate cached selection before work. Publication consumes both the marker and
selection, checks create/replace mode, and selects candidate bytes only on native
success. Refusal/uncertainty leaves no cached state for barriers. Persistence
requires a selected successful read or publication and runs the existing strict
namespace sequence; failure clears selection and requires a fresh read. The core
still authenticates reconciliation and never substitutes residue or infers rollback.

## Verification

Synthetic data on macOS 26.3.1 (25D2128), Apple silicon/local APFS, 2026-11-05:

- Five new adapter tests cover absent selection, failed reads, oversized replacement
  stages preserving residue, publication conflict/consumption with retained writer
  exclusion, and fresh selection after a refused barrier/storage-policy recovery.
- All 11 existing transaction-test functions passed through this implementation
  in 59.09s: creation, content save/delete, reusable recovery, conflict/lock,
  uncertain reconciliation, residue, process kills and real kernel write refusal.
  The wrapper now only records events/injects protocol failures; it no longer
  implements another native read/stage/publication/barrier state machine.
- Separate external compile checks refuse the writer export by default, compile
  the feature-enabled managed constructor without executing it, and refuse public
  arbitrary-path construction and fault controls with the feature enabled.
- Feature doctests compile the public managed constructor without running it and
  refuse arbitrary construction, stage cloning and double publication. Native
  all-target/all-feature Clippy, default compilation, formatting and whitespace pass.
  Allocator exhaustion is not injected; reservation failure paths are reviewed.

Hosted CI checks default builds separately and tests/doctests/Clippy with all
workspace features. Current-head/base hosted results provide the full workspace
and platform matrix; focused local results do not substitute for that required gate.

## Remaining acceptance

The [managed workflow](macos-managed-workflow.md) now uses this adapter directly
to exercise acknowledged saves, termination, exclusion and recovery on local APFS.
The headless harness supplies a disposable home to the managed resolver.
The owner accepted the [bounded macOS storage policy](adr/0006-managed-working-location.md)
on 2026-11-07. The [headless host](headless-vault-host.md) integrates the opt-in writer
on that same disposable path.

Real-vault admission, quota/media and OS-crash/power-loss evidence, desktop host
integration, Windows/Linux adapters and full Milestone 2 acceptance remain pending.
Backup/copy/restore and UI stay deferred.
