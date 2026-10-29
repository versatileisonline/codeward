# Architecture decision records

The [charter](../charter.md) is the confirmed scope input. In Milestone 1 the owner
selected Rust for the core, Tauri for the desktop host, React/TypeScript for the UI,
and macOS/Windows/Linux targets with UI/core/local-vault separation. ADR-0001 records
those confirmed decisions and the accepted platform/boundary validation plan. ADR-0002/0004 record the confirmed one-key/two-slot model
and reusable paper secret, plus selected RustCrypto Argon2id, HKDF-SHA256 and
XChaCha20-Poly1305. The owner also approved the binary envelope/encrypted JSON
structure, starting parameter profile, authentication boundaries, file limit and
recovery-code representation. On 2026-10-29 the owner approved the proposed logical
save strategy. Exact encoding, lifecycle, session and platform recommendations are
now concrete in the [Milestone 1 handoff](../milestone-1-handoff.md),
[format proposal](../vault-format-v1.md) and [platform plan](../platform-validation.md).
The owner approved the concrete handoff on 2026-10-29 and explicitly authorized
Milestone 2. ADR-0001–0005 are Accepted as design contracts. Exact package/security
review precedes use; measurements and native runtime evidence remain future work.
Acceptance PR: pending publication. No merge permission is implied.

| ADR | Topic | Status | Needed by |
| --- | --- | --- | --- |
| [0001](0001-implementation-platform.md) | Rust/Tauri/React, desktop targets and tooling | Accepted — platform/boundary design; validation pending | Milestone 1 gate |
| [0002](0002-vault-security-and-format.md) | Security, key relationships, format, compatibility | Accepted — profile and exact format; implementation pending | Milestone 1 gate |
| [0003](0003-committed-writes.md) | Crash-safe commits, durability, conflicting writers | Accepted — logical protocol; adapter validation required | Milestone 1 gate |
| [0004](0004-offline-recovery.md) | Recovery-secret lifecycle and access changes | Accepted — transcription/lifecycle; implementation pending | Milestone 1 gate |
| [0005](0005-session-and-drafts.md) | Lock triggers, session lifetime, drafts, save feedback | Accepted — session/draft contract; implementation pending | Milestone 1 gate |

## Process

Use the [template](template.md) for new topics. Develop placeholders with alternatives,
evidence, consequences, and an explicit proposal. Statuses are `Placeholder`,
`Proposed`, `Accepted`, `Rejected`, or `Superseded`. Owner review in a PR is required
before acceptance; record its PR link/date.

Keep accepted records when decisions change; add a superseding ADR and update this
index. Cross-check requirements and the threat model. Do not mark unresolved
decisions accepted to unblock implementation.
