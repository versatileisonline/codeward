# Project charter

Status: Milestone 1 design accepted; Milestone 2 entry authorized.
Updated: 2026-10-29. Rust/Tauri/React, RustCrypto primitives, the starting profile
and format structure are selected. The logical save protocol is approved; concrete
remaining proposals are ready in the [Milestone 1 handoff](milestone-1-handoff.md).
The handoff records design acceptance and core implementation authorization on
2026-10-29.
No runtime support/security claim is verified.

## Purpose

Build a personal, local-first desktop password manager with understandable credential
storage, local password generation, and an offline path to regain access after
forgetting the master password. Work in small milestones with explicit review gates.

## Confirmed initial-version scope

- An installed desktop application targeting macOS, Windows, and Linux, with a
  local encrypted vault for each person. Initial private prototype audience:
  roughly 20 friends/family at most; no public release or shared-vault service.
- Rust core separated from a React/TypeScript interface hosted by Tauri and the local
  vault file. Users launch an
  application window without opening a browser tab or starting a localhost server.
  A bounded initial matrix and packaging plan are proposed in
  [platform-validation.md](platform-validation.md); runtime evidence is pending.
- Create, open, unlock, lock, and close a vault; view and manage credentials.
- Generate passwords on demand and explicitly save them with credentials.
- Use a master password for normal access and a separate handwritten recovery
  secret for access to an intact vault when the master password is forgotten.
- Protect each vault with one random data key and two independent protected key
  slots. Either the master password or reusable paper secret can open its slot;
  routine recovery replaces the master-password slot without consuming the paper secret.
- Use RustCrypto Argon2id for master-password derivation, HKDF-SHA256 for the
  generated recovery secret, and XChaCha20-Poly1305 for vault/key-slot encryption.
  The selected starting profile and binary envelope/encrypted JSON structure are
  recorded in ADR-0002. Exact bytes are accepted; current crypto release review and
  bounded core evidence are in [Milestone 2 progress](milestone-2-progress.md).
- Preserve committed credentials across application crashes and failed saves.
- Complete all initial runtime workflows offline, without mandatory accounts,
  telemetry, or a hosted service.
- Model theft, tampering, accidental deletion, interrupted writes, and malware,
  including the limits of the initial protection.

## Scope boundaries

The recovery secret is an alternative access credential, not a copy of the vault.
Losing the device and its only vault file cannot be repaired with the paper secret.
Keep it physically separate from the computer and vault. Someone with both a usable
recovery secret and its intact vault may gain access.

Crash safety concerns committed data. A password that has only been generated,
displayed, or copied, and any other uncommitted edit, may be lost on a crash. Users
must see whether a change was saved. Updating a password on an external website
is outside Codeward's transaction boundary.

## Deferred work

| Area | Boundary |
| --- | --- |
| Vault-file recovery | Restoring a deleted, missing, or damaged file; distinct from forgotten-password recovery |
| Backups | Automated copies, version history, restore workflows, off-device storage |
| Online identity/recovery | Optional future service; must preserve fully offline local use |
| Browser integration | Extensions, autofill, imports, website password changes |
| Sync and sharing | Cloud synchronization, multiple devices, collaborative/family vaults |
| Advanced endpoint defenses | Stronger malware resistance; no claim to defeat a compromised OS |
| Mobile platforms | Phone/tablet applications; desktop target families are already in scope |

These are possibilities, not commitments or prerequisites for the prototype.

macOS, Windows, and Linux are target families, not implemented compatibility claims.
Define a bounded support matrix and validate packaging, session events, accessibility,
and storage behavior on each declared target before prototype acceptance. Linux
support does not imply every distribution or desktop environment is supported.

## Success and change control

Milestone 0 succeeds when a documentation-only branch/PR provides a clear
charter, requirements, threat model, Mermaid diagrams, roadmap, ADR placeholders,
and Git practices to review. It contains no application code, dependencies, build
system, language/algorithm selection, or vault-format choice.

The later prototype succeeds when a person can create a vault, commit and retrieve
a credential offline, lock/reopen it, recover using the paper secret and intact
vault, and demonstrate save guarantees through fault tests. These are targets,
not implemented capabilities.

Milestone completion and scope changes require explicit approval. Record unresolved
design choices in [ADRs](adr/README.md); a placeholder or proposal is not an accepted
choice. Begin each later milestone after its preceding gate is reviewed.

Architecture documentation explains component contracts, responsibility boundaries,
failure cases and validation before implementation. Decisions stay open for
discussion and direct design contributions.
