# Milestone 3 recovery evidence

Status: in progress, 2026-11-15. The owner authorized M3 after accepting the
[bounded storage foundation](milestone-2-acceptance.md) and selected macOS for
the first polished demo. M3 completion and M4 entry remain owner decisions.
Review: [PR #55](https://github.com/versatileisonline/codeward/pull/55).

## Scope

Verify FR-06/07/10 through the existing Rust headless host and shared experimental
managed APFS writer, using disposable synthetic account homes. Only account-home
selection is substituted; actual permission, identity, staging, publication and
barrier operations use the shared writer. The test-only fault wrapper records
events and injects the specified definite/unknown failures without adding runtime
paths, faults or APIs.

[Offline recovery guidance](offline-recovery.md) specifies enrollment, restricted
recovery, replacement and file-loss messages for the later UI. No credential
screens or packaged offline-runtime claim are introduced by this slice.

## Acceptance scenarios

The native [recovery module](../crates/codeward-storage-macos/src/staging/transactions/tests/recovery.rs)
contains five scenarios:

| Scenario | Evidence sought |
| --- | --- |
| `confirmed_enrollment_and_two_recoveries_preserve_records_and_paper_slot` | Generated-code confirmation gates creation; two recoveries use one paper slot, reject previous passwords and preserve all credential fields; ordinary authentication reopens actual bytes. Encrypted artifacts contain no tested raw secret strings. |
| `invalid_codes_and_password_confirmation_preserve_authoritative_bytes` | Malformed, mistyped and independently generated valid codes fail without writes; invalid password confirmation preserves bytes and restricted access; a fresh valid request can succeed. |
| `missing_and_tampered_vaults_are_never_recreated_by_recovery` | A complete leftover stage cannot replace missing or malformed/tampered authority. Recovery preserves all residue and never silently initializes a vault. |
| `restarted_enrollment_and_lock_revoke_old_codes_and_queued_replacement` | Restart invalidates earlier confirmation, lock ends code redisplay and cancels an unadmitted password replacement; fresh authentication permits explicit recovery. |
| `failed_and_uncertain_recovery_saves_require_explicit_retry_or_fresh_reconciliation` | Stage failure, unknown before/after publication and barrier failure retain accurate outcomes. Failed IDs do not replay; failed proof/barriers retain uncertainty; fresh reconciliation proves unchanged or committed state without unlocking. |

Existing [native transaction termination tests](macos-transaction-experiment.md)
already cover coherent recovery states at prepared, staged, admitted, published
and barrier-completed child-kill checkpoints. Those checkpoint bounds are retained.

## Native offline verification

Use the repository-pinned toolchain and fetch locked dependencies before testing.
From the repository root on macOS:

```sh
cargo test -p codeward-storage-macos --all-features --locked --offline staging::transactions::tests::recovery:: -- --test-threads=1
sandbox-exec -p '(version 1) (allow default) (deny network*)' cargo test -p codeward-storage-macos --all-features --locked --offline staging::transactions::tests::recovery::offline_network_denial_probe -- --ignored --exact
sandbox-exec -p '(version 1) (allow default) (deny network*)' cargo test -p codeward-storage-macos --all-features --locked --offline staging::transactions::tests::recovery:: -- --test-threads=1
```

The ignored probe requires kernel `EPERM` from even a loopback TCP connection;
ordinary connection refusal is not accepted as offline isolation. Running it
without a network-denied sandbox must fail. The same sandbox profile then runs
the five recovery scenarios. The macOS CI step first lists exactly six tests
(five scenarios and the ignored probe), and separately verifies the exact ignored
probe name, so a missing or renamed probe cannot silently execute zero tests.
Default workspace tests leave the probe ignored.

Local execution on 2026-11-15 used Apple silicon, macOS 26.3.1 (25D2128), local
APFS and the pinned Rust/Cargo 1.99.1 toolchain:

- Existing managed headless creation/save/recovery baseline: one test passed.
- New native recovery scenarios: five passed before network isolation.
- Negative control: the explicitly selected offline probe failed without the
  sandbox, receiving connection refusal (`61`) rather than permission denial (`1`).
- Network-denied probe: one passed, observing `EPERM`.
- Network-denied recovery scenarios: five passed; the separate probe stayed ignored
  during that invocation, rather than being counted as an executed scenario.
- The exact macOS CI run block also checks that all six tests are present and
  that the explicitly selected probe exists in the ignored-test inventory.
- Native all-target/all-feature Clippy with warnings denied, default workspace
  check, workspace rustfmt and whitespace checks passed.

Final PR head/base/test-merge and hosted results must be recorded for the review
revision. Local native results do not stand in for final three-platform CI.

## Independent review

Read-only review of the initial implementation found no runtime defects. It
identified a CI verification gap: six listed tests could still include a renamed
probe while the exact invocation silently ran zero tests. A renamed-probe
regression reproduced that false pass; the corrected inventory requires the exact
probe in the ignored-test listing and rejects the same regression. The probe and
five scenarios remain separately executed under the deny-network profile.

One optional assertion enhancement is deferred: explicitly check denied browsing
and absent refresh on the uncertain completion before the test calls lock.
The current test already checks duplicate recovery authorization without replay
and denied browsing after reconciliation; the normal recovery test directly checks
restricted browsing. No runtime correction is deferred.

## Remaining limits and review

This is synthetic headless macOS evidence. Full installed-app offline creation,
credential management, generation, lock and recovery, network-activity inspection,
UI redaction/accessibility and packaging remain desktop-prototype validation.
Linux/Windows recovery-runtime evidence and their desktop integration are not
inferred from this macOS run. The first demo target does not remove those OS
families from the eventual prototype scope.

No real-credential enablement, security audit, universal sync detection, physical
power-loss/OS-crash support, backups or missing-file repair is claimed. Review
the exact tested revision, scenario results and message contract before M3 acceptance.
