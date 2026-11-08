# macOS capacity and managed-location evidence

Milestone 2 evidence under [ADR-0003](adr/0003-committed-writes.md) and the bounded
macOS acceptance in [ADR-0006](adr/0006-managed-working-location.md). The location
policy covers the nonsandboxed prototype's fixed account-managed path on local
writable APFS.

## Location and leftover-file rules

The account home and its fixed `Library/Application Support/Codeward/Vaults`
descendants must share one filesystem device. The adapter checks the account home
before entering the fixed tree and rejects a device boundary as soon as it sees
one. Retained handles, identity, ownership, ACL, mode and mount checks still apply.
The path to the account home may cross onto an approved APFS volume. The managed
subtree may not cross onto a second volume.

An account home inside `Library/CloudStorage` or `Library/Mobile Documents` is
refused before creating managed directories. The check uses filesystem lookup
and directory identity, which also catches spellings that alias those roots.
Third-party tools can still copy reachable directories. Locations configured for
sync are outside the supported storage scope.
[Dropbox File Provider layout](https://help.dropbox.com/installs/fix-domain-conflict-on-mac) ·
[Apple iCloud container layout](https://developer.apple.com/library/archive/documentation/General/Conceptual/iCloudDesignGuide/Chapters/TestingandDebuggingforiCloud.html)

`vault.cw` is the only authoritative name. A missing vault can start explicit
enrollment. Opening a malformed or tampered vault fails and leaves it untouched.
The writer preserves complete and partial stages, links with stage names and
unrelated files. It does not enumerate them as recovery candidates, promote a
newer revision, follow their links or clean them up. The tests confirm that a
valid higher-revision stage never replaces a missing, malformed, tampered or
older valid named vault. Leftover files can consume space across retries.

## Real disk-full test

The opt-in [runner](../crates/codeward-storage-macos/tests/run_capacity_test.py)
creates and mounts a fixed 160 MiB APFS image in an exclusive temporary directory.
It checks host free space before creation, identifies the disposable mount and
detaches the exact image before removing the temporary directory. The Rust test
checks the canonical temp location, different device and size before bounded
filler writes. It also verifies that an actual mount at `Library` beneath a test
account home is rejected before creating children on that second device.

On Apple silicon macOS 26.3.1 (25D2128), the filler write returned kernel `ENOSPC`.
After freeing a small reserved file, the final run had 7,942,144 bytes available
for an 8,408,900-byte encrypted candidate. The writer reported `Failed(Write)` before
publication, retained a private zero-byte stage and left revision 1 byte-for-byte
intact. Freeing the filler did not replay the failed request. A new explicit
request committed revision 2. Both access routes reopened it, the full snapshot
matched, and the failed stage stayed unchanged.

macOS marks the image removable. A test-only, device-scoped exception masks that
one bit in the capacity case. Native I/O, capacity, managed-path checks and
transaction outcomes are real. Regular builds still refuse removable storage.
The kernel errno is captured from the filler write; the writer reports its fixed
error category. Ordinary CI compiles the capacity test and leaves it opt-in.

From the workspace root with the pinned toolchain and locked dependencies:

```sh
python3 crates/codeward-storage-macos/tests/run_capacity_test.py
```

This evidence covers capacity exhaustion during staging. Quota enforcement,
media faults, physical power loss, and storage outside the declared location
remain separate. The owner accepted the bounded policy on 2026-11-07. The writer
stays default-off for synthetic development. Product integration and Windows/Linux
native storage remain Milestone 2 work.
