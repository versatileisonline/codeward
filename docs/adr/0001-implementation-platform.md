# ADR-0001: Implementation platform

Status: Proposed — Rust/Tauri/React selected; platform evidence and support details open.
Owner decisions: 2026-10-28, in the Codeward design conversation. Review PR: pending.
Target: Milestone 1. This records decisions, not a language-comparison essay.

## Confirmed decisions

| Area | Decision |
| --- | --- |
| Implementation language | Rust for the application core; the owner also wants to learn Rust. |
| Desktop host and UI | Tauri with a React/TypeScript interface, selected by the owner on 2026-10-28. |
| Delivery | Installed desktop application with its own window and bundled interface assets. Users do not start a localhost server or open a browser tab. |
| Runtime | Offline, local vault, no mandatory accounts, telemetry, or hosted service. |
| Platforms | Target macOS, Windows, and Linux. Supported versions, architectures, and Linux distributions still need definition and validation. |
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
remaining policy choices still require review before implementation.
Exact dependency versions, frontend bundler, plugins, and packaging setup remain open.
This selection does not authorize scaffolding, dependency installation, or app code.

## Remaining platform decisions

- Finalize typed UI/core payloads, command permissions and secret-exposure policy.
- Define minimum OS versions, CPU architectures, and a bounded Linux support matrix.
- Evaluate installation/runtime prerequisites and per-platform packaging, signing,
  accessibility, session events, and local storage behavior. Runtime must work offline;
  development tooling and installation prerequisites are separate considerations.
- Establish representative vault sizes, performance targets, and feasibility checks.
- Coordinate security components with ADR-0002/0004 and persistence with ADR-0003.

Linux is a target requiring explicit validation, not a claim to support every
Linux distribution or desktop environment. Implement shared behavior first and
verify platform-specific adapters and installers against the declared matrix.

## Acceptance gate

The owner has selected Rust, Tauri, React/TypeScript and the three desktop platform
families. Keep the whole ADR Proposed until the remaining support/security details and evidence have
received owner review. Record acceptance and its review link/date when available.
No app code, dependencies, cryptographic algorithms, or build setup are added here.
