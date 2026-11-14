# Windows encrypted staging experiment

This extends the [Windows storage foundation](windows-storage-foundation.md)
under [ADR-0003](adr/0003-committed-writes.md) for synthetic NTFS fixtures and
the managed LocalAppData destination. It prepares already-encrypted bytes,
without changing `vault.cw` or acknowledging a save.

## Native boundary

Companion opens/creation, vault reads and stage file operations use
`NtCreateFile` with the retained parent handle as `RootDirectory`. The helper
accepts only the fixed vault/companion names or a stage prefix followed by
exactly 32 lowercase hexadecimal digits. It refuses path separators, streams,
device syntax and other names before native I/O. It requests synchronous I/O,
non-directory files and no reparse traversal. Existing directory preparation
still uses checked normalized paths and retained handles without delete sharing.

Stage creation uses `FILE_CREATE`, never open-or-truncate. A protected DACL is
supplied during creation with grants only to the process user, Local System
and built-in Administrators, matching the existing managed policy. Verification
requires the process user as owner and rejects broader grants. Existing name
collisions are preserved. Names come from 128 bits of system-preferred
`BCryptGenRandom` output with at most eight attempts; errors have no fallback.
The 16 MiB core cap is checked before creating any stage.

Preparation checks complete writes, including short progress and interruptions,
then flushes the file with `FlushFileBuffers` through Rust's `sync_all`.
Before explicitly closing the write handle it opens a read identity guard
that prevents deletion/substitution across close. The write handle initially
denies competing writers. The guard permits write sharing to coexist with it;
after checked close, a final read handle denies writes/deletes. An intervening
writer makes that open fail or is detected by bounded byte-for-byte readback.
The guard pins the file identity throughout this transition.

NTFS can finalize timestamps on write-handle close. Preparation therefore
compares volume/file identity across close and the complete encrypted content,
then selects a stable final observation. The opaque `PreparedStage` owns that
read handle and shares the original companion lease. Dropping the caller's
lease does not release exclusion while a stage exists. Dropping a stage leaves
its bytes intact. No failure performs cleanup, promotion or rollback.

## Dependency and native API review

The locked `windows-sys 0.61.2` and `windows-link 0.2.1` versions are unchanged.
Added binding features are `Wdk_Foundation`, `Wdk_Storage_FileSystem`,
`Win32_System_IO` and `Win32_Security_Cryptography`. No new package, format,
cryptographic algorithm or core dependency is introduced. Stage randomness is
used only for unpredictable file names. The existing package advisory snapshot
is recorded in the [foundation review](windows-storage-foundation.md).

Reviewed primary references:

- [NtCreateFile](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile): root-relative object names, access/sharing, exclusive creation and synchronous options.
- [OBJECT_ATTRIBUTES](https://learn.microsoft.com/en-us/windows/win32/api/ntdef/ns-ntdef-_object_attributes): retained root handle and security descriptor lifetime.
- [BCryptGenRandom](https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/nf-bcrypt-bcryptgenrandom): system provider and checked status.
- [FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers): file flushing and required write access.
- [CloseHandle](https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-closehandle): ownership-consuming checked close.

The FFI uses the locked generated C-layout types, live buffers/descriptors and
one owned handle per successful call. NTSTATUS failures are translated to
controlled library diagnostics; paths and OS text are not exposed. Native API
availability and the tested hosted Windows version do not establish packaged
Windows 11 support.

## Verification and remaining gates

The Windows-target tests cover exclusive collision preservation and rejected
escape/stream names; bounded input and exact-cap staging; bounded collisions
and randomness refusal; zero/partial writes, interrupted/short progress and
flush/close failures; encrypted readback mismatch; private ACLs under managed
directories; preserved old vaults/residue; stage handle restrictions; and
writer exclusion after caller drop, including an independent process.
Real close-boundary probes require deletion/rename refusal under the identity
guard, refusal while a competing writer remains open, and conflict after a
same-size external mutation. A child killed after completed staging must leave
the live vault unchanged, preserve the candidate and release exclusion.
The existing read, junction, ACL and companion tests run against the same
native file boundary. A compile-fail example prevents stage duplication.

Local Rust 1.99.1 Windows-target Clippy checks all targets with warnings denied.
Native execution and full workspace/doctest checks are supplied by the current
PR's three-platform Rust CI. Cross-compilation alone proves no native behavior.

Publication, namespace persistence, core transaction composition, interruption
and native capacity/fault evidence remain pending for Windows. Directory
creation above the file boundary and ancestor/cloud admission still require
review. File flush/close/readback is not a durable save; no product writer or
Milestone 2 acceptance is established here.
