# Rust Password Generator Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task-by-task, then request one independent whole-branch review.

**Goal:** Implement ADR-0005 password generation as a portable, unsaved Rust primitive.

**Architecture:** A standalone `password` module validates preferences, builds the
selected ASCII alphabet and uses OS byte rejection sampling. A dedicated password
owner exposes borrowed plaintext and clears its owned buffer on drop.

**Tech Stack:** Rust 1.99.1; existing pinned getrandom and zeroize dependencies.

**Spec:** [Approved generator design](../specs/2026-11-16-password-generator-design.md).
Owner approval: conversation on 2026-11-16.

## Global Constraints

- Default length 24; accepted lengths 12 through 128 inclusive.
- Lowercase, uppercase, digits, exactly `!@#$%^&*()-_=+[]{}:,.?`; all enabled by default.
- Uniform sampling over the selected union; no forced class membership.
- No dependencies, format, crypto profile, storage, clipboard or UI changes.
- No session state, draft references, inactivity refresh or automatic saving.
- No Debug, Clone, Copy or serialization on generated passwords; no partial output on failure.

## Review Focus

- Byte rejection boundaries must not bias any alphabet (exhaust all byte values).
- Invalid lengths/empty classes must fail before entropy use (panic-on-call entropy test).
- Entropy failure after partial output must return only a controlled error (refill failure test).
- Class unions must contain each permitted byte exactly once (all 15 masks tested).
- Output ownership must not accidentally permit diagnostics or copying (negative doctests).

### Task 1: Bounded generator and reader documentation

**Files:** Create `crates/codeward-core/src/password.rs`,
`crates/codeward-core/src/password/tests.rs`, `docs/password-generation.md`;
modify `crates/codeward-core/src/lib.rs`, core README, root README and `tldr.md`.

**Interfaces:** `PasswordOptions { length: usize, lowercase: bool, uppercase: bool,
digits: bool, symbols: bool }`, `Default` as specified above;
`generate_password(PasswordOptions) -> Result<GeneratedPassword, GenerationError>`;
`GeneratedPassword::as_str(&self) -> &str`. Export length constants and safe errors
`InvalidOptions`, `RandomnessUnavailable`, `AllocationFailed`. Keep entropy
injection private in `generate_with(options, fill)`; no production configurable RNG.

- [x] Write tests first for default and every valid length/class union; exact
  symbols; invalid preferences before entropy; all byte preimages for every union;
  rejected-byte refill; entropy failure at start and after partial output; real OS
  entry point. Compile-fail doctests deny Debug, Clone, Copy and serialization.
- [x] Run `cargo test -p codeward-core --lib --locked --offline password::tests`.
  Expected initially: missing generator interfaces, then all scenarios pass.
- [x] Implement minimal bounded alphabet construction, fallible zeroizing String
  allocation, private entropy sampling and explicit password accessor. The maximum
  alphabet is 84 distinct ASCII bytes; use a widened 256-based rejection limit.
- [x] Document API usage without printing passwords, sampling, failure behavior,
  plaintext lifetime limits and deferred session/UI integration; update reader maps.
- [x] Run focused tests and `cargo test -p codeward-core --doc --locked --offline password`;
  expected: all generator tests and four negative doctests pass.
- [x] Run default workspace check, workspace all-feature tests, rustfmt,
  all-target/all-feature Clippy with warnings denied and `git diff --check`.
  Expected: all checks pass; record native/manual ignored-test limits.
- [x] Commit explicit paths, request independent branch review, fix material
  findings with failing regressions, and leave the branch reviewable without merge.

## Execution notes

Use the existing clean `feat/password-generator` branch based on merged PR #56.
No extra workspace is needed for this one-module change. Existing toolchain is at
`/private/tmp/codeward-cargo/bin`, with CARGO_HOME `/private/tmp/codeward-cargo`
and RUSTUP_HOME `/private/tmp/codeward-rustup`; do not change repository config.
Baseline: ten existing crypto tests passed on 2026-11-16.
