# macOS namespace synchronization experiment

Scope: synthetic-only candidate barriers for ADR-0003 step 5 and reconciliation.
This does not implement the writable core adapter or establish a save guarantee.

## Native API review

No new dependency, version, feature, vault byte or cryptographic-profile change.
The existing `libc` bindings supply `fsync`/`F_FULLFSYNC`. A small checked fsync
wrapper makes one call and returns interruption without retry. The existing
checked full-flush wrapper is shared with staging. Installed
macOS 26.3.1 `fsync(2)`/`fcntl(2)` manuals and SDK declarations were inspected on
2026-11-04. Apple's pinned [fsync manual](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/man/man2/fsync.2)
describes flushing modified data/attributes and drive-cache limitations. Its
[fcntl manual](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/man/man2/fcntl.2)
describes `F_FULLFSYNC` as synchronizing before requesting a device-cache flush,
including APFS support. Neither document alone proves namespace persistence on
every APFS/storage combination. The existing advisory snapshot is reused.

An initial local APFS probe accepted both directory `fsync` and `F_FULLFSYNC`.
The candidate sequence reopens/checks the actual file, synchronizes it when present,
then synchronizes the retained parent directory, requiring both operations on each.
Expected absence still requires both parent operations. Errors, including unsupported
calls or interruption, have no downgrade or automatic retry. A visible matching
file alone is never substituted for the barrier sequence.

The later [managed workflow](macos-managed-workflow.md) compares the expected bytes
on the descriptor it flushes and extends managed directory barriers through the
account home. The file/parent experiment below describes the original sequence.

Successful native calls and process-kill evidence are distinct from OS-crash or
power-loss validation. Complete location/privacy policy, authoritative residue
handling, transaction integration and platform acceptance remain outstanding.

## Implemented boundary

`ReadLease::synchronize_namespace` takes the complete expected encrypted bytes or
expected absence. It bounds the supplied length before native work, compares the
actual bounded read, and rechecks the retained ancestors/companion, actual file
metadata, restrictive mode, single link, empty parent/file ACLs and APFS filter.
It shares publication's nonblocking lease-local mutex from validation through the
final observation check, excluding same-lease publication and other barriers.

For a present file, the sequence is file fsync, file F_FULLFSYNC, parent fsync,
parent F_FULLFSYNC. For absence, only the two parent calls apply; an abandoned stage
does not become authority. File/parent handles remain retained through the calls.
Every failed synchronization returns a fixed `SyncNamespace` category and stops
the sequence. Unsupported/interrupted calls are never downgraded or retried.
Observed changes refuse the requested observation; they never authorize restoring
an older file, cleaning residue or replaying a mutation.

The trusted caller must authenticate the expected state and retain unknown-outcome
blocking/reconciliation policy. Successful completion reports the candidate native
sequence only, not `Saved`, `Ready` or `RecoveryComplete`. No core `WriterLease`
implementation, authentication/session authorization, cleanup or rollback is added.
Last-moment nonparticipating writers remain outside advisory exclusion. Read-only
descriptor release is ordinary `File` drop; staging already separately checks the
completed write's close. No general read-descriptor close-error evidence is claimed.

## Focused evidence

On macOS 26.3.1 (25D2128), Apple silicon/local APFS, 2026-11-04:

- Native crate tests: 46 runtime functions (42 assertion functions and four child
  helpers), plus two compile-fail doctests pass. Native all-target Clippy with
  warnings denied, workspace formatting and whitespace checks pass.
- Real create/replacement/reconciliation sequences preserve complete independently
  generated synthetic envelopes and unrelated residue. The replacement remains
  authentic through the paper route. Expected absence runs both parent barriers
  without promoting a complete abandoned stage. Exact 16 MiB input succeeds;
  oversized supplied/actual input and mismatched or missing bases fail before sync.
- Injected failures at every file/parent sync step cover EIO, EINTR, EINVAL,
  ENOTSUP and ENOTTY, including after actual publication. Call counts show no retry
  or weaker fallback; destination bytes and residue remain. These are injected
  failures, not actual media errors, capacity exhaustion or signal interruption.
- Native cases refuse ACLs, exposed modes, hard/symbolic links and observed parent,
  companion or file changes. A paused barrier holds exclusion against another
  barrier and a same-lease publication; contention refuses without changing bytes.
- Sixteen real child kills cover creation, replacement and expected absence before,
  between and after the applicable barrier calls. Reopen sees the expected whole
  file/absence, exclusion is reacquired, the sequence can be repeated, and residue
  remains. These checkpoints do not establish interruption inside a syscall,
  OS-crash behavior, physical power-loss durability or complete commit acceptance.

Next complete authoritative residue and location/privacy gates, integrate the core
transaction protocol, and validate the full acknowledgement/reconciliation path.
Windows/Linux native storage, OS-crash/power-loss and packaged support remain pending.

The subsequent [test-only transaction experiment](macos-transaction-experiment.md)
now composes these operations with the core worker. Its synthetic integration
evidence does not export or accept a runtime writable adapter.
