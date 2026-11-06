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
- [json-dependency-review.md](docs/json-dependency-review.md): JSON package pins, feature/advisory review and decoder allocation boundaries.
- [vault-format-v1.md](docs/vault-format-v1.md): Accepted vault bytes, schema and paper-code encoding.
- [platform-validation.md](docs/platform-validation.md): Initial validation matrix and evidence plan.
- [macos-storage-foundation.md](docs/macos-storage-foundation.md): Synthetic native path/read/exclusion evidence, dependency review and limitations.
- [macos-staging.md](docs/macos-staging.md): Synthetic encrypted staging, native API/ACL review, file synchronization and failure evidence.
- [macos-publication.md](docs/macos-publication.md): Synthetic atomic creation/replacement, stale-base checks, uncertainty and process-kill evidence.
- [macos-namespace.md](docs/macos-namespace.md): Candidate file/parent barriers, expected-state checks, failure and process-kill evidence.
- [macos-transaction-experiment.md](docs/macos-transaction-experiment.md): Test-only core/native transaction composition, residue authority and interruption evidence.
- [macos-acl-admission.md](docs/macos-acl-admission.md): Synthetic parent/companion/read ACL refusal, revalidation and failure evidence.
- [macos-location-admission.md](docs/macos-location-admission.md): Destination mount-policy revalidation, injected transitions and remaining cloud/ancestor gates.
- [macos-managed-location.md](docs/macos-managed-location.md): Reviewed managed-directory APIs, ancestor policy, synthetic preparation evidence and limits.
- [continuous-integration.md](docs/continuous-integration.md): Rust CI checks, action pins, hosted evidence and local validation policy.

`docs/adr/`:

- [README.md](docs/adr/README.md): Architecture decision index.
- [0001-implementation-platform.md](docs/adr/0001-implementation-platform.md): Rust/Tauri/React and desktop platform decisions.
- [0002-vault-security-and-format.md](docs/adr/0002-vault-security-and-format.md): Crypto parameters, key slots, and vault format.
- [0003-committed-writes.md](docs/adr/0003-committed-writes.md): Crash-safe saves.
- [0004-offline-recovery.md](docs/adr/0004-offline-recovery.md): Reusable paper-secret recovery.
- [0005-session-and-drafts.md](docs/adr/0005-session-and-drafts.md): Locking and unsaved edits.
- [0006-managed-working-location.md](docs/adr/0006-managed-working-location.md): Managed working file, platform location proposal and backup/portability discussion.
- [template.md](docs/adr/template.md): New decision template.

`.github/`:

- [pull_request_template.md](.github/pull_request_template.md): PR checklist.
- [workflows/rust.yml](.github/workflows/rust.yml): Formatting, three-platform Rust checks and required-check gate.

`crates/codeward-core/`:

