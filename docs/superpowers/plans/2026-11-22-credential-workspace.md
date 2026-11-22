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

- [x] Migrate desktop screens to React/TypeScript and a locked Vite build; preserve
  separate session-probe pages and restrictive production CSP.
- [x] Add bounded, scoped desktop commands for paginated summaries, selected
  non-password details and explicit password reveal, with stale-context guards.
- [x] Build a searchable two-pane list/detail workspace with empty/loading/error
  states and a stacked narrow-window layout using shared UI styles.
- [x] Add desktop create/update/delete commands and operation status; keep
  passwords unchanged unless explicit replacement is selected.
- [x] Add create/edit/cancel and confirmed deletion with pending, saved, failed,
  conflict and uncertain feedback. Refresh only authoritative committed data.
- [x] Complete dirty navigation/close handling and security redaction. Save,
  Discard and Cancel apply to ordinary navigation; security locks clear secrets
  immediately and stale replies cannot restore them.
- [x] Integrate frontend checks/build before native CI and packaging; update
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

## Verification record

- Local pinned Rust 1.99.1 workspace all-feature tests and doctests passed on
  macOS, including 181 core tests and 21 desktop bridge tests. Platform fixtures
  explicitly marked ignored retain their existing limits.
- Default-feature workspace check, rustfmt and all-target/all-feature workspace
  Clippy with warnings denied passed. Native desktop binary build passed.
- React typecheck, production build and 23 screen interaction regressions passed
  before adding the separately tested development-only browser demo.
- Independent code review found no remaining Critical or Important findings.
- Playwright checked both themes, desktop 1280×900 and narrow 430×932 layouts,
  editing/dirty navigation and locking without console errors or overflow.
- Installed-app end-to-end save/restart, live OS lock/sleep, VoiceOver and native
  Windows/Linux desktop acceptance remain unexecuted; synthetic data only.
- Owner subsequently installed the project oil-ui skill and requested a local
  browser demo. Its review and demo are follow-up commits on this same branch.

### Browser demo and oil-ui follow-up

- Installed project-local `oil-ui` at owner request and applied its screenshot
  and independent visual-review workflow. Preserved the agreed visual direction.
- Development-only `?demo=1` preview supports synthetic in-memory CRUD, reveal
  and password-preserving edits; reload resets it. Production bundle inspection
  confirms demo code, sample passwords and banner are excluded.
- Final frontend typecheck/build and 27 React/demo tests passed. Final native
  binary rebuild passed after bundling the final frontend assets.
- Oil screenshot checks at 1440×900 and 390×844 for workspace/editor report no
  issues or lint. Labels, placeholder contrast, narrow inputs/actions and local
  selection indicators were refined from review. Vite preview runs on localhost
  port 4173 for owner feedback; it is not a hosted deployment.
