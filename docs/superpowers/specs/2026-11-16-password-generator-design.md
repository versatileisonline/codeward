# Rust password generator

The owner selected password generation as the next implementation on 2026-11-16.
This small portable Rust slice implements ADR-0005's generation policy for FR-04.
It prepares the macOS demo without advancing milestone acceptance.

## Approach and scope

Add a bounded generator to `codeward-core`, using its existing pinned `getrandom`
and `zeroize` dependencies. A standalone primitive fits the current core boundary;
adding session-owned draft references now would also require edit/save lifecycle
work, and implementing generation in the renderer would move secret generation
outside the accepted Rust boundary. Session drafts and UI wiring follow separately.

## Behavior

- Default length: 24 ASCII characters; accepted lengths: 12 through 128 inclusive.
- Select any nonempty combination of lowercase ASCII, uppercase ASCII, digits and
  exactly `!@#$%^&*()-_=+[]{}:,.?`. Default to all four classes (builder's choice).
- Sample uniformly from the union of the selected classes using rejection sampling
  over OS random bytes. Do not require a character from every selected class.
- Validate preferences before requesting randomness. Return controlled errors for
  invalid preferences, unavailable randomness or allocation failure. Never fall
  back to predictable randomness or return a partial password.
- Return a dedicated owned password type backed by a zeroizing buffer, with an
  explicit borrowed plaintext accessor and no Debug, Clone, Copy or serialization.
  Returned plaintext/caller copies cannot be guaranteed erased.
- Generation does not authenticate, refresh session inactivity, create a draft
  reference, touch storage or save a credential. The primitive holds no session
  state; a later scoped host operation must authorize generation/draft ownership.

No dependencies, cryptographic profiles, vault format, clipboard, screens, Tauri
commands or persistence behavior change. No real-vault readiness is claimed.

## Verification

Use deterministic private entropy injection to exhaustively check the rejection
threshold and equal accepted preimage counts for every supported class union.
Cover default and endpoint lengths, all class combinations, invalid preferences
before entropy use, exact punctuation, rejection/refill behavior, and entropy
failure before or after partial generation. Check public OS-backed generation and
compile-fail examples for accidental diagnostics/cloning/serialization. Run focused
tests, default workspace check, rustfmt and all-feature Clippy; final CI/review
records distinguish local evidence from hosted results.
