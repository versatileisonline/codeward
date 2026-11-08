# macOS storage foundation

Experimental native path resolution, bounded unauthenticated reads, stable
directory-level exclusion, encrypted staging and atomic publication for synthetic
data. Rust 1.99.1 is pinned.

`VaultPath::managed()` prepares the current OS account's fixed
`Library/Application Support/Codeward/Vaults/vault.cw` destination. It has no UI
path/HOME override and creates missing fixed directories with restrictive modes.
It rechecks root/user-owned ancestors without ACL grants and private app directories
without any ACLs. Existing entries are never repaired, replaced or cleaned up.
Preparation does not create a vault or establish a committed/ready state.
The generic `resolve` entry remains synthetic infrastructure, not runtime permission
to open arbitrary working locations.

`VaultPath::resolve` retains directory handles and rejects ambiguous paths/symlink
traversal. `acquire` owns the stable `.codeward.writer-lock` OS lock; all vaults in
one directory share it. `ReadLease::read_current` caps input allocation at 16 MiB,
rejects linked/nonregular or POSIX-exposed files and detects observed changes.
Parent, companion and vault extended ACLs, including deny-only entries, are refused
at admission and revalidation without changing permissions.
Shared verification also repeats the destination's local/writable/nonremovable
APFS check; an earlier permitted mount observation does not authorize later work.
It returns encrypted bytes, never authenticated access or a committed revision.

`ReadLease::prepare_stage` prepares a fresh private sibling with bounded complete
writes, file synchronization and checked close. It refuses parent/stage extended
ACLs, preserves residue and returns an owned marker retaining the same writer state.
Stages can move into a worker independently of the caller's lease; final-owner drop
releases exclusion. The transaction owner retains its own lease through barriers
and reconciliation. See [stage ownership](../../docs/macos-stage-ownership.md).
Preparation does not publish the vault or acknowledge a save.

Consuming `PreparedStage::publish_create` atomically refuses existing destinations;
`publish_replace` requires the complete expected encrypted base to match. Both
revalidate identities/permissions and preserve uncertainty after native errors.
Success means visibility only. Residue is never cleaned up or promoted on restart.

`synchronize_namespace` checks expected complete encrypted bytes or absence and
flushes the checked file descriptor under the publication guard. Managed paths
require fsync and F_FULLFSYNC on the retained directories through the account home.
It preserves errors and observed conflicts for reconciliation.
This candidate sequence does not establish storage acceptance or acknowledge saves.

A [default-off experimental adapter](../../docs/macos-experimental-writer.md)
implements the core storage traits behind `experimental-writer`. Its only public
constructor is `ExperimentalStorage::managed()`. It retains the actual native lease
and stage, uses bounded fallible encrypted copies and invalidates failed observations.
The feature is for synthetic development; real-vault/storage acceptance is pending.
A private `cfg(test)` fault/event wrapper delegates to that same implementation for
creation, changes, recovery, uncertainty and residue tests. Arbitrary test paths and
fault controls are absent from runtime builds.
Its [kernel write-failure cases](../../docs/macos-write-failures.md) now exercise
zero/partial native writes in isolated children on managed synthetic homes, with
no false commit or replay when the process file-size limit is restored.
The [managed workflow](../../docs/macos-managed-workflow.md) tests this shared
writer directly through create, save, process kill, reopen and recovery.
The [capacity runner](../../docs/macos-capacity-and-admission.md) adds opt-in real
disk-full testing on a disposable APFS image. Managed admission refuses a second
device inside the account subtree and recognized cloud roots. Leftover files
are preserved, including when the named vault is missing or invalid.

The separate crate contains narrowly documented native calls through already
locked libc 0.2.191. `codeward-core` remains unsafe-free. No new registry version or
dependency feature is introduced; the writer uses an explicit local opt-in feature.
This crate exports no implementation on non-macOS
targets and does not establish Windows/Linux compatibility.

Default builds omit the writable core adapter. Namespace acceptance,
cloud-location policy and complete storage acceptance remain outstanding. Use
synthetic inputs only. See the [foundation](../../docs/macos-storage-foundation.md)
and [staging](../../docs/macos-staging.md) and
[publication](../../docs/macos-publication.md) and
[namespace evidence and limitations](../../docs/macos-namespace.md).
The [core/native experiment](../../docs/macos-transaction-experiment.md) records
the additional integration evidence and remaining acceptance boundaries.
The [ACL admission evidence](../../docs/macos-acl-admission.md) records the shared
read/exclusion checks and remaining location-policy limits.
The [mount-policy evidence](../../docs/macos-location-admission.md) records repeated
filesystem checks; cloud-location admission remains incomplete.
The [managed-location evidence](../../docs/macos-managed-location.md) and
[working/copy policy](../../docs/adr/0006-managed-working-location.md) distinguish
native preparation from backup, portability workflows and runtime acceptance.

From the workspace root, using the configured toolchain homes:

```sh
cargo check --workspace --no-default-features --locked --offline
cargo test --workspace --all-features --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
```
