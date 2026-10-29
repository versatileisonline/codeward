# Codeward core

First Milestone 2 slice: dependency-free structural parsing and public AEAD/HKDF
metadata construction for the [accepted format](../../docs/vault-format-v1.md).
Rust 1.99.1 is pinned in the workspace. No third-party crates or runtime services
are used in this slice; cryptographic package pinning/security review precedes
the later implementation that uses them.

`UnauthenticatedEnvelope::parse` borrows already-read bytes without allocating,
rejects unsupported profiles and malformed lengths, and returns only encrypted
bytes/public metadata. A future file reader must cap reads before allocating its
input. Passing this parser does not authenticate a vault or validate decrypted JSON.
The deliberately named type cannot authorize credential exposure.

AAD is constructed from the exact original header bytes. Slots exclude changing
revision and each other; the payload includes the complete header and both slots.
The deterministic hex fixtures have an independent Python-standard-library
generator and are deliberately not valid encrypted vaults.

From the workspace root with the pinned Rust toolchain installed:

```sh
cargo test --workspace --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
```

No unlock, KDF/AEAD operation, recovery transcription, credential schema, file
reader/writer, native durability adapter, session implementation or desktop UI
exists yet. Never use these fixtures or this incomplete library for real credentials.
