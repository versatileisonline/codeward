# ADR-0003: Committed writes and durability

Status: Placeholder — undecided. Decision: none. Target: Milestone 1.

## Context and questions

Define commit boundary/success acknowledgement, storage operations, temporary
artifact protection, writer exclusion/conflict detection and restart behavior.
No protocol, database, filesystem technique or algorithm is selected now.

## Evaluation needed

- Distinguish app termination from OS crash and sudden power loss. Specify local
  filesystem/hardware assumptions and required durability operations.
- Ensure interrupted creation never presents a partial vault as ready; interrupted
  updates retain a complete old or new commit.
- Keep credential data and master-password/recovery metadata mutually consistent.
- Define capacity, permission, I/O, lock and ambiguous-acknowledgement handling.
- Prevent silent writer conflicts; review replacement with old authentic copies.
- Handle leftover artifacts without deleting the last usable state.

## Acceptance gate

Review a protocol and fault-injection matrix before/during/after every stage,
including acknowledgement and access changes. Map to NFR-03/04/08. Document
power-loss limits before making claims. Unsaved drafts stay excluded. This does
not implement backups or restore a deleted vault.
