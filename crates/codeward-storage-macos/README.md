# macOS storage foundation

Experimental native path resolution, bounded unauthenticated reads and stable
directory-level writer exclusion for synthetic data. Rust 1.99.1 is pinned.

`VaultPath::resolve` retains directory handles and rejects ambiguous paths/symlink
traversal. `acquire` owns the stable `.codeward.writer-lock` OS lock; all vaults in
one directory share it. `ReadLease::read_current` caps input allocation at 16 MiB,
rejects linked/nonregular or POSIX-exposed files and detects observed changes.
It returns encrypted bytes, never authenticated access or a committed revision.

The separate crate contains narrowly documented native calls through already
locked libc 0.2.191. `codeward-core` remains unsafe-free. No new registry version or
resolved feature is introduced. This crate exports no implementation on non-macOS
targets and does not establish Windows/Linux compatibility.

It does not implement the writable core adapter. Native staging, publication,
persistence barriers, ACL and cloud-location policy validation remain outstanding.
Use synthetic inputs only. See the [native evidence and limitations](../../docs/macos-storage-foundation.md).

From the workspace root, using the configured toolchain homes:

```sh
cargo test --workspace --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
```
