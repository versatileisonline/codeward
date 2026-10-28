# ADR-0005: Sessions, locking and drafts

Status: Placeholder — undecided. Decision: none. Target: Milestone 1.

## Context and questions

Decide inactivity timeout, macOS session events, manual lock/close behavior,
secret lifetime, draft handling and accessible save feedback. No timeout, toolkit,
autosave behavior or draft-retention policy is selected.

## Evaluation needed

Specify dirty-edit behavior on lock/close and handling of in-flight credential/
access-metadata commits. Hide secrets/end access without waiting indefinitely for
an edit. Do not silently save drafts. Explain retry/discard and ambiguous commits.

Define generation controls and any reveal/copy interaction. If clipboard use is
added, assess exposure/clearing limits. Review OS dumps, screenshots, memory and
assistive-technology limits.

## Acceptance gate

Provide state/interaction examples and tests for FR-04/05/08 and NFR-05/07.
Distinguish generated, saving, saved and failed/uncertain states. Uncommitted
passwords/edits must never acquire a durability promise.
