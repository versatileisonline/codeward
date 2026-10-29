# Core cryptography dependency review

Reviewed 2026-10-29 for the Milestone 2 crypto foundation under ADR-0002.
This is a release/feature/advisory integration review, not an independent audit
of these libraries or Codeward. The accepted algorithms, costs and file bytes
are unchanged. Credential JSON dependencies are not included in this review.

## Direct pins and enabled features

All direct requirements use exact `=version` pins and `default-features = false`.
`Cargo.lock` pins the complete resolved graph and registry checksums.

| Package | Version | Explicit features | Purpose |
| --- | --- | --- | --- |
| `argon2` | 0.6.0 | `alloc`, `zeroize` | Explicit Argon2id v0x13, 65536 KiB / 3 / 4, 32-byte output |
| `hkdf` | 0.13.0 | None | Full HKDF extract/expand for generated recovery input |
| `sha2` | 0.11.1 | `zeroize` | SHA-256 for HKDF and recovery checksum |
| `chacha20poly1305` | 0.11.0 | `alloc`, `zeroize` | Full-round XChaCha20-Poly1305 |
| `getrandom` | 0.4.3 | None | Native OS randomness; no browser/custom backend |
| `zeroize` | 1.9.1 | `alloc` | Owned secret buffers and explicit Argon2 work memory |
| `hmac` | 0.13.0 | `zeroize` | Enable available cleanup in HKDF's existing dependency |
| `poly1305` | 0.9.1 | `zeroize` | Enable available cleanup in AEAD's existing dependency |
| `ctutils` | 0.4.7 | None | Fixed-length recovery-secret/checksum comparisons |

The HMAC/Poly1305 declarations unify cleanup features on already-required packages;
ctutils is also already resolved through the crypto graph and is pinned directly
for recovery confirmation. These do not introduce alternative primitives.
The resolved feature tree excludes
PHC/password-verifier support, parallel Rayon execution, reduced-round AEAD,
browser randomness and generic KDF integration features. `password-hash` and
`phc` still occur in Cargo's lockfile resolution but are not enabled in the build.
Default features in transitive manifests remain visible in `cargo tree -e features`;
disabling a direct dependency's defaults does not disable all transitive defaults.

## Locked inventory

All 28 registry versions were checked against official registry metadata for
checksum equality and non-yanked status. Their published manifests declare
compiler requirements no higher than Rust 1.88, below the pinned Rust 1.99.1.
All offer MIT or Apache-2.0 licensing; `r-efi` additionally offers LGPL-2.1-or-later.
No project license or distribution permission is inferred from this review.

| Packages | Locked versions, in matching order |
| --- | --- |
| aead, argon2, base64ct, blake2 | 0.6.1, 0.6.0, 1.8.3, 0.11.0 |
| block-buffer, cfg-if, chacha20, chacha20poly1305 | 0.12.1, 1.0.5, 0.10.2, 0.11.0 |
| cipher, cmov, cpufeatures, crypto-common | 0.5.2, 0.5.4, 0.3.1, 0.2.2 |
| ctutils, digest, getrandom, hkdf | 0.4.7, 0.11.3, 0.4.3, 0.13.0 |
| hmac, hybrid-array, inout, libc | 0.13.0, 0.4.15, 0.2.5, 0.2.191 |
| password-hash, phc, poly1305, r-efi | 0.6.1, 0.6.1, 0.9.1, 6.0.0 |
| sha2, typenum, universal-hash, zeroize | 0.11.1, 1.20.1, 0.6.1, 1.9.1 |

Only `getrandom` and `libc` declare build scripts in this inventory. Inspection
found configuration/sanitizer/compiler/platform detection, with no package
download step. No procedural-macro package is resolved. Native backends contain
upstream unsafe code; Codeward continues to forbid unsafe code in its own crate.
Only the macOS host is exercised; this review does not establish target support.

## Advisory review

Matched every locked package name against the complete RustSec database snapshot
[`ee1a49a2769a24a596afc1c20cb33748d2769586`](https://github.com/RustSec/advisory-db/tree/ee1a49a2769a24a596afc1c20cb33748d2769586).
Read all five matching advisory records and compared their patched ranges:

| Advisory | Locked package | Patched range | Result |
| --- | --- | --- | --- |
| [RUSTSEC-2019-0019](https://rustsec.org/advisories/RUSTSEC-2019-0019.html) | blake2 0.11.0 | >= 0.8.1 | Patched |
| [RUSTSEC-2019-0029](https://rustsec.org/advisories/RUSTSEC-2019-0029.html) | chacha20 0.10.2 | >= 0.2.3 | Patched |
| [RUSTSEC-2021-0100](https://rustsec.org/advisories/RUSTSEC-2021-0100.html) | sha2 0.11.1 | >= 0.9.8 | Patched |
| [RUSTSEC-2026-0003](https://rustsec.org/advisories/RUSTSEC-2026-0003.html) | cmov 0.5.4 | >= 0.4.4 | Patched |
| [RUSTSEC-2026-0385](https://rustsec.org/advisories/RUSTSEC-2026-0385.html) | ctutils 0.4.7 | >= 0.4.4 | Patched |

No applicable advisory was found in this snapshot. This is a recorded manual
snapshot review, not a `cargo audit` run, vulnerability-free guarantee or future
monitor. Recheck when dependencies change and before distribution.

## Secret-lifetime findings

Argon2's allocating helper releases its backing blocks without clearing them.
Use `hash_password_into_with_memory` with a Codeward-owned `Zeroizing<Vec<Block>>`
instead. Keep owned keys and plaintext in zeroizing buffers, avoid secret-bearing
Debug implementations, and fail if OS randomness fails.

The selected features enable available cleanup for hash/cipher/MAC state. They
do not prove all compiler temporaries, HKDF intermediate outputs, allocator copies,
caller-owned inputs, registers, swap or crash dumps are erased. No memory locking,
complete-erasure or constant-time implementation audit is claimed.

## Primary sources and reproduction

Review exact published manifests and feature lists for
[argon2](https://docs.rs/crate/argon2/0.6.0/features),
[HKDF](https://docs.rs/crate/hkdf/0.13.0/features),
[SHA-2](https://docs.rs/crate/sha2/0.11.1/features),
[XChaCha20-Poly1305](https://docs.rs/crate/chacha20poly1305/0.11.0/features),
[getrandom](https://docs.rs/crate/getrandom/0.4.3/features),
[zeroize](https://docs.rs/crate/zeroize/1.9.1/features),
[HMAC](https://docs.rs/crate/hmac/0.13.0/features) and
[Poly1305](https://docs.rs/crate/poly1305/0.9.1/features).
Registry version metadata is available at
`https://crates.io/api/v1/crates/<package>/<version>`; compare checksums with
`Cargo.lock`. Inspect the host and declared target graphs with
`cargo tree --locked --offline -e features` and `--target <triple>`.
Run workspace tests, rustfmt and Clippy with the pinned compiler, locked/offline.
