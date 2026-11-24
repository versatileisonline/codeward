# Everyday desktop workflow

This increment builds on desktop recovery (PR #62, base commit
`63489bc18b68c3638b79693e55fb43a0d8e18797`). It keeps the single managed vault,
landing page, recovery workflow, encryption and inactivity policies.

## Whole-vault search

`search_credentials` accepts the current epoch/revocation, query and optional
opaque cursor. Rust searches the authenticated committed snapshot and returns
bounded pages of title/username summaries with the existing scoped page shape.
Queries are limited to 4,096 UTF-8 bytes and use Rust lowercase substring matching
separately within each title and username; there is no trimming, fuzzy matching,
notes/password search, or field-boundary matching. An empty query lists normally.
Cursors are bound to query, caller, session generation and committed revision.
Restricted recovery, locked, expired and pending/uncertain write states cannot
search. No full snapshot is returned to React.

The desktop debounces typing by 200 ms, rejects superseded replies, and separates
loading, failed search and no matches. Changing the query preserves an open draft;
selecting another record still uses Save/Discard/Cancel protection. Load more
continues only the current query. Clearing search returns to ordinary listing.

## Editing continuity

Confirmed create/edit results can carry `credential_id`, the Rust-assigned affected
record identity. It is available only after confirmed commit and successful
snapshot adoption; pending/failure/unknown/deletion and failed-refresh results do
not expose an identity as usable. The UI refreshes the current results and reads
that record's details through the refreshed scope, including when it does not
match the active query. Duplicate titles are never used to infer identity.

A definite failure preserves the draft. Uncertain/conflicting outcomes retain
existing blocking/reopen guidance. Deletion clears selected details and refreshes
the active query. Generation and save remain separate explicit actions.

## Generator and keyboard

Typed password entry is the initial state. Generate password uses the existing
24-character defaults; Options reveals length and character-class controls.
Changes to options apply only on explicit generation. Reveal, regenerate and
return to typed input retain existing scoped draft lifetimes. An empty length
stays blank while editing, becomes zero on blur, and clears on refocus.

Cmd+F focuses search, Cmd+N starts a credential through the dirty guard, and Cmd+S
saves an active editor. Escape hides revealed passwords or cancels the active
dialog without silently discarding edits. New forms focus Title and dialogs
retain focus trapping/restoration.

## Presentation and validation

The workflow pass precedes visual polish. The existing two-pane layout, pure black
rail, near-black content and warm light palette remain. Controls share consistent
spacing, radii, focus rings and stable hover states; destructive actions remain
separate. Lock uses an outlined lock icon.

Unit/bridge fixtures validate search, identity, save continuity, stale replies and
keyboard behavior. Product previews and UI interactions use the native desktop
application, with frontend assets rebuilt into its executable and local app bundle.
Full installed-app acceptance and real-credential admission remain separate gates.
Clipboard integration, sorting and additional vaults are outside this increment.
