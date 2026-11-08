# ADR-0006: Managed working vault and portable encrypted copies

Status: Proposed implementation contract, 2026-11-05. The owner selected managed
working storage and authorized its implementation after discussing alternatives;
the concrete platform/behavior contract below is submitted for PR review.

## Context and decision

The first prototype uses one active vault per OS account in Codeward-managed
application data. Creation/opening obtains that destination inside Rust, not from
an arbitrary path supplied by the UI. This narrows storage validation and avoids
requiring users to manage filenames for ordinary use (FR-01/02, NFR-03/04/08/09).

| Platform | Working file policy | Native status |
| --- | --- | --- |
| macOS, initial nonsandboxed desktop package | OS-account home + `Library/Application Support/Codeward/Vaults/vault.cw` | Directory preparation in the macOS experiment; runtime writer pending |
| Windows | OS-resolved LocalAppData + `Codeward/Vaults/vault.cw` | Native resolution/admission pending |
| Linux | Validated XDG_DATA_HOME, normally `~/.local/share`, + `codeward/vaults/vault.cw` | Native resolution/admission pending |

Resolve these through the platform/account APIs and validate actual storage;
display strings/environment values are not authorization. A sandboxed package
needs its own reviewed app-data resolution rather than assuming this macOS layout.
The working file remains the unchanged version-one Codeward envelope. No KDBX
compatibility, online account, device-bound encryption or new key store is added.

The managed directory holds the working file, stable writer companion and encrypted
transaction stages. Private app-owned directories require effective-user ownership,
mode 0700 and empty ACLs; trusted ancestors require root/effective-user ownership,
no group/world write and no ACL grants. Deny-only ancestor ACLs are permitted.
Never follow symlinks, repair permissions, replace unrelated entries or clean up
ambiguous residue to obtain a usable path. Unsupported storage refuses operations.

For macOS, require one local writable nonremovable APFS device from the account
home through the fixed managed subtree. Refuse account homes within the recognized
`Library/CloudStorage` and `Library/Mobile Documents` roots. Arbitrarily configured
third-party synchronization remains outside the supported location. Treat the
named `vault.cw` as sole authority and preserve all leftover stages and unrelated
entries, even when the vault is missing or invalid. See
[capacity and admission evidence](../macos-capacity-and-admission.md).

The existing generic native resolver remains synthetic experiment infrastructure;
the future application runtime must obtain the managed destination. Its UI command
surface cannot bypass admission through arbitrary file selection. A missing file
does not authorize silent empty-vault creation: enrollment remains an explicit
operation, and readiness still requires the initial committed write.

## Backup and portability discussion

- **Working location:** the single authoritative active file on this computer.
  Directory preparation, staged files and successful reads are not saved-state
  acknowledgement, retained versions or a backup service.
- **Encrypted backup:** a separate complete vault copy. Automatic backups,
  retention/versioning and a backup/restore UI remain deferred. Before real-user
  distribution, review clear data-loss guidance and the intended copy/restore
  workflow; do not represent folder management as protection against deletion,
  disk failure or loss of the only device. OS backups are separate from Codeward.
- **Portability:** the file format carries its protected key slots and encrypted
  contents. An intact compatible copy still needs its valid master password or
  paper secret. Moving a copy does not re-encrypt it or require a Codeward account.
  Actual opening on another platform still requires that platform's validated
  adapter; the format is portable, packaged support is not yet established.
- **Later copy/restore operation:** prefer an encrypted copy with a clearly stated
  committed revision, rather than plaintext export. Preserve the source; validate
  the destination and require explicit overwrite intent. Import/restore/migration
  and custom working folders are separate future workflows, not implemented here.
- **Recovery limits:** paper restores access to an intact vault, not missing bytes.
  Keep it physically separate. Old copies retain old password slots; slot changes
  cannot revoke them or repair exposure of their shared data key (ADR-0002/0004).
- **Removal:** updates, ordinary close and uninstall must preserve managed data by
  default. A future delete/reset flow needs explicit destructive intent; reinstall
  must not overwrite an existing working file. No installer/reset implementation
  or data-removal guarantee is supplied by this native preparation experiment.

## Alternatives, consequences and validation

User-chosen working folders offer flexibility and easier visible file handling,
but broaden cloud-sync, external/network drive, permission and conflict cases.
Managed storage is the selected initial behavior; custom working paths may be
reviewed later. Backup destinations can be a separate decision from the active
working location. No backup/sync scope is silently added to the prototype.

A standard app-data path alone does not certify absence of third-party syncing.
Keep local filesystem, ancestor/identity/permission and mount admission checks;
retain the unsupported-location and compromised-OS limits. Real-vault admission,
native persistence acceptance, runtime sessions, UI and per-platform installation
still need evidence. Do not advance the milestone based on folder preparation.

Verify fresh/idempotent preparation, malformed account results, existing data,
links/special-file collisions, ACLs/inheritance, restrictive umasks, substitutions,
competing preparations and failures without repair/deletion. Track native scope in
[macOS managed-location evidence](../macos-managed-location.md), and keep the
[requirements](../requirements.md), [UI/core contract](../ui-core-contract.md),
[platform plan](../platform-validation.md) and [roadmap](../roadmap.md) consistent.
