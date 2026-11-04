# macOS encrypted staging experiment

Scope: synthetic-only preparation of a complete encrypted candidate beside the
destination, under the existing retained directory lease. This is ADR-0003 step 3,
not a writable core adapter, committed save or support claim. A subsequent
[publication experiment](macos-publication.md) consumes prepared stages; preparation
itself never publishes.

## Dependency and native API review

No dependency, registry version, resolved feature, vault byte or cryptographic
profile changes. The existing pinned `libc = 0.2.191` supplies `getentropy`,
`fcntl(F_FULLFSYNC)` and `close`. SDK declarations supply the small Darwin ACL
bindings absent from this libc release. The existing reviewed dependency/advisory
snapshot is reused; no fresh advisory-database review is claimed.

The installed macOS 26.3.1 SDK's `sys/acl.h`, `sys/fcntl.h`, `sys/stat.h` and local
manuals were inspected. Descriptor-relative exclusive creation uses the existing
no-follow/open wrapper. `getentropy` fills 16 random bytes for a bounded sibling
name; failure has no fallback. Names contain no credential metadata.

The ACL policy conservatively refuses **any** extended ACL on the destination
parent or candidate, including deny-only ACLs, without editing permissions. Mode
0600 alone does not exclude inherited ACL grants. `acl_get_fd_np` returns a copied
ACL; `acl_free` releases it exactly once. Apple's pinned
[descriptor implementation](https://github.com/apple-oss-distributions/Libc/blob/71bbe350ab79eef58113991d817ccc6165061a64/posix1e/acl_file.c)
queries `FILESEC_ACL` after `fstatx_np`; the
[property implementation](https://github.com/apple-oss-distributions/Libc/blob/71bbe350ab79eef58113991d817ccc6165061a64/gen/filesec.c)
returns `ENOENT` for an absent ACL property. For a retrieved valid empty ACL,
[`acl_get_entry`](https://github.com/apple-oss-distributions/Libc/blob/71bbe350ab79eef58113991d817ccc6165061a64/posix1e/acl_entry.c)
returns -1/`EINVAL` on the first entry. Other query failures refuse staging.

File synchronization requires both `fsync` and `F_FULLFSYNC` (SDK value 51), with
no downgrade on failure. Explicit close consumes descriptor ownership exactly once;
an error, including `EINTR`, never triggers a close retry or a second `File` drop.
Apple's [flush discussion](https://developer.apple.com/videos/play/wwdc2019/419/)
does not establish directory/name persistence from file flushing. Publication,
namespace barriers and OS-crash/power-loss evidence remain separate work.

## Implemented boundary

`ReadLease::prepare_stage` checks the 16 MiB cap before creating a file. It verifies
the retained parent/companion, refuses parent ACLs, and exclusively creates a mode
0600 sibling with an unpredictable 128-bit suffix. Eight collision attempts are
allowed; existing entries are never followed, truncated or removed. Vault names
in the whole `.codeward.` namespace are reserved case-insensitively. Reserving the
prefix also excludes APFS long-s/Kelvin-sign aliases in the stage/companion suffix.

Before the first payload byte, the created file must be regular, single-linked,
effective-user-owned, mode 0600 and free of extended ACL entries. The write loop
handles short progress and interrupted calls, rejects zero progress, and requires
file synchronization and checked close. File length, permissions and observed
parent/companion/stage substitutions are checked before returning an opaque
`PreparedStage` borrowing the lease. No raw handle or stage name is exposed.

The trusted caller supplies encrypted bytes; staging does not authenticate,
encrypt or establish a valid envelope. Every error leaves the destination
unmodified by this API; created residue remains. Dropping a successful stage also
preserves it. Preparation has no cleanup, automatic promotion, publication,
committed revision, `Saved`, `Ready` or `RecoveryComplete` operation.

## Focused evidence

On macOS 26.3.1 (25D2128), Apple silicon/local APFS, 2026-11-03:

- `cargo test -p codeward-storage-macos --locked --offline`: 25 native test
  functions pass, including ten new staging assertion functions and one new
  subprocess helper; the lease-borrow compile-fail doctest also passes.
- `cargo clippy -p codeward-storage-macos --all-targets --locked --offline -- -D warnings`
  and workspace formatting passed during development.
- Real native cases cover exact-cap input, pre-creation over-cap/randomness
  refusal, exclusive regular/symlink/hardlink/directory/FIFO collisions, bounded
  collision exhaustion, reserved Unicode suffix aliases, restrictive child-process
  umasks, ACL grants/inheritance, deny-only ACL refusal, and observed
  parent/companion/stage changes.
- Injected write interruption/short progress succeeds; zero progress, partial
  writes followed by a capacity error, synchronization failure, and close
  `EIO`/`EINTR` return failure with residue and no destination change. These are
  injected failures, not actual disk-full, media or close-error experiments.
- A real child process is killed after exclusive creation, a partial write, or
  completed file synchronization. The original complete synthetic vault remains
  byte-identical; the empty/partial/complete stage remains unpromoted, and another
  process can reacquire the companion. These are pre-publication process-kill
  experiments, not commit interruption or power-loss evidence.

The first test run correctly failed closed on macOS's absent-ACL `ENOENT` result;
the reviewed descriptor/property semantics above resolved that interpretation.
No file permissions were weakened to make progress. Read/exclusion APIs retain
their documented POSIX-only privacy limits. The subsequent
[publication experiment](macos-publication.md) adds synthetic atomic creation and
replacement. Cloud-location policy, namespace barriers, authoritative residue
handling and core transaction integration remain native work. Full
macOS/Windows/Linux storage acceptance and packaged runtime evidence remain absent.
