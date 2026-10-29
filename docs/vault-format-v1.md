# Vault format version 1

Status: Accepted version 1 design, 2026-10-29, through the
[approved handoff](milestone-1-handoff.md). This is the normative byte/schema
specification. Independent implementation/security review and runtime evidence
are still required; acceptance does not certify an implementation.

## Envelope

All integer fields are unsigned little-endian. Byte arrays have exactly the
specified length; there is no implicit padding, optional section or trailing data.
The total file length must equal `280 + payload_length`, at most 16,777,216 bytes.
`payload_length` includes its full 16-byte authentication tag.

| Offset | Bytes | Field / required value |
| --- | --- | --- |
| 0 | 8 | Magic: ASCII `CODEWARD` |
| 8 | 2 | Format version: `1` |
| 10 | 2 | Suite: `1` (the approved Argon2id/HKDF-SHA256/XChaCha20-Poly1305 construction) |
| 12 | 16 | Independently random stable vault ID |
| 28 | 8 | Revision: starts at `1`, increments on every mutation; reject overflow |
| 36 | 4 | Header length: `280` |
| 40 | 4 | Payload length: at least `16`, at most `16,777,216 - 280` |
| 44 | 1 | Master slot role: `1` |
| 45 | 1 | Master KDF ID: `1` (Argon2id) |
| 46 | 1 | Argon2 version: `0x13` |
| 47 | 1 | Reserved: `0` |
| 48 | 4 | Argon2 memory KiB: `65536` |
| 52 | 4 | Argon2 passes: `3` |
| 56 | 4 | Argon2 lanes: `4` |
| 60 | 16 | Master-slot salt |
| 76 | 24 | Master-slot nonce |
| 100 | 48 | Encrypted 32-byte data key followed by 16-byte tag |
| 148 | 1 | Recovery slot role: `2` |
| 149 | 1 | Recovery KDF ID: `2` (HKDF-SHA256) |
| 150 | 2 | Reserved: `0` |
| 152 | 32 | Recovery HKDF salt |
| 184 | 24 | Recovery-slot nonce |
| 208 | 48 | Encrypted 32-byte data key followed by 16-byte tag |
| 256 | 24 | Payload nonce |
| 280 | payload_length | Encrypted UTF-8 JSON followed by 16-byte tag |

Suite/KDF identifiers are Codeward-local identifiers. Version 1 accepts exactly
the above values and lengths. Keys are always 32 bytes; tags are always 16 bytes.
The master and recovery slots occupy exactly 104 and 108 bytes respectively.
Creation generates independent random data key, vault ID, salts and nonces.

Before allocation/KDF work, bound actual bytes read as well as reported file size,
check all lengths with checked arithmetic, reject unsupported profiles, require
exact end-of-file and reject revision zero. A file growing during reading cannot
bypass the cap. Failed parsing/authentication must never rewrite the input.

## Authenticated bytes and derivation

The notation `[a,b)` means the original file bytes from offset `a` up to but
excluding `b`. `||` means concatenation. ASCII literals include a final NUL only
where `\0` is explicitly shown. Fixed lengths make each concatenation unambiguous.

- Master-slot AAD: ASCII `Codeward/slot/v1\0` || `[0,28)` || `[44,100)`.
- Recovery-slot AAD: the same ASCII prefix || `[0,28)` || `[148,208)`.
- Payload AAD: ASCII `Codeward/payload/v1\0` || `[0,280)`.
- Recovery HKDF info: ASCII `Codeward/recovery-wrap/v1\0` || `[8,28)`.
- HKDF input: decoded 32-byte recovery secret; salt: `[152,184)`; full
  extract-and-expand; output: 32 bytes.
- Argon2 input: exact entered UTF-8 password bytes, without normalization,
  trimming or case conversion; salt: `[60,76)`; approved explicit profile;
  output: 32 bytes, not a password-verifier string. No Argon2 pepper/secret or
  optional associated-data input is used in version 1.

Slots exclude revision and the other slot from their AAD. Payload AAD includes
both encrypted slots, revision, lengths and nonce. Any changed slot requires fresh
payload encryption. Ordinary content saves retain both slots byte-for-byte.
Every new encryption/rewrap/retry uses a fresh random 24-byte nonce under its key.
Failure to obtain randomness aborts before publication; never substitute clocks,
counters, deterministic seeds or partially filled buffers.

Unwrap a slot only after structural/profile checks. Authenticate the payload and
validate the full schema before exposing any credential or entering a normal
unlocked session. One valid slot does not waive damaged-header rejection.

## JSON snapshot

UTF-8 without BOM. One object with exactly `schema` (integer `1`) and
`credentials` (array, at most 10,000 records). Empty arrays are valid. Each record
has exactly these fields; optional user content uses an empty string:

