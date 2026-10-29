# Threat model

Status: initial design model; no implemented or audited security claims.
Scope: one person per local vault; installed macOS/Windows/Linux desktop targets,
Rust core, separate UI responsibility, handwritten recovery secret. Private beta
audience size does not change the per-vault threat model.

## Assets and boundaries

| Asset | Security concern |
| --- | --- |
| Credentials and sensitive metadata | Confidentiality/integrity at rest and during use |
| Master password and recovery secret | Unauthorized unlock, offline guessing, UI/log disclosure |
| Vault data key and two protected key slots | Either route grants access; exposed data keys can affect later contents encrypted with the same key |
| Last committed vault and access metadata | Complete, coherent persistence across failure |
| Paper recovery copy | Theft, transcription errors, loss, co-location with the computer |
| Session memory and displayed contents | Another local user, screen capture, malware, OS facilities |

Boundaries: person/React UI, WebView/Tauri IPC adapter, adapter/Rust core,
core/filesystem, core/OS session,
and person/physical notebook. Treat UI requests and file input as untrusted until validated. Assume
OS/hardware behave as documented; same-user malware or privileged compromise can
violate this assumption. There is no initial runtime network dependency.

## Threats and intended response

| ID | Scenario | Initial response / requirement | Residual limit and later evidence |
| --- | --- | --- | --- |
| T-01 | Locked computer or copied vault is stolen. | Protect contents with one random data key; protect its copies through independent master-password/recovery slots; no persisted plaintext secrets/keys (FR-01/02, NFR-02/05). | Attackers can target either slot offline. Password derivation costs and recovery-secret entropy need review in ADR-0002/0004; an unlocked stolen device can expose active secrets. |
| T-02 | Attacker obtains notebook and vault. | Treat the reusable paper secret as an independent recovery-slot credential; separate it physically and confirm transcription in Rust (FR-06/07). | Valid paper secret plus usable matching file grants access without the master password. Reuse is intentional; entropy, explicit replacement and compromise handling remain in ADR-0004. No remote revocation. |
| T-03 | User forgets master password. | Offline recovery and replacement password against intact vault (FR-07). | Loss of both valid unlock paths can make contents inaccessible. No operator override. |
| T-04 | Vault deleted, only computer lost, or only file corrupted. | Explain file-recovery distinction; preserve input on error (FR-09/10). | Paper secret cannot restore data. Backup/restore features are deferred; this availability threat remains unresolved initially. |
| T-05 | App terminates during creation/save. | Complete committed states, acknowledgement after commit, coherent recovery metadata (FR-01/05, NFR-03/04). | Drafts may be lost. ADR-0003 defines power-loss boundary and fault matrix. |
| T-06 | Capacity, permissions, or I/O fail. | Preserve prior commit; report failure without false success (NFR-04). | No progress guarantee on unavailable storage. Test each failure stage. |
| T-07 | Malicious or damaged input is opened. | Validate format/integrity before exposure; no destructive repair (FR-09, NFR-02). | Parser defects remain possible. Negative tests/security review follow format selection. |
| T-08 | Older authentic copy replaces latest vault, or another writer overwrites it. | Prevent silent concurrent overwrites (NFR-08); do not claim integrity detects authentic old copies. | Rollback detection across lost local state is unresolved. Future backup/sync versioning needs review. |
| T-09 | Malware/keylogger/debugger/privileged process observes use. | Minimize secret lifetime, avoid logging, lock on chosen triggers (FR-08, NFR-05). | Initial release cannot defeat a compromised OS or guarantee active-secret protection. Advanced defenses deferred, risk retained. |
| T-10 | Diagnostics/temp files/display/OS memory leak secrets. | Review persistence, diagnostics, UI exposure and lifetime (NFR-02/05/07). | Swap, dumps, screenshots, memory remnants depend on stack/platform; no absolute erasure claim. Review clipboard risk if copying is added. |
| T-11 | Website password changed, then app crashes before local save. | Distinguish generated/draft/saved (FR-04/05). | Website updates are outside local transaction. Unsaved generation excluded; no browser recovery promise. |
| T-12 | Faulty or compromised UI invokes privileged actions or retains plaintext. | Scope custom Tauri commands to intended callers; Rust validates session, vault, payload and revision; minimize returned data; avoid UI file/key access (NFR-09). | Compromised unlocked UI can invoke permitted reveals and observe displayed/entered plaintext. IPC permissions are not proof of human intent, and clearing JavaScript references cannot prove erasure. |
| T-13 | Late secret-bearing reply or notification restores data after locking or switching vaults. | Core-owned session generations, scoped references, stale-reply rejection and status-only notifications under NFR-09. | Previously exposed plaintext cannot be revoked; verify lock/in-flight-commit coordination and UI redaction before relying on it. |
| T-14 | Attacker obtains a vault key using an old file and an exposed old unlock secret. | Distinguish replacing a protected slot from rotating the data key; design compromise handling in ADR-0002/0004 and coherent writes in ADR-0003. | Old files cannot be revoked. Updating only a slot does not protect later ciphertext under the already-exposed data key; complete rotation behavior is still open. |

## Assumptions and follow-through

The person can retain a legible paper copy separately; the app cannot verify its
ongoing physical security. Losing recovery media does not authorize transmitting
secrets to an operator or service.

Vault encryption does not prevent deletion, hardware loss, or unlocked-session
observation. Device login and full-disk encryption may add protection, but are not
substitutes for vault security or guarantees made by Codeward.

Before implementation, ADR-0002/0004 must describe key/access relationships,
offline guessing through both paths, exposed metadata, and reviewed security
components without inventing cryptography. ADR-0003 must define failure behavior.
Revisit this model for accepted ADRs and before adding services, extensions,
imports, backups, or other integrations.

For each declared platform, review file permissions, local-storage durability,
session events, memory/dump limits, and installer/runtime dependencies. No platform
security behavior is assumed identical solely because the core is shared Rust.

The [UI/core contract](ui-core-contract.md) also requires bundled assets, restrictive
content loading, no remote runtime origins, and no broad filesystem/shell access
from React. Custom-command permissions must be deliberately configured and verified;
framework defaults are not evidence of a restricted application command surface.
