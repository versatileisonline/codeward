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
bindings. Its localized value was checked against that SDK during this review.
`sys/fcntl.h`/`sys/stat.h` supply the descriptor-relative open/stat contracts.

Thin unsafe wrappers must document descriptor ownership and output initialization;
the framework-independent core retains its unsafe-code prohibition. System calls
are not generic UI operations and controlled errors contain no paths/input.

## Implemented boundary

`VaultPath::resolve` accepts an absolute path with ordinary components only;
relative, repeated-separator, dot/parent, NUL and reserved companion names fail.
Every directory component is opened with no-follow descriptor-relative operations,
and the full ancestor handle chain is retained. Before lease acquisition and reads,
named directory identities must still match those handles. The parent must be
owned by the effective user and not group/world writable by POSIX mode. Reported
mount flags must describe local writable nonremovable APFS.

`acquire` exclusively creates or opens `.codeward.writer-lock` without truncation,
refuses symlink/hardlink/nonregular, foreign-owner, nonempty or non-0600 companions,
and takes a nonblocking exclusive OS lock. All vault names in one directory share
that companion, conservatively serializing access even across case/Unicode aliases.
The lease exposes no clone, raw descriptor or public unlock. Last-owner drop explicitly
unlocks before close; termination releases the process's descriptors. The adapter
never unlinks the companion. Observed replacement invalidates the old lease; locks
coordinate participants and cannot stop arbitrary external programs bypassing them.

`ReadLease::read_current` opens the named regular, single-linked, effective-user-owned
vault without following links, requiring owner-read and no execute/group/world mode
bits. It rejects lengths above 16 MiB before reserving its input buffer. It reserves
only the checked length and checks every append; a stack probe detects over-cap
growth without enlarging the allocation. Before/after descriptor metadata and the
named file, companion and parent identities must agree. Absence is distinct from
empty/malformed bytes, I/O failure or an observed conflict. Input is never modified.

The returned bytes remain unauthenticated. The existing core still owns structural
parsing, key-slot/payload authentication, strict schema validation and session access.
The later [staging experiment](macos-staging.md) adds synthetic encrypted sibling
preparation with ACL refusal and file synchronization. This crate does **not**
implement the core's writable `Storage`/`WriterLease` traits and cannot acknowledge
a save or complete recovery. A subsequent [publication experiment](macos-publication.md)
adds synthetic atomic creation/replacement without namespace persistence.

## Evidence and limits

Native tests ran on macOS 26.3.1 (25D2128), Apple silicon, local APFS. Fourteen native
test functions include thirteen assertion cases and one subprocess helper. Cases
cover exact/over-cap and changing files, symlink/hardlink/nonregular refusal, POSIX
modes, parent/companion substitution, same-process contention and actual competing
subprocesses. Killing a lock-holding subprocess and normal exit both permit later
acquisition without removing/recreating the companion. One encrypted synthetic
fixture is read unchanged and then authenticated through the existing recovery gate.
Filesystem flag decisions are unit-tested; no network/removable volume was mounted.

A repeated test exposed transient WriterUnavailable after dropping a unique lease
while another test launched processes. Explicit unlock before close fixed the
observed behavior; 100 complete native-suite repetitions subsequently passed.
Transient inherited descriptors before close-on-exec are the inferred cause, not
an independently traced result. This is bounded regression evidence, not proof of
all scheduling behavior. Full workspace checks are recorded in
[Milestone 2 progress](milestone-2-progress.md).

The initial read/exclusion checks were POSIX-only. The subsequent
[ACL admission experiment](macos-acl-admission.md) conservatively refuses all
direct-parent/companion/vault extended ACLs and rechecks them around reads;
staging already refuses candidate ACLs. Mount flags do not
establish cloud-synchronization/location policy or prove every physical device's
characteristics. These remain gates before a writable adapter can support real
vaults. Last-moment changes by nonparticipants remain outside advisory exclusion;
no compromised-OS or authentic rollback protection is claimed.

The later [mount-policy revalidation](macos-location-admission.md) repeats the
destination-parent filesystem query at every shared verification checkpoint.
This does not establish non-cloud admission or an ancestor access policy.

The [staging evidence](macos-staging.md) now covers complete writes, checked close,
file synchronization and pre-publication process kills. The
[publication evidence](macos-publication.md) adds synthetic atomic operations and
surrounding process kills; namespace barriers, full commit interruption,
OS-crash/power-loss and Windows/Linux storage evidence remain absent. No real
credentials or private vaults were used.
Further native work must integrate publication/persistence with the transaction
protocol, preserving uncertain outcomes and artifacts.

A subsequent [namespace experiment](macos-namespace.md) now exercises candidate
file/parent barriers for a checked present or absent state. It does not complete
native transaction, location/privacy or storage acceptance.
