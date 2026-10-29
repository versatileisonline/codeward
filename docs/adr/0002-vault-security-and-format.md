# ADR-0002: Vault security and format

Status: Proposed — key model and RustCrypto algorithms selected; profile/format open.
Owner decision: 2026-10-28, Codeward design conversation. Review PR: pending.
Target: Milestone 1. No application implementation is authorized.

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
- Authenticate slot/payload data and bind relevant metadata to the intended vault
  and slot roles. Exact protection and serialization details remain open.

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

These selections were confirmed by the owner on 2026-10-28. They do not pin package
versions/features, install dependencies, finalize the format, or establish audit
coverage for Codeward. Review a compatible release set, advisories, audit scope,
and target support before implementation dependency setup.

Argon2id uses the key-derivation API to produce a wrapping key, not a serialized
password-verifier string used as an encryption key. Define explicit memory/time/
parallelism parameters and persist their identifiers/version, salt and values;
do not silently depend on library defaults. Bound untrusted costs before allocation.

HKDF-SHA256 is appropriate only because recovery input is independently generated
with sufficient entropy. It does not strengthen a weak user-chosen password.
Bind the output to a recovery-specific context and vault identity; the exact
salt/context encoding remains part of the format decision. Produce the key length
required by XChaCha20-Poly1305; output length does not create additional entropy.

XChaCha20-Poly1305 requires a 32-byte key and a 24-byte nonce. Protect the payload
with the vault key and each slot with its own wrapping key. Use a fresh securely
generated nonce for each new encryption under a key, including saves/rewraps;
never deliberately reuse a key/nonce pair. Store nonces and tags with ciphertext.
Define authenticated associated data for vault identity, format and slot roles;
exact metadata encoding, mutation rules and resource limits remain open.

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

## Still open before acceptance

- Argon2id cost/salt profile and bounds, recovery entropy/encoding, randomness
  provider, precise nonce/metadata layout, and compatible package versions/features.
  Supporting memory-handling helpers are not automatically selected by this ADR.
- Format/versioning, authenticated metadata, slot binding and exposed metadata.
- Bounded parsing and KDF parameters before accepting untrusted file input; malformed
  or tampered input must fail without destructive repair or credential disclosure.
- Offline guessing through both paths, migration, rollback and key-rotation policy.
- Dependency/security review and negative tests coordinated with ADR-0003/0004.

The paper secret cannot recreate missing or corrupted vault data. No file backup,
cloud recovery, KeePassXC compatibility, custom cryptography or runtime guarantee
is introduced by selecting this model.

## Acceptance gate

Keep this ADR Proposed until the remaining profile/component details, format
contract, negative-test plan and residual-risk assessment against FR-01/02/07/09
and NFR-02/05 receive owner review. Key model, algorithms and library family are
selected; implementation, measured parameter tuning and security claims are not complete.
