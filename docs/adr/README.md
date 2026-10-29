# Architecture decision records

The [charter](../charter.md) is the confirmed scope input. In Milestone 1 the owner
selected Rust for the core, Tauri for the desktop host, React/TypeScript for the UI,
and macOS/Windows/Linux targets with UI/core/local-vault separation. ADR-0001 records
those confirmed decisions but remains Proposed while support/security details and
platform evidence are open. The other records remain placeholders; application
implementation has not been authorized.

| ADR | Topic | Status | Needed by |
| --- | --- | --- | --- |
| [0001](0001-implementation-platform.md) | Rust/Tauri/React, desktop targets and tooling | Proposed — stack selected; platform details open | Milestone 1 gate |
| [0002](0002-vault-security-and-format.md) | Security, key relationships, format, compatibility | Placeholder — undecided | Milestone 1 gate |
| [0003](0003-committed-writes.md) | Crash-safe commits, durability, conflicting writers | Placeholder — undecided | Milestone 1 gate |
| [0004](0004-offline-recovery.md) | Recovery-secret lifecycle and access changes | Placeholder — undecided | Milestone 1 gate |
| [0005](0005-session-and-drafts.md) | Lock triggers, session lifetime, drafts, save feedback | Placeholder — undecided | Milestone 1 gate |

## Process

Use the [template](template.md) for new topics. Develop placeholders with alternatives,
evidence, consequences, and an explicit proposal. Statuses are `Placeholder`,
`Proposed`, `Accepted`, `Rejected`, or `Superseded`. Owner review in a PR is required
before acceptance; record its PR link/date.

Keep accepted records when decisions change; add a superseding ADR and update this
index. Cross-check requirements and the threat model. Do not mark unresolved
decisions accepted to unblock implementation.
