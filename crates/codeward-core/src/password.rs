//! Local password generation without session or storage effects.
//!
//! Generation alone neither authenticates nor saves a credential. Session/draft
//! authorization and cleanup of caller/UI plaintext copies belong to the host.

use std::{error::Error, fmt};

use zeroize::Zeroizing;

/// Smallest accepted generated-password length in ASCII characters.
pub const MIN_PASSWORD_LENGTH: usize = 12;
/// Default generated-password length in ASCII characters.
pub const DEFAULT_PASSWORD_LENGTH: usize = 24;
/// Largest accepted generated-password length in ASCII characters.
pub const MAX_PASSWORD_LENGTH: usize = 128;

const LOWERCASE: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const UPPERCASE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &[u8] = b"0123456789";
const SYMBOLS: &[u8] = b"!@#$%^&*()-_=+[]{}:,.?";
const ALPHABET_CAPACITY: usize = LOWERCASE.len() + UPPERCASE.len() + DIGITS.len() + SYMBOLS.len();

/// Generation preferences. At least one class must be enabled.
///
/// Selected classes form one alphabet; individual class membership is not forced.
/// Default preferences enable all four classes and request 24 characters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PasswordOptions {
    /// Requested ASCII length, from 12 through 128 inclusive.
    pub length: usize,
    /// Include ASCII `a` through `z`.
    pub lowercase: bool,
    /// Include ASCII `A` through `Z`.
    pub uppercase: bool,
    /// Include ASCII `0` through `9`.
    pub digits: bool,
    /// Include exactly `!@#$%^&*()-_=+[]{}:,.?`.
    pub symbols: bool,
}

impl Default for PasswordOptions {
    fn default() -> Self {
        Self {
            length: DEFAULT_PASSWORD_LENGTH,
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: true,
        }
    }
}

/// Controlled failures that never carry password or entropy bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerationError {
    /// The length is outside 12–128 or no character class is selected.
    InvalidOptions,
    /// Native OS randomness failed; no password or partial output is returned.
    RandomnessUnavailable,
    /// The bounded output allocation could not be reserved.
    AllocationFailed,
}

impl fmt::Display for GenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidOptions => "invalid password generation preferences",
            Self::RandomnessUnavailable => "secure randomness is unavailable",
            Self::AllocationFailed => "password generation memory is unavailable",
        })
    }
}

impl Error for GenerationError {}

/// Unsaved password whose owned buffer is cleared on drop.
///
/// Borrow plaintext explicitly using [`Self::as_str`]. Caller copies and all
/// allocator/OS memory copies cannot be guaranteed erased. This type deliberately
/// provides no diagnostics, cloning, copying or automatic serialization.
///
/// ```compile_fail
/// use codeward_core::password::{generate_password, PasswordOptions};
/// let password = generate_password(PasswordOptions::default()).unwrap();
/// let _ = format!("{password:?}");
/// ```
///
/// ```compile_fail
/// use codeward_core::password::{generate_password, PasswordOptions};
/// let password = generate_password(PasswordOptions::default()).unwrap();
/// let _ = password.clone();
/// ```
///
/// ```compile_fail
/// use codeward_core::password::GeneratedPassword;
/// fn requires_copy<T: Copy>() {}
/// requires_copy::<GeneratedPassword>();
/// ```
///
/// ```compile_fail
/// use codeward_core::password::{generate_password, PasswordOptions};
/// let password = generate_password(PasswordOptions::default()).unwrap();
/// let _ = serde_json::to_string(&password);
/// ```
pub struct GeneratedPassword(Zeroizing<String>);

impl GeneratedPassword {
    /// Explicitly borrow the plaintext. Avoid logs and persistent UI state.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Generate an unsaved password using only native OS randomness, without fallback.
///
/// Validate preferences before entropy use. Each character is sampled uniformly
/// from the selected alphabet using rejection sampling, without forcing any class.
/// No session, clock, filesystem or vault state is read or changed.
///
/// ```
/// use codeward_core::password::{generate_password, PasswordOptions};
/// let password = generate_password(PasswordOptions::default())?;
/// assert_eq!(password.as_str().len(), 24);
/// // Pass borrowed plaintext only to an explicitly authorized preview/save flow.
/// # Ok::<(), codeward_core::password::GenerationError>(())
/// ```
pub fn generate_password(options: PasswordOptions) -> Result<GeneratedPassword, GenerationError> {
    generate_with(options, |bytes| {
        getrandom::fill(bytes).map_err(|_| GenerationError::RandomnessUnavailable)
    })
}

fn generate_with(
    options: PasswordOptions,
    mut fill: impl FnMut(&mut [u8]) -> Result<(), GenerationError>,
) -> Result<GeneratedPassword, GenerationError> {
    if !(MIN_PASSWORD_LENGTH..=MAX_PASSWORD_LENGTH).contains(&options.length) {
        return Err(GenerationError::InvalidOptions);
    }
    let mut alphabet = [0u8; ALPHABET_CAPACITY];
    let mut count = 0;
    for (enabled, class) in [
        (options.lowercase, LOWERCASE),
        (options.uppercase, UPPERCASE),
        (options.digits, DIGITS),
        (options.symbols, SYMBOLS),
    ] {
        if enabled {
            alphabet[count..count + class.len()].copy_from_slice(class);
            count += class.len();
        }
    }
    if count < 2 {
        return Err(GenerationError::InvalidOptions);
    }

    let mut password = Zeroizing::new(String::new());
    password
        .try_reserve_exact(options.length)
        .map_err(|_| GenerationError::AllocationFailed)?;
    let mut entropy = Zeroizing::new([0u8; 64]);
    // Keep 256 representable, including alphabets that divide it exactly.
    let limit = 256 - 256 % count;
    loop {
        fill(entropy.as_mut())?;
        for &byte in entropy.iter() {
            let value = usize::from(byte);
            if value < limit {
                password.push(char::from(alphabet[value % count]));
                if password.len() == options.length {
                    return Ok(GeneratedPassword(password));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
