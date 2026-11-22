# macOS synthetic desktop prototype

## Bundled React interface

The default window now displays the Codeward welcome page: an animated wordmark,
slim Home/About sidebar and a Get started button. The sidebar starts collapsed,
and the bottom theme control remains visible. Dark mode is the initial theme;
the toggle switches between dark and light for the current window.
Get started now opens connected setup: enter/confirm a master password, write
Rust's generated recovery code, transcribe it, then create the encrypted vault.
The code disappears before transcription and after cancellation/lock. The window
reports **Vault saved** only after the core returns a committed revision, then
leaves the vault locked. **Unlock vault** on the welcome/result screen opens the
managed vault and authenticates the entered password in a Rust worker. The
unlocked screen opens a credential workspace with a list on the left and selected
entry details/editor on the right. Add, edit and delete use scoped Rust operations;
password reveal is explicit and temporary. The same dark/light styles apply to
setup, unlock and credential screens. Generation controls and forgotten-password
recovery UI remain later slices.

Use synthetic input only. The opt-in desktop feature now enables the existing
experimental APFS writer at the fixed account location:
`~/Library/Application Support/Codeward/Vaults/vault.cw`. An existing vault is
refused and preserved; the renderer cannot choose another path. This does not
admit real credentials or complete installed-app/milestone acceptance. A browser
layout preview cannot create a vault; use the Tauri application for setup.

Master passwords accept exact UTF-8 bytes (1–4,096), without normalization. The
renderer clears password fields immediately after dispatch; Rust retains the
bounded zeroizing enrollment password until the attempt completes or is revoked.
Canceling/navigation/hidden-document handling clears renderer secrets and sends
an epoch-scoped cancellation. Late code/opening replies are discarded. An
already admitted save can still finish; cancellation never claims rollback.
Unknown outcomes block new creation in that process. Keep the paper code and
reopen to inspect actual state; no automatic retry or reconciliation UI is added.
JavaScript/IPC/OS copies cannot be proven erased. Native live probes remain
separate and retain their synthetic session behavior.

From the repository root, with Node/npm and the pinned Rust toolchain installed:

```sh
npm ci --prefix crates/codeward-host-macos/frontend
npm run build --prefix crates/codeward-host-macos/frontend
cargo fetch --locked
cargo run -p codeward-host-macos --features desktop-pilot --bin codeward-desktop-pilot --locked --offline
```

After changing frontend source, rebuild the assets and quit/rerun the native app.
The production build writes locally bundled files to `desktop-dist`; no localhost
server is needed when the installed app runs. For a browser-only layout preview:

```sh
npm run dev --prefix crates/codeward-host-macos/frontend -- --host 127.0.0.1
```

Use the URL printed by Vite. Browser preview has no native vault access; setup,
unlock and credential persistence require Tauri. Browser checks with mocked IPC
can validate UI behavior, but cannot establish native persistence or OS-event
handling.

## Synthetic host foundation

The optional `desktop-pilot` feature in
[`codeward-host-macos`](../crates/codeward-host-macos/Cargo.toml) builds a Tauri
window with a bundled welcome page and separate synthetic enrollment probes. Its
probe adapter can only observe an absent destination. The separate enrollment
bridge uses the experimental managed writer for synthetic encrypted creation.
Neither surface exposes arbitrary filesystem operations or full-vault contents. Unlocking never writes, creates or repairs the vault file; absent
and structurally damaged vaults receive distinct controlled results. Wrong passwords preserve
the file and permit an explicit retry. Lock, inactivity and trusted OS events
revoke native access; late authentication results cannot restore it.

