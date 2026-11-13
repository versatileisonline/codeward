# Milestone 2 implementation evidence

Status: in progress, 2026-11-09. Authenticated vault operations now compose with
session-bound mutation admission and a fault-injected committed-write model.
The bounded macOS storage policy is accepted for synthetic use. Product storage
admission and complete application workflows remain pending.
Design baseline: [accepted handoff](milestone-1-handoff.md), recorded in
[PR #2](https://github.com/versatileisonline/codeward/pull/2), which is now merged.
The parser/metadata step is tracked in [PR #4](https://github.com/versatileisonline/codeward/pull/4),
the encoder in [PR #5](https://github.com/versatileisonline/codeward/pull/5), and the
cryptographic foundation in [PR #6](https://github.com/versatileisonline/codeward/pull/6).
Separate subsequent commits add encoding, reviewed crypto dependencies,
cryptographic primitives and paper-secret transcription checks. All three
implementation PRs are merged into `main`. The credential-schema slice was merged
in [PR #8](https://github.com/versatileisonline/codeward/pull/8), following guidance
cleanup in [PR #7](https://github.com/versatileisonline/codeward/pull/7). The current
authenticated-vault slice was merged in [PR #9](https://github.com/versatileisonline/codeward/pull/9).
The transaction/session-admission slice was merged in
[PR #10](https://github.com/versatileisonline/codeward/pull/10). The current native
macOS foundation adds bounded reads and writer exclusion; native storage commits
and complete application sessions remain absent.

[Hosted Rust CI](continuous-integration.md) now validates core tests/doctests and
Clippy on Linux, Windows and macOS, plus the native foundation tests on macOS.
This adds validation evidence without advancing the milestone or establishing
packaged support, native writes or durability.

## Implemented first slice

The [Rust core](../crates/codeward-core/README.md) began as a framework-independent
library with no third-party dependencies. Its parser borrows a complete byte slice, checks actual
size and declared lengths, rejects unsupported version/suite/Argon2 profiles and
reserved fields, and exposes only unauthenticated public metadata/encrypted bytes.
It builds slot/payload AAD and recovery HKDF info from the exact original header
under the [normative format](vault-format-v1.md).

The result is deliberately named `UnauthenticatedEnvelope`. Parsing never unlocks
a vault, validates an AEAD tag or exposes decrypted credentials. A future bounded
file reader must enforce the cap before allocating its input; checking an existing
slice is not evidence that file loading already has that protection.

## Implemented second slice

[`EnvelopeParts::encode`](../crates/codeward-core/src/encoding.rs) assembles the
exact version 1 layout from fixed public metadata and already-encrypted slots
and payload. Fixed-size arrays constrain slot/salt/nonce lengths; the encoder
emits only the accepted identifiers/profile and zero reserved fields. It rejects
zero revision, a payload shorter than its tag, and output exceeding the whole-file
cap before allocating output. Supplied bytes are preserved.

This is an in-memory format encoder, not vault creation or a file writer. It
does not generate randomness, encrypt, authenticate, validate JSON, increment a
revision or authorize a mutation. Callers remain responsible for those higher-level
operations; structurally valid output can contain invalid ciphertext/tags.

Five new tests compare complete output against the independently generated Python
fixture, cover minimum/exact-cap/over-cap sizes and invalid revisions/tag lengths,
and exercise varied metadata, ciphertext preservation and parser round trips.
No new dependency, algorithm, parameter or format change is introduced.

## Cryptographic foundation

[`crypto`](../crates/codeward-core/src/crypto.rs) now implements the accepted
Argon2id profile, recovery HKDF-SHA256 derivation and full XChaCha20-Poly1305
encryption/authentication through pinned RustCrypto packages. The
[dependency review](dependency-review.md) records exact releases/features,
all 28 registry packages, compiler/license checks and advisory results before use.

Master derivation preserves entered bytes, uses a nonblocking process-wide guard
to permit only one attempt at a time, and explicitly owns/clears its 64 MiB work
buffer. Keys and temporary plaintext use zeroizing owned buffers. New encryption
accepts only an OS-generated nonce object that is consumed on an attempt; even a
failed attempt cannot reuse that object. A compile-fail test exercises this rule.
Size checks precede plaintext copies; bounded working allocations use fallible
reservation. Randomness failure returns no partial result and has no fallback.

Primitive authentication returns no plaintext on failure. Successful decryption
produces authenticated bytes only: the separate credential decoder must validate
the returned bytes before exposure. The vault layer below composes these primitives
with both protected slots and schema validation; application authorization and
session creation remain absent. These library operations are not UI commands.

## Paper-code component

[`recovery`](../crates/codeward-core/src/recovery.rs) generates an independent
32-byte secret, formats its canonical Base32/checksum representation into 15 groups
of four, and validates bounded transcription input. Decoding permits only the
specified ASCII case/separators, enforces unused-bit canonicality and verifies
the checksum before returning a secret that can derive a wrapping key.

Confirmation compares decoded secrets with the reviewed fixed-length comparison
primitive. Owned secret/display buffers are cleared on drop. This component does
not enforce enrollment state, limit display to enrollment, persist confirmation,
unlock a vault or replace a master password on its own. The vault layer below adds
volatile enrollment and authenticated recovery preparation; application display and
commit controls remain future work. Checksum success alone does not authorize access.

## Credential schema component

Implemented 2026-11-01 under the unchanged version 1 schema. The
[`credentials` module](../crates/codeward-core/src/credentials.rs) owns validated
records and sorts snapshots by ID, rejecting duplicates. Text is preserved exactly
in zeroizing strings; fields are private and exposed as borrows for core operations.
No Debug/Clone/Copy implementation or generic public Serde serialization is provided.
Constructors validate IDs, UTF-8 byte bounds and timestamp range/order before copying.

`CredentialSnapshot::decode` caps existing plaintext bytes before UTF-8 parsing.
A nonallocating preflight enforces depth three and bounded field-name tokens.
Explicit visitors reject unknown/duplicate/missing fields and excess record counts;
borrowed raw values prevent eager string/container decoding. Field decoded-byte
counts, including Unicode escapes/surrogate pairs, are checked before unescaping.
Wrong collection types are rejected as borrowed tokens, avoiding large decoded
strings in Serde's type-error path. Integer fields reject fractional/exponent forms.
Failed decoding returns no partial snapshot and leaves input untouched. Library
errors are translated into fixed diagnostics without source text.

`encode` writes compact fixed-order JSON with ascending IDs through a capped
zeroizing writer. It returns no partial output on failure and checks the full
plaintext limit before extending the output buffer. This is serialization, not
vault encryption or a file commit; individually valid records can exceed the total
encoded snapshot cap and are then rejected by the writer.

The [JSON dependency review](json-dependency-review.md) records exact pins, all
new locked packages/features and a fresh full-graph advisory snapshot. Serde's
bounded escaped-string scratch is not zeroized. Caller copies, allocator movement,
compiler temporaries, swap and dumps remain residual exposure; no complete-erasure
claim or allocation-failure injection is made.

Twenty new tests cover field/model boundaries, unknown/duplicate/missing fields,
IDs, timestamps, wrong types, malformed Unicode/escapes, every truncated prefix
of a synthetic record snapshot, trailing bytes and input preservation. They cover
exact/over-cap snapshots, 10,000 versus 10,001 records, escaped decoded-field
limits, deep containers, oversized keys, fixed diagnostics and deterministic
serialization. Unicode length tests exercise every valid BMP scalar plus selected
surrogate pairs; these finite tests are not an exhaustive parser/security audit.
Two Python-standard-library fixtures independently encode synthetic UTF-8 and
ASCII-escaped snapshots; Rust output matches the compact UTF-8 fixture exactly.

Schema validity alone does not authenticate input or grant access. The vault layer
now invokes this decoder only after complete payload authentication; application
authorization, file I/O and session/UI workflows remain unimplemented.

## Authenticated in-memory vault component

Implemented 2026-11-02 under unchanged bytes, profiles and schema. The
[`vault` module](../crates/codeward-core/src/vault.rs) first structurally validates
input, then authenticates the selected slot and complete payload using the original
header AAD. It returns contents only after strict credential decoding. Wrapping
keys and temporary plaintext are released when no longer needed. `AuthenticatedVault`
owns the data key/snapshot in Rust; consuming `lock` releases that owned material.
This is not an application session, caller authorization or proof of complete erasure.

`Enrollment` generates a paper secret, checks exact master-password confirmation
and paper transcription, and prepares an encrypted empty candidate. Success drops
the enrollment secret and disables further display/preparation; failures permit an
explicit retry with newly generated material. Password inputs are borrowed with
ADR-0005's 1–4,096 UTF-8 byte bound. Caller display/input copies require cleanup.

`RecoveryAccess` authenticates/validates without a credential-browsing API. It can
prepare a replacement master slot and payload together, retaining the data key and
paper salt/slot. Content preparation retains both slots without Argon2. Both paths
check the expected authenticated revision, reject overflow and use fresh encryption
nonces. Preparation never advances the source object or mutates borrowed bytes.
`VaultCandidate` contains only complete encrypted bytes; it is not a saved/ready
vault or completed recovery. The transaction component below adds admission,
base checks, reconciliation and commit acknowledgement through a trusted adapter.

Header encoding and slot/payload AAD construction are shared with the existing
format components, avoiding a second implementation of offsets/context. No Rust
runtime dependency, cryptographic profile or on-disk byte change was introduced.

Thirteen new runtime tests cover independent encrypted fixtures through both routes,
authenticated invalid JSON, wrong secrets, master-password bounds, all single-byte
mutations of one complete fixture through recovery, selected master-route damage,
cross-vault slot transplantation, confirmed enrollment, fresh update nonces, retained
slots, repeated paper recovery, oversized update failure and revision conflicts/
overflow. Three compile-fail doctests total cover nonce reuse, consumed vault access
and absence of ordinary browsing on recovery contexts.

The [full-envelope generator](../crates/codeward-core/tests/fixtures/generate_vault.py)
uses a synthetic Python-standard-library XChaCha/Poly1305 integer reference model
plus the existing argon2-cffi 25.1.0 C derivation. It checks published HChaCha and
AEAD vectors from the archived XChaCha draft before producing valid, invalid-schema
and maximum-revision fixtures. This is test-only reference code, never a runtime
crypto fallback. Regeneration reproduces the checked-in bytes; ordinary Rust tests
need no Python or additional package installation. Finite cases are not fuzzing,
an independent audit, or allocator/randomness failure injection at every layer.

The [synthetic round-trip example](../crates/codeward-core/examples/vault_round_trip.rs)
ran successfully: initial candidate, one-credential update at revision 2, and
recovery password replacement at revision 3. It prints no secret values and performs
no vault-file I/O. Storage durability, application/OS lock events, IPC permissions,
packaging and Windows/Linux execution remain outstanding.

## Transaction and session-admission component

Implemented 2026-11-02 under ADR-0003/0005 and the UI/core boundary. The
[`transaction` module](../crates/codeward-core/src/transaction.rs) owns caller,
opened-vault and generation binding, expected revisions and bounded request status.
It composes authenticated master access, restricted paper recovery and confirmed
creation with one encrypted-only commit worker. These are core APIs, not IPC commands.

The session owns decrypted access and a random intent-matching key. The worker
owns only encrypted base/candidate bytes, fixed-size intent digests, status and an
exclusive storage lease. Lock increments generation, releases owned access/key
material and atomically cancels publication that has not been admitted. An admitted
worker continues without reopening access. A separate-thread test blocks staging
while the session locks; no storage lock is needed to revoke session access.
Caller-owned snapshot/input copies and UI redaction remain the caller's responsibility.

The protocol stages a full envelope, rechecks vault ID/revision/full-file SHA-256
against the exact authenticated source, atomically admits publication against lock,
publishes and completes the adapter's final barriers. Only barrier success advances
the committed revision. Initial creation requests atomic no-replace. Definite
pre-publication failures permit a new explicit request with fresh encryption;
observed conflicts require reopen/review. Publication ambiguity or barrier failure
retains OutcomeUnknown and blocks mutations. Reconciliation reads actual bytes
under exclusion, requires exact known-base/candidate equality plus fresh slot,
payload and schema authentication, then repeats barriers. Absence after uncertain
creation requires the adapter's namespace barrier before an Unchanged result.
Mismatch, failed authentication/read/barrier or repeated interruption stays unknown;
there is no speculative rollback, stage promotion or cleanup.

Identical authorized requests return the recorded reference before encryption;
changed arguments or generation reuse are rejected. Per-generation HMAC-SHA256
digests avoid retaining plaintext replacement passwords or unkeyed password hashes.
The 64-entry history fails closed at capacity and never evicts IDs to permit replay.
Post-lock status is caller/scoped and contains no credentials. Reopening loads an
authenticated visible baseline but loses historical status and cannot prove a
previous process's barriers; an unavailable result never means an operation did not commit.
The original admission slice required explicit reload/authentication after every
commit. [Committed content refresh](#committed-content-refresh) now offers an
in-session path for ordinary content commits. Draft management and a complete
application dispatcher remain pending.

Sixteen new tests cover every modeled staging failure (creation, short write,
close and sync), read failure, definite/ambiguous publication and final barriers;
they also cover lost replies, exact-base conflicts, safe explicit retries,
caller/vault/generation binding, history capacity, lock before/after admission,
blocked storage, coherent creation/recovery and repeated reconciliation failure.
Modeled termination at protocol boundaries reopens only full old/new envelopes
through both access routes. This is in-memory fault injection, not actual
process-kill, OS-crash, power-loss, permissions, disk-capacity or multi-process evidence.

`Storage`/`WriterLease` define the trusted adapter contract: bounded reads before
allocation, stable companion exclusion, validated path identity, restrictive
exclusive same-directory staging, checked whole writes/close/sync, atomic publication
and declared persistence barriers. Only the test model implements the complete
writable interface. The separate native foundation below supplies read/exclusion
prerequisites; no native vault write or durability guarantee is established.
No dependency, feature, vault bytes, schema, primitive or KDF-cost change was made.
Existing pinned SHA-256/HMAC now also provide internal fingerprint/intent matching;
the [dependency review](dependency-review.md) records this use and snapshot limits.

## Cancellable core authentication

The [split-phase authentication API](session-authentication.md) adds locked open,
single-use master/recovery jobs and opaque completions. The control-side session
can revoke without waiting for authentication/storage. Installation rechecks trusted
caller, opened-worker scope, generation, current attempt and exact committed-base
fingerprint. Stale success/failure cannot replace or revoke newer access. Recovery
stays restricted; unknown/conflicted writes remain subject to the existing worker.
Synchronous reload convenience calls delegate to the same path.

The focused synthetic tests cover blocked authentication/lock, late completions,
replacement, drop/reopen, caller/worker isolation, malformed/tampered input, exact
fingerprints and transaction-policy preservation. Jobs/results are core-only secret
owners; an executing KDF is not preempted and hosts must promptly drain stale
completions. Timers, drafts, automatic refresh, OS events and full dispatch remain
pending. This changes no format, crypto profile, dependency or native acceptance.

Local synthetic evidence on 2026-11-05: all 26 transaction test functions passed
(100.48s), including ten new authentication cases. All five core compile-fail
doctests passed, including consumed-job and result-formatting boundaries. Core
all-target Clippy with warnings denied, workspace formatting and whitespace passed.
Six affected Markdown files/172 local links and anchors plus fences passed;
Mermaid diagrams are unchanged. This is focused local evidence; the full workspace
and three-platform run remain subject to current-head/base hosted Rust CI.

## Core inactivity and security-event policy

The [inactivity policy](session-inactivity.md) adds the accepted fixed five-minute
monotonic deadline to normal/restricted access, enrollment and pending authentication.
Privileged mutation/code-display/completion gates check expiry; late activity cannot
revive access. Opaque references bind keyboard/pointer/assistive activity to actual
opened scope, caller and generation. Background work and auth completion do not
refresh it. Typed host security events revoke through the existing lock path,
including resume/reconnection without an observed suspend. Clock regression and
checked deadline overflow fail closed; generation exhaustion remains enforced.

Expiry cancels pending authentication/publication admission without waiting for
workers. Admitted commits still resolve; unknown outcomes are not cleared or
reported saved. Tests use controlled private observations, not sleeps or actual
OS notifications. The host must schedule idle polls and deliver validated native
events/suspend-gap observations. No new dependency, format/schema, crypto profile
or native writer feature; full dispatch, drafts, refresh, OS/UI behavior and native
acceptance remain pending.

Local evidence on 2026-11-06: ten policy tests passed (20.86s) and ten authentication
tests passed (25.89s) after final deadline cleanup. An earlier 35-function transaction
regression run passed (119.74s). All five core compile-fail doctests, workspace
all-target/all-feature Clippy with warnings denied, formatting and whitespace passed.
Seven affected Markdown files/184 local links and anchors plus fences passed;
Mermaid unchanged. Full current-head/base workspace coverage uses hosted Rust CI;
no full local workspace test repeat was performed.

## Committed content refresh

The trusted Rust host can call
[`Session::refresh_committed_contents`](../crates/codeward-core/src/transaction/session/refresh.rs)
after an ordinary content save under FR-02/03/05 and ADR-0003/0005. The operation
reference must belong to the current caller, opened worker and session generation.
The worker must be idle and its current revision must match the referenced
barrier-completed commit. This includes a candidate whose uncertain outcome was
successfully reconciled. An unchanged outcome or observed conflict cannot refresh.

The session authenticates the worker's exact committed payload and validates its
schema with the existing data key. Both key slots were preserved by the content
update. The session replaces its snapshot and source only for the same vault at
the next revision. A repeated refresh for the current commit is safe. No password
derivation, storage I/O, credential reply, request history reset or deadline
extension occurs. A failed payload/schema check leaves the previous snapshot and
source in place. Lock and expiry still revoke access, including when the write
already committed. A refresh error leaves the worker's commit status intact.

The host invokes refresh explicitly when handling a result. Without it, a stale
session still requires an explicit reload before another mutation. Enrollment
and restricted recovery require ordinary master authentication to grant normal
access. Host result dispatch, drafts, UI and native storage acceptance remain pending.

Focused local tests on 2026-11-07 passed all five refresh cases, the definite
nonpublication and restricted recovery regressions, and all fourteen vault tests
(including one refresh case). Tests cover successive saves, request deduplication,
caller/scope/generation isolation, expiry, reconciled commits, unchanged outcomes,
conflicts, and failure to authenticate or validate a replacement payload. The
vault test holds the master derivation guard while refreshing. All five core
compile-fail doctests, workspace formatting and all-feature/all-target Clippy with
warnings denied passed locally. Six affected Markdown files passed 171 local link
and anchor checks and fence checks. Mermaid diagrams are unchanged.

## Scoped credential listing

[`Session::list_credentials`](../crates/codeward-core/src/transaction/session/listing.rs)
implements the Rust listing boundary for FR-02/03 and NFR-09 under
[ADR-0001](adr/0001-implementation-platform.md) and
[ADR-0005](adr/0005-session-and-drafts.md). The trusted host supplies the caller,
opened worker and expected session generation. Listing requires ordinary master
access to the worker's current authenticated committed snapshot. The session polls
inactivity before preparing and before returning a page. Reads leave the deadline
and generation alone.

The requested page size is 1–100. A page can be shorter or empty at the end of
the snapshot. Summaries carry the stored record ID, title and username in ascending ID
order. The owned fields clear on drop. Each opaque continuation is bound to the
caller, opened worker, session generation and exact committed fingerprint. Lock,
reauthentication, reopening and a newer commit invalidate it.

The core pauses listing during pending or uncertain writes. When reconciliation
confirms the base is unchanged, its cursor is usable again. A new commit requires
refresh or reload before starting a new list. An observed conflict requires reopen.
The host must keep the original caller and opened-worker context, poll for expiry,
and check the context, generation and revision before delivering queued pages.
Copies already handed to a caller require its own cleanup. Search, selected-record
details, password reveal, IPC and presentation remain pending.

Focused local checks on 2026-11-07 passed all five listing tests, all five
committed-content refresh regressions and all five core compile-fail doctests.
The listing tests cover the cap, stable order, exact summary fields, caller and
vault scope, lock/expiry, restricted access, write reconciliation, conflicts and
cursor invalidation. Workspace formatting and all-feature/all-target Clippy with
warnings denied passed. Five affected Markdown files passed 172 local link and
anchor checks and fence checks. Mermaid diagrams are unchanged.

## Native macOS read and exclusion foundation

Implemented 2026-11-02 in a separate
[`codeward-storage-macos` crate](../crates/codeward-storage-macos/README.md), preserving
the core's unsafe-code prohibition. Reviewed thin native calls use existing locked
libc 0.2.191 without adding a registry package/version/feature. The
[native evidence](macos-storage-foundation.md) records its dependency/API review,
descriptor ownership, mode/location limits and tests.

Absolute unambiguous paths resolve through retained no-follow directory handles.
The effective-user-owned parent and reported local writable nonremovable APFS
are checked. One stable companion per directory uses nonblocking OS exclusion,
so aliases and different vault names there share writer ownership. No companion
is truncated or unlinked. Explicit lease-drop unlock corrects an observed transient
reacquisition failure; 100 native-suite repetitions passed after that focused fix.
Reads reject linked/nonregular/POSIX-exposed files, check the 16 MiB limit before
buffer reservation, cap every append and detect observed file/parent/companion changes.
Returned bytes still require authentication/schema validation in the core.

Fourteen native test functions include thirteen assertion cases and a subprocess
helper. Actual process contention, normal exit and killed lock-holder release
were exercised, alongside caps, changing files, links, POSIX modes and substitutions.
These are native read/exclusion tests, not commit-interruption or storage-crash
evidence. ACL grants and cloud-location policy remain unvalidated. The foundation
does not implement writable Storage/WriterLease, publication or namespace barriers,
and is restricted to synthetic data. The later staging experiment below adds
encrypted sibling preparation. There is no macOS storage support claim.

## Native macOS encrypted staging experiment

Implemented 2026-11-03 under unchanged ADR-0003/format/dependencies. A lease-bound
`prepare_stage` exclusively creates an unpredictable private sibling, checks the
16 MiB input cap before creation, refuses parent/candidate extended ACLs, completes
short/interrupted writes, requires fsync plus F_FULLFSYNC, and checks explicit
close. Observed substitutions fail closed. Errors and dropped stage markers
preserve residue; nothing publishes, cleans up, or acknowledges a save.

The [staging review/evidence](macos-staging.md) records the native API review,
conservative ACL policy, and 25 passing native functions plus one compile-fail
doctest. New cases include collision exhaustion, ACL/inherited grants, child
umasks, injected write/sync/close failures and actual process kills after creation,
partial write and file sync. The original synthetic vault remains unchanged and
residue remains unpromoted. These are pre-publication cases, not native committed
write, namespace durability, real disk-full or power-loss evidence. Publication,
location gates and transaction integration remain outstanding.

## Toolchain and component review

Pin Rust 1.99.1, edition 2024, with rustfmt and Clippy. Verify against the official
[stable release manifest](https://static.rust-lang.org/dist/channel-rust-stable.toml)
and [release notes](https://doc.rust-lang.org/releases.html#version-1991-2026-10-15).
The local installer was fetched from Rust's official HTTPS distribution and its
SHA-256 checked against the published installer checksum. Local tooling/caches
are excluded from Git; no shell startup files were changed.

The first two slices were dependency-free. The crypto foundation pinned nine
direct packages and 28 registry packages; the JSON slice adds two direct pins and
eleven locked packages, for eleven direct and 39 total registry packages.
The macOS adapter pins already-resolved libc directly, adding no registry
package/version or resolved feature; the [native review](macos-storage-foundation.md)
records the boundary and reused advisory snapshot.
Reviewed host/Windows/Linux feature graphs do not enable PHC helpers, reduced
rounds or browser randomness. Graph inspection is not cross-platform execution.
The separate JSON review covers its graph.

## Verification performed

Host: macOS 26.3.1 (25D2128), Apple silicon. Rechecked 2026-11-02 with the pinned toolchain:

| Check | Observed result |
| --- | --- |
| `cargo test --workspace --locked --offline` | 104 runtime test functions pass: 90 core and 14 macOS (including the subprocess helper); plus 3 compile-fail doctests |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Pass |
| Synthetic fixture regeneration | Full encrypted fixtures regenerated on 2026-11-02 with the test-only Python reference model and existing argon2-cffi 25.1.0; earlier component fixture evidence remains recorded above |
| Documentation/whitespace | Relative links, fences and `git diff --check` checked; existing Mermaid diagrams unchanged |

Tests cover every truncated prefix of a structural fixture, trailing bytes, exact
and excessive 16 MiB lengths, adversarial declared lengths, unsupported IDs and all
nonzero reserved-byte values, lower/higher/overflow-sized KDF costs, endian fields,
input preservation and AAD binding/exclusion behavior. Exact metadata fixtures are
constructed independently in Python and consumed as checked-in hex by Rust tests.
The structural/metadata fixtures are deliberately not valid encrypted vaults.
The separate complete encrypted fixtures above also contain only synthetic data.

Mutating ciphertext can still pass structural parsing; primitive AEAD tests now
reject modified ciphertext/tags, every nonce/AAD byte, wrong keys, truncation and
extra bytes. Both layers preserve their inputs. Malformed credential-JSON rejection
has component tests; whole-envelope authentication now calls the schema gate
before either ordinary access or restricted recovery is returned.

Known-answer tests use [RFC 9106 section 5.3](https://www.rfc-editor.org/rfc/rfc9106.html#section-5.3),
[RFC 5869 appendix A.1](https://www.rfc-editor.org/rfc/rfc5869.html#appendix-A.1)
and the XChaCha vector in
[draft-irtf-cfrg-xchacha-03 appendix A.3.1](https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03#appendix-A.3.1).
The XChaCha source is an archived Internet-Draft, not an RFC. The Argon2 RFC vector
uses test-only small costs/secret/associated data; the production API exposes only
the approved profile, separately compared with independently computed C fixtures.
Those fixtures include spaces, case and composed/decomposed Unicode passwords.
HKDF and paper-code expected bytes are independently computed with Python's
standard library. Normal tests need neither Python nor a network after Cargo fetch.

Paper tests cover all 1,860 alternative single-character substitutions of one
60-character fixture, unequal adjacent transpositions, all 15 noncanonical unused-bit
variants, lookalikes/padding/invalid lengths, the 256-byte raw bound, mismatched valid
confirmation, ASCII normalization and 256 repeated-byte round trips. These finite
examples are not a guarantee against every checksum collision or parser defect.
No memory-erasure, timing-side-channel or independent security audit is claimed.
Only the host above was exercised; no Windows/Linux runtime claim.

## Initial master-derivation measurement

The [synthetic timing example](../crates/codeward-core/examples/measure_master.rs)
runs six sequential derivations with the exact 65536 KiB / 3-pass / 4-lane profile
and clears each derived key. In a release build, the measured attempts were
88.938, 69.475, 71.580, 170.593, 76.219 and 69.272 ms. The first is the first call
within that process, not a guaranteed cold-machine measurement. `/usr/bin/time -l`
reported maximum resident set size 68,698,112 bytes (about 65.5 MiB) for the entire
process, not a separately measured KDF allocation peak.

Reproduce with `cargo run --release --locked --offline --example measure_master`
and `/usr/bin/time -l target/release/examples/measure_master` on macOS. This is a
small, noisy single-host observation, not a performance guarantee or the required
slowest-target benchmark. The rough 0.5–1 second target remains unevaluated across
the platform matrix; no cost was changed in response to this faster host.

## Review findings and verification

The pre-merge review checked the core source, tests and accepted byte/profile
contracts across PR #4–6. No blocking parser or encoder defect was found. One
resource-handling defect in the crypto helper was fixed: unsupported password
lengths could allocate the 64 MiB Argon2 work buffer before the upstream library
rejected them. The helper now checks Argon2's existing maximum before locking or
allocation. A new test covers the length boundary using scalar lengths, without
allocating a multi-gigabyte password. Source review verifies the guard's placement.
This does not introduce a new application password policy or alter KDF costs.

The exact PR #4 and #5 snapshots were tested in isolated temporary source copies:
17 and 22 tests respectively, plus rustfmt and Clippy with warnings denied.
At that pre-merge review, the corrected combined core passed 41 tests plus the
nonce compile-fail doctest.
These are local macOS checks; the PRs have no hosted check runs/statuses at review.
This review does not replace an independent security audit or establish integrated
vault, schema, storage or platform guarantees. PR #4–6 were retargeted to `main`
in dependency order and merged with merge commits; no hosted CI evidence is claimed.

## Native macOS namespace synchronization experiment

The [namespace experiment](macos-namespace.md) adds checked expected-state file/parent
fsync and F_FULLFSYNC operations under the same lease-local guard as publication.
Present files require all four calls; expected absence still requires both parent
calls. Changed observations or failed/unsupported/interrupted calls preserve
uncertainty without retry, fallback, rollback, cleanup or promotion. No dependency,
format or cryptographic profile changes, writable core adapter or save acknowledgement.

Focused local native evidence on 2026-11-04: 46 runtime functions, two compile-fail
doctests, native all-target Clippy, formatting and whitespace pass. Sixteen child
kills around the barrier checkpoints preserve whole observations/residue and permit
reacquisition/repetition. Exact/over-cap, per-step injected sync errors, ACLs,
substitutions and same-lease contention are covered. This does not establish
inside-syscall interruption, OS-crash/power-loss or native transaction acceptance.

## Managed macOS commit workflow

The [headless managed workflow](macos-managed-workflow.md) exercises the shared
writer directly on a disposable Apple silicon APFS home. It covers initial
creation, process termination before publication and after an acknowledged save,
exclusive ownership, reopen, lock, paper recovery and reconciliation after a
barrier refusal. The full recovered snapshot and preserved encrypted residue are
checked. Replacement and barriers now compare bytes from one retained descriptor.
The managed directory barriers run from `Vaults` through the account home.
The locked/offline native crate suite and doctests passed locally on 2026-11-07,
along with workspace formatting and all-feature/all-target Clippy with warnings
denied. This evidence covers the tested synthetic configuration.

## macOS capacity and admission

The [capacity and admission slice](macos-capacity-and-admission.md) requires one
device from the account home through the managed subtree and refuses recognized
cloud roots. The shared writer keeps the named vault authoritative and preserves
complete, partial, linked and unrelated leftovers. A disposable APFS image test
observed real kernel `ENOSPC`, preserved the previous committed bytes when staging
failed and completed a new explicit retry after releasing space. It also exercised
refusal of a real nested mount and reopened the retry through both access routes.
The native suite and doctests passed locked/offline locally. The opt-in capacity
case and final focused admission/residue tests passed separately, along with
workspace formatting and all-feature/all-target Clippy with warnings denied.

## Headless vault lifecycle

The [headless host](headless-vault-host.md) owns the session on the control side
and dispatches authentication, encrypted commits and reconciliation to workers.
Lock and close can revoke while a worker is blocked. Confirmed ordinary saves
refresh current access without changing commit status if refresh is refused.
The model tests exercise cancellation, uncertainty and worker loss. The managed
macOS test runs create, unlock, save, close, reopen and recovery through the host.
The owner accepted the bounded macOS storage policy on 2026-11-07 under
[ADR-0006](adr/0006-managed-working-location.md).

The [headless timer and macOS event source](headless-session-events.md) now
schedule idle expiry and process revocation before commands and worker replies.
Workspace sleep/wake and session switch observations use AppKit. Undocumented
distributed screen-lock hints can only revoke. Bracketed Mach clocks provide
a suspend-gap check even when a wake notice is missed. The
[live probes](headless-session-events.md#live-macos-probe) passed for real
lock/unlock and sleep/wake on macOS 26.3.1. Each revoked synthetic enrollment
and confirmed that paper-code access remained denied afterward.

The [desktop-driven mode](headless-session-events.md#driving-the-control-loop-from-a-desktop-host)
lets an outer macOS loop collect delivered revocations and schedule deadlines
without nested event dispatch. Process-local tests cover the control boundary.

The [nonblocking storage open](headless-session-events.md#opening-storage-without-blocking-event-delivery)
moves managed-path preparation, writer acquisition and baseline reads to a worker.
Model tests cover revocation while a read is blocked and suppression of a completed
opening when a lock arrives before its reply.

[Write preparation](headless-vault-host.md#write-replies) for creation, content
saves and password replacement now runs on a worker. Lock can revoke access
while preparation is blocked, and a candidate waits for another event check
before staging. Model tests cover cancellation, stale results, enrollment retry
and request deduplication.

[Post-save snapshot refresh](headless-vault-host.md#write-replies) also runs on
a worker. It decodes the committed bytes without retaining the writer lease.
The control loop continues processing revocation and reports the proven commit
even when expiry, source failure or worker loss prevents refresh.

The opt-in [macOS desktop pilot](macos-desktop-pilot.md) drives this loop from
Tauri and exposes only scoped synthetic-session status and lock controls.
The mock runtime exercises the generated permissions. The
[packaged probes](macos-desktop-pilot.md) exercised native lock/unlock and
sleep/wake, redaction of synthetic WebView fields and continued session
revocation on Apple silicon macOS 26.3.1.

## Linux storage foundation

The [Linux foundation](linux-storage-foundation.md) adds bounded reads and an
exclusive companion lease for synthetic files on ext4. Hosted Ubuntu tests use
a disposable ext4 image and separate processes to exercise exclusion. The
[managed entry](linux-storage-foundation.md#managed-location-experiment)
prepares and revalidates the fixed location for synthetic reads.
[Encrypted staging](linux-storage-foundation.md#encrypted-staging-experiment)
prepares private synchronized candidates under the existing lease.
[Synthetic publication and namespace operations](linux-storage-foundation.md#atomic-publication-and-namespace-experiment)
exercise atomic create and replace and checked file and directory synchronization.
The [opt-in transaction adapter](linux-storage-foundation.md#core-transaction-experiment)
connects these operations to core commits and reconciliation for synthetic data.

## Windows storage foundation

The [Windows foundation](windows-storage-foundation.md) adds bounded reads and
cross-process companion exclusion for synthetic NTFS fixtures. The [managed
experiment](windows-storage-foundation.md#managed-location-experiment) adds
LocalAppData resolution and bounded ACL admission. Handle-relative creation,
staging, publication and persistence remain pending.

## Remaining work

1. Implement and validate Windows native persistence. The bounded macOS
   storage policy is accepted.
2. Review the Linux location policy and close remaining native fault-evidence
   gaps for the declared Linux target.
3. Demonstrate the Milestone 2 gate with synthetic vaults, including malformed
   input, competing writers, real process interruptions and native storage
   failures. Confirm that save acknowledgement follows the required barriers.

Offline recovery acceptance follows in Milestone 3. Draft handling, close
interactions, Linux and Windows desktop events, and integrated credential
redaction belong to [Milestone 4](roadmap.md#path-to-desktop-integration).
Real-vault admission, production save guarantees, OS-crash and power-loss
evidence, and distribution package validation remain outstanding.

## Earlier macOS storage evidence

A [shared macOS writer](macos-experimental-writer.md) now implements the core storage
traits for explicit experimental feature opt-in; default runtime builds omit it.
The existing synthetic lifecycle, uncertainty, process-kill and kernel-failure
tests delegate to this implementation. Public construction uses only the managed
location; arbitrary test paths/fault controls stay private. Bounded fallible copies
and observation invalidation do not replace native admission/persistence acceptance.

[Owned native stages](macos-stage-ownership.md) now retain the same writer state
independently of the caller's lease. The test-only transaction worker stores the
actual marker, removing private field extraction/reconstruction. Final-owner drop
releases exclusion; a worker still retains its own lease through barriers and
reconciliation. This prepares runtime integration without exporting or accepting
a writable adapter.

A [kernel write-failure experiment](macos-write-failures.md) now exercises actual
macOS EFBIG/short progress through native staging and the test-only core bridge in
managed synthetic homes. Creation, content and recovery failures never advance the
committed revision or publish; base access and partial residue are preserved.
Known IDs do not replay after the limit is removed; a new explicit request can
retry, with fresh enrollment required for creation. This closes that bounded native
error-path gap without claiming disk-full/quota/media evidence or enabling a writer.

A [managed working-location entry](macos-managed-location.md) now selects the
fixed macOS account app-data tree and prepares private directories without
creating a vault. Managed paths revalidate trusted ancestors and private app
directories through the existing native operations. [ADR-0006](adr/0006-managed-working-location.md)
records the selected direction and backup/portability discussion; its concrete
contract is submitted for review. No runtime writer or real-vault admission added.

A [destination mount-policy check](macos-location-admission.md) now repeats the
local/writable/nonremovable APFS query through path verification, including reads,
staging, publication and barriers. Injected refusals/query errors preserve definite
failure before publication and uncertainty afterward. Cloud/ancestor admission and
runtime storage acceptance remain incomplete.

A [macOS ACL admission experiment](macos-acl-admission.md) now refuses extended
ACLs on the direct parent, stable companion and vault reads before returning
bytes. Existing lease checks extend this refusal through staging, publication and
namespace barriers. Eight new tests cover initial admission, observed changes,
preserved artifacts and uncertain outcomes; cloud-location policy and runtime
storage acceptance remain incomplete.

A [test-only macOS transaction experiment](macos-transaction-experiment.md) now
composes the real native operations with the core worker under `cfg(test)`. Eight
new test functions cover confirmed creation, content save/delete, repeat recovery,
uncertain outcomes/reconciliation, explicit retries, conflict/lock boundaries,
preserve-all residue authority and 15 actual child kills at protocol checkpoints.
The bridge is absent from runtime builds. This is synthetic integration evidence,
not native adapter acceptance, complete application sessions or a usable vault.

The [synthetic macOS publication experiment](macos-publication.md) now extends
staging with atomic no-replace creation and expected-base replacement under the
retained lease. Publication errors remain uncertain; visibility is never a saved
state. Focused local evidence includes 36 native test functions, two compile-fail
doctests, native Clippy/formatting, concurrent readers and 14 child kills around
publication. The namespace experiment above adds candidate barriers; neither
experiment establishes complete persistence acceptance or the writable adapter.
