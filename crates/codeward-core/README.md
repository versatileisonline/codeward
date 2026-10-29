# Codeward core

Milestone 2 library: structural parsing/encoding, public authentication metadata,
bounded cryptographic primitives and paper-secret transcription checks for the
[accepted format](../../docs/vault-format-v1.md). Rust 1.99.1 is pinned.
[Exact crypto pins/features and advisory review](../../docs/dependency-review.md)
are recorded; there are no runtime services.

`UnauthenticatedEnvelope::parse` borrows already-read bytes without allocating,
rejects unsupported profiles and malformed lengths, and returns only encrypted
bytes/public metadata. A future file reader must cap reads before allocating its
input. Passing this parser does not authenticate a vault or validate decrypted JSON.
The deliberately named type cannot authorize credential exposure.

`EnvelopeParts::encode` assembles an in-memory envelope from public metadata and
already-encrypted bytes. It checks revision/payload bounds before allocating
output and emits the exact accepted profile. It neither encrypts/authenticates
those bytes nor generates randomness, increments revisions or saves a file.

AAD is constructed from the exact original header bytes. Slots exclude changing
revision and each other; the payload includes the complete header and both slots.
The deterministic hex fixtures have an independent Python-standard-library
generator and are deliberately not valid encrypted vaults.
`crypto` provides fixed-profile master/recovery derivation and bounded authenticated
encryption. Owned keys/plaintext and explicit Argon2 work memory use zeroizing
buffers. One master derivation runs per process; a new OS-generated nonce object
is consumed on each encryption attempt. `open` returns authenticated bytes, not
validated credentials or session authorization. Do not expose these low-level
operations as generic UI commands.

`recovery` encodes/validates the canonical paper code and checks transcription
confirmation. Application enrollment/display rules and complete recovery are absent.

41 tests cover parsing/metadata (17), encoding (5), crypto (10) and paper codes (9).
One compile-fail doctest verifies nonce ownership. Published primitive vectors and
independent Python/C fixtures supplement negative and round-trip cases. See
[evidence and measurements](../../docs/milestone-2-progress.md) for exact limits.

From the workspace root with the pinned Rust toolchain installed:

```sh
cargo test --workspace --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
```

The optional synthetic timing example runs with
`cargo run --release --locked --offline --example measure_master`.

No integrated vault creation/unlock, credential schema, file
reader/writer, native durability adapter, session implementation or desktop UI
exists yet. Never use these fixtures or this incomplete library for real credentials.
