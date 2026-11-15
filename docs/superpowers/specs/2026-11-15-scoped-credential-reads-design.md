# Scoped selected-credential reads

The owner directed continued macOS demo work after the recovery PR merged.
The next small deliverable supplies the selected-record reads already specified
by the UI/core contract. It does not introduce screens, real-vault enablement or
an implicit milestone acceptance decision.

## Approach

Extend the existing session and headless host, alongside scoped credential
listing. Return a password-free detail payload and a separately requested password
payload. Reusing these boundaries preserves Rust authorization; exposing a complete
snapshot to the UI would unnecessarily disclose other records and passwords.

Both methods require the trusted caller/opened worker, current session generation,
expected committed revision and exact lowercase-hex record ID. Reject locked,
enrollment and recovery access, pending/uncertain writes, conflicts, stale snapshots,
stale selections and malformed/missing IDs without writing storage or extending idle.

## Result contract

- `CredentialDetails`: generation, revision, ID, title, username, URL, notes,
  creation/update timestamps. No password field.
- `PasswordReveal`: generation, revision, ID, password and a monotonic `expires_at`
  no later than 30 seconds from the request or the existing inactivity deadline.
- All owned text uses the existing fallible zeroizing copies. Neither result
  implements Debug, Clone, Copy or serialization. No keys or other records escape.
- Trusted hosts retain caller/opened-vault context and recheck generation/revision
  before delivering queued results. The metadata is correlation, not authorization.
- The deadline does not erase returned copies or create a background timer. The
  eventual interface must mask by default and clear reveals at deadline, lock,
  navigation and focus loss. No clipboard or UI command is added in this slice.

## Verification

Exercise exact field preservation and maximum bounds, missing/malformed IDs,
caller/opened-vault/generation/revision mismatch, recovery/enrollment denial,
pending/uncertain/conflicted writes, refresh/deletion, inactivity expiry and the
bounded reveal deadline. Add headless-host coverage and compile-fail examples for
accidental diagnostics, copying, serialization and password access through details.

Keep Rust 1.99.1, dependencies, vault format, crypto profile and storage features
unchanged. This portable core work prepares the macOS interface without claiming
installed UI behavior or broader desktop support.
