# Architecture decision records

The [charter](../charter.md) is the confirmed scope input. All numbered records
below are **placeholders with no selected option**. They track decisions for
Milestone 1; they do not approve technology or authorize implementation now.

| ADR | Topic | Status | Needed by |
| --- | --- | --- | --- |
| [0001](0001-implementation-platform.md) | Language, macOS UI/platform, tooling | Placeholder — undecided | Milestone 1 gate |
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
