# Git and contribution practices

Work in small reviewed milestones. Milestone 0 is documentation only. Read the
[charter](docs/charter.md) and [ADRs](docs/adr/README.md) before implementation or
scope expansion.

## Branches and commits

- Keep `main` as the reviewed baseline. Use separate branches such as
  `docs/milestone-0-foundation`, `feat/<topic>` or `fix/<topic>`.
- Make focused commits, e.g. `docs: establish Milestone 0 project foundation`.
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
and scope. Milestone 0 needs no application tests or build setup. Later code PRs
need checks appropriate to security, recovery and persistence requirements.

## Secrets and test data

Never commit real credentials, master passwords, recovery secrets, personal vaults,
keys, tokens or sensitive logs/dumps. Use synthetic examples in docs/screenshots/
tests. Inspect staged content even with ignore rules; ignored patterns do not remove
tracked secrets. Git is not a personal vault backup. After actual disclosure,
revoke/rotate affected credentials; deleting a line alone does not erase history.

## Repository settings

Recommended repository setup: protect `main` from direct/force pushes and require PRs.
This is documented policy, not a claim of enforced settings. At the inspected
Milestone 0 baseline `main` reported unprotected. No settings were changed here.
Choose required checks when automation exists. No CI/build stack or license choice
is introduced in this milestone.
