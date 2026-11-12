# Linux synthetic storage foundation

The Linux-only crate exercises native reads and advisory writer exclusion on a
disposable ext4 filesystem. It uses the core's 16 MiB input cap and leaves
envelope parsing, authentication and schema validation to the core. This slice
supports [FR-09 and NFR-08](requirements.md) and the exclusion boundary in
[ADR-0003](adr/0003-committed-writes.md).

## Native boundary

- The synthetic caller supplies an existing private fixture directory. Linux
  opens each component from a directory descriptor with `openat2` and refuses
  symlinks. It checks ancestor ownership and write permissions and requires the
  final directory to be owned by the effective user with mode 0700 and no
  POSIX access or default ACL.
- `statx` gives the retained directory's mount ID. The matching record in
  `/proc/self/mountinfo` must identify writable ext4 with `nodev`. The experiment
  permits crossing into the fixture mount during ancestor traversal and refuses
  mount crossings for the vault and companion entries.
- The fixed companion name is `.codeward.writer-lock`. A new empty mode-0600
  file is created exclusively. Existing entries are checked without truncating
  them. An independently opened companion must take a nonblocking exclusive
  lock. Dropping the lease closes its descriptor and leaves the companion intact.
- The vault must be an effective-user-owned, single-link regular file with mode
  0600 and no POSIX access ACL. Reads recheck directory and companion identity,
  verify the descriptor and named observations, and distinguish confirmed
  absence from a conflicting or unsupported entry. Buffer allocation is
  bounded by the initial checked length, with a one-byte growth probe.

Native references include [openat2](https://man7.org/linux/man-pages/man2/openat2.2.html),
[statx](https://man7.org/linux/man-pages/man2/statx.2.html),
[mountinfo](https://man7.org/linux/man-pages/man5/proc_pid_mountinfo.5.html),
[flock](https://man7.org/linux/man-pages/man2/flock.2.html) and
[POSIX ACLs](https://man7.org/linux/man-pages/man5/acl.5.html).
The crate reuses the locked `libc 0.2.191` pin with default features disabled
and no added features. The RustSec database at
[`fd6f398`](https://github.com/RustSec/advisory-db/commit/fd6f39814c4d3c359fdbe223d0280dec4933f78f)
had no entry for that package at review. No third-party package is added to the
workspace lockfile.

## Verification and limits

Ubuntu Rust CI creates and removes a private loop-mounted ext4 image. It
runs the native tests for absence, bounded reads, permissions, links and special
entries, default ACL refusal, directory/companion/vault substitutions and
cross-process exclusion.
The standard test run also checks the mount record parser and invalid paths.

The app does not use this experiment. Managed XDG location resolution, removable
and sync-location admission, staging, publication, barriers and fault evidence
remain open. The advisory lease coordinates cooperating processes. Observed
metadata comparisons do not give a snapshot guarantee against arbitrary
same-user writers. The experiment does not implement saves or recovery.
