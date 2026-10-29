# File map

Root:

- [README.md](README.md): Project overview.
- [AGENTS.md](AGENTS.md): Project context and agent working rules.
- [CONTRIBUTING.md](CONTRIBUTING.md): Git and review practices.

`docs/`:

- [charter.md](docs/charter.md): Goals and scope.
- [requirements.md](docs/requirements.md): Required behavior and acceptance checks.
- [threat-model.md](docs/threat-model.md): Security threats and limits.
- [system-design.md](docs/system-design.md): System design and Mermaid diagrams.
- [roadmap.md](docs/roadmap.md): Milestones and completion gates.
- [milestone-0-review.md](docs/milestone-0-review.md): Foundation checklist and verification.

`docs/adr/`:

- [README.md](docs/adr/README.md): Architecture decision index.
- [0001-implementation-platform.md](docs/adr/0001-implementation-platform.md): Rust/Tauri/React and desktop platform decisions.
- [0002-vault-security-and-format.md](docs/adr/0002-vault-security-and-format.md): Vault security and format.
- [0003-committed-writes.md](docs/adr/0003-committed-writes.md): Crash-safe saves.
- [0004-offline-recovery.md](docs/adr/0004-offline-recovery.md): Paper-secret recovery.
- [0005-session-and-drafts.md](docs/adr/0005-session-and-drafts.md): Locking and unsaved edits.
- [template.md](docs/adr/template.md): New decision template.

`.github/`:

- [pull_request_template.md](.github/pull_request_template.md): PR checklist.
