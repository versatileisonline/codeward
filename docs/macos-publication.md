# macOS atomic publication experiment

Scope: synthetic-only ADR-0003 step 4, following [encrypted staging](macos-staging.md).
Publication changes visibility; it does not complete namespace persistence,
acknowledge a save, or establish writable-adapter support.

## Dependency and native API review

No dependency, version, feature, format or cryptographic-profile change. The
already reviewed `libc = 0.2.191` binds `renameatx_np` and `RENAME_EXCL = 4`.
The installed macOS 26.3.1 SDK and `rename(2)` manual were inspected on 2026-11-04.
The call uses the retained parent descriptor for both single-component names.
Replacement uses flags zero; creation uses `RENAME_EXCL` to atomically refuse an
existing destination, including one introduced after an absence check. Apple's
[exclusive-renaming documentation](https://developer.apple.com/documentation/foundation/urlresourcevalues/volumesupportsexclusiverenaming)
describes the filesystem capability. Unsupported operations fail; there is no
delete-first, copy, cross-filesystem fallback or interrupted-call retry.

The prior dependency/advisory snapshot is reused; no new advisory refresh is
claimed. Namespace barriers, cloud-location policy, authoritative residue handling
and core transaction integration remain separate acceptance gates. Process-kill
experiments cannot establish OS-crash or power-loss durability.

## Implemented boundary

`PreparedStage::publish_create` and `publish_replace` consume a prepared marker;
the same marker cannot be published twice. Preparation retains the completed
stage metadata. Publication reopens it without following links and checks its
identity, timestamps, length, restrictive mode, single link and absence of ACLs.
The retained directory chain/companion and parent ACL policy are rechecked.
An additional nonblocking lease-local mutex covers validation through postchecks;
threads sharing one lease cannot publish concurrently against the same base.
Contention or poisoned ownership refuses before publication.

Replacement requires bounded byte-for-byte agreement with the complete original
encrypted base supplied by the trusted core caller, plus destination metadata and
ACL checks. This comparison does not authenticate a base or authorize a session.
The stable companion excludes cooperating writers; last-moment arbitrary external
writers remain outside that guarantee, as in ADR-0003.

Validation refusal is `NotPublished`. Once the native call is invoked, every
reported error is conservatively `OutcomeUnknown`, including `EINTR`, `EIO` and
`EEXIST`. The call is never retried. Observed changes after successful rename are
also unknown. A successful call verifies that the destination names the same
complete candidate; it reports visibility only. No namespace barrier, committed
revision, `Saved`, `Ready` or `RecoveryComplete` result exists.

The experiment performs no artifact cleanup, promotion on restart or rollback.
The renamed stage becomes the destination; all other residue is preserved.
Callers must block mutations and reconcile after unknown outcomes. The future
transaction adapter must enforce that state; this experiment is not that adapter.
Read/exclusion privacy remains POSIX-only; complete ACL/cloud policy is pending.

## Focused evidence

On macOS 26.3.1 (25D2128), Apple silicon/local APFS, 2026-11-04:

- `cargo test -p codeward-storage-macos --locked --offline`: 36 native test
  functions pass (33 assertion functions and three child helpers), plus two
  compile-fail doctests covering lease lifetime and consumed-stage reuse.
- Native all-target Clippy with warnings denied, workspace formatting and
  whitespace checks pass. No dependency or cryptographic profile changed.
- Native cases cover create/replacement, stale/missing/oversized expected bases,
  changed stage content/identity, links, modes, ACLs, parent/companion changes and
  unrelated residue preservation. A recovery-password replacement authenticates
  through the new master and unchanged paper routes; the old master fails.
  A deterministic same-lease thread race pauses one publisher after preflight;
  its competitor is refused, and a subsequent stale-base replacement conflicts.
- Creation races against a regular file, symlink, directory and APFS case alias
  produce native `EEXIST` without overwriting the introduced object or target.
  Injected native errors/lost replies before or after an actual rename remain
  unknown, with exactly one invocation and no restore/cleanup. These do not prove
  actual media, capacity or kernel interruption error behavior.
- A concurrent reader observes only complete old/new 64 KiB byte patterns during
  32 real replacements. Fourteen real child kills exercise creation/replacement
  before rename, after rename, after visibility return, and eight races against
  rename. Reopening yields absent/complete initial state or complete old/new bytes,
  with recovery authentication where present and companion reacquisition. Timing
  races do not prove the kill occurred inside the syscall. There is no save
  acknowledgement or namespace/power-loss experiment.

Next implement and validate namespace persistence, authoritative residue handling,
location/privacy gates and core transaction integration. Milestone 2 remains in
progress; no usable credential store or native save/durability guarantee exists.

The subsequent [namespace experiment](macos-namespace.md) adds candidate file/parent
barriers with expected-state checks. Native transaction and persistence acceptance
remain outstanding; publication alone still does not acknowledge a save.
