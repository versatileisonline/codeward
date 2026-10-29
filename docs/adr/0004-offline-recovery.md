# ADR-0004: Offline recovery-secret lifecycle

Status: Accepted — reusable-secret transcription/lifecycle contract; implementation unverified.
Decision dates: 2026-10-28; concrete handoff approved 2026-10-29.
Acceptance record: [PR #2](https://github.com/versatileisonline/codeward/pull/2). Target: Milestone 1 design gate.

## Confirmed lifecycle

- Rust generates an independent, high-entropy recovery secret for handwritten
  storage. It protects a separate slot containing the same vault data key as the
  normal master-password slot; see [ADR-0002](0002-vault-security-and-format.md).
- Derive its wrapping key with HKDF-SHA256 using RustCrypto `hkdf` and `sha2`,
  with recovery-specific context and vault binding. Protect the slot with
  XChaCha20-Poly1305 from RustCrypto `chacha20poly1305`. These algorithms are selected;
  the approved profile and remaining byte details are recorded below.
- The user transcribes and confirms the secret during enrollment. The core checks
  the transcription before the vault is presented as ready, and the initial data
  and access metadata must commit coherently. Do not persist a plaintext paper
  secret beside the vault or in logs, browser storage, or diagnostics.
- The paper secret is reusable. A normal successful recovery does not consume it
  or automatically replace it. It remains valid for its current recovery slot
  until an explicit replacement under a reviewed policy.
- Recovery requires the intact compatible vault and its usable recovery slot.
  The paper secret alone opens that slot without the master password. It restores
  access and allows committing a replacement master-password slot; it does not
  reveal or reconstruct the forgotten master password.
- Preserve the existing paper secret during routine master-password recovery.
  Report completion only after the access change commits; wrong input or failed
  authentication leaves the file unchanged. Interrupted changes must retain a
  coherent prior or new state under ADR-0003.

## Selected secret representation and derivation

- Generate an independent random 32-byte secret (256 bits). Display it as 52 Base32
  characters grouped for handwriting, plus a transcription checksum. The checksum
  detects errors; it adds neither secret entropy nor proof of authorization.
- Derive a 32-byte wrapping key using HKDF-SHA256's full extract-and-expand with
  an independent random 32-byte salt stored in the recovery slot.
- Feed decoded secret bytes into HKDF, excluding display separators and checksum.
  Bind context to Codeward, format version, vault ID and recovery-wrapping purpose.
- Base32 alphabet, padding/unused-bit rules, group sizes, checksum algorithm/length,
  allowed normalization and exact context bytes still need specification. Do not
  invent those choices from the approved 52-character representation.
- Keep transcription confirmation in Rust. Do not replace generated entropy with a
  user-chosen secret. Master-password input has separate exact UTF-8 preservation
  rules in [ADR-0002](0002-vault-security-and-format.md).

These are confirmed design choices, not implemented or measured behavior.
Routine recovery preserves the same reusable secret, HKDF salt and recovery slot;
the replacement password slot changes the header authenticated by the payload.

## Protection limits

Someone with the paper secret and a usable matching vault can gain access. Keep
the paper separate from the computer/file. This is an alternative access path,
not an extra factor required alongside the master password or an online identity.

A missing/deleted/corrupted vault is a distinct file-recovery problem. If neither
the master password nor valid recovery secret is available, there is no operator
override or account reset. File backups and online recovery remain deferred.

Replacing a password or recovery slot cannot revoke old file copies. If an old
copy exposes the vault key, newer contents under the same data key are also at
risk. Suspected exposure needs a reviewed secret-replacement and data-key-rotation
strategy; no complete compromise-recovery feature is claimed by this decision.

## Transcription and lifecycle contract

The [format proposal](../vault-format-v1.md) specifies RFC 4648 Base32, no padding,
52 secret characters plus an eight-character SHA-256-derived transcription
checksum, four-character groups and strict ASCII input normalization. It also
specifies HKDF/AAD bytes, bounds, candidate randomness helpers and negative tests.
These exact choices were accepted through the handoff on 2026-10-29; the
previously selected entropy, HKDF salt/output and reusable-secret model are unchanged.

Show a generated secret only during enrollment/explicit replacement, with a
separate transcription-confirmation step validated inside Rust. Never implement
later redisplay by storing its plaintext or a copy encrypted under the data key.
Release enrollment secret material on success, cancel or lock. Losing an
unconfirmed enrollment restarts it; no initial vault is ready before the coherent
commit. Apply ADR-0005's proposed inactivity/redaction policy.

For later explicit paper-secret replacement, require a normal unlocked session
and fresh master-password reauthentication. Generate a new independent 32-byte
secret and HKDF salt; transcribe/confirm before committing the new recovery slot
and freshly encrypted payload. Retain the old slot until publication; after an
uncertain outcome reconcile before reporting which code applies to the current
file. The initial implementation slice does not require this later user interface.

Routine forgotten-password recovery remains separate: use the existing paper
secret, keep its salt/slot, commit a replacement master slot and payload under
ADR-0003, and report completion only after commit. Do not silently replace paper.

Recommend deferring an in-place data-key-rotation workflow in version 1. Explicit
slot replacement must explain that old copies remain usable and a disclosed data
key may expose newer contents. No complete compromise-recovery claim is made.
If compromise is suspected, stop relying on that vault/key; creating a new vault
on a trusted device and replacing affected service credentials is separate work,
not an automatic repair or a promise to erase old copies. A future rotation feature
requires its own reviewed format/commit/session design.

## Acceptance gate

This contract was accepted together with ADR-0002/0003 through the handoff on
2026-10-29. Implement later tests for repeat recovery with the same confirmed paper secret, invalid input, transcription
errors, replacement credentials, interrupted access changes, missing/corrupted files,
and old-copy/compromise limits. Map to FR-06/07/10 and T-02/03/04/14. Preserve offline
runtime. This is design documentation, not evidence of working recovery.
