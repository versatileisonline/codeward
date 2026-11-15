# Selected credential details and password reveals

The Rust core and headless host provide selected-record reads for the forthcoming
credential interface under FR-03 and NFR-05/09. They extend the existing scoped
summary listing without returning a complete decrypted snapshot. No Tauri commands
or credential screens are introduced by this slice.

## Entry points and payloads

`Session::get_credential_details` and `Session::reveal_password` take the opened
worker, trusted caller, session generation, expected committed revision and record
ID. `HeadlessVault` exposes the same reads with its retained caller/worker context.
Use the revision from the list or selected view; a generation alone cannot detect
a selection that became stale after a content save in the same session.

| Result | Returned data |
| --- | --- |
| `CredentialDetails` | Generation, revision, ID, title, username, URL, notes, creation/update timestamps. There is no password field. |
| `PasswordReveal` | Generation, revision, ID, the explicitly requested password and monotonic `expires_at`. No other record fields or keys. |

Owned text uses the existing fallible zeroizing copies. Values retain exact UTF-8,
whitespace, embedded NULs and permitted empty fields. Schema limits apply: title
512 bytes, username 1,024, password/URL 4,096 and notes 65,536. The ID is exactly
32 lowercase hexadecimal characters, using the same validator as record creation.
Binary lookup uses the authenticated snapshot's canonical ID order.

Neither result implements Debug, Clone, Copy or automatic serialization. Their
Rust `u64`/`Instant` metadata is not a finished JSON wire schema. A future limited
adapter must deliberately shape the fields allowed for the requesting view.

## Authorization and stale state

Checks require the correct trusted caller/opened worker, ordinary master access,
current generation, idle/nonconflicted worker and an authenticated session snapshot
matching the worker's committed fingerprint. The expected revision must match
before ID lookup. Enrollment, restricted paper recovery and locked access return
no record data, including when an ID is malformed or missing.

| Condition | Controlled result |
| --- | --- |
| Wrong caller/opened worker | `Unauthorized` |
| Locked, restricted, expired or stale generation | `Locked` |
| Preparation/write/refresh in flight, or uncertain transaction | `Busy` |
| Conflicted worker or session snapshot not refreshed after a commit | `ReloadRequired` |
| Selection revision differs from the current authenticated revision | `Vault(RevisionConflict)` |
| Malformed ID after access checks | `InvalidCredentialId` |
| Valid ID absent from the current snapshot | `CredentialNotFound` |

A confirmed deletion or edit invalidates the prior selection revision. Reads wait
for refresh or explicit reload and cannot substitute an old record/password.
Neither method accesses storage, derives keys, writes files or refreshes inactivity.
The core checks expiry again after copying a reply, dropping that reply on revocation.

## Reveal lifetime and delivery

`PASSWORD_REVEAL_DURATION` is 30 seconds. `expires_at` is no later than the request
time plus that duration or the existing inactivity deadline, whichever comes first.
An already-expired request returns no password. The host must schedule redaction;
this reply does not create a background timer or erase memory at its deadline.

Trusted Rust callers retain the original caller/opened-vault context and recheck
current generation/revision and time before delivering queued replies. Matching
numeric metadata alone is not authorization across close/reopen or a different
vault. Do not log, broadcast or persist these payloads or serialize them generically.

The future interface masks passwords by default and clears reveals on deadline,
lock, navigation or focus loss, per [ADR-0005](adr/0005-session-and-drafts.md).
Already-returned copies cannot be revoked or proven erased by the core. Caller/UI
cleanup, display behavior and the limited IPC surface require later integration
tests. No clipboard behavior or extra reauthentication policy is added here.

## Verification

Focused tests live in the [session selected-read module](../crates/codeward-core/src/transaction/session/selected/tests.rs)
and [headless-host module](../crates/codeward-core/src/transaction/host/tests/selected.rs).
They cover exact fields and maximum bounds, invalid/missing IDs, caller/opened-vault
and generation/revision mismatch, restricted modes, pending/unknown/conflicted
writes, refresh/deletion, expiry, deadline capping and unchanged idle/storage state.
Compile-fail examples reject password access through details, diagnostics, cloning
and automatic reveal serialization.

```sh
cargo test -p codeward-core --locked --offline transaction::session::selected::tests -- --test-threads=1
cargo test -p codeward-core --locked --offline transaction::host::tests::selected -- --test-threads=1
cargo test -p codeward-core --doc --locked --offline transaction::session::selected
```

Use the pinned Rust 1.99.1 toolchain and synthetic data. Current-head hosted
three-platform CI and independent review are recorded in the PR; focused local
results do not establish a finished credential UI or real-vault readiness.