- [README.md](crates/codeward-core/README.md): Core boundary and verification commands.
- [src/envelope.rs](crates/codeward-core/src/envelope.rs): Structural parsing and public authentication metadata.
- [src/encoding.rs](crates/codeward-core/src/encoding.rs): In-memory envelope encoding from public metadata and encrypted bytes.
- [src/crypto.rs](crates/codeward-core/src/crypto.rs): Bounded derivation/encryption primitives and secret ownership.
- [src/credentials.rs](crates/codeward-core/src/credentials.rs): Validated credential records and snapshots.
- [src/credentials/bounds.rs](crates/codeward-core/src/credentials/bounds.rs): Nonallocating JSON resource preflight and decoded byte counts.
- [src/credentials/decoding.rs](crates/codeward-core/src/credentials/decoding.rs): Strict duplicate-rejecting snapshot decoding.
- [src/credentials/encoding.rs](crates/codeward-core/src/credentials/encoding.rs): Compact fixed-order JSON with capped plaintext output.
- [src/credentials/tests.rs](crates/codeward-core/src/credentials/tests.rs): Credential model tests; codec tests live beside their modules.
- [tests/fixtures/generate_credentials.py](crates/codeward-core/tests/fixtures/generate_credentials.py): Independent synthetic credential JSON fixtures.
- [src/recovery.rs](crates/codeward-core/src/recovery.rs): Paper-code formatting, validation and transcription confirmation.
- [src/vault.rs](crates/codeward-core/src/vault.rs): Authenticated master access, restricted recovery and encrypted candidate types.
- [src/vault/enrollment.rs](crates/codeward-core/src/vault/enrollment.rs): Confirmed paper enrollment and empty encrypted-vault preparation.
- [src/vault/updates.rs](crates/codeward-core/src/vault/updates.rs): Coherent content/password-change candidates and revision checks.
- [src/vault/tests.rs](crates/codeward-core/src/vault/tests.rs): Independent fixture, authentication, schema and enrollment tests; update tests live beside their module.
- [src/transaction.rs](crates/codeward-core/src/transaction.rs): Scoped request/status types, complete-file fingerprints and atomic publication admission.
- [src/transaction/session.rs](crates/codeward-core/src/transaction/session.rs): Secret-owning admission, generation revocation and explicit reload.
- [src/transaction/worker.rs](crates/codeward-core/src/transaction/worker.rs): Encrypted commit stages, bounded request history and unknown-outcome reconciliation.
- [src/transaction/storage.rs](crates/codeward-core/src/transaction/storage.rs): Trusted exclusive storage-adapter contract; no native implementation.
- [src/transaction/tests.rs](crates/codeward-core/src/transaction/tests.rs): Commit fault, scope, lock and recovery tests using the in-memory model in tests/model.rs.
- [tests/fixtures/generate_vault.py](crates/codeward-core/tests/fixtures/generate_vault.py): Vector-checked independent synthetic encrypted envelopes.
- [tests/envelope.rs](crates/codeward-core/tests/envelope.rs): Synthetic format and metadata tests.
- [tests/encoding.rs](crates/codeward-core/tests/encoding.rs): Exact encoding fixture, bounds and round-trip tests.
- [src/crypto/tests.rs](crates/codeward-core/src/crypto/tests.rs): Primitive vectors and failure/bounds tests.
- [src/recovery/tests.rs](crates/codeward-core/src/recovery/tests.rs): Paper-code fixtures and transcription-error tests.
- [examples/measure_master.rs](crates/codeward-core/examples/measure_master.rs): Synthetic local derivation timing.
- [examples/vault_round_trip.rs](crates/codeward-core/examples/vault_round_trip.rs): Synthetic in-memory creation/edit/lock/recovery demonstration.

`crates/codeward-storage-macos/`:

- [README.md](crates/codeward-storage-macos/README.md): Native experiment scope and checks.
- [src/lib.rs](crates/codeward-storage-macos/src/lib.rs): macOS-only boundary and controlled error types.
- [src/native.rs](crates/codeward-storage-macos/src/native.rs): Small documented descriptor-relative native-call wrappers and filesystem filters.
- [src/namespace.rs](crates/codeward-storage-macos/src/namespace.rs): Expected-state synthetic file/parent synchronization under publication exclusion.
- [src/namespace/tests.rs](crates/codeward-storage-macos/src/namespace/tests.rs): Native barrier order, errors, observation changes, contention and child-kill tests.
- [src/path.rs](crates/codeward-storage-macos/src/path.rs): Retained directory identities and stable companion writer lease.
- [src/path/managed.rs](crates/codeward-storage-macos/src/path/managed.rs): OS-account managed destination, private directory preparation and ancestor revalidation.
- [src/path/managed/tests.rs](crates/codeward-storage-macos/src/path/managed/tests.rs): Synthetic preparation, collisions, inheritance, substitutions, competing preparations and child-umask cases.
- [src/reading.rs](crates/codeward-storage-macos/src/reading.rs): Bounded unauthenticated reads with change detection.
- [src/reading/tests.rs](crates/codeward-storage-macos/src/reading/tests.rs): Synthetic native path/read and subprocess-lock tests.
- [src/staging.rs](crates/codeward-storage-macos/src/staging.rs): Lease-bound encrypted sibling preparation; no publication or save acknowledgement.
- [src/staging/tests.rs](crates/codeward-storage-macos/src/staging/tests.rs): Synthetic collisions, ACLs, staged I/O failures and subprocess interruption tests.
- [src/staging/publication.rs](crates/codeward-storage-macos/src/staging/publication.rs): Consuming synthetic publication with no-replace creation and expected-base replacement.
- [src/staging/publication/tests.rs](crates/codeward-storage-macos/src/staging/publication/tests.rs): Native publication races, uncertainty, coherent access and process-kill tests.
- [src/staging/transactions.rs](crates/codeward-storage-macos/src/staging/transactions.rs): Test-only native Storage/WriterLease bridge, absent from the runtime library.
- [src/staging/transactions/tests.rs](crates/codeward-storage-macos/src/staging/transactions/tests.rs): Synthetic native lifecycle, uncertainty, residue and process-kill checks through the core worker.
- [src/test_support.rs](crates/codeward-storage-macos/src/test_support.rs): Shared synthetic native test directories and fixture loading.
