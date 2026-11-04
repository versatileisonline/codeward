# macOS atomic publication experiment

Scope: synthetic-only ADR-0003 step 4, following [encrypted staging](macos-staging.md).
Publication changes visibility; it does not complete namespace persistence,
acknowledge a save, or establish writable-adapter support.

## Dependency and native API review

No dependency, version, feature, format or cryptographic-profile change. The
already reviewed `libc = 0.2.191` binds `renameatx_np` and `RENAME_EXCL = 4`.
The installed macOS 26.3.1 SDK and `rename(2)` manual were inspected on 2026-11-04.
The call uses the retained parent descriptor for both single-component names.
Replacement uses flags zero; creation uses `RENAME_EXCL` to atomically refuse an
existing destination, including one introduced after an absence check. Apple's
[exclusive-renaming documentation](https://developer.apple.com/documentation/foundation/urlresourcevalues/volumesupportsexclusiverenaming)
describes the filesystem capability. Unsupported operations fail; there is no
delete-first, copy, cross-filesystem fallback or interrupted-call retry.

The prior dependency/advisory snapshot is reused; no new advisory refresh is
claimed. Namespace barriers, cloud-location policy, authoritative residue handling
and core transaction integration remain separate acceptance gates. Process-kill
experiments cannot establish OS-crash or power-loss durability.
