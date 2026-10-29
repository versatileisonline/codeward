# ADR-0004: Offline recovery-secret lifecycle

Status: Proposed — reusable paper secret and recovery slot confirmed; details open.
Owner decision: 2026-10-28, Codeward design conversation. Review PR: pending.
Target: Milestone 1. No application implementation is authorized.

## Confirmed lifecycle

- Rust generates an independent, high-entropy recovery secret for handwritten
  storage. It protects a separate slot containing the same vault data key as the
  normal master-password slot; see [ADR-0002](0002-vault-security-and-format.md).
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

## Remaining decisions

- Randomness source, entropy, human-readable encoding, checksum/error detection,
  normalization, and transcription UX. A checksum can help detect errors; it is
  not proof of authorization. No word list, code length, or encoding is selected.
- Recovery wrapping-key derivation, authenticated slot metadata and format with
  ADR-0002. No KDF, cipher, library, or parameter set is selected here.
- Display/re-display and in-memory lifetime policy; do not add stored recovery
  material merely to enable re-display without review.
- Explicit replacement after paper loss/disclosure, data-key compromise handling,
  confirmation and interrupted changes. Reuse is settled; replacement mechanics
  and any data-key rotation still require design.

## Acceptance gate

Review with ADR-0002/0003 before marking the whole ADR Accepted. Define later tests
for repeat recovery with the same confirmed paper secret, invalid input, transcription
errors, replacement credentials, interrupted access changes, missing/corrupted files,
and old-copy/compromise limits. Map to FR-06/07/10 and T-02/03/04/14. Preserve offline
runtime. This is design documentation, not evidence of working recovery.
