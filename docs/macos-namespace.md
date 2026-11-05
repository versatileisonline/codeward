# macOS namespace synchronization experiment

Scope: synthetic-only candidate barriers for ADR-0003 step 5 and reconciliation.
This does not implement the writable core adapter or establish a save guarantee.

## Native API review

No new dependency, version, feature, vault byte or cryptographic-profile change.
The existing `File::sync_all`/`libc::F_FULLFSYNC` operations are reused. Installed
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

Successful native calls and process-kill evidence are distinct from OS-crash or
power-loss validation. Complete location/privacy policy, authoritative residue
handling, transaction integration and platform acceptance remain outstanding.
