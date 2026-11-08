# macOS managed commit workflow

Milestone 2 evidence for FR-01/02/09 and NFR-03/04/08 on Apple silicon macOS
26.3.1 (25D2128) with local APFS. The headless test uses the shared
[experimental writer](macos-experimental-writer.md) with a disposable synthetic
account home. The private harness supplies the home to the managed resolver.
Path checks, writer exclusion, publication and barriers use the normal adapter.

## Commit checks

The bounded native read retains the validated descriptor, its metadata and the
bytes read through it. Replacement checks the expected base using that observation.
The namespace barrier compares bytes and synchronizes the same file descriptor.
Final checks verify that the name still identifies that file and that its metadata
and the retained directory and companion checks still match.

Managed saves synchronize the vault file, then `Vaults`, `Codeward`,
`Application Support`, `Library` and the existing account home in that order.
Each handle must pass `fsync` and `F_FULLFSYNC`. An expected absence uses the same
directory sequence without a file. The extra directories cover names created
during managed preparation, including preparation by a prior process. The core
advances the committed revision only after the whole sequence and its final
checks succeed. Barrier failures after publication enter `OutcomeUnknown` under
[ADR-0003](adr/0003-committed-writes.md).

## Headless workflow and failure evidence

- Confirm paper enrollment and commit creation. The initial revision advances
  after barriers. Close the session and reopen through split-phase master
  authentication in a separate process.
- Kill a child after staging and verify byte-for-byte preservation of the old
  vault. Kill another child after a barrier-confirmed save and verify that the
  credential reopens at the committed revision. A competing writer is refused
  at both checkpoints. The abandoned encrypted stage stays untouched across
  later saves and recovery. The named vault remains authoritative.
- Verify that an incorrect password leaves the file unchanged and that lock
  blocks listing. Replace the master password through paper recovery. An
  injected mount-policy refusal after publication leaves the outcome unknown.
  Fresh paper authentication and the full barrier sequence establish the
  committed recovery. The new password opens the exact saved snapshot, the old
  password fails, and the same paper code still works.
- Native barrier tests verify descriptor identities and flush order through a
  reopened managed tree, both with and without a vault. Injected `ENOSPC` and
  `EIO` at ancestor barriers stop the sequence and preserve the visible bytes.
  Revalidation refuses an ancestor ACL change during the last barrier.

## Acceptance boundary

This evidence covers application termination at the stated checkpoints on the
tested local configuration. The [native kernel write tests](macos-write-failures.md)
cover real `EFBIG` and partial writes separately. Actual capacity/quota exhaustion,
media faults, OS crashes and power loss require separate evidence. A fixed
managed path and APFS checks cannot establish that third-party sync is absent.
Review the supported location and residue policy under
[ADR-0006](adr/0006-managed-working-location.md) and
[ADR-0003](adr/0003-committed-writes.md) before real-vault admission.
Host result dispatch, packaging, Windows/Linux storage and full Milestone 2
acceptance remain pending. The writer stays default-off and synthetic-only.
