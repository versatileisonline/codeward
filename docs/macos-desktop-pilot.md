# macOS synthetic desktop pilot

The optional `desktop-pilot` feature in
[`codeward-host-macos`](../crates/codeward-host-macos/Cargo.toml) builds a Tauri
window with bundled status controls for a volatile synthetic enrollment. Its
adapter can only observe an absent destination. It exposes no credential or
paper-code display, vault paths, or persistent writes. It exercises FR-02 and
the control-loop and permission foundations of NFR-09 under ADR-0001/0005.

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
three commands for the single local `main` WebView:
`open_synthetic_session`, `get_session_state`, and `lock_vault`.
Rust checks the trusted invoking window and WebView and their current bundled
document. Commands take no arguments and return only a status or a fixed
error. The request guard rejects every nonempty raw body and every JSON
value other than an empty object. Tauri decodes its JSON transport before the
command guard. This pilot establishes no predecode bound for future payloads.

The window denies external navigation and additional windows. Its policy
allows bundled scripts and local IPC connections. The capability grants no
plugin or core event permissions. The bundled page polls status and ignores
older replies after a newer command. No secret-bearing replies cross this
boundary.

## Pinned dependencies

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

Build an ad-hoc-signed local `.app` with the pinned toolchain and cached
dependencies. The script prints its path and verifies the bundle signature.
The WebView assets are embedded in the release executable.
[Apple bundle layout](https://developer.apple.com/documentation/bundleresources/placing-content-in-a-bundle)

```sh
app=$(sh crates/codeward-host-macos/tests/package-desktop-pilot.sh)
"$app/Contents/MacOS/codeward-desktop-pilot" --live-lock
"$app/Contents/MacOS/codeward-desktop-pilot" --live-sleep
node --test crates/codeward-host-macos/tests/desktop-redaction.cjs
```

After `ARMED Lock`, the operator locks and unlocks the Mac. After
`ARMED Sleep`, the operator sleeps and wakes it. Run each probe in a logged-in
session. The probe exits after a verified `PASS`, a failure, or a 120-second
awake timeout. An interrupted run needs to be repeated.

The probe uses a separate local WebView and capability. The normal view has
its original three commands. The probe view can read cached status and
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
