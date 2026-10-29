# Platform and implementation validation plan

Status: Accepted initial validation plan, 2026-10-29; no support claims verified.
Scope: ADR-0001/0003/0005 and NFR-03/04/06/08/09.

## Bounded starting matrix

| Target | CPU / filesystem | Desktop runtime and planned package |
| --- | --- | --- |
| macOS 26.3.1 | Apple silicon / local APFS | System WebKit; installed `.app` in a DMG |
| Windows 11 25H2 | x64 / local NTFS | WebView2; per-user installed desktop package |
| Ubuntu 24.04 LTS | x64 / local ext4 | GNOME, test Wayland and X11; WebKitGTK 4.1; `.deb` |

This declares a small test matrix, not support for every newer OS, architecture,
filesystem or Linux desktop. Earlier macOS versions, macOS Intel, Windows ARM and other distributions
remain candidates; they are not silently excluded from the product's longer-term
three-family target. Record actual OS/kernel/runtime builds with each result.
Before prototype distribution, recheck vendor security support and expand or
update the declared matrix through review rather than claiming untested support.

Bundle UI assets locally using Tauri 2, React/TypeScript and Vite. Build tooling
is separate from the installed runtime. Windows installation must include an
offline-capable WebView2 prerequisite path; Linux package dependencies must be
explicit. Do not require users to run a server, browser tab or Node/Rust toolchain.
Follow [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) and verify
installation and launch with network denied. The selected Windows release is
listed in [Microsoft's lifecycle](https://learn.microsoft.com/en-us/lifecycle/products/windows-11-home-and-pro).
Signing/notarization credentials and public distribution are separate tasks; do
not purchase, publish or invent signing credentials as part of core implementation.

## Filesystem adapter acceptance

Support only regular files in owner-controlled local directories on the declared
filesystems. Reject symlink/reparse-point vault paths and hard-linked vault files;
bind operations to validated parent directories/handles to avoid path substitution.
Exclude network shares, removable filesystems and cloud-synchronized locations from
the guarantee. Location checks cannot prove another program will never touch files.

Every adapter must satisfy ADR-0003's same-filesystem staging, publication,
non-overwriting creation, permission, exclusion and error/outcome contracts.
Do not generalize a successful Rust filesystem call into a cross-platform guarantee.

| Adapter | Implementation candidate / required evidence |
| --- | --- |
| Linux | Restrictive exclusive staging creation; sync file; same-directory atomic rename; sync parent directory. Use an atomic no-replace publication for initial creation. Validate exact ext4/mount behavior. |
| macOS | Same logical stages; use the required native flush/cache barrier including evaluation of `F_FULLFSYNC`, and validate directory/name persistence on APFS. Unsupported barriers fail the requested durability contract. |
| Windows | Evaluate handle-based `SetFileInformationByHandle` rename with `FileRenameInfoEx` for replacement, no-replace creation and sharing modes; flush candidate data. Verify NTFS publication/failure outcomes before accepting the adapter. Do not assume file flushing alone persists namespace changes. |

The Windows API above is a candidate to test, not established atomicity or
power-loss evidence. `ReplaceFileW` is not an automatic fallback: its documented
failure cases can change pathnames and its write-through flag is unsupported.
Never fall back to deleting the destination first. Unsupported or unproven
publication behavior blocks that adapter's use with real vaults; shared core work
and synthetic experiments can proceed after the design gate without claiming support.

Primary references: [Linux rename](https://man7.org/linux/man-pages/man2/rename.2.html),
[Linux fsync](https://man7.org/linux/man-pages/man2/fsync.2.html),
[Apple cache/flush behavior](https://developer.apple.com/videos/play/wwdc2019/419/),
[Windows rename information](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info),
[Windows file flushing](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers),
and [ReplaceFileW failure states](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew).

## Evidence stages

1. Pure core: bounded parsing, known crypto vectors, malformed/authentication
   failures, exact serialization, state authorization and fault-injected storage.
2. Native storage: two synthetic complete snapshots; inject interruption before,
   during and after each write/flush/publication/acknowledgement boundary. Inspect
   entire files and both independent unlock paths after reopening.
3. OS-crash behavior: separate VM/storage-crash or equivalent harness preserving
   relevant cache/order behavior. Killing an application or resetting a VM whose
   host already flushed its disk does not prove physical power-loss durability.
4. Desktop: scoped Tauri permissions, stale-reply rejection, OS lock/sleep events,
   keyboard and screen reader behavior, installation and offline runtime per row.

Use synthetic empty, 100-record, 10,000-record and near-cap snapshots plus invalid
oversized inputs. Measure Argon2 cold/warm latency and peak memory on the slowest
declared machine; the 0.5–1 second target remains unmeasured and must not silently
weaken the approved profile. Test capacity/quota, permissions, I/O/barrier failures,
competing instances, aliases, externally replaced files and cleanup interruptions.

Application-termination safety is the initial required guarantee. No OS-crash or
power-loss claim is enabled until the specific adapter and filesystem/storage
combination has evidence and reviewed assumptions. Honest OS/device reporting of
flushes is an assumption; damaged/unreadable media and a compromised OS remain out
of scope. Do not require administrator volume-wide flushing for ordinary saves.
