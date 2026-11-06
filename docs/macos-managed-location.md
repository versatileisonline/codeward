# macOS managed working-location preparation

Scope: the managed-location choice for the first prototype. Native preparation
does not create a committed vault, enable a runtime writer or establish a save
guarantee. The product policy and backup/portability boundaries are recorded in
[ADR-0006](adr/0006-managed-working-location.md).

## Native API review

The existing locked libc 0.2.191 supplies `getpwuid_r` and `mkdirat`; the installed
macOS 26.3.1 SDK supplies the additional `acl_get_tag_type` declaration. No new
package/version/feature, vault bytes or cryptographic profile is introduced.
The existing dependency/advisory snapshot is reused; no fresh advisory scan claimed.

The [account API](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/getpwuid_r.3.html)
returns its error number directly; success with no result is absence. The wrapper
uses one 64 KiB caller-owned buffer and refuses `ERANGE` rather than retrying
without a bound. It checks the result pointer, effective uid and home pointer/NUL
within that buffer before copying at most PATH_MAX OS bytes. Empty/relative homes
are refused; no UTF-8 assumption, HOME fallback or UI-supplied home is used.
The buffer bound does not bound account-service latency or certify network accounts.

Descriptor-relative `mkdirat` requests mode 0700 once. Errors preserve any existing
or possibly-created directory. Existing entries are reopened without following
links and validated; no permission/ACL repair or failure cleanup is authorized.

Managed ancestors may have deny-only ACLs, which cannot grant access beyond POSIX
permissions. The [tag API](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/acl_get_tag_type.3.html)
and SDK enum distinguish deny from every other tag. The
[pinned Darwin implementation](https://github.com/apple-oss-distributions/Libc/blob/71bbe350ab79eef58113991d817ccc6165061a64/posix1e/acl_entry.c)
supports FIRST=0/NEXT=-1, returns 0 per entry and -1/EINVAL at a valid ACL's end.
Enumeration is bounded by SDK ACL_MAX_ENTRIES=128 and frees its copied ACL once on
every exit. Any allow/unknown tag refuses; query/free errors fail closed. Private
Codeward/Vaults directories, vaults, stages and companions still require empty ACLs.

## Implemented boundary

`VaultPath::managed()` has no caller-supplied home/path argument. The private test
home provider is compiled only for tests. It appends the fixed layout from ADR-0006,
checks the complete path before native work, and walks it through retained no-follow
directory handles. Only missing fixed components after the existing account home
may be created. Each `mkdirat` is preceded by retained-chain and mount checks;
creation races reopen and validate the existing directory rather than replacing it.

Every managed ancestor must remain root/effective-user owned, POSIX non-writable
by others and without ACL grants. This refuses shared sticky writable ancestors.
Codeward/Vaults additionally require effective-user ownership, exact mode 0700
and empty ACLs. Their identities and rules are rechecked after each open and at
every existing lease verification point, including post-publication/barrier checks.
Deny-only ACL compatibility on ancestors does not relax private-file/directory rules.

Preparation preserves existing vaults, lock objects, unrelated files and transaction
residue. It leaves the vault absent on a fresh location; explicit enrollment and
the full native commit gate are still required. There is no automatic migration,
backup, restore, permission repair, deletion, plaintext export or runtime writer.

## Focused evidence

Synthetic homes on macOS 26.3.1 (25D2128), Apple silicon/local APFS, 2026-11-05:

- Account-buffer tests cover empty/relative/unterminated/oversized paths, uid
  mismatch, null/out-of-buffer pointers and non-UTF-8 OS bytes. Actual current-account
  lookup is exercised read-only; no real managed home tree is created by tests.
- Fresh/idempotent preparation preserves private directory identities; complete
  encrypted fixtures, unrelated files and staged residue remain unchanged. Competing
  preparations share the directory and existing OS writer exclusion.
- Fixed-component file/symlink collisions, exposed private modes, ACL grants and
  deny-only private ACLs refuse without repair. Inherited deny ACLs may pass ancestor
  checks but refuse new private directories, preserving their ACLs and residue.
- Substitutions after opening either private directory refuse without removing
  the displaced or replacement directory. A dedicated child with umask 0777 leaves
  a refused mode-000 directory; the implementation never chmods it to continue.
- Intermediate ancestor ACL/mode changes refuse reads, new stages, prepared
  publication and barriers. Mid-read changes return no bytes; post-rename changes
  retain `OutcomeUnknown`; final-barrier changes refuse present/absent success.
- All 87 native runtime functions pass, including the previous synthetic recovery,
  lifecycle and process-kill cases. Native all-target Clippy with warnings denied,
  workspace formatting and whitespace checks pass. Full workspace/doctest and
  three-platform evidence belongs to current-head/base hosted Rust CI.

Windows/Linux native managed resolution, packaged runtime integration and native
persistence acceptance remain pending. Account/mount/query failures have no HOME or
arbitrary-folder fallback. A managed standard path does not certify absence of
third-party sync, compromised-OS protection, or OS-crash/power-loss persistence.
