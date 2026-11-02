# Codeward core

Milestone 2 library: structural parsing/encoding, public authentication metadata,
bounded cryptographic primitives, paper-secret transcription checks and strict
credential JSON handling and authenticated in-memory vault operations for the
[accepted format](../../docs/vault-format-v1.md). Rust 1.99.1 is pinned.
[Exact crypto pins/features and advisory review](../../docs/dependency-review.md)
are recorded; there are no runtime services.

`UnauthenticatedEnvelope::parse` borrows already-read bytes without allocating,
rejects unsupported profiles and malformed lengths, and returns only encrypted
bytes/public metadata. A future file reader must cap reads before allocating its
input. Passing this parser does not authenticate a vault or validate decrypted JSON.
The deliberately named type cannot authorize credential exposure.

`HeaderParts` supplies shared fixed-header encoding; `EnvelopeParts::encode`
assembles an in-memory envelope from public metadata and
already-encrypted bytes. It checks revision/payload bounds before allocating
output and emits the exact accepted profile. It neither encrypts/authenticates
those bytes nor generates randomness, increments revisions or saves a file.

AAD is constructed from the exact original header bytes. Slots exclude changing
revision and each other; the payload includes the complete header and both slots.
The structural hex fixtures are deliberately not valid encrypted vaults. Separate
full encrypted fixtures test both access routes and schema gating independently.
`crypto` provides fixed-profile master/recovery derivation and bounded authenticated
encryption. Owned keys/plaintext and explicit Argon2 work memory use zeroizing
buffers. One master derivation runs per process; a new OS-generated nonce object
is consumed on each encryption attempt. `open` returns authenticated bytes, not
validated credentials or session authorization. Do not expose these low-level
operations as generic UI commands.

`recovery` encodes/validates the canonical paper code and checks transcription
confirmation. The vault layer adds volatile enrollment lifetime/confirmation;
application display/session rules and committed recovery remain absent.

`credentials` validates record IDs, field byte limits, timestamp order, unique IDs
and the exact version 1 JSON schema. Its decoder rejects duplicate/unknown/missing
fields and malformed/oversized input before exposing a complete snapshot. The
serializer emits compact fixed-order JSON with sorted IDs through a capped writer.
Owned strings/output are zeroizing; bounded Serde escape scratch is not. These
types establish schema validity, not authentication or access permission. See the
[JSON package review](../../docs/json-dependency-review.md) for dependency/cleanup limits.

`vault` composes these components: `Enrollment` confirms the paper code and master
password, then produces an encrypted empty `VaultCandidate`. `unlock_with_master`
authenticates slot/payload and validates credentials before returning an
`AuthenticatedVault`; `begin_recovery` returns a restricted `RecoveryAccess` with
no browsing method. The former prepares content updates; the latter prepares a
replacement master slot and payload while retaining the reusable paper slot.
Both check expected revisions and return unsaved encrypted bytes. Preparation
leaves the authenticated source unchanged; it never acknowledges a file commit.
Consuming `lock` releases owned Rust key/record buffers. Caller copies, application
permissions, automatic locking, session generations and UI redaction are separate.

Tests cover format, primitives, paper codes, credentials and composed vault
operations using published vectors and independent synthetic Python/C fixtures.
Compile-fail examples cover nonce ownership, consumed access and restricted
recovery. See [evidence and limits](../../docs/milestone-2-progress.md) for counts,
exact validation scope and residual memory/storage/platform limitations.

From the workspace root with the pinned Rust toolchain installed:

```sh
cargo test --workspace --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
```

The optional synthetic timing example runs with
`cargo run --release --locked --offline --example measure_master`.

Run `cargo run --locked --offline --example vault_round_trip` for a synthetic
in-memory creation/edit/lock/recovery demonstration. It prints no secret values.

No file reader/writer, commit protocol, native durability adapter, application
session implementation or desktop UI exists yet. Never use these fixtures or this
incomplete library for real credentials.
