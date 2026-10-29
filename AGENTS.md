# Codeward project context

## Start here

Read [tldr.md](tldr.md) for file locations. Before changing scope or design, read
`docs/charter.md`, `docs/requirements.md`, `docs/threat-model.md`,
`docs/system-design.md`, `docs/ui-core-contract.md`, `docs/roadmap.md`, and
`docs/adr/README.md` as relevant.
Inspect current Git status and preserve unrelated work; do not assume an old
branch or chat summary reflects the repository's current state.

## Current stage

Milestone 0's foundation was merged in PR #1, followed by navigation/context docs.
Milestone 1 is in progress. The owner selected Rust for the core and macOS, Windows,
and Linux as desktop targets, and approved the UI/core/local-vault separation.
The owner has now selected Tauri with a React/TypeScript UI. ADR-0001 records these
decisions but remains Proposed while support/security details and platform evidence
are open. The other four ADRs
remain placeholders; tooling, algorithms, format, recovery construction, and
persistence mechanisms remain undecided.
No app code, build system, or dependencies exist. See [the review record](docs/milestone-0-review.md)
for the foundation evidence. Repository edits for design decisions are authorized;
application implementation is explicitly prohibited for this update. Do not treat
stack selection as milestone completion or permission to scaffold/install dependencies.

## Confirmed scope

- Personal, local-first password manager targeting macOS, Windows, and Linux.
  Installed desktop app with its own window; users do not start a server or open
  a browser tab. Initial runtime is offline, without telemetry, hosted services,
  or mandatory accounts. Exact platform support matrices are still open.
- Initial audience is a private prototype for at most about 20 friends/family,
  each using a local vault. No public release or shared-vault service is in scope.
- Normal access uses a master password. A handwritten offline recovery secret
  restores access only with an intact compatible vault; it cannot recreate a
  deleted, missing, or corrupted file. Keep access recovery and file recovery distinct.
- Crash safety applies to committed writes. Preserve a complete old or new state
  across interrupted updates and acknowledge saves only after commit. Newly
  generated passwords and other uncommitted edits have no durability guarantee.
- Rust core, Tauri host, and React/TypeScript UI are selected. Cryptographic algorithms,
  key derivation, vault format, and persistence mechanism remain undecided. Record explicit owner
  choices as Milestone 1 progresses. Keep language comparisons and career advice in
  chat; repository ADRs should briefly record decisions, boundaries, and open work.
- Follow `docs/ui-core-contract.md`: React owns presentation, Tauri exposes limited
  commands, and Rust authorizes access and owns commits. Session keys/bulk vault state
  stay in Rust; UI plaintext exists only as required for input/display. Do not claim
  complete erasure or protection against a compromised unlocked UI. UI permissions,
  revisions, session generations and targeted replies must be enforced and tested.
- File recovery, automated backups, online recovery, browser integration, sync,
  and mobile platforms remain deferred. Model malware without claiming protection
  against a compromised OS.

## Working rules

- Use Mermaid for diagrams. Keep documentation concise and `tldr.md` a simple file map.
- Teach through architecture: explain a component's contract, ownership boundary,
  failure cases, and validation before implementation. The owner wants conceptual
  understanding and room for their own design contributions while using AI agents.
- Follow [CONTRIBUTING.md](CONTRIBUTING.md). Make focused, modular commits. Use a
  separate branch/PR unless the owner explicitly authorizes a direct main update.
  Never infer merge permission or milestone advancement from a request to prepare work.
- Before implementation, obtain the required milestone/ADR review. Record accepted
  decisions with evidence; never present placeholders as choices already made.
- Tell the owner when runtime evidence requires implementation: identify the design
  question and smallest synthetic-data experiment before writing code or installing
  dependencies. Wait for explicit authorization. Product implementation begins only
  after the Milestone 1 gate and owner authorization; no feasibility spike is implied.
- Verify document links, fences, whitespace, and relevant Mermaid rendering for
  documentation changes. Run appropriate checks for future code; report limitations.
  Never describe documented runtime requirements as implemented or tested behavior.
- Use synthetic examples only; never commit credentials, paper secrets, personal
  vaults, tokens, private keys, or sensitive diagnostics.
- Update this file when milestone status, accepted scope, decisions, validation
  workflow, or repository layout changes. Keep the roadmap, ADR index, review record,
  and file map consistent with those changes; keep history in the detailed docs.
