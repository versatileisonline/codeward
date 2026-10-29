# UI/core contract

Status: Milestone 1 design baseline, 2026-10-28. Rust, Tauri, and React/TypeScript
are owner-selected. This is a behavioral contract, not implemented commands or a
final transport schema. Security/recovery, commit, and session details remain in
[ADR-0002](adr/0002-vault-security-and-format.md),
[ADR-0004](adr/0004-offline-recovery.md),
[ADR-0003](adr/0003-committed-writes.md), and
[ADR-0005](adr/0005-session-and-drafts.md).

## Responsibilities

- **React UI:** navigation, focus, form input, temporary drafts, rendering, and
  accurate display of core-reported status. It never decides that a vault is
  unlocked or that a write committed.
- **Tauri adapter:** a narrow IPC entry point, structured payload decoding, trusted
  caller/window context, and translation into core operations. It is not the vault
  implementation and must not replace core authorization with UI checks.
- **Rust core:** validates intent and state, controls secrets and credential data,
  authorizes access, generates passwords, runs recovery, and owns committed writes.
  Its application operations must be testable without React or the WebView.
- **Platform adapters:** native file selection, filesystem behavior, session events,
  and packaging integration for the declared macOS/Windows/Linux support matrix.

The selected topology is in [system-design.md](system-design.md). UI state may
cache presentation data; authoritative session/vault state stays in Rust.

## Command and data surface

Names below identify intended operations. Exact argument types and grouping can
be refined before implementation without widening privileges silently.

| Operation | UI to core | Core to requesting UI |
| --- | --- | --- |
| `select_vault` | Request native file selection | Scoped vault reference, display name, locked/open status; no decrypted contents |
| `begin_create_vault` | Approved destination reference and entered master password | Volatile enrollment reference and recovery secret for transcription display; not a ready/saved vault |
| `confirm_recovery_enrollment` | Enrollment reference and transcribed recovery secret for core validation | Ready status only after the complete initial vault/access metadata commits |
| `unlock_vault` | Vault reference and entered master password | Authoritative session status/generation or controlled error; no session keys |
| `begin_recovery` / `complete_recovery` | Vault reference, entered paper secret, and replacement master password at the appropriate step | Restricted recovery state, then completion only after the access change commits; no ordinary credential browsing during recovery |
| `list_credentials` / `search_credentials` | Query and bounded pagination for the selected vault | Only required summaries such as record ID, title, username and revision; no passwords or full-vault dump |
| `get_credential_details` / `reveal_password` | Record ID and requested details/reveal action | Only the selected fields, including plaintext when authorized for that interaction; sensitive notes are not included in list results |
| `generate_password` | Validated generation preferences | Volatile draft reference; plaintext only when needed for preview/use; generation does not save |
| `save_credential` | Changed fields or generated-draft reference, record ID when editing, expected revision and request ID | Committed revision after success, or controlled conflict/failure/unknown-outcome status |
| `delete_credential` | Record ID, expected revision, request ID and explicit delete intent | Committed revision after deletion commits, or controlled failure; no undelete guarantee |
| `lock_vault` / `get_session_state` | Lock request or state query | Locked/open state, session generation, and safe operation status; never secret material |
| `get_operation_status` | Request/operation reference | Proven commit outcome/revision if available, otherwise an explicit unresolved outcome; no credential payload |

Closing/canceling releases the relevant volatile operation context and denies new
access. ADR-0005 defines the accepted dirty-draft and in-flight-write policies;
acceptance is not implemented behavior.
The reusable paper secret opens the independent recovery slot for the same vault
key as the master-password slot. Routine recovery commits a new master-password
slot and keeps the paper secret valid; it never returns the old master password.
The Rust core performs derivation/unwrapping and retains the data key. Slot/derivation
metadata is not a reason to send key material or bulk decrypted data to React.
ADR-0002 selects RustCrypto Argon2id, HKDF-SHA256 and XChaCha20-Poly1305 for those
operations. They execute in Rust, not in React or generic UI-exposed crypto commands.
The selected recovery representation is 52 grouped Base32 characters plus a checksum
for 256 random bits; the Rust core validates/decodes it and supplies HKDF only the
secret bytes. Preserve master-password UTF-8 bytes without silent normalization.
The [format proposal](vault-format-v1.md) supplies exact encoding/checksum/context
bytes for review. ADR-0004 proposes no later secret redisplay, explicit confirmed
slot replacement and deferral of in-place data-key rotation. Reuse remains confirmed.
ADR-0005 recommends omitting app clipboard commands from the initial prototype;
adding them later needs a separate exposure/clearing policy and command review.

## Authorization and payload rules

1. Allow only named application operations. No generic `read_file`, `write_file`,
   arbitrary SQL, shell, unrestricted decryption, or broad filesystem interface is
   exposed to the UI. Native file selection yields a reference whose destination
   and overwrite behavior the core validates; a UI-provided path is not permission.
2. Bind references to the appropriate caller/window, vault and operation context.
   Record/draft IDs are identifiers, not authorization tokens. The selected vault
   reference may survive locking so it can be unlocked again; session/draft access
   must not survive lock/revocation. Enrollment has its own restricted context.
