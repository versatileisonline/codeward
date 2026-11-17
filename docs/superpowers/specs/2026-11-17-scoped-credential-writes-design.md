# Scoped credential password saves and deletion

The owner requested continued implementation through a new PR on 2026-11-17.
Build on merged PR #57 with explicit application of a generated password draft
to an existing credential, plus explicit selected-record deletion. These complete
the next bounded Rust save path under FR-03/04/05/08 and ADR-0003/0005, without
introducing UI, record creation or a complete credential edit form.

## Approach

Extend the existing shared write preparation with targeted password replacement
and deletion. The session authorizes current references and copies only bounded
command inputs. The existing background preparation worker owns snapshot copying,
schema validation and encryption. The UI never supplies or receives a full snapshot.
Returning a whole snapshot to the renderer or performing unrevocable preparation
on the control thread would weaken existing boundaries.

## Behavior

- `Session::prepare_generated_password` / `HeadlessVault::save_generated_password`
  take request ID/expected revision, target record ID and the current draft ref.
  Require correct caller/opened scope, ordinary access, current generation/source,
  current draft identity and expected revision before retaining command inputs.
- `Session::prepare_delete_credential` / `HeadlessVault::delete_credential` take
  current generation, request ID/expected revision and selected record ID. A named
  delete operation expresses explicit intent; future UI must supply confirmation.
- Validate canonical IDs after authorization. Missing records fail during worker
  preparation without writes. Never insert or recreate a missing record.
- Replacement changes only the selected password and update timestamp. Preserve
  exact ID/title/username/URL/notes/creation time and every other record. Rust reads
  bounded Unix milliseconds and clamps updates to the prior updated timestamp if
  wall time moved backward. Unavailable/out-of-range time fails without a save.
- Deletion removes only the selected record. No undo or automatic recovery claim.
- Hash operation domain, expected revision and exact inputs into existing keyed
  request history. Deduplicate before clock use, snapshot copying or encryption.
  Current draft/state authorization still precedes command handling; after a
  successful commit invalidates a ref, use operation status rather than replaying
  a stale draft command. A reused ID with changed inputs cannot publish again.
- Retain one session-owned draft until lock/reload/discard or changed-snapshot
  refresh. A definite save failure preserves it for an explicit new-request retry;
  uncertain outcomes block access/mutations until existing reconciliation proves
  unchanged or committed bytes. A retry never invents success or rollback.
- Locks revoke draft/session access immediately. Queued/unadmitted saves cancel;
  admitted publication may finish while locked, returning status only. Polling,
  refresh, history bounds and native storage behavior retain existing contracts.
- No storage writes or inactivity refresh during command authorization. Only the
  normal commit protocol can produce a committed revision. Returned input copies
  remain zeroizing and owned by revocable worker preparation, not UI replies.

Keep dependencies, vault format, crypto profile, native adapters and Tauri surface
unchanged. General field editing, new-record IDs/creation, ordinary-close dialogs,
clipboard and installed UI remain separately scoped. No milestone advancement.

## Verification

Test password save and deletion through session and host, reopen actual committed
bytes, preserve every unaffected field/record, cap schema/timestamps, deny malformed
and missing IDs and stale/foreign/restricted/expired references, retain drafts on
preparation/storage failure, deduplicate known failed IDs and reject changed intent,
exercise uncertainty/reconciliation and locks during queued/admitted work. Confirm
no write/idle extension before admission. Run focused Rust tests/default/lint/docs,
independent review and final exact-head three-platform CI for the PR.
