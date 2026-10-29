# ADR-0001: Implementation platform

Status: Accepted — design and validation plan; runtime evidence outstanding.
Owner decisions: 2026-10-28; concrete handoff approved 2026-10-29.
Acceptance PR: pending publication; approval recorded in the handoff.
Target: Milestone 1. This records decisions, not a language-comparison essay.

## Confirmed decisions

| Area | Decision |
| --- | --- |
| Implementation language | Rust for the application core; the owner also wants to learn Rust. |
| Desktop host and UI | Tauri with a React/TypeScript interface, selected by the owner on 2026-10-28. |
| Delivery | Installed desktop application with its own window and bundled interface assets. Users do not start a localhost server or open a browser tab. |
| Runtime | Offline, local vault, no mandatory accounts, telemetry, or hosted service. |
| Platforms | Target macOS, Windows, and Linux. The bounded matrix below defines initial validation targets; actual support requires evidence. |
| Separation | UI sends limited commands to the Rust core; the core controls vault access, sessions, recovery, and persistence. The encrypted vault lives locally. |
| Audience | Private prototype for roughly 20 friends/family at most; each person uses a local vault. Public distribution and shared vaults are outside initial scope. |

The Rust/C++ comparison stays in chat at the owner's request. Language selection
alone does not complete this platform ADR or authorize application implementation.

## Selected UI architecture

Tauri hosts the installed application and embedded system WebView. React/TypeScript
owns presentation; a thin Rust Tauri adapter translates limited IPC commands into
framework-independent core operations. The Rust core remains authoritative for
session state, vault access, recovery and committed writes. Interface assets are
bundled locally; no remote UI, user-started server, or browser tab is required.
See Tauri's
[process model](https://v2.tauri.app/concept/process-model/) and
[bundled-assets configuration](https://v2.tauri.app/reference/config/#frontenddist).
No prototype, dependency assessment, or cross-platform UI verification has been done.
The [UI/core contract](../ui-core-contract.md) defines intended commands, data
exposure, authorization, lock/reply behavior and save outcomes. Exact schemas and
the accepted session and persistence policies complete that boundary.
The proposal below names the bundler, plugin boundary and packaging targets;
exact dependency patches/features and installer configuration need review before use.
The [accepted handoff](../milestone-1-handoff.md) authorizes Milestone 2 core work;
exact dependency review precedes setup/use.

## Platform and boundary contract

Use the [platform validation plan](../platform-validation.md): macOS 26.3.1 on Apple
silicon/APFS, Windows 11 25H2 on x64/NTFS, and Ubuntu 24.04 LTS on x64/ext4/GNOME
(Wayland and X11). These are initial test targets, not verified support. Recommend
Tauri 2, React/TypeScript and Vite with bundled assets and no remote runtime content.
The plan records packaging/prerequisite paths, native persistence candidates and
smallest synthetic experiments; measurements and package verification follow
during authorized implementation. Review exact releases/features before setup.

Use one main application window initially. Explicitly allowlist each custom app
command through Tauri's application command permissions and limit capabilities to
that window; omit filesystem, shell, HTTP, clipboard, updater and other broad
plugins. Native file selection passes only a scoped reference to the core. Rust
still enforces the [UI/core contract](../ui-core-contract.md); caller identity
comes from Tauri, not UI fields. Permissions are not proof of human intent.

Mutation payloads carry a 32-character lowercase-hex request ID, expected global
vault revision, session generation and operation-specific fields. Encode revisions/generations as
decimal strings at the TypeScript boundary to avoid JavaScript integer rounding;
validate them as unsigned 64-bit values in Rust. Decimal strings contain 1–20
ASCII digits with no leading zeros, except the single value `0` where permitted.
Core-issued opaque context IDs encode 16 random bytes as 32 lowercase hex
characters and are bound to the core instance/caller/vault. They are identifiers,
not authorization tokens. Cap each command input at 512 KiB before decoding and
enforce the same field limits as the vault schema. List results are bounded to 100 summaries per request;
search/query text is bounded to 512 UTF-8 bytes. No bulk-vault/key response is allowed.
List cursors bind to the current revision/session and become invalid on mutation
or lock. Wire structs and error enums must express these rules and be tested before
any command is exposed; TypeScript definitions alone do not enforce them.

Linux is a target requiring explicit validation, not a claim to support every
Linux distribution or desktop environment. Implement shared behavior first and
verify platform-specific adapters and installers against the declared matrix.

## Acceptance gate

The owner accepted the concrete matrix, permission contract and evidence plan
through the [handoff](../milestone-1-handoff.md) on 2026-10-29. Do not advertise
support until native/packaged validation passes. Exact release/feature/advisory
review remains a prerequisite for using packages, not an unreviewed product policy.
