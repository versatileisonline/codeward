# Milestone 0 review

Date: 2026-10-28. Status: foundation merged in [PR #1](https://github.com/versatileisonline/codeward/pull/1);
navigation/context follow-ups were pushed to main at the owner's request.
The owner has since started Milestone 1; see the [roadmap](roadmap.md) for current work.

## Repository inspected

Repository: [versatileisonline/codeward](https://github.com/versatileisonline/codeward).
Baseline: `main` at `424c0e0f215c1c270850aea33c813458fb0749d3` (initial commit).
The tree contained only a 35-byte README: no code, design docs, repository agent
instructions, or build/test configuration. No open PR found at inspection.
Main reported unprotected; no settings changed by this work.

## Deliverables

| Foundation | Location |
| --- | --- |
| Simple file map | [tldr.md](../tldr.md) |
| Persistent agent context | [AGENTS.md](../AGENTS.md) |
| Charter/scope | [charter.md](charter.md) |
| Functional/nonfunctional requirements and acceptance evidence | [requirements.md](requirements.md) |
| Threat model/residual limits | [threat-model.md](threat-model.md) |
| Mermaid context and vault lifecycle | [system-design.md](system-design.md) |
| Roadmap/completion gates | [roadmap.md](roadmap.md) |
| ADR index/undecided placeholders | [adr/README.md](adr/README.md) |
| Git/review/secret practices | [CONTRIBUTING.md](../CONTRIBUTING.md) |
| PR checklist | [PR template](../.github/pull_request_template.md) |

## Historical Milestone 0 owner checklist

These items describe the original macOS-only, language-undecided baseline.
Milestone 1 subsequently selected Rust and macOS/Windows/Linux desktop targets;
current scope lives in the charter and roadmap.

- [ ] Personal macOS app; all initial runtime workflows offline.
- [ ] No mandatory accounts, telemetry or hosted dependency.
- [ ] Paper secret restores access only with an intact compatible vault.
- [ ] File recovery/automated backups deferred.
- [ ] Committed writes crash-safe; unsaved generation/edits excluded.
- [ ] Malware, unlocked devices, paper theft and old-copy access limits explicit.
- [ ] Rust/C++, algorithms, format, tooling and persistence remain undecided.
- [ ] Online recovery, browser integration and backups are future options.
- [ ] Documentation accepted before Milestone 1 decisions begin.

## Verification boundary

| Check | Result |
| --- | --- |
| Milestone 0 follow-up link check | At completion: 18 files; all 55 relative links resolved. This is a historical snapshot before Milestone 1 edits. |
| Structure and whitespace | Balanced code fences; no trailing whitespace; diff whitespace check passes |
| Requirements | 10 functional and 8 nonfunctional IDs, unique and sequential |
| Scope and decision review | Confirmed boundaries preserved; all five ADRs undecided |
| Mermaid | Both diagrams render in GitHub Markdown; rendered states and transition labels inspected and corrected |
| Runtime/build tests | Not applicable: no application or build configuration |

Milestone 0 cannot validate encryption, recovery, crash safety, accessibility or
offline runtime. Those remain later acceptance obligations. No application,
dependencies or technology choices are included. PR #1's merge records delivery of
the foundation; unchecked items remain available for explicit owner review and do
not certify future runtime behavior. Starting later work requires its own owner
instruction, as now received for Milestone 1 design work. This record is a historical
snapshot, not an override of later confirmed platform or language choices.
