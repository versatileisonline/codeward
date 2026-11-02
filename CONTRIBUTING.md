# Git and contribution practices

Work in small reviewed milestones. Milestone 1 design is accepted and Milestone 2
core implementation is in progress. Read the
[charter](docs/charter.md) and [ADRs](docs/adr/README.md) before implementation or
scope expansion.

## Branches and commits

- Keep `main` as the reviewed baseline. Use separate branches such as
  `docs/<topic>`, `feat/<topic>` or `fix/<topic>`.
- Make small focused commits, including for smaller tasks; keep dependency review,
  models and decoding changes independently reviewable.
- Inspect full diffs/staged paths; stage explicit paths and preserve unrelated work.
- No force-pushing shared branches or rewriting reviewed history. Direct pushes to
  `main` require explicit authorization for the update. Merge only with explicit
  approval; a PR is not merge approval.

## Pull requests and decisions

Describe the concrete change, affected requirement/ADR IDs, scope, verification and
remaining limits. Use the [PR template](.github/pull_request_template.md). Review is
required before merge/milestone advancement. Record accepting review before
changing an ADR from undecided to accepted.

For docs, verify relative links, Markdown tables/fences, Mermaid rendering, terms
and scope. Code PRs require locked/offline workspace tests, rustfmt check and
Clippy with warnings denied using the pinned toolchain; add checks appropriate
to security, recovery and persistence requirements.

## Secrets and test data

Never commit real credentials, master passwords, recovery secrets, personal vaults,
keys, tokens or sensitive logs/dumps. Use synthetic examples in docs/screenshots/
tests. Inspect staged content even with ignore rules; ignored patterns do not remove
tracked secrets. Git is not a personal vault backup. After actual disclosure,
revoke/rotate affected credentials; deleting a line alone does not erase history.

## Repository settings

Recommended repository setup: protect `main` from direct/force pushes and require PRs.
This is documented policy, not a claim of currently enforced settings. Verify
repository settings when changing review automation. Choose required checks when
automation exists; local passing checks do not establish hosted CI coverage.
