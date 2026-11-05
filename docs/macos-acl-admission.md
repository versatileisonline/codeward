# macOS ACL admission experiment

Scope: synthetic-only permission admission for Milestone 2, ADR-0003,
NFR-02/04/08 and T-06/10. This extends the existing conservative staging policy
to path resolution, writer exclusion and bounded reads. No runtime writer or
real-vault support is enabled.

## Policy and ownership

Refuse any extended ACL, including deny-only entries, on the direct destination
parent, stable companion and opened vault. Do not interpret grants or edit ACLs
to make a location usable. Existing POSIX owner/mode, identity, link and size
checks remain. ACL presence returns `Permissions`; inability to establish the
empty-ACL condition returns `Unsupported`.

The existing descriptor-based ACL helper and reviewed native bindings are reused;
only the fixed error mapping moves from staging to the shared path layer. There
is no new native API, dependency, feature, vault byte or cryptographic change.
The [staging API review](macos-staging.md#dependency-and-native-api-review) remains
the source for absent/empty ACL semantics. Its dependency/advisory snapshot is
reused; no new advisory-database review is claimed.

`VaultPath::verify` checks the parent before companion creation/acquisition and
at every existing lease verification checkpoint. A companion descriptor is
checked before OS locking and again under the acquired lease. Reads check the
vault descriptor before buffering and after reading, then revalidate the lease
and named file. Expected absence still requires valid parent/companion admission.
Returned encrypted bytes remain unauthenticated input to the Rust core.

Staging, publication and namespace barriers share the stronger lease check;
duplicate parent-only ACL checks are removed without removing any verification
checkpoint. An observed companion ACL before publication refuses the operation.
An observed ACL after native publication preserves `OutcomeUnknown` and the
visible candidate; post-barrier ACL failure cannot establish a successful result.
No refusal removes, repairs, promotes or rolls back a file.

## Focused evidence

On macOS 26.3.1 (25D2128), Apple silicon/local APFS, 2026-11-04:

- Six new admission tests reproduced the previous omissions before the fix.
  All six pass after the fix, along with two new publication/barrier cases.
- Fourteen ACL-focused native tests pass. Allow-read and deny-only ACLs are refused
  while parent/file POSIX modes remain restrictive. A parent changed after
  resolution is refused before a companion is created. Existing companions retain
  their inode, bytes and ACL; a refused attempt does not retain an OS lock.
- ACLs added after acquisition or during a read refuse present/absent observations
  without returning bytes. Removing only the test-inserted ACL permits valid reads
  or fresh acquisition; tests verify existing exclusion remains held meanwhile.
- Companion ACL changes refuse new staging, already-prepared creation/replacement
  and barriers while preserving destination/residue. Changes after actual rename
  retain unknown outcomes; changes at the last barrier refuse present/absent results.
- All 62 native runtime test functions pass, including the existing synthetic
  core/native lifecycle, process-kill and recovery cases. Native all-target Clippy
  with warnings denied and workspace formatting pass. Full workspace/doctest and
  three-platform results belong to the current-head/base hosted Rust CI check.

These checks do not establish ancestor-directory policy, cloud-location admission,
protection against last-moment nonparticipating changes, native persistence
acceptance, OS-crash/power-loss behavior or Windows/Linux storage support. The
[core/native experiment](macos-transaction-experiment.md) remains test-only.
