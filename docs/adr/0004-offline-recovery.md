# ADR-0004: Offline recovery-secret lifecycle

Status: Placeholder — undecided. Decision: none. Target: Milestone 1.

## Context and questions

Confirmed requirement: a handwritten secret enables offline access to an intact
vault after losing the master password. It is not a vault backup, device passkey,
online identity, or file-reconstruction mechanism. Construction/workflow undecided.

## Evaluation needed

Define generation, guessing resistance, human-readable encoding, transcription
confirmation, display/re-display policy and secret lifetime. Specify its access
relationship and required vault metadata. No plaintext secret persisted beside it.

Decide reuse/rotation/replacement, loss/disclosure handling, confirmation and
interrupted changes. Explain replacement master-password commits and continued
access to old copies via old credentials. Do not promise global copy revocation.

## Acceptance gate

Review with ADR-0002/0003. Specify and later test valid/invalid recovery,
transcription errors, replacement credentials, interrupted metadata changes and
missing/corrupted files. Map to FR-06/07/10 and T-02/03/04. Preserve offline runtime;
optional online recovery requires a future proposal.
