# Project charter

Status: Milestone 0 foundation with owner-confirmed Milestone 1 scope updates.
Updated: 2026-10-28. Platform/UI details remain under design review.

## Purpose

Build a personal, local-first desktop password manager with understandable credential
storage, local password generation, and an offline path to regain access after
forgetting the master password. Work in small milestones with explicit review gates.

## Confirmed initial-version scope

- An installed desktop application targeting macOS, Windows, and Linux, with a
  local encrypted vault for each person. Initial private prototype audience:
  roughly 20 friends/family at most; no public release or shared-vault service.
- Rust core separated from the interface and local vault file. Users launch an
  application window without opening a browser tab or starting a localhost server.
  UI framework, supported OS versions/architectures, and Linux distributions remain open.
- Create, open, unlock, lock, and close a vault; view and manage credentials.
- Generate passwords on demand and explicitly save them with credentials.
- Use a master password for normal access and a separate handwritten recovery
  secret for access to an intact vault when the master password is forgotten.
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

Milestone 0 succeeds when a documentation-only branch/PR gives the owner a clear
charter, requirements, threat model, Mermaid diagrams, roadmap, ADR placeholders,
and Git practices to review. It contains no application code, dependencies, build
system, language/algorithm selection, or vault-format choice.

The later prototype succeeds when a person can create a vault, commit and retrieve
a credential offline, lock/reopen it, recover using the paper secret and intact
vault, and demonstrate save guarantees through fault tests. These are targets,
not implemented capabilities.

The owner reviews milestone completion and scope changes. Record unresolved
design choices in [ADRs](adr/README.md); a placeholder or proposal is not an accepted
choice. Begin each later milestone after its preceding gate is reviewed.

Learning follows architecture: discuss component contracts, ownership boundaries,
failure cases, and evidence so the owner can understand and contribute design
decisions while using AI agents for implementation.