This window provides synthetic session-event evidence for Milestone 2.
The [local Apple silicon credential demo](roadmap.md#local-apple-silicon-demo)
is the first UI integration slice in Milestone 4, after persistence and offline
recovery are accepted.

## Main-thread driver

The desktop window owns a `MacosEventSource::for_desktop_loop` and
`HeadlessControlLoop` on the OS main thread. Opening starts on a worker.
The host calls `step(Duration::ZERO)` and reschedules from `next_poll_in`.
A deadline task queues one main-thread wakeup and waits for it to finish before
queueing another. The native clock recheck and inactivity deadline continue to
run independently of renderer polling. Revocation closes the volatile session.
A queued open request that observes revocation returns Locked. A fresh request
can start another enrollment. Window destruction and app exit release the loop.

## IPC boundary

The generated Tauri application-command manifest and
[capability](../crates/codeward-host-macos/capabilities/desktop-pilot.json) allow
the existing synthetic commands plus enrollment, unlock and credential operations for the local
`main` WebView:

| Command | Input | Output |
| --- | --- | --- |
| `begin_vault_setup` | Exactly password and confirmation strings | Epoch and safe opening state |
| `get_vault_setup_state` | Empty body | Epoch, state, fixed error category |
| `get_setup_recovery_code` | Current epoch | Targeted temporary recovery code |
| `confirm_vault_setup` | Current epoch and paper transcription | Safe saving state; completion is polled |
| `cancel_vault_setup` | Current epoch | Safe state after revocation |
| `open_managed_vault` | Empty body | Epoch and read-only opening/locked/missing state |
| `unlock_managed_vault` | Current epoch, observed revocation counter and master password | Authenticating state, then polled native result |
| `lock_managed_vault` | Current epoch | Locked state; no wait for a running KDF |
| `list_credentials` | Epoch, revocation counter, optional opaque cursor | Up to 100 title/username summaries, decimal-string generation/revision, next cursor |
| `get_credential_details` | Epoch, revocation, generation, revision and selected ID | One record's non-password fields and timestamps |
| `reveal_credential_password` | Same selected-record context | One temporary password and remaining display lifetime |
| `create_credential` | Epoch, revocation, generation, revision and complete form | Opaque operation reference and saving state |
| `update_credential` | Same form context plus selected ID | Opaque operation reference and saving state |
| `delete_credential` | Same selected-record context | Opaque operation reference and saving state |
| `get_credential_operation` | Epoch and operation reference | Saving, saved, failed, conflict, unknown or locked state |
| `set_desktop_dirty` | Epoch, revocation and dirty flag | Scoped native close-guard status |
| `resolve_desktop_close` | Epoch, revocation and explicit allow flag | Close confirmation status |
| `record_desktop_activity` | Epoch and revocation | Refresh inactivity for permitted user interaction |

Credential forms include title, username, URL, notes and explicit password intent:
`keep` preserves an existing password; `provided` supplies a replacement, including
an explicit empty value. Creation requires a provided password. Record IDs and
timestamps are assigned by Rust. Generation/revision values are canonical decimal
strings at the frontend boundary. Cursor tokens are scoped continuations rather
than renderer-chosen offsets. An uncertain operation blocks further mutation;
quit and restart the app to inspect authoritative state rather than automatically
replaying a save. Same-process reopening is blocked after an uncertain outcome. Success follows the complete core commit result.

Rust checks the trusted invoking window/WebView and bundled document. Enrollment
and credential commands are denied to the live-probe view, other windows and remote origins.
Input shapes reject extra keys and enforce core-compatible bounds before copying;
Tauri decodes JSON first, so this is not a predecode allocation limit. Rust owns
request identity, expected creation revision, native events, deadlines and outcomes.
The main-thread scheduler polls pending workers independently of renderer polling.
Secret replies are targeted, not broadcast; epoch and renderer sequence guards
reject stale deliveries after navigation/cancellation. A monotonic revocation
counter also clears typed/revealed inputs when a lock event arrives while the
form is already in the locked state. Unlock requests must match that observed
counter after Rust checks pending events; a password submitted before a lock
cannot start new authentication after that lock. Revocation during opening
requires explicit reopening rather than offering an unusable password form.
Authentication returns status only; vault
keys and decrypted snapshots remain in Rust. Error/status payloads never
include user inputs or native paths.

Navigation and new windows remain denied. CSP admits only bundled scripts/styles
and local IPC. No plugin/event permissions, clipboard commands, browser persistence
or network calls are added. The storage adapter and zeroize reuse existing pinned
workspace dependencies; no registry version or vault format changes.

### Connected setup verification

```sh
cargo test -p codeward-host-macos --all-features --locked --offline
cargo clippy -p codeward-host-macos --all-targets --all-features --locked --offline -- -D warnings
npm test --prefix crates/codeward-host-macos/frontend
node --test crates/codeward-host-macos/tests/desktop-redaction.cjs
```

The setup bridge tests use a synthetic in-memory storage model with real core
cryptography. They cover confirmation/no early readiness, exact encrypted reopening,
invalid codes, existing-byte preservation, stale epochs, event revocation, bounded
input, definite/unknown failure and cancellation on both sides of publication
admission. Unlock tests cover actual core authentication/list authorization,
wrong-password retry, explicit/native locks, stale epochs, command-boundary
revocation and canceled KDF completion, plus missing/corrupt no-write behavior. IPC tests exercise generated ACLs/origin checks and invalid bodies.
Local browser checks use mocked IPC for recovery/transcription/retry, distinct
committed/unknown/existing results, delayed begin/code cancellation, secret clearing
and dark/light desktop/narrow layouts. Unlock browser checks likewise use mocked
IPC for retries, lock events, stale responses and responsive layout; dependency-free
React interaction regressions run alongside the separate probe redaction checks
in CI. Those browser checks do not establish a
live end-to-end native save or new live sleep/redaction evidence. Existing native
APFS recovery tests remain the native writer evidence; run the actual desktop flow
with synthetic inputs for installed-app acceptance.

## Pinned dependencies

The frontend's `package.json` pins React/React DOM and development tools, while
`package-lock.json` records the complete npm graph. Use `npm ci` rather than an
unlocked install when reproducing checks. React executes inside the existing
Tauri WebView; the migration does not replace Tauri or move vault ownership out
of Rust. Vite and the test tools are build-time dependencies and are not runtime
network services.


The macOS-only feature uses [Tauri 2.12.5](https://docs.rs/crate/tauri/2.12.5)
with `custom-protocol` and `wry`, Tauri Build 2.7.4 with `config-json`,
Tokio 1.54.1 with `sync` and `time`, and Serde 1.0.229 with `derive` and
`std`. Direct dependencies disable default features. The separate
`desktop-ipc-tests` feature enables Tauri's mock runtime.
`Cargo.lock` pins 428 registry package versions, adding 389 while retaining
the previous 39. The Apple silicon normal/build feature graph contains 235
locked registry versions. The default workspace build omits the pilot.

Compared the locked package versions and selected feature graph against
[RustSec snapshot `28d73fc`](https://github.com/RustSec/advisory-db/tree/28d73fc95e5972af3403225a217def57dcaba3c4).
The version check found no affected entries in the selected macOS graph. The
full lock includes inactive Linux dependencies `glib 0.18.5`
([unsoundness](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)) and
`proc-macro-error 1.0.4`
([unmaintained](https://rustsec.org/advisories/RUSTSEC-2024-0370.html)).
Recheck the graph before enabling another target. Tauri 2.12.5 is outside
the affected ranges of the published
[origin](https://github.com/tauri-apps/tauri/security/advisories/GHSA-7gmj-67g7-phm9),
[channel-fetch](https://github.com/tauri-apps/tauri/security/advisories/GHSA-w28w-mhc8-qvjv),
and [iframe](https://github.com/tauri-apps/tauri/security/advisories/GHSA-57fm-592m-34r7)
IPC advisories. Permission and command choices follow the official
[capability](https://v2.tauri.app/security/capabilities/) and
[command](https://v2.tauri.app/develop/calling-rust/) references.

## Verification

```sh
cargo test -p codeward-host-macos --all-features --locked --offline
cargo tree -p codeward-host-macos --features desktop-pilot --target aarch64-apple-darwin --locked --offline -e features
```

The host tests cover pending open, explicit lock, source failure, and revocation
before a repeated open. Tauri's mock runtime uses the generated ACL and
bundled context to exercise permitted calls and refuse other views, nonlocal
origins, nonempty payloads, and absent permissions. The existing native smoke
exercises process-local macOS event delivery separately.

### Packaged live probes

Build an ad-hoc-signed local `.app` with Node/npm and the pinned Rust toolchain.
The script installs locked npm dependencies, builds the React assets, then builds
Rust with cached Cargo dependencies. npm installation may require network access;
that build-time access is separate from the offline installed-app requirement.
Build output goes to stderr; stdout contains only the app path. The script
verifies the bundle signature.
The WebView assets are embedded in the release executable.
[Apple bundle layout](https://developer.apple.com/documentation/bundleresources/placing-content-in-a-bundle)

```sh
app=$(sh crates/codeward-host-macos/tests/package-desktop-pilot.sh)
"$app/Contents/MacOS/codeward-desktop-pilot" --live-lock
"$app/Contents/MacOS/codeward-desktop-pilot" --live-sleep
npm test --prefix crates/codeward-host-macos/frontend
node --test crates/codeward-host-macos/tests/desktop-redaction.cjs
```

After `ARMED Lock`, the operator locks and unlocks the Mac. After
`ARMED Sleep`, the operator sleeps and wakes it. Run each probe in a logged-in
session. The probe exits after a verified `PASS`, a failure, or a 120-second
awake timeout. An interrupted run needs to be repeated.

The probe uses a separate local WebView and capability. The normal view uses the scoped enrollment, unlock and credential commands.
The probe view can read cached status and
acknowledge the initial and redacted renderings. A pass requires that the
WebView first report visible synthetic preview and draft values, the desktop
loop revoke on `OsSessionLocked` or `Suspend`, and the matching lock or sleep
pair be observed. Sleep can follow an automatic screen lock. In that case the
probe keeps the session locked, requires both `Suspend` and `Resume`, and
reports which event revoked access first. The renderer must report both fields
emptied and hidden. Renderer polling does not drive the native loop. The probe
logs only status and event names.

When the start and return notices coalesce, the log identifies the batch.
A coalesced batch does not prove arrival order.
`Reconnected` does not uniquely identify the raw notification. The Mach
gap fallback remains separately unit-tested. This local bundle has no
distribution signing or notarization. The probe exercises synthetic DOM
redaction, with product credential fields and OS memory outside its evidence.