3. Derive caller context from Tauri, not a claimed window name in the payload.
   Validate allowed command, session state, ownership of referenced data, payload
   bounds, and revisions in Rust. TypeScript types and hidden buttons are not access control.
4. Require expected revisions on existing-data mutations to prevent stale writes.
   Carry request IDs for outcome correlation; do not log secret-bearing arguments.
   ADR-0003 selects in-memory result tracking without a durable operation ledger;
   restart reloads the actual vault and may leave historical request status unknown.
   A timeout or lost reply does not prove that nothing committed.
5. Return explicitly shaped data. Summaries, usernames, queries and notes remain
   private even when they are not passwords. Return no credential data while locked.

Tauri's registered application commands are accessible to app windows/WebViews by
default unless deliberately restricted. Its documented `AppManifest::commands`
permission mechanism must be evaluated and used to explicitly scope the custom
command surface to intended callers. Plugin permissions are also minimized;
capability configuration alone is not a substitute for core authorization.
See [Tauri capabilities](https://v2.tauri.app/security/capabilities/).

## Secret lifetime and UI limits

Session encryption keys and bulk decrypted vault state stay in the core. A typed
master password or recovery secret originates in UI memory and crosses IPC;
plaintext must also reach the UI when a credential is displayed or edited.
Minimize copies, retention and returned fields. Clear/redact displayed data and
references on lock, close, canceled operations and completed sensitive forms.

No credentials, master/recovery secrets, or sensitive form/query data in UI/core
logs, error reports, URL parameters, analytics, localStorage, IndexedDB, persisted
UI state, or debugging snapshots. Ordinary browser-backed persistence must not
be used as a credential/draft backup. In-memory drafts have no crash guarantee.

Clearing JavaScript references cannot prove every copy is erased. A compromised
unlocked UI can invoke permitted reveal actions and observe displayed/entered
secrets. A UI request does not prove a human gesture. Optional native confirmation
or reauthentication for selected operations requires a separate design decision;
no such extra step is silently added here.

## Replies, notifications and locking

- Commands return targeted structured results. Possible categories include
  `Locked`, `InvalidInput`, `Conflict`, `StorageFailure`, and `OutcomeUnknown`.
  Safe field identifiers may guide corrections; messages never echo secret input,
  raw payloads, or sensitive stack traces. Exact wire schema remains open.
- `Saved`/`Created`/`RecoveryComplete` are emitted only after the appropriate
  complete state commits. The UI can show pending progress but cannot invent success.
- Notifications such as `VaultLocked`, `VaultRevisionChanged` and operation
  progress carry only minimal status/references. Never broadcast credential
  contents, passwords, master passwords, or recovery secrets.
- The core owns a session generation that changes when access is revoked. Replies
  carry enough context for the UI to discard stale results after locking, switching
  vaults or starting a new session. Reject new privileged requests while locked;
  do not populate the screen from an old secret-bearing response.
- Locking must hide UI secrets promptly. An in-flight commit must resolve to a
  coherent state under ADR-0003; a security lock must not be postponed indefinitely
  by an edit. Final cancellation, memory-lifetime and commit coordination belongs
  in ADR-0005. Already exposed plaintext cannot be revoked retrospectively.

Tauri's [command documentation](https://v2.tauri.app/develop/calling-rust/) explains
request/reply and notification transports. Choosing a transport does not establish
ordering or save durability for Codeward; those guarantees must be implemented and tested.

## Packaged interface and permissions

Bundle interface assets, styles, fonts and icons locally. No remote pages, scripts,
iframes, automatic site-icon fetches or runtime analytics. Render stored credential
strings as text, not executable HTML. Explicitly configure a restrictive content
security policy that permits required local assets/IPC and no external network
origins. Do not copy example policies containing remote font/CDN permissions.
See [Tauri CSP](https://v2.tauri.app/security/csp/).

The installed app opens its own window and uses its bundled UI. A development
server, if later used, is development tooling; users do not start one. Internal
WebView/IPC protocol URLs do not authorize a remote service. Verify actual packaged
offline behavior and platform prerequisites before claiming support.

ADR-0001 proposes a single allowed main window, explicit custom-command
permissions, bounded paginated summaries and decimal-string revisions/generations
at the TypeScript boundary. The [platform plan](platform-validation.md) records
the test matrix. The owner accepted these contracts in the handoff; validate them before exposing
commands. Authorization and duplicate/bounds checks remain in Rust.

## Required evidence before relying on the boundary

- Deny unauthorized-window, malformed, locked-state and cross-vault requests.
- Reject stale revisions; preserve valid committed data on storage failures.
- Exercise enrollment/recovery state restrictions and interrupted access changes.
- Inject late replies around lock/switch/reunlock; verify no stale secret is shown.
- Inspect logs and persistence for sensitive data; acknowledge memory-erasure limits.
- Verify command permissions, absence of broad plugins/remote content, and offline
  packaged behavior on the bounded macOS/Windows/Linux matrix.
- Check password entry, focus, keyboard navigation, accessibility and lock redaction.

These are future tests, not evidence already obtained. No executable, configuration,
dependencies, mock commands, or runtime tests are created by this design document.
Follow the [implementation gate](roadmap.md#when-implementation-becomes-necessary)
before any feasibility code or product implementation.
