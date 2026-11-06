# macOS kernel write-failure evidence

Scope: synthetic native failure evidence for ADR-0003 and NFR-04. The existing
test-only transaction bridge now also takes a prepared managed `VaultPath`.
No runtime writer, vault-format change or new dependency is added.

## Native calls and isolation

The installed macOS 26.3.1 SDK and existing locked libc 0.2.191 declarations for
`getrlimit`, `setrlimit`, `signal`, `rlimit`, `RLIMIT_FSIZE` and `SIGXFSZ` were
checked before use. The existing dependency/advisory snapshot is reused; no
fresh advisory scan is claimed. These calls exist only in native test code.

Apple's [resource-limit manual](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/setrlimit.2.html)
describes a process file-size limit and its associated signal. The
[write manual](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/write.2.html)
identifies EFBIG for an exceeded limit. This harness uses real kernel refusal;
it does not inject a short-write count or replace the write error.

Each case launches the exact child test with one test thread and an exclusively
created synthetic home. Only that child ignores SIGXFSZ and lowers its soft file
size limit to zero or 17 bytes. The hard limit remains unchanged. Setup, read-back,
explicit restoration of the original soft limit and restoration of the previous
signal disposition are checked. A failed assertion terminates only the child.
The parent enforces a 60-second child deadline and checks its own limits afterward.
No real account directory, global shell limit or filesystem capacity is changed.

## Observed behavior

On macOS 26.3.1 (25D2128), Apple silicon/local APFS, 2026-11-05:

- Direct native staging at limit zero returns EFBIG without progress. At limit 17,
  the real first write returns 17, and the next returns EFBIG. An observer delegates
  unchanged buffers to the actual writer and checked close. Both cases return
  `NativeError::Write`, perform one successful close and no synchronization, and
  preserve the exact private empty/prefix stage. No destination is published.
- Six managed transaction cases cover creation, content replacement and recovery
  password replacement at both limits. The core reports `Failed(Write)` with no
  publication/barrier event or revision advancement. Creation stays absent; failed
  updates retain the byte-identical independently generated base, readable through
  its original master and paper routes. The proposed replacement master fails.
- Known failed request IDs return the recorded failure without further storage work
  both before and after limit restoration. Only a new explicit request commits
  after restoration. Creation additionally requires a fresh enrollment and paper
  transcription, since preparation consumed its previous enrollment.
- Successful retries run the existing native publication/barrier sequence. Both
  access routes validate the selected complete file; recovery rejects the replaced
  master. Reopening reacquires exclusion through the managed entry. The earlier
  empty/partial stage remains byte-identical alongside the destination.

Three new native runtime functions (two assertion cases and a child helper) cover
eight child executions. All 11 transaction-test functions passed locally in 57.86s,
including the previous lifecycle, uncertainty, residue and process-kill cases.
Native all-target Clippy with warnings denied, workspace formatting and whitespace
checks passed. Current-head/base hosted Rust CI provides the full workspace/doctest
and three-platform result; local focused checks do not substitute for that gate.

## Limits and remaining work

This is EFBIG/file-size-limit evidence, not actual ENOSPC, quota exhaustion,
media errors, failed close, in-syscall termination or physical power-loss evidence.
The committed retry results remain outcomes of the private test adapter; they do
not accept a shipping storage contract. Runtime storage, complete location and
namespace acceptance, application sessions, Windows/Linux adapters and packaged
support remain pending. Backups and restoration remain deferred under
[ADR-0006](adr/0006-managed-working-location.md).
