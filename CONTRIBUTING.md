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
and scope. Code PRs require the [Rust CI checks](docs/continuous-integration.md):
locked/offline workspace tests including doctests, rustfmt and all-target Clippy
with warnings denied using the pinned toolchain.

Run focused local checks while developing a change. A successful hosted `Rust CI`
run for the current PR head/base satisfies the full-suite requirement; do not
repeat that entire suite locally without a change, failure or unresolved concern.
Record the run URL and tested revision, and distinguish hosted results from local
commands. Old, skipped, canceled or unavailable runs do not satisfy the requirement.
If CI is unavailable, run the equivalent complete checks locally and report the
limitation; a required GitHub check still must pass before merging.

Security, recovery, native storage, fault injection and platform-specific changes
still need meaningful focused tests/experiments appropriate to their risks. Hosted
CI does not replace independent review or establish desktop/storage support.

## Secrets and test data

Never commit real credentials, master passwords, recovery secrets, personal vaults,
keys, tokens or sensitive logs/dumps. Use synthetic examples in docs/screenshots/
tests. Inspect staged content even with ignore rules; ignored patterns do not remove
tracked secrets. Git is not a personal vault backup. After actual disclosure,
revoke/rotate affected credentials; deleting a line alone does not erase history.

## Repository settings

Recommended repository setup: protect `main` from direct/force pushes and require PRs.
Verify live repository settings when changing review automation. Require the stable
`Rust CI` gate from GitHub Actions once its real PR runs have been verified, with
up-to-date branch checks so new base changes require fresh evidence. Preserve the
other main protections. Settings and passing checks are not a backup or merge approval.
