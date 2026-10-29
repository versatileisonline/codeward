# File map

Root:

- [README.md](README.md): Project overview.
- [CONTRIBUTING.md](CONTRIBUTING.md): Git and review practices.
- [Cargo.toml](Cargo.toml): Rust workspace.
- [rust-toolchain.toml](rust-toolchain.toml): Pinned compiler and developer tools.

`docs/`:

- [charter.md](docs/charter.md): Goals and scope.
- [requirements.md](docs/requirements.md): Required behavior and acceptance checks.
- [threat-model.md](docs/threat-model.md): Security threats and limits.
- [system-design.md](docs/system-design.md): System design and Mermaid diagrams.
- [ui-core-contract.md](docs/ui-core-contract.md): UI commands, data boundaries, and permissions.
- [roadmap.md](docs/roadmap.md): Milestones and completion gates.
- [milestone-1-handoff.md](docs/milestone-1-handoff.md): Design review summary and first implementation slice.
- [milestone-2-progress.md](docs/milestone-2-progress.md): Core implementation scope and evidence.
- [dependency-review.md](docs/dependency-review.md): Exact crypto package pins, features and advisory review.
- [vault-format-v1.md](docs/vault-format-v1.md): Accepted vault bytes, schema and paper-code encoding.
- [platform-validation.md](docs/platform-validation.md): Initial validation matrix and evidence plan.

`docs/adr/`:

- [README.md](docs/adr/README.md): Architecture decision index.
- [0001-implementation-platform.md](docs/adr/0001-implementation-platform.md): Rust/Tauri/React and desktop platform decisions.
- [0002-vault-security-and-format.md](docs/adr/0002-vault-security-and-format.md): Crypto parameters, key slots, and vault format.
- [0003-committed-writes.md](docs/adr/0003-committed-writes.md): Crash-safe saves.
- [0004-offline-recovery.md](docs/adr/0004-offline-recovery.md): Reusable paper-secret recovery.
- [0005-session-and-drafts.md](docs/adr/0005-session-and-drafts.md): Locking and unsaved edits.
- [template.md](docs/adr/template.md): New decision template.

`.github/`:

- [pull_request_template.md](.github/pull_request_template.md): PR checklist.

`crates/codeward-core/`:

- [README.md](crates/codeward-core/README.md): Core boundary and verification commands.
- [src/envelope.rs](crates/codeward-core/src/envelope.rs): Structural parsing and public authentication metadata.
- [src/encoding.rs](crates/codeward-core/src/encoding.rs): In-memory envelope encoding from public metadata and encrypted bytes.
- [tests/envelope.rs](crates/codeward-core/tests/envelope.rs): Synthetic format and metadata tests.
- [tests/encoding.rs](crates/codeward-core/tests/encoding.rs): Exact encoding fixture, bounds and round-trip tests.
