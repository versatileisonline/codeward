# macOS owned staging and writer lifetime

Scope: remove the native transaction bridge's reconstructed-stage workaround under
ADR-0003. The native API can now return a prepared stage without a caller lifetime.
This does not export a writable core adapter or accept storage for real vaults.
No dependency, feature, unsafe call, format or cryptographic change is introduced.
Acquisition/read/namespace method signatures stay unchanged; `PreparedStage` no
longer has a lifetime argument in this experimental native API.

## Ownership boundary

`ReadLease` privately owns an `Arc<LeaseState>`. The one state contains the retained
directory chain, companion descriptor/identity and publication mutex. Preparing a
stage retains that same state; it never reopens/duplicates a descriptor, acquires
a second lock or creates a second mutex. Neither lease nor stage exposes Clone,
raw handles, unlock or cleanup. There is no cycle back from state to its owners.

`PreparedStage` is an owned, noncloneable marker. It can be stored in a worker or
moved to a thread after its caller's lease is dropped. The companion remains locked
until the caller and every prepared stage release ownership. Only the final state
drop runs the existing explicit unlock and closes the owned descriptors. Dropping
a stage preserves its file; publication still consumes it on success or failure.

All existing admission, stage/base checks, publication classification and namespace
barriers use the same handles and mutex. Multiple stages cannot bypass publication
serialization after the original lease is dropped. A failed/consumed stage releases
only its own share; it cannot unlock another retained owner.

Publication remains visibility only. Consuming the final stage can release writer
exclusion if no separate owner remains. The transaction worker must retain its own
lease through barriers and unknown-outcome reconciliation. The private synthetic
bridge does so and now stores/publishes the actual stage, eliminating its copied
name/metadata and reconstructed marker. No raw-stage API is added.

## Focused evidence

Synthetic data on macOS 26.3.1 (25D2128), Apple silicon/local APFS, 2026-11-05:

- Four new runtime functions cover multiple-stage last-owner release, another
  process's contention/reacquisition, moving a stage into an unscoped thread,
  publication after caller drop, rejection/lost reply with and without a retained
  caller, and unwinding the last stage owner. Companion identity and residue remain.
- The existing paused-publication test now drops the original lease first. Separate
  owned stages still share exclusion, refuse concurrent publication and reject a
  stale follow-up base. Existing namespace contention checks cover that same guard.
- The former lease-borrow compile-fail example is replaced by a stage-cloning
  refusal. The consumed-stage/double-publication compile-fail check remains.
- All 94 native runtime functions passed locally in 61.61s, including existing
  transaction lifecycle/recovery, uncertainty, process-kill and kernel write-limit
  cases. Both native compile-fail doctests, native all-target Clippy with warnings
  denied, workspace formatting and whitespace passed. Full workspace and
  three-platform results belong to current-head/base hosted Rust CI. No power-loss
  or shipping-storage conclusion follows from these checks.

The [transaction experiment](macos-transaction-experiment.md) remains test-only.
Location/native persistence acceptance, production storage, application sessions,
Windows/Linux adapters and packaged support remain pending. Managed directories,
transaction residue and owned markers do not add backups or restoration.
