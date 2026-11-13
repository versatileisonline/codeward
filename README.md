# Codeward

Codeward is an offline desktop password manager in development for a small
private friends-and-family prototype. It uses a local encrypted vault and a
handwritten recovery secret for an intact vault. The planned app requires no
account or telemetry. The accepted design uses a Rust core with a Tauri host
and React/TypeScript UI, and targets macOS, Windows and Linux.

## Current status

Milestone 2 is in progress. The Rust core implements vault authentication,
credential validation, session locking and the commit protocol. Platform
evidence so far uses synthetic data:

- macOS has a default-off writer tested for create, save, process termination
  and reopen on managed APFS storage. A separate packaged pilot exercised
  lock, sleep and redaction of synthetic WebView fields.
  See the [managed workflow](docs/macos-managed-workflow.md) and
  [desktop pilot](docs/macos-desktop-pilot.md#packaged-live-probes).
- Linux has bounded reads and writer exclusion tested on a disposable ext4
  filesystem. See the [Linux storage foundation](docs/linux-storage-foundation.md).
- Windows has bounded reads and writer exclusion for synthetic NTFS fixtures.
  See the [Windows storage foundation](docs/windows-storage-foundation.md).

Linux has an opt-in synthetic transaction adapter. Windows has a synthetic managed
read path. The next Windows storage step is handle-relative creation and staging.
Desktop UI integration starts in Milestone 4, after persistence and offline
recovery are accepted. See the [milestone sequence](docs/roadmap.md#path-to-desktop-integration).
Use synthetic data for development and testing.

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
| [Architecture decisions](docs/adr/README.md) | Accepted architecture decisions and change process |
| [Accepted design handoff](docs/milestone-1-handoff.md) | Milestone 1 review record and authorized implementation sequence |
| [Core progress](docs/milestone-2-progress.md) | Implemented scope, test evidence and remaining Milestone 2 work |
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

The vault uses RustCrypto Argon2id for master-password derivation, HKDF-SHA256
for the generated recovery secret, and XChaCha20-Poly1305 for contents and key
slots. The format uses a binary envelope with an encrypted JSON snapshot, a
starting Argon2id profile and a 16 MiB file limit in [ADR-0002](docs/adr/0002-vault-security-and-format.md). The accepted
[format specification](docs/vault-format-v1.md) defines exact bytes and recovery
encoding; [ADR-0003](docs/adr/0003-committed-writes.md) defines save semantics.
Real-vault use requires dependency and security review and native persistence
evidence. Browser integration, online recovery, sync, and automated backups
remain future options.

## Working on the core

Install the pinned Rust toolchain, then run the commands in the
[core README](crates/codeward-core/README.md). The
[Rust CI workflow](docs/continuous-integration.md) runs formatting, tests and Clippy
for PRs on hosted Linux, Windows and macOS. Use focused local checks during
development and current hosted results for final full-suite validation. The
[dependency review](docs/dependency-review.md) records exact crypto pins and features;
the [JSON review](docs/json-dependency-review.md) covers the credential codec.
[Progress evidence](docs/milestone-2-progress.md) records the current tests, limits
and initial single-host timing measurement. The synthetic
[vault round-trip example](crates/codeward-core/examples/vault_round_trip.rs) exercises
creation, content updates and recovery preparation without file writes or secret
output.
