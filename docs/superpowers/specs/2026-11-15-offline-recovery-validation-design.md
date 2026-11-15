# Offline recovery validation

The owner authorized Milestone 3 on 2026-11-15 after accepting the storage
foundation and choosing macOS for the first polished demo. This slice verifies
FR-06/07/10 through the existing Rust headless host and managed APFS writer.
The credential UI and installed-app offline validation follow in Milestone 4.

## Approach

Reuse the existing recovery/session/transaction APIs and the thin native
fault wrapper. Add focused acceptance tests rather than another recovery
controller or a change to the vault format. Component tests alone would leave
host/native composition unverified; building UI now would exceed this gate.

## Outcomes

- Fresh enrollment requires matching transcription; readiness follows commit.
  An enrollment code cannot be redisplayed after creation, cancel or lock.
- Paper authentication grants restricted recovery, never credential browsing.
  A replacement password is complete only after confirmed persistence.
- Two successive recoveries reuse the same paper slot, preserve the complete
  credential snapshot and reject the preceding master password on the updated file.
- Invalid codes, mismatched passwords and canceled work leave authoritative
  bytes unchanged. Missing or tampered authority is preserved and never replaced
  by an empty vault or a leftover stage.
- Failed and uncertain password saves retain their distinct outcomes. Known
  request IDs do not blindly replay; uncertain outcomes require fresh proof
  and persistence barriers. Reconciliation does not unlock the session.
- Native acceptance scenarios execute with macOS network operations denied.
  A separately selected negative-control probe must observe kernel denial;
  ordinary workspace tests do not assume networking is disabled.
- Written recovery guidance covers physical separation, invalid transcription,
  save failure/uncertainty and the inability to recreate missing/corrupt data.

## Constraints and review

Use synthetic data and exclusively created disposable homes. Retain the exact
Rust 1.99.1 toolchain, dependency pins, Argon2 profile, file format and default-off
writer boundary. Do not add a public arbitrary-path constructor, secret logging,
backup/restore behavior, UI commands or real-credential enablement.

This proves a bounded headless macOS recovery workflow, not an installed desktop
application or all-platform offline support. Existing application-termination
evidence remains bounded to its tested checkpoints. M3 completion and M4 entry
require owner acceptance of the resulting evidence and limitations.
