# Credential workspace implementation plan

Approved in conversation on 2026-11-22. Work proceeds on
`feat/credential-workspace`, with focused local commits and no merge/publication.

## Goal and architecture

Keep Tauri and the Rust vault core. Migrate the bundled frontend to React and
TypeScript while preserving the current dark/light design, setup, paper-code
confirmation and unlock behavior. Replace the unlocked placeholder with a
credential list and selected details/editor. Rust remains authoritative for
access, record identity/timestamps, revisions and complete commit outcomes.

## Tasks and commit boundaries

- [ ] Migrate desktop screens to React/TypeScript and a locked Vite build; preserve
  separate session-probe pages and restrictive production CSP.
- [ ] Add bounded, scoped desktop commands for paginated summaries, selected
  non-password details and explicit password reveal, with stale-context guards.
- [ ] Build a searchable two-pane list/detail workspace with empty/loading/error
  states and a stacked narrow-window layout using shared UI styles.
- [ ] Add desktop create/update/delete commands and operation status; keep
  passwords unchanged unless explicit replacement is selected.
- [ ] Add create/edit/cancel and confirmed deletion with pending, saved, failed,
  conflict and uncertain feedback. Refresh only authoritative committed data.
- [ ] Complete dirty navigation/close handling and security redaction. Save,
  Discard and Cancel apply to ordinary navigation; security locks clear secrets
  immediately and stale replies cannot restore them.
- [ ] Integrate frontend checks/build before native CI and packaging; update
  launch documentation and record verification and residual limits.

## Interfaces and constraints

Requests carry current session context and revisions/generations use decimal
strings at the TypeScript boundary. Return paginated summaries and one selected
record; password reveal is separate and deadline-limited. No complete decrypted
snapshot, arbitrary filesystem access, remote assets, browser persistence or
secret logging. Preserve the vault format, cryptographic profiles and fixed
managed storage location. Use only synthetic credentials.

Generation controls, clipboard integration, forgotten-password recovery UI,
Windows/Linux desktop integration and real-credential admission remain later
work. The current theme is the starting point; further visual corrections follow
owner feedback in focused commits.

## Verification

Run frontend type checks, production build and interaction regressions, including
setup/unlock cancellation and retry, dirty navigation, selected reads, mutations,
late replies, reveal expiry and lock redaction. Verify command authorization,
malformed/stale input and failed/uncertain saves in Rust. Run relevant native
checks plus workspace formatting, tests/doctests and warning-denied Clippy.
Inspect both themes and desktop/narrow layouts. Exercise native synthetic
create/view/edit/delete/reopen where possible; document unexecuted live OS event,
VoiceOver, offline-runtime and installed-app checks instead of claiming them.

## Progress

- Baseline: clean `main` at `059ce39`; 12 existing Node renderer/probe tests passed.
- Implementation is split between the scoped Rust bridge and frontend; delivery
  integration follows their final paths and scripts.
