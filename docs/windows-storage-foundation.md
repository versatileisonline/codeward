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
The fixture inherits the runner's permissions. Per-user LocalAppData resolution,
ACL validation, fully handle-relative access, staging, publication and persistence
remain separate work. Drive classification and the tested fixture do not
establish a production policy for removable or synchronized locations.
