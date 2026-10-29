# ADR-0003: Committed writes and durability

Status: Accepted — logical save contract; platform adapter evidence pending.
Owner decision: 2026-10-29, concrete handoff approval. Acceptance record: [PR #2](https://github.com/versatileisonline/codeward/pull/2).
Target: Milestone 1. No implemented persistence guarantee.

## Context and alternatives

FR-01/05/07 and NFR-03/04/08 require coherent credentials and access metadata,
accurate acknowledgement and no silent conflicting overwrite. ADR-0002 already
selects a complete encrypted snapshot. In-place overwrite risks destroying the
last good state; separate slot/payload writes risk mixed access configurations.
A database/journal adds a new format/protocol beyond the selected small envelope.
Use whole-file staging and publication, with platform-specific validation.

## Approved logical protocol

Rust owns all stages. The UI cannot infer commit success from progress or a timeout.

1. Authorize the mutation/session and expected revision. Serialize commits and
   hold writer exclusion. Verify the current file still matches the authenticated
   base snapshot using vault ID, revision and SHA-256 of the complete original file.
2. Construct and validate one complete candidate envelope: header, both protected
   slots and encrypted snapshot. Increment revision with checked overflow; enforce
   schema/size limits. Use fresh randomness/nonces for each new encryption/retry.
3. Exclusively create a restrictive staging file in the validated destination
   directory, on the same filesystem. Write all encrypted bytes; check short writes,
   write/close errors and required file synchronization before publication. Never
   truncate or patch the live vault, or persist plaintext temporary snapshots.
4. Publish via the adapter's validated atomic replacement operation. Initial
   creation must atomically refuse an existing destination. Never delete the live
   file first or fall back to copy-and-delete across filesystems.
5. Complete the adapter's required namespace/storage persistence barriers. Then
   advance the core's committed revision and acknowledge the specific request.
   Enrollment confirmation must precede creation; readiness follows commit.

Publication is the visibility boundary. Completion of the specified persistence
barriers is the acknowledgement boundary. A successful file write, close, rename,
read-back or IPC send alone does not establish that latter boundary.

Normal updates retain both protected slots. Replacing a slot re-encrypts the
payload with a fresh nonce because its authenticated header changed. Routine
recovery keeps the recovery slot/salt and commits the new master slot and payload
together. Readers accept one complete old or new envelope, never a mixture.

## Failure and restart contract

| Stage / outcome | Required behavior on supported storage |
| --- | --- |
| Before publication, definite failure | Keep previous committed vault; retain draft while session permits; creation is not ready. Report failure, no success. |
| Application termination during atomic publication | Reopening accepts the complete previous or new envelope. Initial creation yields absent or complete file. |
| Publication may have occurred, or final barrier fails | Report `OutcomeUnknown`; block further mutations and reconcile under exclusion. Do not promise the old state is current. |
| Persistence complete, reply lost | New state is committed; lack of acknowledgement does not undo it. |
| OS crash / power loss | No blanket old-or-new or acknowledged-survival claim until the declared adapter/filesystem/storage has appropriate evidence. |

After an uncertain outcome, reopen the actual destination under writer exclusion,
bound/parse/authenticate it, compare to the known candidate/base if still available,
and complete the required barriers before reporting durable success. A visible
matching file does not itself prove durability. If the result cannot be established,
keep it unknown and preserve artifacts; never blindly retry or restore an old file
over a possibly committed new one. No automatic rollback is promised after I/O errors.

Track request IDs/results in bounded memory, bound to caller/vault/session context.
While an ID is known, an identical authorized request returns its existing status;
reuse for different arguments is rejected and never starts a second mutation.
An expired/unavailable status remains unknown, not permission for automatic replay.
Version 1 has no durable request ledger or exactly-once history. On restart, load and
validate the current vault; historical operation status may remain unknown. Require
reload/review before a new explicit mutation rather than replaying a lost request.
Status lookup returns no credentials and obeys the UI/core access boundary. If
authentication is needed after lock, require a new authorized unlock/recovery
context; never keep session keys solely to answer later status queries.

Stage names are unpredictable, contain no credential information, and are created
exclusively with the same confidentiality restrictions as the vault. On restart,
do not promote the highest revision, use modification times as authority or delete
ambiguous candidates. Cleanup may remove only known owned abandoned stages once
a usable authoritative state is established; never delete the only usable candidate.
These artifacts are transaction residue, not backups or a file-restore feature.

## Writer exclusion and external replacement

Use an OS-managed exclusive lock on a stable companion object, held for the
writable vault session. Do not lock only the file object that publication replaces,
unlink/recreate the companion while it may be locked, or treat a PID file as a lock.
A second Codeward instance refuses writable access; closing/crashing releases the
OS lock. Session locking may retain writer ownership while the vault remains open.

Expected revisions still reject stale UI mutations. Recheck the base fingerprint
before publication while holding exclusion. Revision comparison alone leaves a
check/replace race between cooperating writers. Define one supported path identity,
reject unsupported aliases and bind operations to validated directory handles.

Companion locks coordinate participating Codeward processes. Arbitrary external
writers can bypass them; a last-moment external replacement is outside that
coordination guarantee. Detect observed changes and stop rather than merge or
overwrite silently. A revision/fingerprint retained in memory detects some changes
during the session, not authentic rollback after that reference is lost. No defense
against a compromised OS is claimed.

## Platforms and validation

The [platform plan](../platform-validation.md) defines the proposed matrix,
candidate APIs, required barriers and storage exclusions. Linux file and directory
sync differ; macOS cache flushing needs native review; Windows replacement and
failure semantics require their own evidence. Reject unsupported adapters instead
of silently downgrading their contract. Application termination is the initial
requirement; power-loss support is a separately evidenced claim.

| Fault matrix | Required observation |
| --- | --- |
| Kill before/during/after each write, sync, publication and reply | Full old/new snapshot as allowed above; no partial credentials/access metadata; no premature success |
| Capacity/quota, permissions, short writes, I/O, close and sync errors | Known failure or explicit unknown outcome; no destruction through cleanup/retry |
| Two processes, stale UI requests, aliases, external replacement | Refusal/conflict; no silent cooperating-writer overwrite |
| Creation, content change, deletion, password change and routine recovery | All applicable stages; both independent unlock routes agree with the selected full state |
| Leftover/ambiguous stages and repeated interruption during reconciliation | No automatic promotion or loss of the only usable candidate |
| OS/storage crash separately from process kill | Record actual cache/filesystem assumptions; no inference of physical power-loss safety from process-kill tests |

Use synthetic data only. Unsaved generation/edits, external website updates,
missing/corrupted-file reconstruction, backups and authentic rollback detection
remain outside this contract. The owner accepted the combined handoff on
2026-10-29; implementation and platform evidence belong to authorized later work.
