# Milestone 1 design handoff

Status: Accepted by owner, 2026-10-29. Implementation entry authorized.
Acceptance PR: pending publication. No runtime evidence is implied.

The owner explicitly approved this concrete handoff on 2026-10-29 and authorized
starting Milestone 2. The acceptance covers ADR-0001–0005, the version 1 byte/schema
specification and the bounded validation/evidence plan. It records design approval,
not a claim that the software or native adapters have been implemented or tested.

## Review summary

| Area | Accepted contract | Detail |
| --- | --- | --- |
| Saves | One writer, complete encrypted staging file, atomic publication, persistence barriers before success; reconcile uncertain outcomes before another mutation | [ADR-0003](adr/0003-committed-writes.md) |
| Locking | Lock after 5 minutes without app interaction, on OS session lock/sleep, and on manual lock; resuming stays locked | [ADR-0005](adr/0005-session-and-drafts.md) |
| Drafts | Explicit saves only. Security locks discard unsaved drafts without waiting for confirmation. Ordinary close with a draft offers Save, Discard, Cancel | [ADR-0005](adr/0005-session-and-drafts.md) |
| Secret display | Passwords hidden by default; explicit reveal lasts at most 30 seconds. No app clipboard command in the initial prototype | [ADR-0005](adr/0005-session-and-drafts.md) |
| Recovery | Keep the approved reusable paper secret. Display only during enrollment; explicit replacement requires transcription confirmation and a coherent commit. Slot replacement does not cure data-key compromise | [ADR-0004](adr/0004-offline-recovery.md) |
| File compatibility | Freeze one exact version 1 encoding and bounded schema before implementation; reject unsupported input without repair or overwrite | [Vault format](vault-format-v1.md) |
| Initial validation targets | macOS 26.3.1/Apple silicon/APFS; Windows 11 25H2/x64/NTFS; Ubuntu 24.04 LTS/x64/GNOME/ext4. Other combinations remain targets for later review | [Platform plan](platform-validation.md) |
| Crash guarantee | Initial requirement is application-termination safety. No OS-crash/power-loss support claim until the specific adapter/filesystem/storage behavior has evidence | [ADR-0003](adr/0003-committed-writes.md) |

Unsaved generated passwords may be lost. A website password change is outside the
vault transaction. Paper recovery cannot reconstruct a missing or corrupted vault.
No existing decision is reopened about Rust, Tauri/React, cryptographic algorithms,
the two independent slots, approved KDF costs, or the 16 MiB cap.

## Authorized implementation sequence

1. Review exact dependency releases/features and advisories, pin the toolchain and
   lockfile, then create a small framework-independent Rust core. A failed dependency
   review blocks that dependency; it does not silently change the crypto profile.
2. Implement the bounded parser/serializer and independent crypto test vectors with
   synthetic fixtures. Test malformed inputs before adding UI or real credentials.
3. Implement the persistence contract first against fault-injected operations,
   then the macOS adapter. The smallest storage experiment writes two synthetic
   snapshots, pauses at each boundary, terminates the process and checks reopening.
4. Validate Windows and Linux adapters separately. Process termination does not
   stand in for storage-crash testing. Do not advertise unverified platform support.
5. Demonstrate creation, independent unlock routes, locking, coherent mutations and
   controlled errors for Milestone 2. Recovery UI and the desktop interface follow
   their existing later milestones.

Argon2 timing, dependency integration, persistence experiments and packaged
platform behavior need executable evidence during authorized work. The design
review approves contracts and the evidence plan, not results that do not exist.

## Acceptance boundary

The owner approval satisfies the [roadmap](roadmap.md)'s combined design review
and explicitly authorizes the implementation needed to obtain evidence. The
Milestone 1 pull request records that approval; it does not fabricate a GitHub
review or imply permission to merge. Keep Milestone 2 work on a separate branch,
based on these approved contracts while the documentation PR remains open.
No merge, public release, signing purchase, new service or real credential use
is authorized by this handoff. Exact dependency review still precedes package use.
