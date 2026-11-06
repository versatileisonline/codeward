# macOS core/native transaction experiment

Scope: synthetic integration evidence for ADR-0003 and Milestone 2. The private
`staging::transactions` module is compiled only for native crate tests. It connects
the core's existing transaction worker to real native reads, staging, publication
and [candidate namespace barriers](macos-namespace.md). No runtime adapter is
exported, and real-vault admission/location policy remains unfinished.

## Ownership and authority

The test bridge owns one native lease, the last successful encrypted observation
and at most one prepared stage. It retains the prepared name/metadata privately
without a self-referential lease borrow; publication reconstructs a marker under
the same owned lease and executes the existing native checks. A successful
publication selects the candidate for barriers. Reconciliation instead selects
the actual result of a fresh bounded read. Failed reads invalidate that observation.
No snapshot in this bridge grants authentication or session access.

The core remains responsible for caller/session/revision authorization, fresh
authentication, publication admission against lock, request history and unknown-
outcome blocking. Staging and publication never advance its committed revision;
the test worker does so only after the candidate barrier sequence completes.
Its `Committed` results are test outcomes under these adapter assumptions, not
an accepted application/storage guarantee or a UI success event.

Residue handling deliberately preserves every stage. Startup reads only the named
destination: it never scans, promotes the highest revision, substitutes a stage
for a corrupt/missing destination, or deletes a complete/partial/unrelated file.
Reopening authenticates visible current bytes; it does not reconstruct historical
request status or prove an earlier process completed its barriers. Cleanup is
optional under ADR-0003 and is not added by this experiment.

No dependency, feature, unsafe syscall, runtime API, vault byte, cryptographic
profile or accepted contract changes. Shared synthetic fixture parsing and a
test-only master-derivation guard avoid duplicate loaders and incidental contention
with the core's intentionally nonblocking single-derivation policy.

## Focused evidence

On macOS 26.3.1 (25D2128), Apple silicon/local APFS, 2026-11-04:

- Eight new native test functions (seven assertion cases and a subprocess helper)
  pass. Native all-target Clippy with warnings denied and whitespace checks pass.
- Confirmed creation, credential save/delete and two consecutive master-password
  replacements through the same enrolled paper secret run through the core worker
  and real native operations. Both unlock routes validate complete selected files;
  the replaced master fails on the updated state. Recovery cannot browse/save
  ordinary credentials. Revision advancement waits for the barrier stage.
- Injected definite stage/read failures preserve the base. Known request IDs do
  not replay; a new explicit request can retry. Injected unknown-before-rename,
  lost reply after actual rename and barrier refusal block new mutations. Invalid
  recovery, read failure and repeated barrier refusal retain uncertainty; a fresh
  authenticated base/candidate observation plus native barriers resolves it without
  republishing. These harness faults are not actual kernel/media/capacity failures.
- Native creation refuses a destination introduced after admission; a stale base
  preserves an external replacement and requires reload/review. Session lock
  cancels an unadmitted stage; admitted publication/barriers finish without
  reopening the locked session. Writer exclusion remains held until worker drop.
- Startup with a complete maximum-revision stage, a partial stage and an unrelated
  file preserves all three. Missing/corrupt destinations never acquire authority
  from that residue; an absent destination remains uncommitted enrollment.
- Fifteen actual child kills cover creation, content update and recovery at the
  core's prepared, staged, admitted, published and barrier-completed checkpoints.
  Reopening yields absent/complete creation or coherent old/new updates. Published
  recovery accepts the replacement master and existing paper route. Creation
  children are verified through their known synthetic master; the enrolled paper
  is volatile. Unpublished stages survive; published stages become the destination.
  These checkpoints do not prove an in-syscall interruption or physical power loss.

The bridge is test infrastructure, not the writable adapter intended for shipping.
Complete privacy/location gates, native persistence acceptance, application session
orchestration and Windows/Linux storage remain outstanding. UI remains deferred.

The subsequent [kernel write-failure cases](macos-write-failures.md) reuse this
bridge with managed synthetic homes. Actual child-local EFBIG failures now verify
no false commit, preserved bases/residue, request deduplication and explicit retry.
They do not establish disk-full, quota, media-error or persistence acceptance.
