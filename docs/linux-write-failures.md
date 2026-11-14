# Linux kernel write-failure evidence

This synthetic experiment exercises the Linux managed transaction path under
[ADR-0003](adr/0003-committed-writes.md) and FR-01/02/09. It lowers
`RLIMIT_FSIZE` only inside dedicated child processes on the disposable ext4
CI mount. No application API or storage policy changes.

## Cases and required outcomes

The raw staging cases pass complete encrypted fixture bytes to actual kernel
writes. With a zero-byte limit the first write must return `EFBIG`; with a
17-byte limit it must first write 17 bytes and then return `EFBIG`. The stage
operation must fail, preserve the private partial file, and leave the vault
absent. After restoring the limit, a separate preparation must succeed without
changing the abandoned file.

The managed cases use the shared experimental writer through the core worker
for initial creation, content updates and master-password replacement. Each
runs with both limits. A staging failure must produce `Failed(Write)` without
advancing the committed revision. An existing vault must remain byte-for-byte
unchanged and authenticate through both access routes. A failed creation must
leave the vault absent.

Retrying the same request ID must preserve the failed status and residue,
both while the limit applies and after it is removed. A new explicit request
may commit after restoring the limit. Creation requires fresh enrollment and
paper confirmation. Successful retries must preserve the expected complete
credential snapshot, authenticate with the expected password and paper code,
and reopen through the managed adapter. Revision advancement remains gated
on the final persistence barrier.

The parent checks that its own soft and hard limits are unchanged. Children
run one exact test with a 60-second deadline; timeout kills and reaps the child.
Only child helpers ignore `SIGXFSZ`, allowing inspection of `EFBIG`, and they
restore the soft limit and signal disposition before successful completion.

## Verification and limits

The tests are selected by the existing `ext4_` CI filter. The child helper is
ignored during ordinary tests and called explicitly by its parent. Native
results require the Ubuntu ext4 job; cross-compilation on macOS checks types
and lints only.

Local checks on 2026-11-14 passed with Rust 1.99.1: Linux-target all-feature,
all-target Clippy with warnings denied; Linux-target default-feature test
compilation with `cargo check --tests`; workspace formatting and whitespace;
and balanced fences/existing local link targets in the four affected Markdown
files. Native execution remains pending until the branch can be published.

This is file-size-limit evidence, not disk-full, quota, media-error, OS-crash
or power-loss evidence. It does not change Linux location admission or accept
a product writer. See [Linux storage boundaries](linux-storage-foundation.md).
Linux location/fault review, Windows native writes and integrated acceptance
remain required before Milestone 2 completion and Milestone 3 entry.

Native references: [Linux resource limits](https://man7.org/linux/man-pages/man2/getrlimit.2.html)
and [write errors and partial progress](https://man7.org/linux/man-pages/man2/write.2.html).
