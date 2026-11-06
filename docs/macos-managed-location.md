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
