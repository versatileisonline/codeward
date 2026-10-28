# Milestone roadmap

Numbers indicate order, not dates. Milestone 0's foundation was merged in PR #1.
Milestone 1 is in progress. The owner selected a Rust core, installed desktop
delivery, UI/core/local-vault separation, and macOS/Windows/Linux targets, recorded
in [ADR-0001](adr/0001-implementation-platform.md). UI and detailed platform support
remain open; implementation is not yet authorized. Later milestones require owner
review of the previous gate and retain the [charter](charter.md)'s scope.

| Milestone | Deliverable | Completion gate |
| --- | --- | --- |
| 0 — Foundation and system design | Charter, requirements, threat model, Mermaid diagrams, roadmap, ADR placeholders, Git practices | Documentation-only branch/PR reviewed by owner; recovery/file restoration and committed/unsaved distinctions clear. No implementation or technology selection. |
| 1 — Design decisions | Complete UI/platform boundaries around the chosen Rust core; resolve security/format, recovery, commits, session policy and validation | ADR-0001–0005 accepted after review; threat model, declared macOS/Windows/Linux matrix, storage assumptions and fault-test matrix updated. No hidden unresolved security/persistence contracts. |
| 2 — Vault core and persistence | Create/unlock/lock, record persistence, coherent access metadata, safe errors | FR-01/02/09 and NFR-02/03/04/08 demonstrated with synthetic data, malformed input, conflicting writes and interruptions. No premature save acknowledgement. |
| 3 — Offline recovery | Paper enrollment/confirmation, forgotten-password access and replacement master password | FR-06/07/10 verified offline; invalid secret/interrupted access changes preserve valid state; file-loss limits explicit. |
| 4 — Desktop prototype | Credential UI, generation, search, save indicators, deletion and session locking for declared macOS/Windows/Linux targets | FR-03/04/05/08 and NFR-01/05/06/07/09 demonstrated per target, including keyboard/assistive technology, UI/core command checks, lock events, errors and draft-loss boundaries. |
| 5 — Validation and hardening | Integrated checks, dependency/security review, documented limits, reproducible packaging | All initial requirements have evidence or owner-reviewed scope changes; recovery/fault tests pass; offline runtime and secret handling verified. Use synthetic data until prototype review is satisfied. |

Recovery metadata belongs in the Milestone 1 design and Milestone 2 coherent
persistence contract, even though its user workflow arrives in Milestone 3. Do not
design a vault requiring an incompatible recovery retrofit.

## Next Milestone 1 steps

1. Complete ADR-0001: evaluate the recommended Tauri/TypeScript/React UI, specify
   limited core commands and data exposure, and define OS versions/architectures,
   Linux distribution/desktop targets, installer/runtime prerequisites, and validation.
   Keep the Rust core and installed/offline experience as confirmed inputs.
2. Develop ADR-0002 and ADR-0004 together: define vault protection, master-password
   and paper-secret access relationships, format, dependency criteria, recovery
   enrollment, and credential changes. Review choices before acceptance.
3. Develop ADR-0003: define the commit/acknowledgement boundary, coherent metadata
   updates, error handling, writer conflicts, and per-platform interruption tests.
4. Develop ADR-0005: resolve lock events, draft handling, session/secret lifetimes,
   save feedback, and accessible user interactions across supported targets.
5. Review the combined design, threat model, and evidence plan with the owner.
   Advance to Milestone 2 implementation after this gate is accepted.

For each step, discuss the contract, boundary, failure cases, and validation with
the owner. A design recommendation is not an accepted choice; diagrams describe
intended behavior, not a working app. Linux is in scope but requires its own
support matrix and verification; it is not automatically covered by a shared codebase.

## Future options, separately scoped

| Track | Preconditions for a later proposal |
| --- | --- |
| Backups/vault-file restore | Copies, retention, storage locations, loss/corruption cases, restore validation; paper access recovery stays distinct |
| Optional online recovery | New trust/privacy/account model, service failure behavior, preserved offline local use |
| Browser/autofill | Origin matching, permissions, secret transfer and application/browser boundary review |
| Sync/sharing/mobile platforms | Conflict/version model, expanded threat model and explicit product scope |
| Stronger endpoint defenses | Realistic malware protection goals and platform limits |

## Later prototype acceptance scenarios

1. Create offline, confirm paper secret, save credential, lock, reopen, retrieve.
2. Forget master password, recover with intact file/paper secret, replace password,
   reopen with replacement.
3. Terminate saves at fault points: acknowledged updates persist; interrupted
   updates reopen as complete old or new states.
4. Generate without saving, terminate, and confirm there was no durability promise
   or false saved indicator.
5. Try invalid credentials, tampered input, inaccessible storage and missing files;
   receive accurate errors without destroying a usable prior state.

These are future targets, not runtime tests performed in Milestone 0. See the
[current review](milestone-0-review.md) for documentation deliverables.
