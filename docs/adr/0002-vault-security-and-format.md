# ADR-0002: Vault security and format

Status: Proposed — vault-key/slot model confirmed; primitives and format open.
Owner decision: 2026-10-28, Codeward design conversation. Review PR: pending.
Target: Milestone 1. No application implementation is authorized.

## Confirmed key model

- Generate one random data-encryption key per vault. It protects the vault contents.
- Store two separately protected copies of that same key: a master-password slot
  and an independent recovery slot. These are two unlock paths, not two data keys.
- Derive the normal wrapping key from the master password using a password KDF,
  salt and reviewed cost parameters. Derive the recovery wrapping key independently
  from the app-generated paper secret. Either valid path is sufficient; both are
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

- Cipher, password/recovery KDFs, library, key sizes, randomness source, cost settings,
  nonce rules and secret encoding. Prior algorithm recommendations are not selections.
- Format/versioning, authenticated metadata, slot binding and exposed metadata.
- Bounded parsing and KDF parameters before accepting untrusted file input; malformed
  or tampered input must fail without destructive repair or credential disclosure.
- Offline guessing through both paths, migration, rollback and key-rotation policy.
- Dependency/security review and negative tests coordinated with ADR-0003/0004.

The paper secret cannot recreate missing or corrupted vault data. No file backup,
cloud recovery, KeePassXC compatibility, custom cryptography or runtime guarantee
is introduced by selecting this model.

## Acceptance gate

Keep this ADR Proposed until the remaining construction/component choices, format
contract, negative-test plan and residual-risk assessment against FR-01/02/07/09
and NFR-02/05 receive owner review. The key model is confirmed; implementation,
parameter tuning and security claims are not complete.
