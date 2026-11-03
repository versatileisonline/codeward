# macOS encrypted staging experiment

Scope: synthetic-only preparation of a complete encrypted candidate beside the
destination, under the existing retained directory lease. This is ADR-0003 step 3,
not a writable core adapter, publication, committed save or support claim.

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
