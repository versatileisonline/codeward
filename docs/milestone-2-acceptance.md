# Milestone 2 bounded acceptance record

Status: complete and accepted by the owner on 2026-11-15. After reviewing the
bounded evidence and remaining validation limits, the owner confirmed:
“we can mark milestone 2 as complete for now. i accept.” Milestone 3 entry is
authorized. Real-credential use and Milestone 4 UI integration retain their gates.

## Decision scope

The [roadmap](roadmap.md) defines Milestone 2 as the vault core and persistence
gate for FR-01/02/09 and NFR-02/03/04/08 using synthetic data. The accepted
[handoff](milestone-1-handoff.md), [write protocol](adr/0003-committed-writes.md)
and [validation plan](platform-validation.md) remain binding. This packet collects
that gate's implementation and evidence; it does not redefine the requirements.

The accepted scope consists of the Rust vault/session/commit core and default-off
macOS, Linux and Windows experimental managed adapters. The core owns access,
request history, exact-base matching and acknowledgement. Native adapters retain
exclusion and perform staging, atomic publication and required barriers. No
success follows from visibility alone, and unknown outcomes block new mutations
until fresh authenticated reconciliation establishes the actual known state.

## Requirement evidence

| Gate | Evidence to review |
| --- | --- |
| FR-01 — coherent committed creation | Confirmed enrollment and both slots in core vault/transaction tests; [macOS managed workflow](macos-managed-workflow.md), Linux native writer/capacity cases and Windows core lifecycle. No committed revision before final barriers. Windows artifact checks reject raw master/replacement/paper/credential test values; source review follows encrypted `VaultCandidate` bytes through the worker into native stages, with no key/plaintext serialization path. |
| FR-02 — open/unlock/lock/close and failed access | Core scoped/split-phase session tests, host revocation tests and native workflows. Wrong passwords leave bytes unchanged; locked access is refused; reopened actual bytes authenticate through the appropriate route. |
| FR-09 — malformed/tampered input preserved | Independent envelope fixtures, parser/schema/AEAD tamper tests and native malformed-authority/preserve-all cases. No silent empty initialization, repair or stage promotion. |
| NFR-02 — approved stored cryptography | [Exact format](vault-format-v1.md), [crypto review](dependency-review.md), [JSON review](json-dependency-review.md), independent primitive/envelope fixtures, nonce/ownership doctests and native coherent slot/snapshot reopen checks. No profile, key or format changes in closeout. |
| NFR-03 — application-termination safety | Core fault model, macOS native boundary/managed kills, Linux staged/acknowledged kills and Windows staging/publication/core kills. Complete old/new states and both access routes are checked at the specifically tested checkpoints. |
| NFR-04 — error outcomes and explicit retry | Core write/close/flush/publication fault matrix; macOS kernel EFBIG and APFS ENOSPC; Linux kernel EFBIG/partial progress and ext4 ENOSPC; Windows native stage/barrier faults and repeated fresh reconciliation. No false acknowledgement, rollback or failed-ID replay. |
| NFR-08 — cooperating writers and stale state | Core caller/generation/request/base-digest checks; native cross-process companion exclusion, stage ownership, stale-base refusals and identity/admission checks. Arbitrary writers ignoring the companion remain outside coordination guarantees. |

Implementation references: [core evidence](milestone-2-progress.md),
[Linux capacity/admission](linux-capacity-and-admission.md),
[Windows writer/barriers](windows-core-writer.md) and the existing native macOS
evidence linked above. The review record below identifies the exact tested PR
head, base, test-merge commit and final combined hosted run.

## Native evidence boundaries

- macOS: retain the owner's previously accepted bounded Apple silicon/local APFS
  managed policy and application-termination/capacity evidence. No expanded
  hardware or filesystem claim is requested.
- Linux: Ubuntu-hosted disposable ext4 provides native write, ENOSPC and submount
  evidence. The accepted bounded parameterless managed policy preserves validated XDG
  selection, rejects recognized sync components and fails closed on unknown,
  removable, USB, layered or virtual device topology. Direct PCI NVMe/ATA is the
  initial bounded device family. Test-only loop fixtures are explicit synthetic
  infrastructure; successful real-home/fixed-device admission is not inferred.
