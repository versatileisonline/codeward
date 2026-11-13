# Windows synthetic storage foundation

The Windows-only adapter reads a fixed vault name and holds an exclusive
companion handle on a disposable NTFS fixture. It implements bounded read
and writer exclusion experiments under [ADR-0003](adr/0003-committed-writes.md).
The application does not call this adapter.

## Native boundary

- Accept a drive-absolute fixture path. Reject relative paths, UNC and device
  namespaces, streams, dot components, ambiguous suffixes and reserved DOS
  names. Require a fixed drive and a writable NTFS volume.
- Open each directory without following its final reparse point and retain
  the handles without delete sharing. Check normalized volume-GUID paths to
  bind each opened directory to the validated parent and avoid re-resolving
  the drive letter. Recheck the retained directories before and after reads.
  The adapter checks the opened companion and vault against the retained
  parent after opening. Confining creation to a directory handle remains
  follow-up work.
- Use an empty, single-link `.codeward.writer-lock` file opened without sharing.
  Keep the handle until lease drop and preserve the companion itself.
- Open only the named `vault.cw` with read sharing, excluding concurrent writes
  and deletes while its handle is open. Reject reparse points, directories,
  and hard links. Check the core's 16 MiB limit before allocating, and verify
  the observed length and timestamps after the bounded read. The core remains
  responsible for parsing and authentication.

The adapter uses the existing locked `windows-sys 0.61.2` with default
features disabled and `Win32_Foundation`, `Win32_Storage_FileSystem` and
`Win32_System_SystemServices` enabled. Its existing `windows-link 0.2.1`
dependency is unchanged. The RustSec database at
[`af7b3ec`](https://github.com/RustSec/advisory-db/commit/af7b3ec33460b8e2de8d422e16d628518891a519)
had no advisory for either package at review.
See [CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew),
[volume information](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumeinformationbyhandlew)
and [normalized handle paths](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfinalpathnamebyhandlew).

## Evidence and limits

The Windows CI suite exercises reads and the input cap, hard-link and junction
refusal, preservation of a nonempty companion, and exclusion across processes.
The original fixture entry inherits the runner's permissions. The managed
experiment below adds per-user resolution and bounded ACL admission. Staging,
publication and persistence remain separate work. Drive classification and the tested fixture do not
establish a production policy for removable or synchronized locations.

## Managed location experiment

`ReadLease::acquire_managed_synthetic()` resolves the current process account's
LocalAppData through `SHGetKnownFolderPath`, then uses
`Codeward\Vaults\vault.cw`. It does not consult the environment or accept an
override. The base must already exist on a supported volume. New app directories
and the companion receive a protected DACL at creation with explicit grants for
the process user, Local System and built-in Administrators. Existing entries
are inspected without changing their security descriptors or contents.

LocalAppData, both app directories, the companion and any vault must be owned
by the process user. Admission rejects null DACLs, unrecognized ACE types and
allow ACEs for other principals. It checks those objects through their handles
before use and rechecks admission around bounded reads. Ancestors above
LocalAppData receive the existing path and reparse checks. Their ACLs, fully
handle-relative creation, and admission for real vaults remain open.

This adds the `windows-sys 0.61.2` features `Win32_Security`,
`Win32_Security_Authorization`, `Win32_System_Com`, `Win32_System_Threading`
and `Win32_UI_Shell` without changing the locked dependencies. See
[known folder resolution](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shgetknownfolderpath),
[handle security information](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-getsecurityinfo)
and [ACE inspection](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-getace).
