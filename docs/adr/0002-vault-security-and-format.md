# ADR-0002: Vault security and format

Status: Accepted — normative format/profile and evidence plan; implementation unverified.
Owner decisions: 2026-10-28; concrete handoff approved 2026-10-29.
Acceptance PR: pending publication. Target: Milestone 1 design gate.

## Confirmed key model

- Generate one random data-encryption key per vault. It protects the vault contents.
- Store two separately protected copies of that same key: a master-password slot
  and an independent recovery slot. These are two unlock paths, not two data keys.
- Derive the normal wrapping key from the master password using Argon2id, a random
  salt and an explicit cost profile. Derive the recovery wrapping key independently
  with HKDF-SHA256 from the app-generated paper secret. Either valid path is sufficient; both are
  not required together, and neither path reconstructs the other secret.
- Keep the unwrapped vault/wrapping keys in the Rust core. Store encrypted contents,
  protected key slots and necessary public format/derivation metadata in the vault;
  never plaintext unlock secrets or an unwrapped vault key.
- Authenticate slot/payload data and bind metadata to the intended vault and slot
  roles under the selected construction below. The format document linked below
  defines the accepted exact byte encodings.

This is a key-wrapping design, consistent with established
[data-key and key-encryption-key guidance](https://cheatsheetseries.owasp.org/cheatsheets/Cryptographic_Storage_Cheat_Sheet.html#encrypting-stored-keys).
See [system-design.md](../system-design.md#vault-key-and-unlock-slots) for the diagram
and [ADR-0004](0004-offline-recovery.md) for the reusable paper-secret lifecycle.

## Selected algorithms and Rust libraries

| Purpose | Owner-selected algorithm | RustCrypto package |
| --- | --- | --- |
| Master-password wrapping key | Argon2id | [`argon2`](https://docs.rs/argon2/latest/argon2/) |
| Recovery wrapping key | HKDF-SHA256 | [`hkdf`](https://docs.rs/hkdf/latest/hkdf/) with `sha2` |
| Vault contents and both key slots | XChaCha20-Poly1305 authenticated encryption | [`chacha20poly1305`](https://docs.rs/chacha20poly1305/latest/chacha20poly1305/) using its XChaCha variant |

The owner also approved the format and parameter recommendations below in the
Codeward design conversation. Package versions/features and exact byte encodings
are not selected, and no dependencies are installed. Review compatible releases,
advisories, audit scope and target support before dependency setup; these decisions
do not establish audit coverage or runtime evidence for Codeward.

## Selected starting parameter profile

| Setting | Owner-approved starting value |
| --- | --- |
| Vault data key | Independently generated random 32 bytes |
| Master-password KDF | Argon2id version `0x13`, memory `65536` KiB (64 MiB), `3` passes, `4` lanes |
| Master-password salt | Independent random 16 bytes; replace on master-password change |
| Master-password wrapping key | 32-byte Argon2id output from the key-derivation API, not a serialized password verifier |
| Recovery secret | Independent random 32 bytes (256 bits of entropy) |
| Recovery KDF | HKDF-SHA256, full extract-and-expand, 32-byte output |
| Recovery HKDF salt | Independent random 32 bytes, stored in the recovery slot |
| Slot and payload AEAD | XChaCha20-Poly1305, 32-byte key, fresh random 24-byte nonce, full 16-byte tag |

The Argon2id settings match [RFC 9106's second recommended profile](https://www.rfc-editor.org/rfc/rfc9106.html#section-4),
chosen as a lower-memory cross-platform starting point instead of its 2 GiB first
recommendation. The target is roughly 0.5–1 second for derivation on the slowest
supported computer, not a measured result. Benchmark during authorized implementation
before finalizing the profile; never silently weaken it on a slower machine.

Initially accept only this explicitly supported Argon2id profile. Persist its
algorithm/version, salt and cost values and validate them before allocation/KDF
execution. Allow only one derivation attempt at a time in the app. Preserve master
passwords as entered UTF-8 bytes without trimming, case changes or Unicode normalization.

HKDF input is the decoded random recovery-secret bytes, excluding display separators
and checksum. Its unambiguously encoded context identifies Codeward, the format
version, vault ID and recovery-wrapping purpose. The normative format document
defines the exact context bytes.
HKDF does not harden a weak user-chosen password or create additional entropy;
see [RFC 5869](https://www.rfc-editor.org/rfc/rfc5869.html#section-3).

Use fresh securely generated nonces for every new encryption, including retries
and rewraps; never deliberately reuse a key/nonce pair. Store nonces and full tags
with ciphertext. The randomness provider and its error handling still require review.

## Selected version 1 file structure

Use one versioned file: a small explicitly specified binary envelope, exactly two
key slots and one encrypted snapshot of all credentials. The payload is UTF-8 JSON.
This keeps envelope fields predictable and the credential schema familiar without
introducing per-record encryption or history management.

| Part | Contents | Protection |
| --- | --- | --- |
| Format header | Codeward identifier, format version, crypto-suite identifier, random vault ID, revision and lengths | Public, authenticated by the payload |
| Master-password slot | Argon2id parameters, salt, nonce, protected copy of the data key and tag | Password-derived wrapping key |
| Recovery slot | HKDF salt, nonce, protected copy of the same data key and tag | Recovery-derived wrapping key |
| Payload | Credentials, titles, URLs, notes, timestamps and internal IDs | One authenticated ciphertext under the data key |

The complete public header includes both encrypted slot records and payload nonce/
length metadata. Exact field order, widths, endianness, identifiers, slot/AAD byte
encoding and JSON schema are still specification work, not implied implementation.
No attachments, compression, per-record encryption or append-only history in version 1.
Each content update encrypts a complete new snapshot. File size, format, public
crypto parameters and stable vault ID remain visible; credential metadata is encrypted.

## Selected authentication boundaries

- Each slot authenticates its own metadata: vault ID, format/suite, slot role,
  derivation parameters/salt and nonce. Define an unambiguous byte encoding.
- Slot associated data excludes the changing content revision and the other slot.
  Updating contents must not require knowledge of both unlock secrets.
- Payload associated data covers the complete public header, including both encrypted
  slots, revision and payload nonce/length. It excludes the payload ciphertext/tag
  itself, avoiding a circular dependency. Associated data is public but tamper-evident.
- Replacing either slot requires re-encrypting the payload with a fresh nonce to
  authenticate the new header. Ordinary content updates retain the existing slots
  and use the unlocked data key without rerunning Argon2id. Routine recovery retains
  the existing recovery slot and does not require the forgotten master password.

Do not release credentials until payload authentication and schema validation
succeed. Corruption of either slot record invalidates header authentication even
when the other slot unwraps successfully. The design requires an intact compatible
file and does not promise partial salvage.

## Selected input limits and failure behavior

Limit the entire version 1 vault file to 16 MiB, checked before large allocations.
Also bound JSON nesting, record counts and field lengths; exact subordinate limits
remain to be specified. Reject unsupported versions/profiles, invalid lengths,
duplicate fields, truncation and authentication failures without modifying the input.

An authenticated revision can assist conflict identification. It cannot detect
replacement by an older authentic file without a separate trusted reference.

## Access changes and limits

Normal unlock opens the master-password slot, then authenticates/decrypts contents.
Recovery opens the recovery slot and permits committing a replacement master-password
slot. It restores access without revealing the forgotten password. All slot/payload
changes follow the coherent commit contract in ADR-0003; success is never announced
before commit, and failed authentication does not mutate the file.

Changing a password slot does not by itself rotate the vault key. Old copies remain
accessible through their old slots. If someone obtained the vault key from an old
copy, changing slots alone cannot protect newer contents encrypted with that same
key. Compromise handling must consider both replacing exposed unlock secrets and
rotating the vault key; that workflow still requires design and review.

## Exact specification and remaining evidence

The [version 1 format](../vault-format-v1.md) specifies offsets,
identifiers, endianness, slot/payload AAD, HKDF context, schema, parser sublimits,
Base32/checksum and candidate component release lines. The owner accepted those
details through the concrete handoff on 2026-10-29. That document is the normative
byte specification; the conceptual
descriptions above do not authorize alternate encodings.

Recommend OS randomness through `getrandom` and owned-buffer cleanup through
`zeroize`, subject to exact release/feature/advisory review before dependency setup.
Fail closed on randomness errors; do not silently use library parameter defaults.
Follow the format's pinning and negative-test plan. No package is installed here.

Keep the approved KDF costs provisional pending measurements during authorized
implementation. Review vectors and exact dependency compatibility before relying
on the implementation. Version 1 supports no automatic migrations or data-key
rotation; unsupported formats are rejected unchanged. ADR-0004 records the proposed
explicit replacement boundary and residual compromise risk. Future migration/key
rotation requires its own reviewed transaction design; changing slots is not rotation.

The paper secret cannot recreate missing or corrupted vault data. No file backup,
cloud recovery, KeePassXC compatibility, custom cryptography or runtime guarantee
is introduced by selecting this model.

## Acceptance gate

The owner accepted the concrete format/component direction, negative-test plan
and residual-risk assessment against FR-01/02/07/09 and NFR-02/05 through the
handoff on 2026-10-29. Exact dependency pinning/advisory review is the first
authorized setup step, before any package is used. Design acceptance does not
claim measured tuning, independent security review or implementation evidence.
