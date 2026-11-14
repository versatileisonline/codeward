# Linux capacity and managed-location experiment

This proposed bounded Linux policy and synthetic failure harness address
ADR-0003/0006 and FR-01/02/09. They do not accept an adapter, a product runtime,
Milestone 2 completion, or OS-crash/power-loss support. Native results must be
recorded against the reviewed PR revision.

## Bounded managed admission

The existing parameterless managed entry preserves XDG selection: an absolute
`XDG_DATA_HOME` selects that existing base; unset, empty or relative values select
`.local/share` under the effective account home returned by `getpwuid_r`.
No `HOME` lookup, arbitrary UI-selected destination or permission repair is added.
The account home is the anchor for the default path; the explicit XDG base is the
anchor otherwise. Existing ownership, ACL, no-symlink, writable ext4/nodev and
fixed `codeward/vaults/vault.cw` checks remain in force.

Managed admission additionally resolves the live mount ID's major/minor device
through `/sys/dev/block`. A partition inherits the parent disk's attributes.
The disk must resolve below `/sys/devices`, report exactly `removable=0`, have an
empty `slaves` directory and have a device association. This initial policy also
requires PCI ancestry and either an NVMe controller reporting `transport=pcie` or
a numbered ATA ancestor. Virtual-device and USB ancestry is refused. Missing or unreadable evidence refuses admission. This
conservative first policy excludes loop devices, device mapper/LVM/dm-crypt,
software RAID, virtio, network transports and unknown topology. It intentionally does not infer fixed local
storage from ext4 alone. The reported topology and removability remain OS/device
assumptions, not proof against a compromised OS or unusual external hardware.

Recognized path components `Dropbox`, `OneDrive`, `Nextcloud`, `ownCloud`,
`Google Drive`, `pCloudDrive`, `MEGA` and `Insync` refuse managed admission before
creating app directories. Matching is case-sensitive and covers those exact
component names. Arbitrary or renamed third-party sync roots are outside this
bounded location: standard XDG spelling cannot certify another program's behavior.
The fixed-device and current mount-option checks repeat during lease verification,
which runs around reads, stage preparation, publication and persistence barriers.
Descriptor-relative `RESOLVE_NO_XDEV` refuses new managed-subtree mounts, including
bind mounts to the same ext4 device.

The explicit synthetic fixture entry remains available for disposable loop ext4.
Only test builds have the direct managed-directory fixture preparer. The public
managed entry has no environment-variable bypass for device policy. Its loop test
checks refusal without app-directory creation; fixture tests exercise successful
fixed-tree preparation and the shared writer independently.

Owner review must decide whether this narrow device topology and recognized-root
policy is sufficient for the synthetic milestone and whether its exclusions are
acceptable for later real-vault admission. Native successful admission on a
qualifying fixed-device ext4 home/XDG base remains separate from loop CI evidence.
Do not describe the recognized-name list as universal cloud-sync detection.

## Real capacity and residue cases

Dedicated child processes exercise initial creation, content replacement and
master-password replacement through the shared experimental writer. Each uses a
fresh private tree on an exclusively used disposable ext4 image. A synthetic filler
uses actual `fallocate` block reservations until Linux returns `ENOSPC`, first in
1 MiB increments and then the filesystem's reported allocation unit. The helper
refuses filesystems larger than 128 MiB. The existing CI image is 64 MiB.
No resource limit, mocked error or host-filesystem filler stands in for disk full.

The failed stage must report `Failed(Write)` or `Failed(SyncStage)` (ext4 may defer
allocation), preserve its private encrypted residue, leave an existing vault
byte-for-byte unchanged or creation absent, and leave the committed revision
unchanged. Both original access routes must still authenticate the base.
Repeating the failed request ID remains failed before and after space is reclaimed.
Only the owned filler is truncated to reclaim space. A new explicit request must
complete all phases and reopen with the expected complete credentials and both
access routes; creation requires fresh enrollment and paper confirmation.
Abandoned stage bytes remain unchanged throughout.

