# macOS storage foundation

Scope: synthetic-only native path validation, bounded encrypted reads and stable
writer exclusion under [ADR-0003](adr/0003-committed-writes.md). This is a prerequisite
for a native writer, not storage-adapter acceptance or a save/durability guarantee.

## Dependency and native API review

The adapter pins `libc = 0.2.191` without default features, already locked and compiled through
the reviewed randomness dependency. The existing registry checksum and source
inventory are reused; no registry package/version or resolved feature is added.
The existing [dependency review](dependency-review.md) and extended
[advisory snapshot](json-dependency-review.md) contain no matching libc advisory.
No new advisory database refresh or independent audit is claimed.

Rust's [File::try_lock](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)
uses nonblocking exclusive `flock` on this Unix host and retains the lock until
the owned file closes. The adapter must never unlink the companion object.
Apple's [local-volume guidance](https://developer.apple.com/library/archive/qa/nw09/_index.html)
documents `statfs`/`MNT_LOCAL`. The installed macOS 26.3.1 SDK's `sys/mount.h`
also defines `MNT_REMOVABLE = 0x00000200`, absent from this libc release's Apple
bindings. Its value will be localized and checked against the SDK in native tests.
`sys/fcntl.h`/`sys/stat.h` supply the descriptor-relative open/stat contracts.

Thin unsafe wrappers must document descriptor ownership and output initialization;
the framework-independent core retains its unsafe-code prohibition. System calls
are not generic UI operations and controlled errors must not contain paths/input.
