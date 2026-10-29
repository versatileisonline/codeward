# Milestone roadmap

Numbers indicate order, not dates. Milestone 0's foundation was merged in PR #1.
Milestone 1 design review is complete; Milestone 2 is in progress. The
[accepted handoff](milestone-1-handoff.md) records approval on 2026-10-29 and defines
the Rust core, Tauri/React interface, three desktop target families, exact vault
format, crypto profiles, reusable recovery, save contract and session policies.
Exact dependency/security review precedes use; bounded core evidence is recorded
below, while integrated/native runtime evidence remains pending.

| Milestone | Deliverable | Completion gate |
| --- | --- | --- |
| 0 — Foundation and system design | Charter, requirements, threat model, Mermaid diagrams, roadmap, ADR placeholders, Git practices | Documentation-only branch/PR reviewed and approved; recovery/file restoration and committed/unsaved distinctions clear. No implementation or technology selection. |
| 1 — Design decisions | Complete UI/platform boundaries around the chosen Rust core; resolve security/format, recovery, commits, session policy and validation | ADR-0001–0005 accepted after review; threat model, declared macOS/Windows/Linux matrix, storage assumptions and fault-test matrix updated. No hidden unresolved security/persistence contracts. |
| 2 — Vault core and persistence | Create/unlock/lock, record persistence, coherent access metadata, safe errors | FR-01/02/09 and NFR-02/03/04/08 demonstrated with synthetic data, malformed input, conflicting writes and interruptions. No premature save acknowledgement. |
| 3 — Offline recovery | Paper enrollment/confirmation, forgotten-password access and replacement master password | FR-06/07/10 verified offline; invalid secret/interrupted access changes preserve valid state; file-loss limits explicit. |
| 4 — Desktop prototype | Credential UI, generation, search, save indicators, deletion and session locking for declared macOS/Windows/Linux targets | FR-03/04/05/08 and NFR-01/05/06/07/09 demonstrated per target, including keyboard/assistive technology, UI/core command checks, lock events, errors and draft-loss boundaries. |
| 5 — Validation and hardening | Integrated checks, dependency/security review, documented limits, reproducible packaging | All initial requirements have evidence or reviewed scope changes; recovery/fault tests pass; offline runtime and secret handling verified. Use synthetic data until prototype review is satisfied. |

Recovery metadata belongs in the Milestone 1 design and Milestone 2 coherent
persistence contract, even though its user workflow arrives in Milestone 3. Do not
design a vault requiring an incompatible recovery retrofit.

## Milestone 1 acceptance and Milestone 2 entry

The complete handoff was approved on 2026-10-29 and Milestone 2 implementation
was authorized. Acceptance record: [PR #2](https://github.com/versatileisonline/codeward/pull/2).
The design was merged in PR #2; application work and documentation follow-ups
proceed in separate focused reviews.

Begin with a framework-independent Rust core, bounded envelope parsing and exact
AAD bytes using synthetic fixtures. Review/pin dependencies before using them;
add schema, cryptographic and persistence behavior in focused tested slices. Keep
implementation on a separate branch based on the approved documentation while
the Milestone 1 PR remains open. The gate accepts the contracts and evidence plan,
not runtime results. Native adapters remain blocked from real-vault use until
platform-specific evidence exists.

## Current Milestone 2 slice

The Rust library implements envelope parsing/encoding, exact public authentication
metadata, fixed-profile cryptographic primitives and paper-code handling in modular
commits. [Dependency review](dependency-review.md) covers the exact crypto graph.
40 tests and one compile-fail doctest pass on macOS;
[progress evidence](milestone-2-progress.md) records vectors, negative cases,
secret-lifetime limits and an initial Argon2 timing/process-memory observation.

No integrated creation/unlock, credential schema, file I/O, session lifecycle or
commit protocol exists. Next review JSON dependencies and implement strict bounded
credential decoding, then integrate authenticated slots/payload with schema gating.
Fault-injected persistence and separately validated native adapters follow.

## When implementation becomes necessary

Recording the stack, UI/core contract, key model and algorithm choices requires
documentation only. Actual
cross-platform text-input/accessibility checks, packaged offline behavior, IPC
permissions, and secret-handling behavior will eventually need executable evidence.
Before any such feasibility spike, explain the question and smallest synthetic-data
experiment and obtain explicit authorization. Do not scaffold the app,
install dependencies, or create a mock executable under this design-only request.

Product implementation starts in Milestone 2 after the remaining security/recovery,
save, session, and platform decisions pass the Milestone 1 review gate and
implementation is explicitly authorized. A narrowly authorized feasibility spike does not itself
complete that gate or authorize the product build.

The accepted handoff now authorizes implementation. Measurements and native
validation follow the agreed evidence plan rather than being claimed in advance.

## Review inputs and implementation prerequisites

- [Vault format proposal](vault-format-v1.md): exact bytes, AAD/HKDF context,
  JSON schema/limits, Base32/checksum, randomness/error rules and component review.
- [ADR-0003](adr/0003-committed-writes.md): approved logical protocol, unknown
  outcomes, writer exclusion, coherent slots and interruption matrix.
- [ADR-0005](adr/0005-session-and-drafts.md): concrete lock/draft/display defaults
  and coordination with commits.
- [Platform plan](platform-validation.md): proposed bounded matrix, native adapter
  candidates, unsupported storage and required evidence. An unproven adapter does
  not gain real-vault support from acceptance of this evidence plan.
- [ADR-0004](adr/0004-offline-recovery.md): explicit replacement proposal and a
  clear deferral of in-place data-key rotation without a compromise-repair claim.

The design gate approves explicit policies, provisional bounded profiles and the
evidence plan; it must not pretend that measurements already exist. Argon2id timing,
library integration and packaged cross-platform checks require code and will be
performed during authorized implementation, or a separately authorized minimal
feasibility spike if needed to resolve a design question earlier. Coding is now authorized under the accepted handoff; no delivery date or runtime
guarantee is inferred from that authorization.

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

These are future integrated validation targets. Only the bounded core format
and crypto/paper-code tests described above have been implemented; no end-to-end
workflow exists yet.
