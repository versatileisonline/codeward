# macOS destination mount-policy revalidation

Scope: synthetic Milestone 2 prerequisite under ADR-0003 and NFR-03/04/08.
The existing local/writable/nonremovable APFS policy now applies at every shared
path-verification checkpoint. This closes a resolution-only admission gap; it
does not establish cloud-location admission or enable a runtime writer.

## Shared boundary

`VaultPath::verify` queries the retained destination-parent descriptor with the
existing `fstatfs` wrapper after the identity, owner/mode and ACL checks. Require
APFS, `MNT_LOCAL`, and neither `MNT_RDONLY` nor `MNT_REMOVABLE`. A disallowed
observation returns `Unsupported`; a failed query returns `Io`. Neither result
is replaced with an earlier successful observation or a weaker policy.

Resolution, acquisition, reads, staging, publication and namespace barriers all
reuse that check. Duplicate resolution/barrier-only queries are removed without
removing verification checkpoints. Ancestor filesystems are not required to be
writable APFS: root/system directories may legitimately be read-only or differ
from the destination filesystem. Ancestor access policy remains separate work.

A refusal before acquisition creates no companion. Refusal before staging or
publication preserves the destination and any already-prepared stage. Later
staging refusals preserve empty/complete residue and still check close. A query
failure or disallowed observation after native rename retains `OutcomeUnknown`
without rollback; post-barrier refusal cannot establish success. The core's
existing authentication, revision, exclusion and reconciliation contracts remain.

No new native API, dependency, feature, vault byte or cryptographic-profile change.
The existing native review/advisory snapshot is reused. macOS 26.3.1 SDK
`sys/mount.h` declarations/flags were rechecked; no fresh advisory scan is claimed.

## Evidence and limits

On macOS 26.3.1 (25D2128), Apple silicon/local APFS, 2026-11-05:

- Six focused behavior tests pass. Five reproduced the previous omissions before
  the fix; the existing namespace postcheck already refused the final-barrier case.
- Injected query refusals and `EIO` cover acquisition, present/absent reads,
  mid-read changes, pre-staging/pre-publication checks, and changes after file
  synchronization, checked close, actual creation/replacement and the final barrier.
  Tests independently inspect destination bytes, empty/complete residue, companion
  preservation and retained exclusion; restoring query availability permits access.
- One additional test validates scoped/nested query injection and isolation from
  another test thread. The guard cannot move between threads. Fault overrides are
  compiled only for tests and restored on drop; runtime queries remain native.
- All 69 native runtime functions pass, including the existing synthetic
  lifecycle, recovery and process-kill cases. Native all-target Clippy with warnings
  denied, workspace formatting and whitespace checks pass. Full workspace/doctest
  and three-platform evidence belongs to current-head/base hosted Rust CI.

The transitions above are injected results, not actual remount, read-only-volume,
network/removable-volume or media-error experiments. Actual native calls exercise
the host's local APFS. Neither these flags nor retained handles prove that a folder
is outside cloud synchronization. Cloud admission, ancestor access policy, native
persistence acceptance and Windows/Linux storage remain outstanding. Use synthetic
inputs only; no real-vault, saved-state or OS-crash/power-loss guarantee is added.
