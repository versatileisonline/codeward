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
