# Codeward

Local password manager targeting macOS, Windows, and Linux desktops.

The initial version will run offline, store credentials in a local encrypted vault,
and support a handwritten offline recovery secret for an intact vault. No account
or telemetry is required. Milestone 1's design was accepted and Milestone 2 implementation
was authorized on 2026-10-29. Rust owns the core; Tauri hosts a React/TypeScript UI.
The planned application installs and opens in its own window. The exact vault
format, save contract and initial validation matrix are specified; actual runtime
support remains unverified. The initial audience is a small private friends/family
prototype, and this documentation branch contains no application code.

## Project foundation

| Document | Purpose |
| --- | --- |
| [File map](tldr.md) | Quick guide to where information lives |
| [Project charter](docs/charter.md) | Goals, scope, exclusions, success criteria |
| [Requirements](docs/requirements.md) | Functional/nonfunctional requirements and acceptance evidence |
| [Threat model](docs/threat-model.md) | Assets, boundaries, threats, assumptions, limitations |
| [System design](docs/system-design.md) | Logical responsibilities and Mermaid context/lifecycle diagrams |
| [UI/core contract](docs/ui-core-contract.md) | Commands, data exposure, permissions and lock/save outcomes |
| [Roadmap](docs/roadmap.md) | Milestones, dependencies, completion gates |
| [Architecture decisions](docs/adr/README.md) | ADR index, proposals, and open decisions |
| [Accepted design handoff](docs/milestone-1-handoff.md) | Milestone 1 review record and authorized implementation sequence |
| [Platform decisions](docs/adr/0001-implementation-platform.md) | Selected Rust/Tauri/React stack and desktop targets |
| [Git practices](CONTRIBUTING.md) | Branches, reviews, commits, secret handling |

## Recovery and save boundaries

- The paper recovery secret restores access after losing the master password,
  provided the encrypted vault file remains intact and the secret is valid for it.
- One random vault key protects the contents. Separate master-password and recovery
  slots protect copies of that same key; either unlock path works independently.
  The paper secret is reusable and is not consumed by routine recovery.
- It cannot recreate a deleted, missing, or corrupted file. Vault-file recovery
  and backup features are deferred.
- An acknowledged save must survive an application crash under the defined storage
  guarantees. Interrupted saves must preserve a complete committed state.
- Newly generated passwords and edits that have not been committed are excluded
  from persistence guarantees.

Rust, Tauri and React/TypeScript are selected. Crypto selections are RustCrypto
Argon2id for master-password derivation, HKDF-SHA256 for the generated recovery
secret, and XChaCha20-Poly1305 for contents/key slots. The selected format uses a binary
envelope with an encrypted JSON snapshot, a starting Argon2id profile and a 16 MiB
file limit in [ADR-0002](docs/adr/0002-vault-security-and-format.md). The accepted
[format specification](docs/vault-format-v1.md) defines exact bytes and recovery
encoding; [ADR-0003](docs/adr/0003-committed-writes.md) defines save semantics.
Exact dependency/security review and native persistence evidence precede use.
Browser integration, online recovery, sync, and automated backups remain future options.
