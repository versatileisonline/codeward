# Rust password generation

`codeward_core::password` implements the local generator policy in
[ADR-0005](adr/0005-session-and-drafts.md) for FR-04. It is a standalone Rust
primitive; no session draft, Tauri command or interface is introduced.

## API and preferences

```rust
use codeward_core::password::{PasswordOptions, generate_password};

let password = generate_password(PasswordOptions {
    length: 24,
    lowercase: true,
    uppercase: true,
    digits: true,
    symbols: true,
})?;
// Borrow with password.as_str() only for an authorized preview or explicit save.
// Do not print, log or persist a generated password as application state.
Ok::<(), codeward_core::password::GenerationError>(())
```

`PasswordOptions::default()` selects 24 characters and all four classes. Lengths
from 12 to 128 inclusive are accepted. Classes are ASCII lowercase, ASCII uppercase,
digits and exactly `!@#$%^&*()-_=+[]{}:,.?` (22 symbols). At least one class must
be enabled; each available class already supplies at least two distinct characters.
The complete alphabet contains 84 distinct characters.

Every character is sampled independently from the selected union. Selecting a
class makes those characters available; it does not require one to appear. Website
password-policy compatibility is not promised.

## Randomness and errors

Only the existing pinned `getrandom` native OS provider supplies entropy. The
generator accepts byte values below the largest multiple of the alphabet size
that is at most 256, then takes their remainder as the character index. Each
character has the same number of accepted byte preimages. Values outside that
range are discarded and fresh bytes requested as needed. A widened threshold
preserves the valid value 256 for alphabets that divide it exactly.

Invalid length or an empty selection returns `GenerationError::InvalidOptions`
before requesting randomness. A failed bounded output reservation returns
`AllocationFailed`. Any entropy failure, including during refill after generating
part of a password, returns `RandomnessUnavailable` with no partial output and no
predictable fallback. Controlled errors carry no password or entropy bytes.

## Ownership and integration limits

`GeneratedPassword` owns a zeroizing string and only exposes plaintext through
the explicit borrowed `as_str()` accessor. Neither the password owner nor its
private field supports Debug, Clone, Copy or automatic serialization. Entropy
buffers and partial output are zeroizing and dropped on failure. The output
allocation is reserved before generation and never grows while holding plaintext.
Caller copies, temporary compiler/allocator copies, swap and OS dumps cannot be
guaranteed erased.

Generation does not read or change a vault, authorize a session, extend inactivity,
create a draft reference or save a credential. A later host operation must enforce
caller/session/draft scope and lock cleanup before exposing it to a renderer.
The [UI/core contract](ui-core-contract.md) and
[session/draft policy](adr/0005-session-and-drafts.md) still govern that integration.
No clipboard, dependency, format or crypto-profile change is included.

## Verification

Seven focused tests cover all 117 accepted lengths across all 15 nonempty class
combinations, exact character sets, default behavior, invalid preferences before
entropy, rejection/refill, entropy failure before/after partial generation and the
public OS-backed entry point. Exhausting all 256 byte values for every class union
checks equal accepted preimage counts and the exact rejected remainder without
relying on probabilistic distribution assertions. Deterministic entropy injection
is private; production callers cannot choose an alternate RNG.

Four compile-fail doctests reject diagnostics, cloning, copying and serialization;
a runnable usage doctest exercises the public entry point without printing secrets.

```sh
cargo test -p codeward-core --lib --locked --offline password::tests
cargo test -p codeward-core --doc --locked --offline password
```

Use the pinned Rust 1.99.1 toolchain and synthetic data. These checks establish
bounded generator behavior, not a completed generation/save UI, platform runtime
acceptance or real-credential readiness.
