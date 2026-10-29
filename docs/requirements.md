# Requirements

Status: design requirements with confirmed Rust/desktop scope updates.
These are intended behaviors, not implemented features.
All requirements apply to the initial version unless explicitly deferred.

## Terms

- **Vault:** the local encrypted file and metadata needed to open it.
- **Master password:** the user's normal vault-unlock secret.
- **Vault key:** one random per-vault data key protecting the encrypted contents.
- **Key slots:** two separately protected copies of the same vault key, opened by
  the master-password-derived wrapping key or the independent recovery wrapping key.
  Either route is sufficient; this is not a requirement to provide both secrets.
- **Recovery secret:** an independently app-generated, reusable offline access
  256-bit secret intended for handwritten storage as 52 Base32 characters plus a
  transcription checksum. HKDF-SHA256 derives its wrapping key. Exact encoding/
  context and replacement contracts were accepted on 2026-10-29; ordinary recovery does not consume it.
- **Draft:** a generated password or edit before a successful commit.
- **Committed save:** a complete vault update meeting the persistence contract.
  A success indication may appear only after this boundary.
- **Intact vault:** complete, authentic, supported vault data with usable access
  and recovery metadata. The recovery secret alone is insufficient.

The acceptance evidence below remains required. Milestone 2 has started with
dependency-free envelope parsing/AAD construction; its limited test evidence is
recorded in [milestone-2-progress.md](milestone-2-progress.md). No complete vault,
unlock, credential or persistence requirement is claimed satisfied by that slice.

## Functional requirements

| ID | Requirement | Acceptance evidence |
| --- | --- | --- |
| FR-01 | Create a local encrypted vault with one random data key and separate master-password and recovery slots protecting copies of that key. It is ready only after the complete initial state commits. | Create, close, reopen, and unlock offline through each independent route; inspect for no plaintext secrets/unwrapped keys. Interrupt creation; no partial file is presented as ready. |
| FR-02 | Open, unlock, lock, and close with clear session state. Incorrect credentials must not expose data or modify the file. | Exercise lifecycle and failed unlocks; compare file before/after failure. |
| FR-03 | Create, view, edit, search, and explicitly delete credentials with title/service, username, password, and optional URL/notes. | Commit mutations, reopen, verify records; cancel deletion and verify no change. |
| FR-04 | Generate passwords locally on demand using security-appropriate randomness. ADR-0005 defines accepted controls. Generation alone does not save a record. | Generate offline; distinguish draft from saved; save and reopen a selected result. |
| FR-05 | Show draft, saving, saved, and failed/uncertain states accurately. Never report failed or uncertain commits as saved. | Inject write failure and interrupt acknowledgement; verify UI reports only what is known. |
| FR-06 | Enroll an independent app-generated reusable paper secret with core-validated transcription confirmation and physical-separation guidance. Never silently persist its plaintext beside the vault. | Confirm a handwritten copy, reject mismatched transcription, inspect artifacts and diagnostics. |
| FR-07 | Recover offline by opening the recovery slot with a valid paper secret and intact vault; commit a replacement master-password slot under the same persistence contract. Routine recovery preserves the paper secret. | Recover, replace, reopen with new master password; old master password fails on the updated vault. Repeat recovery with the same paper secret. Invalid recovery leaves the file unchanged. |
| FR-08 | Support explicit lock and automatic lock on agreed inactivity/OS session events for each supported platform. Resolve timeout and dirty-state policies before implementation. | Verify chosen triggers on the declared macOS/Windows/Linux matrix hide credentials/end access and never misreport drafts as saved. |
| FR-09 | Detect unsupported, incomplete, or tampered vault input before exposing contents; preserve the input file on failure. | Open representative malformed/tampered files; controlled failure without destructive repair. |
| FR-10 | Explain that missing/deleted/corrupted files require a usable separate copy; the paper secret cannot recreate data. | Review recovery/file-error messages; no initial backup-feature promise. |

FR-07 selects reuse without automatic consumption/replacement after routine recovery.
ADR-0004 defines accepted explicit replacement, compromise limits and interrupted
changes. Replacing a master password cannot revoke access to old vault copies still
accessible with old credentials.

The confirmed key-slot model is recorded in
[ADR-0002](adr/0002-vault-security-and-format.md). A master-password change replaces
its protected slot; it does not automatically rotate the vault key. Threat handling
for a disclosed vault key must address newer contents as well as old copies.

## Nonfunctional requirements

| ID | Requirement | Acceptance evidence |
| --- | --- | --- |
| NFR-01 | All initial runtime workflows work offline without required accounts, telemetry, analytics, or remote recovery. | Run creation, CRUD, generation, lock, and recovery with network denied; inspect application-initiated network activity. |
| NFR-02 | Protect stored contents and both key slots with RustCrypto XChaCha20-Poly1305. Derive master-password and recovery wrapping keys with Argon2id and HKDF-SHA256 respectively. Account for authenticated metadata, temporary files and exposed metadata. | Review ADR-0002's profile/format and compatible releases; verify test vectors, nonce discipline, parameter bounds and tamper failures. No runtime evidence exists yet. |
| NFR-03 | Acknowledged commits survive app termination. An interrupted update yields the complete previous or new commit, never a partial mixture or destruction of the last good state. | Terminate before/during/after the defined commit boundary; reopen and compare the entire vault to valid states. |
| NFR-04 | Storage errors preserve the last committed vault; no false success; explicit retry/discard behavior. | Simulate capacity, permission, and I/O failures; verify records and recovery access stay coherent. |
| NFR-05 | No plaintext credentials, master passwords, or recovery secrets in logs, routine diagnostics, or persistent temp artifacts. Minimize secret lifetime; release session secrets on lock as the chosen stack permits. | Inspect logs/files; review lifetime, crash reporting, and platform limits; document residual exposure. |
| NFR-06 | Target installed macOS, Windows, and Linux desktop apps using Tauri, a React/TypeScript UI, and a Rust core. Bundle UI assets locally. Users need neither a browser tab nor a localhost server for runtime workflows. Define OS versions, architectures, Linux distributions and packaging before implementation. | Record a bounded support matrix and installation/runtime prerequisites; install, launch, and exercise offline workflows on each declared target before prototype acceptance. |
| NFR-07 | Lock/save/recovery/error states are understandable and usable by keyboard and assistive technology. Messages reveal no secrets. | Review future UI with keyboard/VoiceOver and representative errors. |
| NFR-08 | Prevent silent overwrites by cooperating Codeward writers. ADR-0003 selects writer exclusion plus stale-revision/base-snapshot checks; validate per platform. | Attempt concurrent updates; verify refusal/conflict handling preserves committed data; document limits for arbitrary external writers. |
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
Argon2id, HKDF-SHA256, XChaCha20-Poly1305 and RustCrypto are selected. ADR-0002
records the approved starting profile (64 MiB, 3 passes, 4 lanes), binary envelope
with an encrypted JSON snapshot, authentication boundaries and 16 MiB whole-file
limit. ADR-0004 records the generated 256-bit recovery secret and representation.
The [format proposal](vault-format-v1.md) specifies exact bytes/schema/checksum/
parser sublimits accepted through the reviewed handoff. The [handoff](milestone-1-handoff.md) and
[platform plan](platform-validation.md) collect the accepted design and evidence plan.
Review/pin exact component releases and advisories before use. Derivation timing
targets and all runtime guarantees remain unmeasured; design acceptance and
implementation authorization must not be mistaken for completed validation.
