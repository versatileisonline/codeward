# macOS storage foundation

Experimental native path resolution, bounded unauthenticated reads, stable
directory-level exclusion, encrypted staging and atomic publication for synthetic
data. Rust 1.99.1 is pinned.

`VaultPath::resolve` retains directory handles and rejects ambiguous paths/symlink
traversal. `acquire` owns the stable `.codeward.writer-lock` OS lock; all vaults in
one directory share it. `ReadLease::read_current` caps input allocation at 16 MiB,
rejects linked/nonregular or POSIX-exposed files and detects observed changes.
It returns encrypted bytes, never authenticated access or a committed revision.

`ReadLease::prepare_stage` prepares a fresh private sibling with bounded complete
writes, file synchronization and checked close. It refuses parent/stage extended
ACLs, preserves residue and returns a marker borrowing the lease. Preparation
does not publish the vault or acknowledge a save.

Consuming `PreparedStage::publish_create` atomically refuses existing destinations;
`publish_replace` requires the complete expected encrypted base to match. Both
revalidate identities/permissions and preserve uncertainty after native errors.
Success means visibility only. Residue is never cleaned up or promoted on restart.

`synchronize_namespace` checks expected complete encrypted bytes or absence and
exercises strict file/parent fsync and F_FULLFSYNC barriers under the publication
guard. It preserves errors/observed conflicts without fallback, rollback or cleanup.
This candidate sequence does not establish storage acceptance or acknowledge saves.

The separate crate contains narrowly documented native calls through already
locked libc 0.2.191. `codeward-core` remains unsafe-free. No new registry version or
resolved feature is introduced. This crate exports no implementation on non-macOS
targets and does not establish Windows/Linux compatibility.

It does not implement the writable core adapter. Namespace acceptance,
cloud-location policy and complete storage acceptance remain outstanding. Use
synthetic inputs only. See the [foundation](../../docs/macos-storage-foundation.md)
and [staging](../../docs/macos-staging.md) and
[publication](../../docs/macos-publication.md) and
[namespace evidence and limitations](../../docs/macos-namespace.md).

From the workspace root, using the configured toolchain homes:

```sh
cargo test --workspace --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
```