- Windows: hosted Windows Server 2025/local NTFS provides native storage evidence,
  including verified restricted effective-thread tests. The accepted bounded managed
  policy uses OS-resolved LocalAppData, fixed NTFS, retained identities, ACL checks
  and recognized-cloud refusal. It does not establish an ordinary-account
  Windows 11 desktop installation or every external-device topology.

Owner acceptance retains these bounds. [ADR-0006](adr/0006-managed-working-location.md)
records the macOS policy and the Linux/Windows policies accepted for bounded
synthetic Milestone 2 evidence.
Neither OS app-data selection nor recognized folder names prove that another
program will never synchronize or alter files. OS/device reporting is trusted.

## Excluded from this closeout

Milestone 3 validates the offline paper enrollment/recovery workflow and messages.
Milestone 4 integrates the credential UI, generator, installed desktop packages,
OS events and accessibility across the declared desktop matrix. Milestone 5
retains integrated hardening and distribution/security review. Real-credential
enablement, successful admission across wider hardware, slowest-device Argon2
timing, OS-crash/power-loss validation, backups, missing-file restoration and
universal sync detection are not claimed here.

## Review and acceptance record

- Core and accepted macOS baseline: existing reviewed main history and linked
  evidence; no new core protocol or macOS policy in this closeout.
- Linux closeout: [PR #52](https://github.com/versatileisonline/codeward/pull/52).
  [Ubuntu run 43651994690](https://github.com/versatileisonline/codeward/actions/runs/43651994690/job/132622286986)
  passed 22 ext4 cases and the separate privileged bind-mount test at head
  `d06ec8813d095ca208ed4e5351a8ba13e6e5e7b7`, base
  `faba4afc3019242dbbb311fde69a537a552b9f8b`, test merge
  `ede5491121142374a43e6c1d1217b7d58f76c4f0`. The later evidence/base-update
  head `597288a56272867d3fc7576fd16b88c664ca7918` is included in the combined candidate.
- Windows closeout: [PR #53](https://github.com/versatileisonline/codeward/pull/53).
  The corrected native Windows suite and explicit capability step passed at head
  `dfd1f9a1075786e72c99d63511b5f903af0316f6` in
  [run 43653030714](https://github.com/versatileisonline/codeward/actions/runs/43653030714).
  That run is native regression evidence, not a substitute for final combined-head CI.
- Final combined validation: [run 43653858082](https://github.com/versatileisonline/codeward/actions/runs/43653858082)
  passed formatting, Ubuntu, Windows, macOS and the overall Rust CI gate. Tested
  head `8e55ea8e79dda29237db83a8da5ea585a88bbd8b`, base
  `bd96d5923608e32c477ec4f52b5b3e80638b321c`, test merge
  `4d8f888b3944ee8ee98ec5079e4d6e3f7cd2021f`. Both reviewed branches were merged
  through PR #53 on 2026-11-14 as `a6816fdeb6d274792d840281b00965ffc70c4b5f`.
- Independent implementation reviews: Linux found no material defects; Windows
  identified a final close/reopen identity gap. The native regression failed before
  correction and passed in the corrected native suite; every file/directory close
  failure position is covered, including the added identity guard/final reader.
- Owner acceptance: recorded on 2026-11-15 in the project conversation, following
  review of this packet and its residual limits. Milestone 2 is complete within
  the bounded synthetic scope; Milestone 3 entry is authorized.

The accepted owner decision covers all of the following:

1. Accept the Linux policy's narrow device family and recognized-name exclusions
   for bounded synthetic M2 evidence, while leaving successful real fixed-device
   home admission unverified and real-credential enablement blocked.
2. Accept Windows Server NTFS/restricted-thread results as bounded synthetic
   storage evidence, retaining Windows 11 desktop/ordinary-account validation for
   its later gate and retaining the managed-location limitations above.
3. Retain the approved Argon2 profile without weakening it; the slowest declared
   machine's timing target remains unmeasured and is part of later validation.
4. Accept the seven M2 requirements under the precise tested application-termination
   and storage-failure bounds, and authorize Milestone 3 entry. PR #53's prior
   merge is recorded separately above.
