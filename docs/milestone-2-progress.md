# Milestone 2 implementation evidence

Status: in progress, 2026-11-01. Format, crypto primitives, paper-code handling
and strict credential snapshots; the milestone is not complete.
Design baseline: [accepted handoff](milestone-1-handoff.md), recorded in
[PR #2](https://github.com/versatileisonline/codeward/pull/2), which is now merged.
The parser/metadata step is tracked in [PR #4](https://github.com/versatileisonline/codeward/pull/4),
the encoder in [PR #5](https://github.com/versatileisonline/codeward/pull/5), and the
cryptographic foundation in [PR #6](https://github.com/versatileisonline/codeward/pull/6).
Separate subsequent commits add encoding, reviewed crypto dependencies,
cryptographic primitives and paper-secret transcription checks. All three
implementation PRs are merged into `main`. The current credential-schema slice
adds validated records and strict JSON decoding/encoding. No integrated
vault creation/unlock or storage behavior is claimed.

## Implemented first slice

The [Rust core](../crates/codeward-core/README.md) began as a framework-independent
library with no third-party dependencies. Its parser borrows a complete byte slice, checks actual
size and declared lengths, rejects unsupported version/suite/Argon2 profiles and
reserved fields, and exposes only unauthenticated public metadata/encrypted bytes.
It builds slot/payload AAD and recovery HKDF info from the exact original header
under the [normative format](vault-format-v1.md).

The result is deliberately named `UnauthenticatedEnvelope`. Parsing never unlocks
a vault, validates an AEAD tag or exposes decrypted credentials. A future bounded
file reader must enforce the cap before allocating its input; checking an existing
slice is not evidence that file loading already has that protection.

## Implemented second slice

[`EnvelopeParts::encode`](../crates/codeward-core/src/encoding.rs) assembles the
exact version 1 layout from borrowed public metadata and already-encrypted slots
and payload. Fixed-size arrays constrain slot/salt/nonce lengths; the encoder
emits only the accepted identifiers/profile and zero reserved fields. It rejects
zero revision, a payload shorter than its tag, and output exceeding the whole-file
cap before allocating output. Supplied bytes are preserved.

This is an in-memory format encoder, not vault creation or a file writer. It
does not generate randomness, encrypt, authenticate, validate JSON, increment a
revision or authorize a mutation. Callers remain responsible for those future
operations; structurally valid output can contain invalid ciphertext/tags.

Five new tests compare complete output against the independently generated Python
fixture, cover minimum/exact-cap/over-cap sizes and invalid revisions/tag lengths,
and exercise varied metadata, ciphertext preservation and parser round trips.
No new dependency, algorithm, parameter or format change is introduced.

## Cryptographic foundation

[`crypto`](../crates/codeward-core/src/crypto.rs) now implements the accepted
Argon2id profile, recovery HKDF-SHA256 derivation and full XChaCha20-Poly1305
encryption/authentication through pinned RustCrypto packages. The
[dependency review](dependency-review.md) records exact releases/features,
all 28 registry packages, compiler/license checks and advisory results before use.

Master derivation preserves entered bytes, uses a nonblocking process-wide guard
to permit only one attempt at a time, and explicitly owns/clears its 64 MiB work
buffer. Keys and temporary plaintext use zeroizing owned buffers. New encryption
accepts only an OS-generated nonce object that is consumed on an attempt; even a
failed attempt cannot reuse that object. A compile-fail test exercises this rule.
Size checks precede plaintext copies; bounded working allocations use fallible
reservation. Randomness failure returns no partial result and has no fallback.

Primitive authentication returns no plaintext on failure. Successful decryption
produces authenticated bytes only: the separate credential decoder must validate
the returned bytes before exposure; access checks and session creation are still
absent. These library operations are not UI commands.
They do not yet wire both protected slots into an unlock/create workflow.

## Paper-code component

[`recovery`](../crates/codeward-core/src/recovery.rs) generates an independent
32-byte secret, formats its canonical Base32/checksum representation into 15 groups
of four, and validates bounded transcription input. Decoding permits only the
specified ASCII case/separators, enforces unused-bit canonicality and verifies
the checksum before returning a secret that can derive a wrapping key.

Confirmation compares decoded secrets with the reviewed fixed-length comparison
primitive. Owned secret/display buffers are cleared on drop. This component does
not enforce enrollment state, limit display to enrollment, persist confirmation,
unlock a vault or replace a master password. Those application controls remain
future work; checksum success alone does not authorize access.

## Credential schema component

Implemented 2026-11-01 under the unchanged version 1 schema. The
[`credentials` module](../crates/codeward-core/src/credentials.rs) owns validated
records and sorts snapshots by ID, rejecting duplicates. Text is preserved exactly
in zeroizing strings; fields are private and exposed as borrows for core operations.
No Debug/Clone/Copy implementation or generic public Serde serialization is provided.
Constructors validate IDs, UTF-8 byte bounds and timestamp range/order before copying.

`CredentialSnapshot::decode` caps existing plaintext bytes before UTF-8 parsing.
A nonallocating preflight enforces depth three and bounded field-name tokens.
Explicit visitors reject unknown/duplicate/missing fields and excess record counts;
borrowed raw values prevent eager string/container decoding. Field decoded-byte
counts, including Unicode escapes/surrogate pairs, are checked before unescaping.
Wrong collection types are rejected as borrowed tokens, avoiding large decoded
strings in Serde's type-error path. Integer fields reject fractional/exponent forms.
Failed decoding returns no partial snapshot and leaves input untouched. Library
errors are translated into fixed diagnostics without source text.

`encode` writes compact fixed-order JSON with ascending IDs through a capped
zeroizing writer. It returns no partial output on failure and checks the full
plaintext limit before extending the output buffer. This is serialization, not
vault encryption or a file commit; individually valid records can exceed the total
encoded snapshot cap and are then rejected by the writer.

The [JSON dependency review](json-dependency-review.md) records exact pins, all
new locked packages/features and a fresh full-graph advisory snapshot. Serde's
bounded escaped-string scratch is not zeroized. Caller copies, allocator movement,
compiler temporaries, swap and dumps remain residual exposure; no complete-erasure
claim or allocation-failure injection is made.

Twenty new tests cover field/model boundaries, unknown/duplicate/missing fields,
IDs, timestamps, wrong types, malformed Unicode/escapes, every truncated prefix
of a synthetic record snapshot, trailing bytes and input preservation. They cover
exact/over-cap snapshots, 10,000 versus 10,001 records, escaped decoded-field
limits, deep containers, oversized keys, fixed diagnostics and deterministic
serialization. Unicode length tests exercise every valid BMP scalar plus selected
surrogate pairs; these finite tests are not an exhaustive parser/security audit.
Two Python-standard-library fixtures independently encode synthetic UTF-8 and
ASCII-escaped snapshots; Rust output matches the compact UTF-8 fixture exactly.

Schema validity does not authenticate input or grant access. No authenticated
vault operation calls this decoder yet; creation/unlock, authorized mutations,
file I/O, persistence and session/UI workflows remain unimplemented.

## Toolchain and component review

Pin Rust 1.99.1, edition 2024, with rustfmt and Clippy. Verify against the official
[stable release manifest](https://static.rust-lang.org/dist/channel-rust-stable.toml)
and [release notes](https://doc.rust-lang.org/releases.html#version-1991-2026-10-15).
The local installer was fetched from Rust's official HTTPS distribution and its
SHA-256 checked against the published installer checksum. Local tooling/caches
are excluded from Git; no shell startup files were changed.

The first two slices were dependency-free. The crypto foundation pinned nine
direct packages and 28 registry packages; the JSON slice adds two direct pins and
eleven locked packages, for eleven direct and 39 total registry packages.
Reviewed host/Windows/Linux feature graphs do not enable PHC helpers, reduced
rounds or browser randomness. Graph inspection is not cross-platform execution.
The separate JSON review covers its graph.

## Verification performed

Host: macOS 26.3.1 (25D2128), Apple silicon. Rechecked 2026-11-01 with the pinned toolchain:

| Check | Observed result |
| --- | --- |
| `cargo test --workspace --locked --offline` | 61 tests pass: 17 parser/metadata, 5 envelope encoding, 10 crypto, 9 paper-code, 20 credentials; plus 1 compile-fail doctest |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Pass |
| Synthetic fixture regeneration | Credential fixtures regenerated with Python standard library on 2026-11-01; foundation structural/crypto/paper regeneration was recorded on 2026-10-29, including argon2-cffi 25.1.0 / bindings 25.1.0 |
| Documentation/whitespace | Relative links, fences and `git diff --check` checked; existing Mermaid diagrams unchanged |

Tests cover every truncated prefix of a structural fixture, trailing bytes, exact
and excessive 16 MiB lengths, adversarial declared lengths, unsupported IDs and all
nonzero reserved-byte values, lower/higher/overflow-sized KDF costs, endian fields,
input preservation and AAD binding/exclusion behavior. Exact metadata fixtures are
constructed independently in Python and consumed as checked-in hex by Rust tests.
They are deliberately not valid encrypted vaults and contain no real keys/secrets.

Mutating ciphertext can still pass structural parsing; primitive AEAD tests now
reject modified ciphertext/tags, every nonce/AAD byte, wrong keys, truncation and
extra bytes. Both layers preserve their inputs. Malformed credential-JSON rejection
now has standalone component tests; whole-envelope authentication and its
integration with schema gating remain absent.

Known-answer tests use [RFC 9106 section 5.3](https://www.rfc-editor.org/rfc/rfc9106.html#section-5.3),
[RFC 5869 appendix A.1](https://www.rfc-editor.org/rfc/rfc5869.html#appendix-A.1)
and the XChaCha vector in
[draft-irtf-cfrg-xchacha-03 appendix A.3.1](https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03#appendix-A.3.1).
The XChaCha source is an archived Internet-Draft, not an RFC. The Argon2 RFC vector
uses test-only small costs/secret/associated data; the production API exposes only
the approved profile, separately compared with independently computed C fixtures.
Those fixtures include spaces, case and composed/decomposed Unicode passwords.
HKDF and paper-code expected bytes are independently computed with Python's
standard library. Normal tests need neither Python nor a network after Cargo fetch.

Paper tests cover all 1,860 alternative single-character substitutions of one
60-character fixture, unequal adjacent transpositions, all 15 noncanonical unused-bit
variants, lookalikes/padding/invalid lengths, the 256-byte raw bound, mismatched valid
confirmation, ASCII normalization and 256 repeated-byte round trips. These finite
examples are not a guarantee against every checksum collision or parser defect.
No memory-erasure, timing-side-channel or independent security audit is claimed.
Only the host above was exercised; no Windows/Linux runtime claim.

## Initial master-derivation measurement

The [synthetic timing example](../crates/codeward-core/examples/measure_master.rs)
runs six sequential derivations with the exact 65536 KiB / 3-pass / 4-lane profile
and clears each derived key. In a release build, the measured attempts were
88.938, 69.475, 71.580, 170.593, 76.219 and 69.272 ms. The first is the first call
within that process, not a guaranteed cold-machine measurement. `/usr/bin/time -l`
reported maximum resident set size 68,698,112 bytes (about 65.5 MiB) for the entire
process, not a separately measured KDF allocation peak.

Reproduce with `cargo run --release --locked --offline --example measure_master`
and `/usr/bin/time -l target/release/examples/measure_master` on macOS. This is a
small, noisy single-host observation, not a performance guarantee or the required
slowest-target benchmark. The rough 0.5–1 second target remains unevaluated across
the platform matrix; no cost was changed in response to this faster host.

## Review findings and verification

The pre-merge review checked the core source, tests and accepted byte/profile
contracts across PR #4–6. No blocking parser or encoder defect was found. One
resource-handling defect in the crypto helper was fixed: unsupported password
lengths could allocate the 64 MiB Argon2 work buffer before the upstream library
rejected them. The helper now checks Argon2's existing maximum before locking or
allocation. A new test covers the length boundary using scalar lengths, without
allocating a multi-gigabyte password. Source review verifies the guard's placement.
This does not introduce a new application password policy or alter KDF costs.

The exact PR #4 and #5 snapshots were tested in isolated temporary source copies:
17 and 22 tests respectively, plus rustfmt and Clippy with warnings denied.
At that pre-merge review, the corrected combined core passed 41 tests plus the
nonce compile-fail doctest.
These are local macOS checks; the PRs have no hosted check runs/statuses at review.
This review does not replace an independent security audit or establish integrated
vault, schema, storage or platform guarantees. PR #4–6 were retargeted to `main`
in dependency order and merged with merge commits; no hosted CI evidence is claimed.

## Remaining work

1. Implement independent slot unlock, payload authentication and schema gating,
   creation and authorized in-memory mutations. No plaintext may be released merely
   because structural parsing or standalone schema validation succeeds. Extend
   secret-lifetime review as integrated operations arrive.
2. Implement the fault-injected commit state machine, unknown-outcome reconciliation
   and writer exclusion, then validate native storage adapters against ADR-0003.
3. Demonstrate the full Milestone 2 acceptance scenarios using synthetic data.

There is no credential store ready for use, vault creation/unlock, file reader or
writer, save guarantee, native lock/session implementation, recovery workflow or
desktop UI yet. OS-crash/power-loss evidence and packaged support remain outstanding.
