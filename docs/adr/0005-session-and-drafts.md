# ADR-0005: Sessions, locking and drafts

Status: Placeholder — undecided. Decision: none. Target: Milestone 1.

## Context and questions

Decide inactivity timeout, macOS/Windows/Linux session events, manual lock/close behavior,
secret lifetime, draft handling and accessible save feedback. Tauri/React is selected
in ADR-0001; no timeout, autosave behavior or draft-retention policy is selected.

## Evaluation needed

Specify dirty-edit behavior on lock/close and handling of in-flight credential/
access-metadata commits. Hide secrets/end access without waiting indefinitely for
an edit. Do not silently save drafts. Explain retry/discard and ambiguous commits.

Apply the [UI/core contract](../ui-core-contract.md): revoke access in the Rust core,
reject stale session replies, redact UI data, minimize secret-bearing payloads and
resolve in-flight commits coherently. Define exact session-generation and dirty-draft
policy without promising complete JavaScript memory erasure or retained unsaved data.

Define generation controls and any reveal/copy interaction. If clipboard use is
added, assess exposure/clearing limits. Review OS dumps, screenshots, memory and
assistive-technology limits.

## Acceptance gate

Provide state/interaction examples and tests for FR-04/05/08 and NFR-05/07.
Distinguish generated, saving, saved and failed/uncertain states. Uncommitted
passwords/edits must never acquire a durability promise.
