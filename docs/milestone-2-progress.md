# Milestone 2 implementation evidence

Status: in progress, 2026-10-29. First slice only; the milestone is not complete.
Design baseline: [accepted handoff](milestone-1-handoff.md), recorded in
[PR #2](https://github.com/versatileisonline/codeward/pull/2), which is now merged.
This review covers only the first completed parser/metadata step; further
implementation waits for selection of the next modular step.

## Implemented first slice

The [Rust core](../crates/codeward-core/README.md) is a framework-independent library
with no third-party dependencies. It borrows a complete byte slice, checks actual
size and declared lengths, rejects unsupported version/suite/Argon2 profiles and
reserved fields, and exposes only unauthenticated public metadata/encrypted bytes.
It builds slot/payload AAD and recovery HKDF info from the exact original header
under the [normative format](vault-format-v1.md).

The result is deliberately named `UnauthenticatedEnvelope`. Parsing never unlocks
a vault, validates an AEAD tag or exposes decrypted credentials. A future bounded
file reader must enforce the cap before allocating its input; checking an existing
slice is not evidence that file loading already has that protection.

## Toolchain and component review

Pin Rust 1.99.1, edition 2024, with rustfmt and Clippy. Verify against the official
[stable release manifest](https://static.rust-lang.org/dist/channel-rust-stable.toml)
and [release notes](https://doc.rust-lang.org/releases.html#version-1991-2026-10-15).
The local installer was fetched from Rust's official HTTPS distribution and its
SHA-256 checked against the published installer checksum. Local tooling/caches
are excluded from Git; no shell startup files were changed.

`Cargo.lock` contains only `codeward-core`. There are no registry dependencies,
crate features, package build scripts or transitive crates in this slice. This
does not constitute a release/feature/advisory review of the planned crypto/JSON
packages. Perform that review and pin exact releases before their later use;
do not change the accepted algorithms or KDF profile to resolve integration issues.

## Verification performed

Host: macOS 26.3.1, Apple silicon. The following pass with the pinned toolchain:

| Check | Observed result |
| --- | --- |
| `cargo test --workspace --locked --offline` | 17 integration tests pass; no external dependency/network needed |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Pass |
| Synthetic fixture regeneration | Python-standard-library generator reproduces checked-in hex fixtures |
| Documentation/whitespace | Relative links, fences and `git diff --check` checked; existing Mermaid diagrams unchanged |

Tests cover every truncated prefix of a structural fixture, trailing bytes, exact
and excessive 16 MiB lengths, adversarial declared lengths, unsupported IDs and all
nonzero reserved-byte values, lower/higher/overflow-sized KDF costs, endian fields,
input preservation and AAD binding/exclusion behavior. Exact metadata fixtures are
constructed independently in Python and consumed as checked-in hex by Rust tests.
They are deliberately not valid encrypted vaults and contain no real keys/secrets.

Mutating ciphertext can still pass structural parsing. A test explicitly preserves
that distinction; later AEAD authentication must reject invalid tags. Neither
metadata fixtures nor these tests are cryptographic known-answer vectors or a
security audit. Only the host above was exercised; no Windows/Linux runtime claim.

## Remaining work

1. Review/pin cryptographic and JSON dependencies/features/advisories; implement
   primitive vectors, secret lifetimes, strict bounded schema and recovery encoding.
2. Implement independent slot unlock, payload authentication and schema gating,
   creation and authorized in-memory mutations. No plaintext may be released merely
   because this first parser succeeds.
3. Implement the fault-injected commit state machine, unknown-outcome reconciliation
   and writer exclusion, then validate native storage adapters against ADR-0003.
4. Demonstrate the full Milestone 2 acceptance scenarios using synthetic data.

There is no credential store ready for use, vault creation/unlock, file reader or
writer, save guarantee, native lock/session implementation, recovery workflow or
desktop UI yet. OS-crash/power-loss evidence and packaged support remain outstanding.
