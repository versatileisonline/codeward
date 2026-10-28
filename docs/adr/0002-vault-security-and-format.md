# ADR-0002: Vault security and format

Status: Placeholder — undecided. Decision: none. Target: Milestone 1.

## Context and questions

Choose reviewed security components, algorithms/parameters, master-password
derivation, randomness, access-key relationships, integrity checks, serialization
and versioning. No cipher, KDF, library, key size, file format, or KeePassXC
compatibility commitment is selected in Milestone 0.

## Evaluation needed

Describe confidentiality/integrity for contents and recovery metadata, exposed
metadata, both offline guessing paths, parsing limits, malformed-input behavior,
upgrades and old-copy/rollback limits. Compare established reviewed constructions;
do not invent cryptography. Coordinate recovery with ADR-0004 and coherent metadata
commits with ADR-0003.

## Acceptance gate

Provide a security construction/component rationale, format/version contract,
negative-test plan and residual-risk assessment against FR-01/02/07/09 and
NFR-02/05. Claims need evidence before acceptance.
