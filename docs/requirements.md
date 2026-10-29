# Requirements

Status: design requirements with owner-confirmed Rust/desktop scope updates.
These are intended behaviors, not implemented features.
All requirements apply to the initial version unless explicitly deferred.

## Terms

- **Vault:** the local encrypted file and metadata needed to open it.
- **Master password:** the user's normal vault-unlock secret.
- **Recovery secret:** a separate offline access secret intended for handwritten
  storage. Its encoding, lifecycle, and construction remain undecided.
- **Draft:** a generated password or edit before a successful commit.
- **Committed save:** a complete vault update meeting the persistence contract.
  A success indication may appear only after this boundary.
- **Intact vault:** complete, authentic, supported vault data with usable access
  and recovery metadata. The recovery secret alone is insufficient.

The evidence below is required in later milestones. No runtime tests have been
performed: the repository still contains documentation only.

## Functional requirements

| ID | Requirement | Acceptance evidence |
| --- | --- | --- |
| FR-01 | Create a local encrypted vault with a master password and enrolled recovery secret. It is ready only after the initial state commits. | Create, close, reopen, and unlock offline. Interrupt creation; no partial file is presented as ready. |
| FR-02 | Open, unlock, lock, and close with clear session state. Incorrect credentials must not expose data or modify the file. | Exercise lifecycle and failed unlocks; compare file before/after failure. |
| FR-03 | Create, view, edit, search, and explicitly delete credentials with title/service, username, password, and optional URL/notes. | Commit mutations, reopen, verify records; cancel deletion and verify no change. |
| FR-04 | Generate passwords locally on demand using security-appropriate randomness. Controls/policy are decided later. Generation alone does not save a record. | Generate offline; distinguish draft from saved; save and reopen a selected result. |
| FR-05 | Show draft, saving, saved, and failed/uncertain states accurately. Never report failed or uncertain commits as saved. | Inject write failure and interrupt acknowledgement; verify UI reports only what is known. |
| FR-06 | Enroll the paper secret with transcription confirmation and physical-separation guidance. Never silently persist its plaintext beside the vault. | Confirm a handwritten copy, reject mismatched transcription, inspect artifacts and diagnostics. |
| FR-07 | Recover offline with a valid secret and intact vault; allow a replacement master password and commit it under the same persistence contract. | Recover, replace, reopen with new master password; old master password fails on the updated vault. Invalid recovery leaves the file unchanged. |
| FR-08 | Support explicit lock and automatic lock on agreed inactivity/OS session events for each supported platform. Resolve timeout and dirty-state policies before implementation. | Verify chosen triggers on the declared macOS/Windows/Linux matrix hide credentials/end access and never misreport drafts as saved. |
| FR-09 | Detect unsupported, incomplete, or tampered vault input before exposing contents; preserve the input file on failure. | Open representative malformed/tampered files; controlled failure without destructive repair. |
| FR-10 | Explain that missing/deleted/corrupted files require a usable separate copy; the paper secret cannot recreate data. | Review recovery/file-error messages; no initial backup-feature promise. |

FR-07 does not choose secret reuse/rotation/replacement. ADR-0004 must specify that
behavior and interrupted-change handling. Replacing a master password cannot
revoke access to old vault copies still accessible with old credentials.

## Nonfunctional requirements

| ID | Requirement | Acceptance evidence |
| --- | --- | --- |
| NFR-01 | All initial runtime workflows work offline without required accounts, telemetry, analytics, or remote recovery. | Run creation, CRUD, generation, lock, and recovery with network denied; inspect application-initiated network activity. |
| NFR-02 | Protect stored credential confidentiality/integrity; account for recovery metadata, temporary files, and exposed metadata. | Review the later security/format ADR; inspect artifacts; tampering fails safely. No algorithm is chosen here. |
| NFR-03 | Acknowledged commits survive app termination. An interrupted update yields the complete previous or new commit, never a partial mixture or destruction of the last good state. | Terminate before/during/after the defined commit boundary; reopen and compare the entire vault to valid states. |
| NFR-04 | Storage errors preserve the last committed vault; no false success; explicit retry/discard behavior. | Simulate capacity, permission, and I/O failures; verify records and recovery access stay coherent. |
| NFR-05 | No plaintext credentials, master passwords, or recovery secrets in logs, routine diagnostics, or persistent temp artifacts. Minimize secret lifetime; release session secrets on lock as the chosen stack permits. | Inspect logs/files; review lifetime, crash reporting, and platform limits; document residual exposure. |
| NFR-06 | Target installed macOS, Windows, and Linux desktop apps using Tauri, a React/TypeScript UI, and a Rust core. Bundle UI assets locally. Users need neither a browser tab nor a localhost server for runtime workflows. Define OS versions, architectures, Linux distributions and packaging before implementation. | Record a bounded support matrix and installation/runtime prerequisites; install, launch, and exercise offline workflows on each declared target before prototype acceptance. |
| NFR-07 | Lock/save/recovery/error states are understandable and usable by keyboard and assistive technology. Messages reveal no secrets. | Review future UI with keyboard/VoiceOver and representative errors. |
| NFR-08 | Prevent silent overwrites by another writer: enforce one active writer or detect conflicting revisions; mechanism undecided. | Attempt concurrent updates; verify refusal/conflict handling preserves committed data. |
| NFR-09 | Make the Rust core authoritative for session state, vault access, recovery and commits. UI requests use a limited validated interface; the UI has no direct vault-file access or session-key access. Return only data needed for the current interaction. | Reject locked-state and malformed UI requests; review command permissions and data exposure. Verify plaintext shown for authorized interaction is cleared/redacted on lock, subject to documented platform limits. |

NFR-09 follows the [UI/core contract](ui-core-contract.md): a scoped Tauri command
surface, trusted caller context, Rust authorization, minimal targeted payloads,
revision conflicts, and stale-reply rejection after lock. Clearing references is
not proof of complete erasure, and a compromised unlocked UI can observe permitted plaintext.

### Persistence boundary

NFR-03 covers application crashes/process termination on supported local storage.
OS crashes and sudden power loss require explicit durability operations, filesystem
assumptions, and evidence in ADR-0003 before claiming those guarantees. Hardware
destruction, unreadable media, deletion, and rollback to an older authentic copy
are not solved by crash-safe commits.

A crash after commit but before acknowledgement may leave the new state saved
without a success message. Reopen must show the actual committed state. A failed
save must not erase the previous complete state. Credentials and the metadata
needed for normal unlock/recovery must commit coherently.

### Exclusions and open decisions

Newly generated but uncommitted passwords and unsaved edits have no persistence
guarantee. File undelete/reconstruction, backups, sync, browser integration, online
recovery, and protection against a fully compromised OS are outside initial guarantees.

Rust, Tauri, React/TypeScript and macOS/Windows/Linux target families are selected. Resolve the
remaining [ADR decisions](adr/README.md) before implementation, including UI/core
communication, exact support matrix, security/recovery construction, and writes.
Establish performance targets and vault size from representative use cases. No
cryptographic algorithm, format, or persistence mechanism is selected yet.
