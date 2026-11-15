# Codeward

Codeward is a local-first desktop password manager being built around an encrypted
vault and a handwritten recovery secret. The goal is to let people store and
manage credentials, generate passwords, and recover access after forgetting their
master password—all offline, without a required account or hosted recovery service.

The repository currently contains the Rust vault and session core, experimental
native storage for macOS, Windows and Linux, and a small macOS desktop pilot.
The complete credential interface and desktop installers are still in development.
Use synthetic data when exploring the project; it is not ready for real passwords.

## What works today

- **Encrypted vault operations:** create and authenticate vault contents, validate
  credential records, prepare complete encrypted updates, and open independent
  master-password or recovery access routes.
- **Session control:** keep authorization in Rust, lock access, reject stale
  requests and authentication results, and enforce an inactivity deadline.
- **Native persistence experiments:** stage and publish encrypted vault updates
  on APFS, NTFS and ext4, coordinate cooperating writers, and handle failed or
  uncertain saves without premature success acknowledgements.
- **Failure testing:** exercise malformed and tampered input, conflicting writes,
  application termination, storage failures and explicit retries using synthetic
  vaults and disposable test locations.
- **macOS desktop pilot:** a Tauri window exercises synthetic session state,
  lock/sleep events and field redaction. It does not browse or save credentials.

The next work covers the complete offline recovery workflow, followed by the
credential UI, password generation, desktop integration and packaging. See the
[roadmap](docs/roadmap.md) for progress and remaining validation.

## How it is built

The Rust core owns vault authentication, session authorization and commit outcomes.
Native adapters handle filesystem operations. The planned credential interface
uses React/TypeScript inside Tauri, with a limited command surface rather than
direct access to vault files or keys.

| Component | What to explore |
| --- | --- |
| [`codeward-core`](crates/codeward-core/README.md) | Vault format, cryptography, credential schema, recovery, sessions and transaction protocol |
| [`codeward-storage-macos`](crates/codeward-storage-macos/README.md) | Managed APFS storage, permissions, staging, publication and persistence barriers |
| [`codeward-storage-windows`](crates/codeward-storage-windows) | Managed NTFS storage, ACL and identity checks, native publication and persistence barriers |
| [`codeward-storage-linux`](crates/codeward-storage-linux) | Managed ext4 storage, location admission, publication and failure testing |
| [`codeward-host-macos`](crates/codeward-host-macos) | Native session events and the optional Tauri desktop pilot |

Native writers are experimental and require explicit feature opt-in. Storage
tests cover specific configurations and interruption points; they do not establish
complete desktop support across every machine in those OS families.

## Try the core

With `rustup` installed, [rust-toolchain.toml](rust-toolchain.toml) selects the pinned
Rust 1.99.1 toolchain. From the repository root:

```sh
cargo fetch --locked
cargo run -p codeward-core --locked --offline --example vault_round_trip
```

The [example](crates/codeward-core/examples/vault_round_trip.rs) creates an encrypted
vault candidate, adds a synthetic credential, locks and reopens it, then prepares
a replacement master password through recovery access. Everything stays in memory;
it writes no vault file and prints no secret values. Dependency fetching requires
network access; the example runs offline after dependencies are available.

For the optional macOS window, see the [desktop pilot guide](docs/macos-desktop-pilot.md).

## Development and testing

After fetching dependencies, the standard workspace checks are:

```sh
cargo check --workspace --no-default-features --locked --offline
cargo test --workspace --all-features --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
```

Platform-specific tests run on their native OS. Some capacity, mount and live-event
tests require explicitly prepared fixtures and are not run by the ordinary test
command. The [CI guide](docs/continuous-integration.md) explains the hosted matrix
and native checks; [contribution practices](CONTRIBUTING.md) cover focused changes,
reviews and synthetic test data.

## Vault and recovery design

One random data key encrypts the credential snapshot. Separate protected key slots
allow either the master password or the generated paper secret to unlock that key.
The implementation uses RustCrypto Argon2id for master-password derivation,
HKDF-SHA256 for recovery, and XChaCha20-Poly1305 for authenticated encryption.
The [format specification](docs/vault-format-v1.md) defines the exact bytes,
authentication boundaries and size limits.

- The reusable paper secret restores access to an intact vault. It cannot recreate
  a missing, deleted or corrupted vault file.
- Save acknowledgement follows the complete commit protocol. Generated passwords
  and edits that have not been committed can still be lost.
- Application-termination tests cover defined checkpoints. OS-crash and physical
  power-loss guarantees require separate evidence.
- Backups, synchronization, browser integration and vault-file restoration are
  outside the current implementation.

The project has not undergone an independent security audit. The
[threat model](docs/threat-model.md) describes its assumptions and limits.

## Further reading

- [System design](docs/system-design.md): architecture, responsibilities and flows.
- [Architecture decisions](docs/adr/README.md): storage, security, recovery and session contracts.
- [Requirements](docs/requirements.md): intended behavior and acceptance checks.
- [UI/core contract](docs/ui-core-contract.md): commands, data exposure and lock/save behavior.
- [Offline recovery](docs/offline-recovery.md): paper enrollment, password replacement and file-loss limits.
- [Roadmap](docs/roadmap.md): completed work and next steps.
- [File map](tldr.md): a guide to the repository's detailed documentation.
