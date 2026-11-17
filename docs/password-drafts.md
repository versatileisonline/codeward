# Session-owned generated-password drafts

The Rust session and headless host retain one volatile generated-password draft
under FR-04/08 and NFR-05/09. The [generator](password-generation.md) remains a
standalone primitive; the draft layer adds authorization, ownership and revocation
for the future credential editor. No Tauri command or credential screen is added.

## Entry points

`Session` methods take the trusted opened worker and caller. `HeadlessVault`
retains that context and supplies thin idle-gated delegates:

| Operation | Input beyond trusted context | Result |
| --- | --- | --- |
| `generate_password_draft` | Current generation, expected committed revision, `PasswordOptions` | Opaque `PasswordDraftRef`; no plaintext and no save |
| `reveal_password_draft` | Borrowed draft reference | Only the generated password, generation, revision and monotonic `expires_at` |
| `validate_password_draft` | Original borrowed reference | Current validity or controlled error; no plaintext |
| `discard_password_draft` | Borrowed draft reference | Drops only the currently referenced draft; no storage change |

The reference exposes generation/revision getters for correlation. It binds the
trusted caller, opened scope, exact authenticated source, generation and unique
draft identity. Neither matching numbers nor a record/draft identifier is authority.
References and reveals have no Debug, Clone, Copy or automatic serialization.
Their Rust types are not a JSON or Tauri wire schema.

## Lifecycle and failure behavior

Only ordinary master access at the current authenticated committed revision may
generate, validate, reveal or discard. Enrollment, restricted recovery, locked or
expired sessions return no password. Wrong caller/opened scope is `Unauthorized`;
stale generations are `Locked`. Pending/uncertain writes are `Busy`; conflicts or
unrefreshed changed snapshots are `ReloadRequired`. An old revision is a controlled
revision conflict. After replacement/discard, the same-revision ref is
`StalePasswordDraft` and cannot remove the new draft.

Successful regeneration replaces the one retained password. Invalid options,
entropy failure and output allocation failure return a controlled
`PasswordGeneration` category and preserve the previous draft while access remains
valid. Expiry during generation wins even if the generation attempt itself failed.
Generation, reveal, validation and discard never refresh inactivity or touch storage.

Manual and security locks, idle expiry, authentication reload and close/drop release
the session-owned draft. A proven changed content commit clears it when snapshot
refresh begins, even if decoding that snapshot subsequently fails. An ordinary
definite save failure preserves it for explicit retry while unlocked. Uncertainty
blocks all draft access until reconciliation: unchanged bytes allow reuse, while a
changed base requires refresh/reload. Discard never establishes rollback or undoes
a possible commit. Generation alone does not create a record or acknowledge a save.

## Reveal and queued delivery

Passwords are returned only by an explicit reveal. Its deadline is no later than
30 seconds after that request or the existing inactivity deadline, whichever is
sooner. Before delivering a queued reply, the trusted host retains and revalidates
the original reference with `validate_password_draft`, checks the original
`expires_at`, and retains the correct caller/opened-vault context. A newer draft
can replace an older one without changing session generation or vault revision;
checking numeric metadata alone would miss that replacement.

The deadline does not start a timer or erase returned copies. The eventual UI must
mask by default and clear previews at deadline, lock, navigation and focus loss.
The core zeroizes its retained password and owned reveal copies on drop; copies
already made by callers, compiler/allocator temporaries, swap and OS dumps cannot
be guaranteed erased. Do not log, broadcast or persist passwords, drafts or
secret-bearing forms. No clipboard command or browser persistence is introduced.

## Scope and verification

This slice owns a generated password, not a complete credential edit form. The
[targeted write layer](scoped-credential-writes.md) now applies a current draft to
an existing credential through explicit background saving. General edit drafts,
ordinary-close Save/Discard/Cancel dialogs, Tauri transport and rendered UI remain later work under
[ADR-0005](adr/0005-session-and-drafts.md) and the
[UI/core contract](ui-core-contract.md). No real-vault admission or milestone
acceptance is inferred.

Eight session scenarios and three headless scenarios cover scoped lifecycle,
replacement/failure preservation, queued-response validation, foreign references,
restricted/expired access, pending/unknown/conflicted saves, reconciliation to
unchanged/changed bytes and confirmed refresh. Six compile-fail doctests protect
draft references and reveals from diagnostics, cloning and serialization.

```sh
cargo test -p codeward-core --lib --locked --offline transaction::session::drafts::tests
cargo test -p codeward-core --lib --locked --offline transaction::host::tests::drafts
cargo test -p codeward-core --doc --locked --offline transaction::session::drafts
```

Use the pinned Rust 1.99.1 toolchain and synthetic data. Final current-head PR/CI
evidence distinguishes portable checks from native platform and installed-UI claims.

## Local evidence and review

On 2026-11-16, Apple silicon macOS 26.3.1 and Rust 1.99.1 passed all eight session
and three host scenarios, six new compile-fail doctests, default workspace check,
all-target/all-feature Clippy with warnings denied, rustfmt, whitespace checks and
184 relative documentation links. These focused checks complement the generator's
[earlier evidence](password-generation.md); final current-head three-platform CI
is recorded in the combined PR rather than inferred from those earlier runs.

Independent read-only review of `7fd7b8e..6f9eeda` found no Critical or Important
issues. One minor cleanup-hardening item is deferred: if `copy_text` allocation
fails during a draft reveal and inactivity expires during that allocation, the
allocation error returns before the final expiry poll. No plaintext is returned;
the retained draft is released by the next scheduled deadline poll or privileged
operation. Successful copying and generation already perform final expiry checks.

Review retained the stated scope: Tauri/UI, clipboard and native event integration;
save-generated-draft/edit/ordinary-close workflows; real-vault admission; and
guaranteed erasure of caller/allocator/OS copies require their separate work. The
additional state-only validation method fulfills queued-delivery obligations
without copying plaintext. Hosted current-head CI remains the full-suite gate.
