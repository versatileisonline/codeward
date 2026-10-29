# Architecture decision records

The [charter](../charter.md) defines scope. ADR-0001–0005 are accepted design
contracts as of 2026-10-29, covering Rust/Tauri/React, desktop targets, the vault
format and cryptography, crash-safe saves, reusable paper recovery, and sessions.
The [handoff](../milestone-1-handoff.md), [format specification](../vault-format-v1.md)
and [platform plan](../platform-validation.md) contain the concrete contracts and
validation sequence. Acceptance record: [PR #2](https://github.com/versatileisonline/codeward/pull/2).

Milestone 2 implementation is authorized. Exact package/security review precedes
use; measurements and native runtime evidence remain future work. Design approval
does not establish implemented behavior or permission to merge.

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
`Proposed`, `Accepted`, `Rejected`, or `Superseded`. Review in a PR is required
before acceptance; record its PR link/date.

Keep accepted records when decisions change; add a superseding ADR and update this
index. Cross-check requirements and the threat model. Do not mark unresolved
decisions accepted to unblock implementation.
