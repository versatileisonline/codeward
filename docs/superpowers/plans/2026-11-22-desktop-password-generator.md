# Desktop password generator integration

Continue the synthetic macOS credential prototype with explicit Rust-generated
passwords in create/edit forms. Keep the current UI style, lock behavior, native
managed writer and complete commit acknowledgement. No clipboard, recovery UI,
vault format, cryptographic profile or real-credential admission changes.

## Implementation

- Extend scoped desktop transport to generate, reveal and discard one retained
  core password draft. Reference identity binds epoch/revocation/generation and
  revision; stale replies/discards cannot affect newer drafts.
- Add generated password intent to complete credential forms. Save by retained
  Rust draft reference, never by returning generated plaintext in Provided mode.
- Expose length 12–128 (default 24), lowercase/uppercase/digits/symbols enabled by
  default. Require at least one class. Classes are available characters, not a
  guarantee each class appears. Generation never saves or renews idle itself.
- Mask by default. Reveal only on explicit Show, clear on deadline/focus loss,
  navigation, lock or selection change; late replies cannot redisplay secrets.
- Preserve previous valid draft on generation/definite-save failure; require
  explicit retry. Uncertainty blocks access and mutation until restart/reopen.
- Keep current unsaved-change handling; scoped discard releases drafts on exit.
- Extend development-only demo transport with synthetic generator behavior.

## Verification and delivery

Test real-core generation, generated create/edit, invalid options, stale/replaced
references, scope/lock revocation and definite retry/unknown behavior. Test UI
options, masked preview/deadline and delayed replies, navigation and generated
form submission. Inspect actual rendered desktop/narrow flows. Run frontend
build/typecheck/tests and native host tests/Clippy, default workspace check,
formatting and full workspace tests before final handoff. Independent review
precedes draft PR; preserve all previous modular commits on this feature branch.
Draft PR includes the unmerged credential workspace, branding and generator work.

## Progress

- Baseline: clean feat/credential-workspace at 139d2d8, main 059ce39.
- Rust bridge and React editor implemented as separate focused changes.
