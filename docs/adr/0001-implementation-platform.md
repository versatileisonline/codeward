# ADR-0001: Implementation platform

Status: Proposed — confirmed decisions recorded; UI and support details open.
Owner decisions: 2026-10-28, in the Codeward design conversation. Review PR: pending.
Target: Milestone 1. This records decisions, not a language-comparison essay.

## Confirmed decisions

| Area | Decision |
| --- | --- |
| Implementation language | Rust for the application core; the owner also wants to learn Rust. |
| Delivery | Installed desktop application with its own window and bundled interface assets. Users do not start a localhost server or open a browser tab. |
| Runtime | Offline, local vault, no mandatory accounts, telemetry, or hosted service. |
| Platforms | Target macOS, Windows, and Linux. Supported versions, architectures, and Linux distributions still need definition and validation. |
| Separation | UI sends limited commands to the Rust core; the core controls vault access, sessions, recovery, and persistence. The encrypted vault lives locally. |
| Audience | Private prototype for roughly 20 friends/family at most; each person uses a local vault. Public distribution and shared vaults are outside initial scope. |

The Rust/C++ comparison stays in chat at the owner's request. Language selection
alone does not complete this platform ADR or authorize application implementation.

## UI recommendation still awaiting a decision

Tauri with a TypeScript/React interface is a candidate, not an accepted stack.
It could reuse existing frontend experience while keeping core logic in Rust.
Tauri uses an embedded WebView and supports bundling frontend assets; that can
provide the intended installed, offline experience. See its
[process model](https://v2.tauri.app/concept/process-model/) and
[bundled-assets configuration](https://v2.tauri.app/reference/config/#frontenddist).
No prototype, dependency assessment, or cross-platform UI verification has been done.

## Remaining platform decisions

- Choose the UI framework and define the UI/core command and secret-exposure boundary.
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

The owner has selected Rust and the three desktop platform families. Keep the
whole ADR Proposed until the remaining platform/UI choices and evidence have
received owner review. Record acceptance and its review link/date when available.
No app code, dependencies, cryptographic algorithms, or build setup are added here.
