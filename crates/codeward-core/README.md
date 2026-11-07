# Codeward core

Milestone 2 library: structural parsing/encoding, public authentication metadata,
bounded cryptographic primitives, paper-secret transcription checks and strict
credential JSON handling and authenticated in-memory vault operations for the
[accepted format](../../docs/vault-format-v1.md). Rust 1.99.1 is pinned.
[Exact crypto pins/features and advisory review](../../docs/dependency-review.md)
are recorded; there are no runtime services.

`UnauthenticatedEnvelope::parse` borrows already-read bytes without allocating,
rejects unsupported profiles and malformed lengths, and returns only encrypted
bytes/public metadata. A future file reader must cap reads before allocating its
input. Passing this parser does not authenticate a vault or validate decrypted JSON.
The deliberately named type cannot authorize credential exposure.

`HeaderParts` supplies shared fixed-header encoding; `EnvelopeParts::encode`
assembles an in-memory envelope from public metadata and
already-encrypted bytes. It checks revision/payload bounds before allocating
output and emits the exact accepted profile. It neither encrypts/authenticates
those bytes nor generates randomness, increments revisions or saves a file.

AAD is constructed from the exact original header bytes. Slots exclude changing
revision and each other; the payload includes the complete header and both slots.
The structural hex fixtures are deliberately not valid encrypted vaults. Separate
full encrypted fixtures test both access routes and schema gating independently.
`crypto` provides fixed-profile master/recovery derivation and bounded authenticated
encryption. Owned keys/plaintext and explicit Argon2 work memory use zeroizing
buffers. One master derivation runs per process; a new OS-generated nonce object
is consumed on each encryption attempt. `open` returns authenticated bytes, not
validated credentials or session authorization. Do not expose these low-level
operations as generic UI commands.

`recovery` encodes/validates the canonical paper code and checks transcription
confirmation. The vault layer adds volatile enrollment lifetime/confirmation;
application display rules and native committed recovery remain absent.

`credentials` validates record IDs, field byte limits, timestamp order, unique IDs
and the exact version 1 JSON schema. Its decoder rejects duplicate/unknown/missing
fields and malformed/oversized input before exposing a complete snapshot. The
serializer emits compact fixed-order JSON with sorted IDs through a capped writer.
Owned strings/output are zeroizing; bounded Serde escape scratch is not. These
types establish schema validity, not authentication or access permission. See the
[JSON package review](../../docs/json-dependency-review.md) for dependency/cleanup limits.

`vault` composes these components: `Enrollment` confirms the paper code and master
password, then produces an encrypted empty `VaultCandidate`. `unlock_with_master`
authenticates slot/payload and validates credentials before returning an
`AuthenticatedVault`; `begin_recovery` returns a restricted `RecoveryAccess` with
no browsing method. The former prepares content updates; the latter prepares a
replacement master slot and payload while retaining the reusable paper slot.
Both check expected revisions and return unsaved encrypted bytes. Preparation
leaves the authenticated source unchanged; it never acknowledges a file commit.
Consuming `lock` releases owned Rust key/record buffers. Caller copies, application
permissions, automatic locking and UI redaction are separate. The transaction
layer adds the bounded admission/session-generation behavior below.

`transaction` separates a secret-owning `Session` from an encrypted-only
`Committer` with a retained exclusive `WriterLease`. The trusted `Storage`
adapter must acquire stable writer exclusion and implement restrictive staging,
atomic create/replace and persistence barriers. The macOS crate now supplies a
[default-off experiment](../../docs/macos-experimental-writer.md) for synthetic
development; no production native adapter is accepted.
The worker checks the complete authenticated base's SHA-256, vault ID and revision
before publication admission. Only final barriers advance the committed revision.
Ambiguous publication/barrier failure blocks mutations until fresh authentication,
exact base/candidate comparison and required barriers establish an outcome.

Caller/opened-vault/generation scope gates preparation. A per-generation keyed
intent digest supports duplicate-request matching without storing passwords.
History holds at most 64 IDs and fails closed rather than evicting/replaying them.
Lock releases owned access and cancels pending admission atomically; an admitted
worker can finish while the session stays locked. After a content commit, the host
can call `Session::refresh_committed_contents` with the current operation reference
to continue editing in the same unlock. It authenticates and validates the exact
committed snapshot with the session's data key. Explicit reload remains available
and is required when the session cannot adopt a commit. See the
[refresh contract](../../docs/milestone-2-progress.md#committed-content-refresh).

`Session::list_credentials` returns up to 100 summaries from the current
authenticated committed snapshot. Each contains the record ID, title and username.
The caller and opened worker scope the request, and a continuation is valid only
for the same generation and revision. See the
[listing contract](../../docs/milestone-2-progress.md#scoped-credential-listing).

The [core inactivity/event policy](../../docs/session-inactivity.md) owns a fixed
five-minute monotonic deadline, checks it before privileged operations and cancels
expired authentication. Only current caller/vault/generation activity refreshes it;
completion/storage progress does not. Trusted security events revoke immediately,
including resume without a prior suspend event. Hosts must schedule idle polls,
deliver validated OS observations and poll before reporting session-state snapshots.
No platform suspend detector, UI redaction or complete auto-lock support is claimed.

[`open_locked` and split-phase authentication](../../docs/session-authentication.md)
let the control side revoke a session while a separate Rust worker authenticates.
Only a current caller/vault/generation/attempt against the same committed base may
install the opaque completion. Master/recovery reload convenience calls use that
same path synchronously. A running KDF is not preempted; hosts must promptly drop
stale results and clear their own input/display copies. Recovery remains restricted,
and an uncertain write still requires the existing reconciliation protocol.

Tests cover format, primitives, paper codes, credentials and composed vault
operations using published vectors and independent synthetic Python/C fixtures.
Compile-fail examples cover nonce ownership, consumed access and restricted
recovery. See [evidence and limits](../../docs/milestone-2-progress.md) for counts,
exact validation scope and residual memory/storage/platform limitations.

From the workspace root with the pinned Rust toolchain installed:

```sh
cargo check --workspace --no-default-features --locked --offline
cargo test --workspace --all-features --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
```

The optional synthetic timing example runs with
`cargo run --release --locked --offline --example measure_master`.

Run `cargo run --locked --offline --example vault_round_trip` for a synthetic
in-memory creation/edit/lock/recovery demonstration. It prints no secret values.

The separate [macOS foundation](../codeward-storage-macos/README.md) experiments
with handle-rooted bounded reads and OS writer exclusion using synthetic data.
No native writer, durability adapter, complete application session
implementation or desktop UI exists yet. Never use these fixtures or this
incomplete library for real credentials.