A separate shared-writer test places complete authenticated and incomplete stages
plus an unrelated entry beside absent, invalid and valid named vaults. Opening
never promotes a leftover candidate. Invalid authority remains untouched. Explicit
enrollment after absence and a successful commit preserve every leftover entry.
No cleanup or backup/restore workflow is added.

The privileged bind-mount test prepares a lease and candidate, then overlays the
managed `codeward` directory with a same-device bind mount. New preparation,
existing reads, publication and namespace synchronization must refuse it, and the
bind source must remain empty. After unmount, the base vault and candidate residue
must still be intact. This is an application-level mount revalidation experiment;
it is not an OS-crash or physical durability test.

## Native invocation and local validation

The existing Ubuntu disposable ext4 job's serial `ext4_ --ignored` selection also
runs the new capacity and preserve-all tests. Keep the test mount exclusive for
the duration: concurrent consumers would invalidate the capacity experiment.
Run the privileged submount check inside the same mount/trap block, after the
normal ext4 selection, with explicit permission to invoke passwordless `sudo`:

```sh
CODEWARD_LINUX_EXT4_FIXTURE="$fixture" \
CODEWARD_LINUX_PRIVILEGED_MOUNT_TEST=1 \
cargo test -p codeward-storage-linux --locked --offline -- \
  tests::admission::privileged_managed_submount_refuses_preparation_reads_and_publication \
  --exact --ignored --test-threads=1
```

This uses the existing `mkfs.ext4`/loop mount setup and requires no new packages or
privileged production code. The test owns and unmounts its bind mount; the CI trap
still owns the outer image. The generic workspace tests do not run privileged or
capacity cases.

On macOS, Linux-target all-feature/all-target Clippy and default-feature test
compilation check types only. The pure admission decision tests can also execute
natively in a temporary Rust test harness importing `src/admission.rs`; red/green
was observed for the recognized-cloud and unknown/removable/device-topology cases.
Those checks do not replace the Ubuntu native write and mount experiments.

Native references: [fallocate and ENOSPC](https://www.man7.org/linux/man-pages/man2/fallocate.2.html),
[sysfs access rules](https://www.kernel.org/doc/html/latest/admin-guide/sysfs-rules.html),
[block-device attributes](https://www.kernel.org/doc/html/latest/admin-guide/abi-stable.html),
[removable-media limits](https://www.kernel.org/doc/html/v6.1/block/capability.html),
and [openat2 mount-crossing restrictions](https://man7.org/linux/man-pages/man2/openat2.2.html).
Quota, native media errors, physical power loss and product acceptance remain
separate evidence/review gates.

Local verification on 2026-11-14 with the pinned Rust 1.99.1 passed:
Linux-target all-target/all-feature Clippy with warnings denied; Linux-target
`cargo check --tests` with default features; the two pure admission tests on macOS;
workspace rustfmt; and `git diff --check`. Initial native Linux execution passed as recorded below. Later revisions require
their own complete current-head/current-base three-platform CI.

## Recorded native execution

[PR #52](https://github.com/versatileisonline/codeward/pull/52)'s
[Ubuntu job](https://github.com/versatileisonline/codeward/actions/runs/43651994690/job/132622286986)
passed all 22 explicitly selected serial ext4 tests, including the real capacity
creation/content/password cases and sole-authority residue test. The separate
privileged same-device bind-mount test also passed. The ordinary workspace run
passed the five Linux tests, including both pure admission decisions, while
leaving native fixtures explicitly ignored. Rust 1.99.1 used head
`d06ec8813d095ca208ed4e5351a8ba13e6e5e7b7` against base
`faba4afc3019242dbbb311fde69a537a552b9f8b`; checkout recorded test merge
`ede5491121142374a43e6c1d1217b7d58f76c4f0`.

An independent read-only review found no Critical, Important or Minor code
findings. The bounded topology/sync-name and absent successful fixed-device home
admission evidence remain explicit review limits. The branch subsequently
incorporates merged PR #51/main and this evidence-only documentation; its final
head must pass its own current-base CI before merge. Native success above is not
misrepresented as a test run for that later documentation revision.
