# Windows persistence API review

Status: research evidence, 2026-11-14. Windows namespace barriers and core writer
composition remain open. This review does not change ADR-0003 or admit real vaults.

## Reviewed baseline

[PR #49](https://github.com/versatileisonline/codeward/pull/49) merged as
`69ede0f679b4b472ef1dd2c19c589f806c60a1e5`. Its
[Rust run](https://github.com/versatileisonline/codeward/actions/runs/43630634692)
passed formatting, all three platform jobs and the combined gate for head
`f7a099ef91831cee62ee434801209d28f0fc6297` against base
`6641f3c085a7d87d7882f9d8a5aa5bdac011b17e`. The Windows checkout logged test merge
`cdc883dfd4d3bf1db025863d6831cf2154c489f5`; this was the PR test merge, not the
later main merge commit.

The [Windows job](https://github.com/versatileisonline/codeward/actions/runs/43630634692/job/132556358893)
passed 35 runtime tests and one compile-fail ownership doctest. Three ignored
runtime entries are child-process helpers invoked by parent tests. The suite
includes four termination checkpoints around create/replace publication. These
results establish the bounded [publication experiment](windows-publication.md),
not namespace durability, Windows 11 packaging or milestone acceptance.

## Existing handle and save boundaries

Staging writes through a private writable file, calls `File::sync_all`, checks
close and reopens for exact encrypted readback. Publication later opens the source
with read and DELETE access, performs the NT rename, checks the visible file and
closes the source. Neither its source nor its readback handle has write access.
Retained directories request traversal, attributes, security read and synchronization.
They deny delete sharing and retain the accepted directory identities.

Consequently, adding `sync_all` to the current post-publication read handle or
copying the Unix directory-sync loop is not an established Windows barrier.
A future barrier must account for required access, sharing transitions, retained
identity, complete expected bytes and the lease lifetime. Any failure following
attempted publication must remain unknown; readback alone cannot acknowledge save.

## Primary API evidence

| Reference | Documented behavior | Remaining question for Codeward |
| --- | --- | --- |
| [FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers) | Requires GENERIC_WRITE on the file handle. A volume-wide flush requires administrative privileges. | How to flush the published candidate while preserving identity and exclusion; this does not supply a parent-directory recipe. |
| [NtFlushBuffersFileEx](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntflushbuffersfileex) | Flags zero flush data/metadata and synchronize storage; missing write/append access can be denied. DATA_ONLY and NO_SYNC omit parts of this work; DATA_SYNC_ONLY excludes directory handles. | Which supported directory access/open sequence, if any, reaches the needed namespace operation from an ordinary user-mode process. |
| [NtCreateFile](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntcreatefile) | Directory opens have distinct access restrictions, including restrictions on file write/append and generic access rights. | How those restrictions compose with the flush access requirement; write-attribute access must not simply be assumed sufficient. |
| [MS-FSA 2.1.5.7](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fsa/0de7dc40-9627-437e-a4df-c4696cdc3d02) and [product note 80](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fsa/4e3695bd-7574-4f24-a223-b4679c065b63) | Describe directory-stream metadata flushing and persistence of directory structure, with an NTFS-specific product qualification. | Filesystem semantics do not by themselves specify an application API, access mask or supported opening sequence. |
| [MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw) | WRITE_THROUGH describes disk completion and explicitly guarantees flushing a copy/delete move. | This does not establish equivalence to the retained-handle rename or persistence of newly created ancestor names. |

Inference from these references: this review has not established a supported
non-administrative directory-flush recipe for the current handle model. It does
not prove that no such recipe exists. A successful syscall alone would establish
observed capability on that system, not the complete public support contract.

## Recommended next evidence

Before exposing a namespace API or implementing the core storage traits, run a
test-only capability probe on disposable local NTFS under an ordinary account.
Compare a writable regular-file flush control with directory flush attempts using
the retained handles and explicitly documented directory access rights. Record
exact masks/options/sharing and numeric native statuses, OS/toolchain/filesystem
versions and token elevation. Keep file identities and companion exclusion pinned;
leave the authoritative vault and transaction residue intact on every refusal.

Keep this probe separate from the passing publication protocol. Do not add broader
access to every retained directory, silently switch to a pathname publication API,
use a copy/delete fallback or request an administrative volume flush. If the probe
cannot establish a supported sequence, report that result and keep the writer gate
closed rather than returning successful namespace synchronization without a barrier.

Two paths remain after that evidence: validate a supported candidate sequence and
compose it with the existing worker, or review a narrower Windows persistence
policy explicitly. The latter would require contract review before implementation.
No choice between those paths is made by this research document.

Application termination remains the initial requirement in
[ADR-0003](adr/0003-committed-writes.md). OS-crash and power-loss claims need separate
evidence; the probe must not make either a hidden prerequisite for the initial
scope or a guarantee inferred from successful process-kill tests. Fresh managed
directory creation, reopen/reconciliation, failure during each barrier and
termination after acknowledgement still need integrated native evidence.

## Capability probe

The test-only [flush probe](../crates/codeward-storage-windows/src/tests/flush_probe.rs)
exercises the existing synthetic create/replace path and compares six handle
cases after each publication: read-only file, writable file control, retained
directory, and directory handles with write-attributes, add-file or
add-subdirectory access. The latter two are directory-specific rights documented
in [File Access Rights Constants](https://learn.microsoft.com/en-us/windows/win32/fileio/file-access-rights-constants);
they share numeric bits with file write/append rights. This matrix tests that
access hypothesis without changing runtime directory rights.

Each opened handle is passed to FlushFileBuffers, NtFlushBuffersFile and
NtFlushBuffersFileEx with flags zero. Numeric statuses, exact requested
access/sharing/options and OS version are printed; filenames, paths and vault
contents are not. Reopened directories must match the retained file identity.
The writable file controls must succeed. Directory/native refusals are reported
as observations, not converted into successful synchronization. Complete visible
bytes, directory entries, abandoned stages and writer exclusion are checked
before/after the matrix; close/reopen checks the last visible candidate.

Hosted runners normally start elevated. Before opening any fixture handle, this
probe creates a same-user restricted token with LUA_TOKEN and
DISABLE_MAX_PRIVILEGE, impersonates it on the test thread, then verifies the
actual thread token has no elevation and no enabled Administrators membership.
Impersonation/query failure stops the test. A scoped guard reverts on return or
unwind and aborts on failed reversion. This is a restricted-thread capability
proxy, not an ordinary-account Windows 11 desktop process or persistence proof.
The token APIs follow [CreateRestrictedToken](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken)
and [ImpersonateLoggedOnUser](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-impersonateloggedonuser).

The Windows CI job explicitly runs the ignored diagnostic once with visible
output, separately from the complete native suite:

```sh
cargo test -p codeward-storage-windows --all-features --locked --offline -- tests::flush_probe::ntfs_flush_capability_probe --ignored --exact --nocapture
```

Cross-target default build and all-target/all-feature Clippy pass locally with
the pinned Rust toolchain. Native results below establish bounded capability;
they leave barrier composition, standard-account desktop validation and
integrated failure/reconciliation evidence open.
The probe records the NTFS filesystem type and Windows OS build; the NTFS on-disk
version is not collected and must not be inferred from the runner label.

### Observed native results

[PR #51](https://github.com/versatileisonline/codeward/pull/51)'s
[Windows job](https://github.com/versatileisonline/codeward/actions/runs/43650206575/job/132616750051)
passed the explicit diagnostic, all 35 existing native runtime tests and the
ownership doctest. The ordinary suite ignored the diagnostic and three child
helpers; the explicit command ran exactly one diagnostic. Head
`057b8fcc3725a1e9591b0b3e29c4340b9661ff35`, base
`faba4afc3019242dbbb311fde69a537a552b9f8b`, test merge
`3140e25b4df1403c6566bfd2ae007e9987aaa513`; Rust 1.99.1, Windows Server 2025
runner reporting build `10.0.26100.33674`, local NTFS. The process token was
elevated; the verified effective thread token had elevation zero and no enabled
Administrators membership. Both publication phases produced identical outcomes:

| Handle | Requested access | FlushFileBuffers result | NtFlush / NtFlushEx-zero status |
| --- | --- | --- | --- |
| File read control | `0x80100000` | Error 5 (access denied) | `0xc0000022` (access denied) |
| File write control | `0xc0100000` | Success | `0x00000000` |
| Retained directory | `0x001200a0` | Error 5 | `0xc0000022` |
| Directory + write attributes | `0x001201a0` | Error 5 | `0xc0000022` |
| Directory + add file | `0x001200a2` | Success | `0x00000000` |
| Directory + add subdirectory | `0x001200a4` | Success | `0x00000000` |

Directory handles shared read/write (`0x3`) and used backup-semantics plus
open-reparse-point flags (`0x02200000`). File controls shared read (`0x1`) and
used synchronous non-directory/reparse-refusing NT create options (`0x00200060`).
Candidate bytes, residue, directory identity and companion exclusion checks
passed before reopening the final visible candidate.

Inference: directory-specific create rights supply a working access combination
on this tested NTFS system. The earlier access ambiguity is now a measured
candidate for a barrier design, rather than a reason to assume directory flushing
is unavailable. Implementing it still requires expected-state binding, guarded
handle transitions, managed ancestor coverage and failure/reconciliation tests.
This experiment does not accept a production writer or prove power-loss durability.

The complete [implementation-head run](https://github.com/versatileisonline/codeward/actions/runs/43650206575)
passed formatting, Ubuntu, Windows, macOS and the combined Rust CI gate.
Subsequent evidence-documentation commits do not change the probe; their own PR
checks must still pass before merge.
