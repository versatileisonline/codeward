# Codeward

Local password manager targeting macOS, Windows, and Linux desktops.

The initial version will run offline, store credentials in a local encrypted vault,
and support a handwritten offline recovery secret for an intact vault. No account
or telemetry is required. Milestone 0 established the foundation; Milestone 1 has
started with Rust chosen for the core and a shared UI/core/local-vault design.
The application will install and open in its own window. UI framework and exact
platform support remain open. There is no application implementation yet; the
initial audience is a small private friends/family prototype.

## Project foundation

| Document | Purpose |
| --- | --- |
| [File map](tldr.md) | Quick guide to where information lives |
| [Agent context](AGENTS.md) | Project context and working rules for future iterations |
| [Project charter](docs/charter.md) | Goals, scope, exclusions, success criteria |
| [Requirements](docs/requirements.md) | Functional/nonfunctional requirements and acceptance evidence |
| [Threat model](docs/threat-model.md) | Assets, boundaries, threats, assumptions, limitations |
| [System design](docs/system-design.md) | Logical responsibilities and Mermaid context/lifecycle diagrams |
| [Roadmap](docs/roadmap.md) | Milestones, dependencies, completion gates |
| [Architecture decisions](docs/adr/README.md) | ADR index, proposals, and open decisions |
| [Platform decisions](docs/adr/0001-implementation-platform.md) | Confirmed Rust/desktop targets and open UI decisions |
| [Git practices](CONTRIBUTING.md) | Branches, reviews, commits, secret handling |
| [Milestone 0 review](docs/milestone-0-review.md) | Deliverables and owner checklist |

## Recovery and save boundaries

- The paper recovery secret restores access after losing the master password,
  provided the encrypted vault file remains intact and the secret is valid for it.
- It cannot recreate a deleted, missing, or corrupted file. Vault-file recovery
  and backup features are deferred.
- An acknowledged save must survive an application crash under the defined storage
  guarantees. Interrupted saves must preserve a complete committed state.
- Newly generated passwords and edits that have not been committed are excluded
  from persistence guarantees.

Rust is selected. UI toolkit, algorithms, key derivation, vault format, and persistence
mechanism remain undecided. Tauri with TypeScript/React is a recommendation only.
Browser integration, online recovery, sync, and automated backups remain future options.
