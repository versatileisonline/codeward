# Windows synthetic core writer and namespace barriers

This Milestone 2 experiment composes the accepted ADR-0003 protocol with native
NTFS staging, atomic publication and the measured directory-flush access from
[the capability probe](windows-persistence-review.md#observed-native-results).
It uses synthetic data only. Default builds omit `experimental-writer` and the
public storage constructor never accepts a UI-supplied path.

## Native sequence and ownership

`ExperimentalStorage::managed()` resolves the current process account's
LocalAppData base. Acquiring storage prepares `Codeward/Vaults`, validates the
existing NTFS/identity/ACL policy and retains the stable companion exclusion.
Recognized cloud-name components are refused before preparation, including
case-insensitive OneDrive (also `OneDrive - ...`), Dropbox, Nextcloud, ownCloud,
Google Drive/GoogleDrive, iCloudDrive/iCloud Drive, pCloudDrive, MEGA and Insync.
Unrecognized/renamed synchronization is outside the bounded location policy.
Fixed-drive and OS filesystem reports remain assumptions, not physical-device
or universal sync detection. No permissions are repaired and no residue is cleaned.

The core owns authentication, caller/session/revision checks, candidate encryption,
request history and acknowledgement. The adapter retains only encrypted copies,
an owned stage and a successful current observation. Every copy is fallibly
allocated under the existing 16 MiB cap. Failed staging revokes the earlier marker;
failed reads/publication/barriers revoke selected observations. Publication consumes
the actual stage and maps definite refusal separately from unknown outcomes.

`synchronize_namespace` holds the same nonblocking local guard as publication.
It requires exact expected complete encrypted bytes or confirmed absence. A
present candidate is opened relative to the retained parent with read/write access,
denying competing writes/deletes. Complete bytes, regular-file identity, canonical
name and ACL admission are checked before and after its flags-zero
`NtFlushBuffersFileEx`. A read guard pins the flushed identity and denies deletion
across writable-handle close; a restrictive final reader then checks the original
identity/metadata and full expected bytes while denying writes/deletes. It stays
protected through the guard close and its own checked final close. Every temporary
close is checked.

Directories are temporarily reopened with the measured directory-specific
`FILE_ADD_FILE` plus traversal/attribute/security/synchronization access and
read/write sharing. This does not broaden long-lived retained handles. Reopened
identities, canonical names, NTFS and admission must match their retained originals
around each flush. Managed order is file, Vaults, Codeward, then the existing
LocalAppData anchor; the final directory flush covers newly created managed names.
Synthetic fixture mode stops at its pre-existing fixture parent. Drive root is
refused as an anchor, and no volume-wide flush or fallback is performed.

Only complete final barriers allow the core to report `Committed`. Failure after
attempted publication stays `OutcomeUnknown`; no rollback, promotion or replay is
authorized. A failed barrier invalidates the adapter observation. Reconciliation
must read the actual named state, freshly authenticate its exact known base or
candidate in the core and repeat barriers while retaining exclusion. It does not
unlock a locked session or recreate lost request history.

## Evidence contract

Namespace tests cover complete presence/absence/stale/capped input, managed anchor
coverage and order, every file/directory flush and checked-close refusal, local
contention, swapped handles, changed ACLs, absence appearing midbarrier and native
open failures. They use the verified restricted same-user thread context from the
probe. Native failures are injected through seams delegating the real operations.

The shared-writer workflow creates a confirmed vault, saves and deletes a complete
record snapshot, replaces the password slot/payload coherently, denies access after
lock and reopens through both access routes. Malformed/tampered authority remains
unchanged and valid or incomplete leftover stages never become authority. A failed
barrier cannot acknowledge or reuse its old observation; repeated failed fresh
reconciliation stays unknown, while successful paper authentication plus barriers
can resolve the candidate without unlocking the session.

Six child kills cover staged, published-but-unacknowledged and committed content
and password changes. Parents check companion exclusion before killing, then full
snapshots, revision/access-slot coherence, both access routes and preserved stage
residue after reopening. Existing publication tests separately cover creation and
replacement immediately before/after native rename. These are application
termination checkpoints, not interruption inside a syscall, OS-crash or power-loss
evidence.

Windows-target default/feature test compilation, Clippy with warnings denied and
formatting pass locally with Rust 1.99.1. Native execution and three-platform
current-head CI are required in the PR; local cross-compilation alone cannot
validate NTFS behavior. Hosted Windows Server restricted-token evidence is distinct
from ordinary-account Windows 11 desktop support. Real-vault admission, physical
device coverage and milestone acceptance require their own explicit review.

## Review regression

Independent review identified a close/reopen identity gap: exact bytes alone could
accept a replacement object after the flushed writable handle closed. A real
Windows regression first failed in
[run 43652628091](https://github.com/versatileisonline/codeward/actions/runs/43652628091)
for head `b14b7f41c79d558d2ba71a0e607057441c8dd9f7`. The identity guard and final
restrictive reader close that observed transition without claiming a snapshot
against an arbitrary external writer after the final protected handle closes.
The same run reproduced an ACL injection fixture error; its security-control
handle now requests READ_CONTROL together with WRITE_DAC. Runtime retained rights
and the restricted effective token are unchanged. Native green results must be
recorded for the corrected revision.
