# Codeward project context

## Start here

Read [tldr.md](tldr.md) for file locations. Before changing scope or design, read
`docs/charter.md`, `docs/requirements.md`, `docs/threat-model.md`,
`docs/system-design.md`, `docs/roadmap.md`, and `docs/adr/README.md` as relevant.
Inspect current Git status and preserve unrelated work; do not assume an old
branch or chat summary reflects the repository's current state.

## Current stage

Milestone 0's documentation foundation was merged in PR #1. This iteration adds
navigation and agent context on main at the owner's explicit request. No app code,
build system, or dependencies exist. Milestone 1 has not been authorized; the five
ADR placeholders remain undecided. See [the review record](docs/milestone-0-review.md)
for evidence and limits. A merge does not establish any runtime security guarantee.

## Confirmed scope

- Personal, local-first macOS password manager. Initial runtime is offline, with
  no telemetry, hosted services, or mandatory accounts.
- Normal access uses a master password. A handwritten offline recovery secret
  restores access only with an intact compatible vault; it cannot recreate a
  deleted, missing, or corrupted file. Keep access recovery and file recovery distinct.
- Crash safety applies to committed writes. Preserve a complete old or new state
  across interrupted updates and acknowledge saves only after commit. Newly
  generated passwords and other uncommitted edits have no durability guarantee.
- Rust/C++, UI toolkit, cryptographic algorithms, key derivation, vault format,
  and persistence mechanism remain undecided. Do not choose them in Milestone 0.
- File recovery, automated backups, online recovery, browser integration, sync,
  and other platforms remain deferred. Model malware without claiming protection
  against a compromised OS.

## Working rules

- Use Mermaid for diagrams. Keep documentation concise and `tldr.md` a simple file map.
- Follow [CONTRIBUTING.md](CONTRIBUTING.md). Make focused, modular commits. Use a
  separate branch/PR unless the owner explicitly authorizes a direct main update.
  Never infer merge permission or milestone advancement from a request to prepare work.
- Before implementation, obtain the required milestone/ADR review. Record accepted
  decisions with evidence; never present placeholders as choices already made.
- Verify document links, fences, whitespace, and relevant Mermaid rendering for
  documentation changes. Run appropriate checks for future code; report limitations.
  Never describe documented runtime requirements as implemented or tested behavior.
- Use synthetic examples only; never commit credentials, paper secrets, personal
  vaults, tokens, private keys, or sensitive diagnostics.
- Update this file when milestone status, accepted scope, decisions, validation
  workflow, or repository layout changes. Keep the roadmap, ADR index, review record,
  and file map consistent with those changes; keep history in the detailed docs.