| Field | Type / decoded UTF-8 bound |
| --- | --- |
| `id` | 32 lowercase hex characters encoding 16 random bytes; unique within the snapshot |
| `title` | String, 1–512 bytes |
| `username` | String, 0–1,024 bytes |
| `password` | String, 0–4,096 bytes |
| `url` | String, 0–4,096 bytes; plain text, never automatically fetched |
| `notes` | String, 0–65,536 bytes |
| `created_at` | Integer Unix milliseconds, 0–253,402,300,799,999 |
| `updated_at` | Same type/range; at least `created_at` |

Reject unknown/duplicate fields, duplicate IDs, nulls, non-integer numeric forms
for integer fields, invalid Unicode, invalid string escapes and trailing input.
Maximum container depth is three: root object, credentials array, record object.
Validate counts/string bounds while decoding, before oversized allocations.
Do not deserialize through an intermediate map that silently discards duplicates.
The envelope bounds total encoded JSON as well as individual decoded fields.

Object field order and insignificant JSON whitespace are not authenticated by
canonicalization: authenticate the exact stored ciphertext first. The serializer
emits compact JSON, fixed schema field order and records in ascending ID order for
repeatable fixtures. This does not make ciphertext deterministic. Timestamps are
informational, not conflict tokens or evidence against rollback. Preserve user
credential strings exactly; do not normalize them on a round trip.

## Recovery transcription

Use the [RFC 4648 Base32 alphabet](https://www.rfc-editor.org/rfc/rfc4648.html#section-6)
`ABCDEFGHIJKLMNOPQRSTUVWXYZ234567`. Encode the 32 secret bytes as 52 characters,
without `=` padding. Require the final character's four unused low bits to be zero.
The checksum is the first five bytes of SHA-256 of ASCII
`Codeward/recovery-check/v1\0` followed by the 32 secret bytes, encoded as eight
Base32 characters. This is transcription-error detection, not authentication.

Append the eight checksum characters to the 52 secret characters and display all
60 as 15 groups of four separated by ASCII hyphens. On input, cap raw UTF-8 at
256 bytes, remove ASCII hyphens/spaces/tabs/CR/LF, and uppercase ASCII letters.
Require exactly 60 remaining characters, decode the first 52 canonically, and
verify all eight checksum characters before any HKDF work. Reject other characters
and Unicode lookalikes; never map `0`/`1` to letters. Derive only from the decoded
secret bytes. Confirmation compares the decoded confirmed secret to enrollment's
secret inside Rust. Do not persist the plaintext code for later redisplay.

## Components and evidence

Use the already selected RustCrypto packages. For implementation entry, recommend
`argon2` 0.6, `hkdf` 0.13 with `sha2` 0.11, and `chacha20poly1305` 0.11. Supporting
recommendations are `getrandom` 0.4 for OS randomness, `zeroize` 1 for owned secret
buffers, and `serde` 1 / `serde_json` 1 with bounded duplicate-rejecting decoding.
These are release-line recommendations, not a compatibility/audit certificate.

Before adding dependencies, review and pin exact stable patch releases/features,
transitive dependencies, MSRV, licenses and current advisories. Disable crypto
crate defaults; explicitly enable only allocation and available zeroization
support needed by the key-derivation/AEAD APIs. Do not enable reduced-round AEAD,
browser randomness, PHC-verifier helpers or alternate KDF profiles. Verify feature
names against each pinned release before using them. Never fall back to different
primitives to make dependencies compile. Keep the resulting lockfile reviewed.

Owned passwords, wrapping/data keys, plaintext snapshots and KDF work memory need
explicit lifetime/cleanup review; a crate feature does not prove all temporaries
are erased. HKDF/hash state, allocator copies, UI strings, swap and crash dumps
remain residual concerns. No memory-locking or complete-erasure claim is made.

Primary API references: [Argon2 key derivation](https://docs.rs/argon2/0.6.0/argon2/),
[HKDF](https://docs.rs/hkdf/0.13.0/hkdf/),
[XChaCha20-Poly1305](https://docs.rs/chacha20poly1305/0.11.0/chacha20poly1305/),
[getrandom](https://docs.rs/getrandom/0.4.3/getrandom/), and
[zeroize limits](https://docs.rs/zeroize/1.8.2/zeroize/).

Before relying on this format, independently review byte ranges/lengths and AAD,
use published primitive vectors, and check deterministic synthetic full-envelope
fixtures with independently computed expected bytes. Negative tests mutate every
header/slot/payload field, splice slots between vaults, truncate at boundaries,
exercise malformed/oversized JSON and verify neither unlock route exposes data.
The checksum needs fixed vectors for unused bits, grouping, transposition, wrong
characters and normalization. No vectors or runtime results exist in this handoff.
