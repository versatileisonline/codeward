# Credential JSON dependency review

Reviewed 2026-11-01 for the strict credential schema under ADR-0002. This extends
[the crypto review](dependency-review.md); algorithms, costs and format bytes are
unchanged. This is a package/feature/advisory review, not an independent audit.

## Pins and resolved graph

| Direct package | Exact version | Explicit features; defaults disabled |
| --- | --- | --- |
| `serde` | 1.0.229 | `std`; no derive macros |
| `serde_json` | 1.0.154 | `std`, `raw_value` |

Source: official [Serde package metadata](https://crates.io/api/v1/crates/serde/1.0.229)
and [serde_json metadata](https://crates.io/api/v1/crates/serde_json/1.0.154).
Cargo.lock adds eleven registry packages, for 39 total; all previously locked
versions/checksums remain unchanged. All eleven added archives matched official
registry SHA-256 metadata, were non-yanked and declared MSRVs at or below Rust 1.71,
below the pinned Rust 1.99.1.

| Added packages | Locked versions, in matching order | Declared licenses |
| --- | --- | --- |
| serde, serde_core, serde_derive | 1.0.229, 1.0.229, 1.0.229 | MIT OR Apache-2.0 |
| serde_json, itoa | 1.0.154, 1.0.18 | MIT OR Apache-2.0 |
| memchr | 2.8.3 | Unlicense OR MIT |
| zmij | 1.0.23 | MIT |
| proc-macro2, quote, syn | 1.0.107, 1.0.47, 3.0.9 | MIT OR Apache-2.0 |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |

`serde_core`'s disabled `cfg(any())` edge retains serde_derive and its macro graph
in the lockfile. They are not enabled/compiled by the selected host, Windows MSVC
or Linux GNU feature graphs. No derive, arbitrary_precision, float_roundtrip,
preserve_order or unbounded_depth feature is enabled. Graph inspection is not
execution on Windows/Linux. No project license/distribution approval is inferred.

New active build scripts are Serde/serde_core, serde_json and zmij. Inspection found
compiler/target/optimization configuration and generated private re-export modules,
without package downloads. Inactive quote/proc-macro2 scripts detect compiler
capabilities, including local compilation probes. No macro dependency is enabled.

## Advisory snapshot

Matched all 39 locked registry package names against the complete RustSec snapshot
[`2295466c4e9df26647fd67d8fa1c705ac2fc7f81`](https://github.com/RustSec/advisory-db/tree/2295466c4e9df26647fd67d8fa1c705ac2fc7f81).
No records matched the eleven added packages. The same five existing crypto records
matched: RUSTSEC-2019-0019, 2019-0029, 2021-0100, 2026-0003 and 2026-0385. Their
patched ranges still include the locked versions, as recorded in the crypto review.
No applicable advisory was found in this manual snapshot comparison. This was not
`cargo audit`, a vulnerability-free guarantee or an ongoing monitor.

## Decoder and plaintext ownership

The planned decoder uses borrowed `RawValue` string tokens, rather than a `Value`
map that loses duplicate keys or allocates strings before field-specific checks.
Reviewed source: [raw values](https://docs.rs/serde_json/1.0.154/serde_json/value/struct.RawValue.html),
[deserializer](https://docs.rs/crate/serde_json/1.0.154/source/src/de.rs) and
[slice reader](https://docs.rs/crate/serde_json/1.0.154/source/src/read.rs).
Borrowed raw strings are scanned without building their decoded contents. A
nonallocating preflight must bound container depth and encoded field-name tokens
before Serde runs; RawValue's ignored-container scanner itself does not enforce
Codeward's depth limit. Count decoded UTF-8 bytes before decoding each field.
Check collection counts before constructing an excess record. Reject duplicates
using explicit schema visitors, not intermediate maps.

Own credential strings and serialized plaintext in zeroizing buffers. Serde's
escaped-string scratch storage is bounded by the checked field but is not zeroized;
caller inputs, temporary copies, allocation movement, swap and dumps remain residual
exposure. No complete-erasure or constant-time parser claim is made. Translate
library failures into fixed errors without including input text or library messages.

Reproduce graph inspection with `cargo tree --locked --offline -e features` and
`--target <triple>` for the three targets above. Recheck advisories when the graph
changes and before distribution. Credential implementation/test evidence is tracked
separately in [Milestone 2 progress](milestone-2-progress.md).
