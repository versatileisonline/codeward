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

The app does not use this experiment. Removable and sync-location admission,
publication, namespace barriers and write fault evidence remain open.
The advisory lease coordinates cooperating processes. Observed
metadata comparisons do not give a snapshot guarantee against arbitrary
same-user writers. The experiment does not implement saves or recovery.

## Managed location experiment

`ReadLease::acquire_managed_synthetic()` resolves the fixed
`codeward/vaults/vault.cw` destination without accepting a path argument.
It uses an existing absolute `XDG_DATA_HOME` directory. An unset, empty or
relative value selects `.local/share` under the effective account's home,
resolved with `getpwuid_r`. The fallback does not consult `HOME`. Ambiguous
absolute paths are refused. A missing explicit XDG base must be prepared by
the caller's environment. The experiment can create the fixed fallback
components under an existing account home. See the [XDG Base Directory
Specification](https://specifications.freedesktop.org/basedir/latest/) and
[Linux account lookup](https://man7.org/linux/man-pages/man3/getpwuid.3.html).

Preparation walks trusted directories without following symlinks and requires
a writable ext4 nodev mount at the selected base. Descendants stay on that
mount. The base and managed descendants must be effective-user-owned. The app
directories require mode 0700, and the base and managed descendants refuse
POSIX default ACLs. Missing directories use descriptor-relative creation with
restrictive modes. Existing entries are left in place when checks fail.
Before acquiring the companion and around reads, the lease reopens the
directory chain and checks mount and retained directory identities. Vault and
companion validation use the same read path as the fixture entry.

The Ubuntu ext4 tests cover a fresh default tree, idempotent reopening,
concurrent preparation, an existing synthetic vault, collision refusal,
ancestor permission changes and substitutions, and the public XDG entry in a
child process.
Managed resolution is synthetic-only. Removable and sync-location policy,
committed writes and application integration remain open.

## Encrypted staging experiment

The synthetic read lease can prepare an already-encrypted candidate under
[ADR-0003](adr/0003-committed-writes.md). It checks the input cap before
creation and uses Linux `getrandom` for a 128-bit sibling name. Exclusive
creation refuses collisions, with at most eight attempts. The stage must be
a private single-link regular file with no POSIX access ACL. Preparation
checks the complete write, file synchronization and explicit close, then
rechecks the lease and named stage. The marker borrows the lease and retains
an identity-only descriptor.

Preparation leaves the named vault untouched. Partial and completed stages
are preserved for inspection. File synchronization provides no evidence of
namespace persistence. Atomic publication and save acknowledgement are
follow-up work. The existing Ubuntu ext4 suite covers private stage creation,
oversized input, stage substitution and a partial-write failure.
