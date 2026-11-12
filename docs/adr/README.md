# Architecture decision records

The [charter](../charter.md) defines scope. ADR-0001–0005 are accepted design
contracts as of 2026-10-29, covering Rust/Tauri/React, desktop targets, the vault
format and cryptography, crash-safe saves, reusable paper recovery, and sessions.
The [handoff](../milestone-1-handoff.md), [format specification](../vault-format-v1.md)
and [platform plan](../platform-validation.md) contain the concrete contracts and
validation sequence. Acceptance record: [PR #2](https://github.com/versatileisonline/codeward/pull/2).

Milestone 2 is in progress. The [progress record](../milestone-2-progress.md)
tracks the shared core and native storage evidence. The accepted macOS policy
covers the managed synthetic workflow, including application termination and
disk-full tests. Linux has
[synthetic ext4 read and exclusion evidence](../linux-storage-foundation.md).
The [macOS desktop pilot](../macos-desktop-pilot.md) has packaged synthetic
lock, sleep and redaction evidence. See the [roadmap](../roadmap.md) for the
remaining acceptance gates.

| ADR | Topic | Status | Needed by |
| --- | --- | --- | --- |
| [0001](0001-implementation-platform.md) | Rust/Tauri/React, desktop targets and tooling | Accepted — platform/boundary design; validation pending | Milestone 1 gate |
| [0002](0002-vault-security-and-format.md) | Security, key relationships, format, compatibility | Accepted — authenticated core and synthetic macOS commits tested | Milestone 1 gate |
| [0003](0003-committed-writes.md) | Crash-safe commits, durability, conflicting writers | Accepted — bounded macOS policy approved; Linux and Windows commits pending | Milestone 1 gate |
| [0004](0004-offline-recovery.md) | Recovery-secret lifecycle and access changes | Accepted — synthetic macOS recovery tested; offline recovery acceptance pending | Milestone 1 gate |
| [0005](0005-session-and-drafts.md) | Lock triggers, session lifetime, drafts, save feedback | Accepted — core revocation and synthetic macOS desktop events tested; drafts and other platform events pending | Milestone 1 gate |
| [0006](0006-managed-working-location.md) | Managed working storage, backup and portability boundaries | Accepted for bounded macOS storage; other platform and future workflow details proposed | Milestone 2 / prototype storage |

## Process

Use the [template](template.md) for new topics. Develop placeholders with alternatives,
evidence, consequences, and an explicit proposal. Statuses are `Placeholder`,
`Proposed`, `Accepted`, `Rejected`, or `Superseded`. Review in a PR is required
before acceptance; record its PR link/date.

Keep accepted records when decisions change; add a superseding ADR and update this
index. Cross-check requirements and the threat model. Do not mark unresolved
decisions accepted to unblock implementation.
