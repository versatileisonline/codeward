//! Version 1 paper-secret encoding and transcription checks, without a UI/workflow.
//!
//! A checksum catches transcription errors; it does not authenticate a vault.
//! Enrollment state, display permission, persistence and recovery commits belong
//! to future application code. Never persist the plaintext code for redisplay.

use std::{error::Error, fmt};

use ctutils::CtEq;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::crypto::{CryptoError, SecretKey, derive_recovery_key, random_secret};

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const SECRET_CHARACTERS: usize = 52;
const CODE_CHARACTERS: usize = 60;
const RAW_INPUT_LIMIT: usize = 256;

/// Controlled transcription error that never includes secret-bearing input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryError {
    /// Excessive length, forbidden characters, noncanonical bits or wrong checksum.
    InvalidCode,
    /// A valid transcription does not match the enrollment secret.
    ConfirmationMismatch,
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidCode => "invalid recovery code",
            Self::ConfirmationMismatch => "recovery confirmation does not match",
        })
    }
}

impl Error for RecoveryError {}

/// Generated or checksum-validated 32-byte secret, cleared on drop.
///
/// Not Debug, Clone or Copy. This value establishes neither vault access nor
/// enrollment confirmation. Callers control the lifetime of display/input copies.
pub struct RecoverySecret(Zeroizing<[u8; 32]>);

impl RecoverySecret {
    /// Generate an independent secret using native OS randomness, without fallback.
    pub fn generate() -> Result<Self, CryptoError> {
        Ok(Self(random_secret()?))
    }

    /// Decode only the exact accepted paper-code representation/normalization.
    ///
    /// Caps raw UTF-8 before processing; uses fixed buffers. Accepts ASCII case,
    /// hyphens, spaces, tabs, CR and LF. Rejects lookalikes, padding and nonzero
    /// unused bits. Verifies the checksum before returning a derivable secret.
    pub fn decode(input: &str) -> Result<Self, RecoveryError> {
        if input.len() > RAW_INPUT_LIMIT {
            return Err(RecoveryError::InvalidCode);
        }
        let mut normalized = Zeroizing::new([0; CODE_CHARACTERS]);
        let mut count = 0;
        for byte in input.bytes() {
            if matches!(byte, b'-' | b' ' | b'\t' | b'\r' | b'\n') {
                continue;
            }
            if count == CODE_CHARACTERS {
                return Err(RecoveryError::InvalidCode);
            }
            let byte = byte.to_ascii_uppercase();
            digit(byte)?;
            normalized[count] = byte;
            count += 1;
        }
        if count != CODE_CHARACTERS {
            return Err(RecoveryError::InvalidCode);
        }

        let mut secret = Zeroizing::new([0; 32]);
        for (i, byte) in normalized[..SECRET_CHARACTERS].iter().enumerate() {
            let value = digit(*byte)?;
            for bit in 0..5 {
                let position = i * 5 + bit;
                let set = (value >> (4 - bit)) & 1;
                if position < 256 {
                    secret[position / 8] |= set << (7 - position % 8);
                } else if set != 0 {
                    return Err(RecoveryError::InvalidCode);
                }
            }
        }
        let expected = checksum_characters(&secret);
        if !expected
            .as_slice()
            .ct_eq(&normalized[SECRET_CHARACTERS..])
            .to_bool()
        {
            return Err(RecoveryError::InvalidCode);
        }
        Ok(Self(secret))
    }

    /// Explicitly expose the enrollment code as 15 groups of four characters.
    ///
    /// The returned owned string is cleared on drop. Future application state must
    /// restrict display to enrollment; this codec alone does not enforce that state.
    pub fn format_for_enrollment(&self) -> Zeroizing<String> {
        let mut characters = Zeroizing::new([0; CODE_CHARACTERS]);
        encode_base32(self.0.as_slice(), &mut characters[..SECRET_CHARACTERS]);
        characters[SECRET_CHARACTERS..].copy_from_slice(checksum_characters(&self.0).as_slice());
        let mut text = Zeroizing::new(String::with_capacity(74));
        for (i, byte) in characters.iter().enumerate() {
            if i > 0 && i % 4 == 0 {
                text.push('-');
            }
            text.push(char::from(*byte));
        }
        text
    }

    /// Check a transcription against this secret after decoding and checksum checks.
    ///
    /// Uses the reviewed fixed-length comparison primitive; does not commit or
    /// advance enrollment state. A malformed transcription is never derived.
    pub fn confirm_transcription(&self, input: &str) -> Result<(), RecoveryError> {
        let confirmed = Self::decode(input)?;
        if self.0.as_slice().ct_eq(confirmed.0.as_slice()).to_bool() {
            Ok(())
        } else {
            Err(RecoveryError::ConfirmationMismatch)
        }
    }

    /// Derive the wrapping key only from this generated/validated decoded secret.
    pub fn derive_key(
        &self,
        salt: &[u8; 32],
        vault_id: &[u8; 16],
    ) -> Result<SecretKey, CryptoError> {
        derive_recovery_key(&self.0, salt, vault_id)
    }
}

fn digit(byte: u8) -> Result<u8, RecoveryError> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'2'..=b'7' => Ok(byte - b'2' + 26),
        _ => Err(RecoveryError::InvalidCode),
    }
}

fn encode_base32(input: &[u8], output: &mut [u8]) {
    for (i, byte) in output.iter_mut().enumerate() {
        let mut value = 0;
        for bit in 0..5 {
            let position = i * 5 + bit;
            let set = if position < input.len() * 8 {
                (input[position / 8] >> (7 - position % 8)) & 1
            } else {
                0
            };
            value = (value << 1) | set;
        }
        *byte = ALPHABET[usize::from(value)];
    }
}

fn checksum_characters(secret: &[u8; 32]) -> Zeroizing<[u8; 8]> {
    let mut hash = Sha256::new();
    hash.update(b"Codeward/recovery-check/v1\0");
    hash.update(secret);
    let digest = Zeroizing::new(<[u8; 32]>::from(hash.finalize()));
    let mut characters = Zeroizing::new([0; 8]);
    encode_base32(&digest[..5], characters.as_mut());
    characters
}

#[cfg(test)]
mod tests;
