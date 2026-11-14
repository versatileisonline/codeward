# Windows synthetic atomic publication

This extends [encrypted staging](windows-staging.md) under
[ADR-0003](adr/0003-committed-writes.md). It consumes a prepared stage and
changes which complete encrypted file is visible as `vault.cw` on synthetic
local NTFS. Success establishes visibility only. It is not a committed save.

## Native operation and ownership

`SetFileInformationByHandle` receives `FileRenameInfoEx`, a source handle with
DELETE access, and the retained destination directory as `RootDirectory`.
The destination is always the fixed relative name `vault.cw`. Creation uses
flags zero; replacement uses only `FILE_RENAME_REPLACE_IF_EXISTS`. There is no
POSIX sharing override, read-only override, copy/delete fallback or path-based
second implementation. Existing readers that deny delete sharing can refuse
replacement; an attempted native refusal remains uncertain at this boundary.

The native buffer uses the locked generated `FILE_RENAME_INFO` header in an
aligned bounded stack allocation. Its flexible UTF-16 tail is contiguous
across header padding, with compile-time size bounds and an explicit byte
length. The filename includes a spare NUL while the counted name excludes it.
Retained directories request traversal, read-attribute, security-read and
synchronization access rather than directory-list data access, permitting the
rename target's internal open without a reverse sharing conflict.

Publication uses a nonblocking guard shared by all stages from the same
lease. Before changing handle sharing, it verifies the stage/admission and
the exact expected current bytes or confirmed absence. It copies the staged
encrypted bytes into a fallibly allocated buffer capped at 16 MiB.

The completed stage initially denies writes and deletes. An identity guard
pins that file object while the read handle is explicitly closed and the
named source is reopened with DELETE access. The guard permits rename sharing,
so substitution is detected by identity. Same-object changes are detected
by metadata and complete encrypted-byte comparison. The reopened source
denies competing writes and deletes. Publication then repeats the expected
base and admission checks immediately before the native rename.

After a successful native reply, checks require the source's new canonical
name and the actual destination to identify the candidate, with the complete
expected encrypted bytes and accepted ACL. The source remains protected
through those checks and its checked final close. A consumed last stage can
release writer exclusion when the call ends; a future transaction owner must
retain its own lease through barriers and reconciliation.

## Failure outcomes

- Refusals before native rename return `NotPublished(NativeError)`. This
  operation has not changed the live vault. External changes observed during
  checks are left intact.
- Every attempted rename error, lost reply, failed postcheck or failed final
  close returns `OutcomeUnknown`. The actual vault may already be the candidate.
- No failure promotes leftover files, removes ambiguous residue, rolls back
  the visible name or automatically retries.

No-replace creation is enforced by the native rename, including a creator
that races after the last absence check. Exact base comparisons and the
companion coordinate cooperating writers. A writer ignoring the companion
can still race the final base comparison and replacement; this experiment
does not claim a snapshot against arbitrary same-user modification.

## Tests and evidence

The Windows suite covers creation, exact-base replacement, stale/absent/existing
conflicts, cap refusal, native no-replace races, source substitution and mutation
across reopen, base changes during the transition, close failures, native errors,
lost replies, postcheck refusal, same-lease contention and blocked destination
readers. Failed operations preserve the current whole file and applicable
staged residue. Consuming the last stage retains exclusion until publication ends.

The managed authenticated workflow creates a vault, replaces contents and
coherently replaces a master-password slot/payload. It checks complete expected
bytes/snapshots and both master/paper access routes after reopening. Separate
children are killed immediately before and after actual creation/replacement
renames. Parents check exclusion before termination and exact absent/old/new
bytes, paper authentication and residue afterward. These are four application
termination cases, not interruption inside a syscall or a power-loss test.

Local Rust 1.99.1 checks use the Windows cross target for default builds and
all-target/all-feature Clippy with warnings denied. Native results require the
current PR's hosted Windows job; full workspace/doctest, formatting and lint
evidence comes from current-head three-platform Rust CI.

## API review and remaining gates

The existing `windows-sys 0.61.2` bindings/features and `windows-link 0.2.1`
are unchanged; no package, crypto profile or vault bytes change. Reviewed
primary references are [SetFileInformationByHandle](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle),
[FILE_RENAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info)
and [FILE_RENAME_INFORMATION](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information).
The last reference specifies root-relative names, DELETE access and target
directory sharing considerations. The [staging review](windows-staging.md)
records the existing relative-open, ACL and file-flush boundary.

Windows namespace/storage persistence barriers, writable core composition,
broader native fault/location admission and integrated Milestone 2 acceptance
remain open. File flushing, close, readback and rename alone do not authorize
a saved indicator. OS-crash, power-loss and packaged Windows 11 evidence remain
separate gates.
