//! Bounded cryptographic building blocks for the Rust core, not UI commands.
//!
//! These operations do not establish an unlocked session. Authenticated plaintext
//! must still pass the credential schema before application code exposes records.
//! Owned secret buffers are cleared on drop; caller inputs and all memory copies
//! are not guaranteed erased. No filesystem access occurs here.

use std::{error::Error, fmt, sync::Mutex};

use argon2::{Algorithm, Argon2, Block, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305,
    aead::{AeadInOut, KeyInit},
};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::envelope::{HEADER_LENGTH, MAX_FILE_LENGTH, TAG_LENGTH};

/// Largest plaintext fitting the accepted version 1 envelope with a full tag.
pub const MAX_PLAINTEXT_LENGTH: usize = MAX_FILE_LENGTH - HEADER_LENGTH - TAG_LENGTH;

// One process-wide derivation, including callers from different core instances.
static MASTER_DERIVATION: Mutex<()> = Mutex::new(());

/// Controlled failures with no input or secret-bearing diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CryptoError {
    /// The OS could not provide complete random bytes.
    RandomnessUnavailable,
    /// A password derivation is already running; no queued allocation is made.
    DerivationBusy,
    /// Derivation failed or its coordination lock is poisoned.
    DerivationFailed,
    /// The bounded working allocation could not be reserved.
    AllocationFailed,
    /// Plaintext/ciphertext/AAD length exceeds the core bound or omits a tag.
    InvalidLength,
    /// Wrong key, nonce, metadata or ciphertext; no plaintext is returned.
    AuthenticationFailed,
    /// Encryption failed; retry requires a new random nonce.
    EncryptionFailed,
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::RandomnessUnavailable => "secure randomness is unavailable",
            Self::DerivationBusy => "a password derivation is already running",
            Self::DerivationFailed => "key derivation failed",
            Self::AllocationFailed => "cryptographic working memory is unavailable",
            Self::InvalidLength => "invalid cryptographic input length",
            Self::AuthenticationFailed => "authentication failed",
            Self::EncryptionFailed => "encryption failed",
        })
    }
}

impl Error for CryptoError {}

/// Owned 32-byte key, cleared on drop; deliberately neither Debug nor Clone/Copy.
pub struct SecretKey(Zeroizing<[u8; 32]>);

impl SecretKey {
    /// Adopt an owned secret buffer. The caller remains responsible for any copies.
    pub fn from_bytes(bytes: Zeroizing<[u8; 32]>) -> Self {
        Self(bytes)
    }

    /// Generate an independent key using only the native OS randomness provider.
    pub fn generate() -> Result<Self, CryptoError> {
        Ok(Self(random_secret()?))
    }

    pub(crate) fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Fresh random public encryption nonce, consumed by one encryption attempt.
///
/// No constructor accepts caller-chosen bytes, and this type is not Clone/Copy.
/// Read its bytes to prepare authenticated metadata before passing it to `seal`.
/// Use ordinary bytes from an existing file with `open`, never as a new nonce.
///
/// ```compile_fail
/// use codeward_core::crypto::{EncryptionNonce, SecretKey, seal};
/// let key = SecretKey::generate().unwrap();
/// let nonce = EncryptionNonce::generate().unwrap();
/// let _ = seal(&key, nonce, b"", b"synthetic");
/// let _ = seal(&key, nonce, b"", b"synthetic"); // nonce was consumed
/// ```
pub struct EncryptionNonce([u8; 24]);

impl EncryptionNonce {
    /// Obtain a fresh nonce. A failed attempt never returns partially filled data.
    pub fn generate() -> Result<Self, CryptoError> {
        Ok(Self(*random_secret()?))
    }

    /// Public bytes for constructing the envelope and its authentication metadata.
    pub fn as_bytes(&self) -> &[u8; 24] {
        &self.0
    }
}

pub(crate) fn random_secret<const N: usize>() -> Result<Zeroizing<[u8; N]>, CryptoError> {
    random_secret_with(|bytes| {
        getrandom::fill(bytes).map_err(|_| CryptoError::RandomnessUnavailable)
    })
}

fn random_secret_with<const N: usize>(
    fill: impl FnOnce(&mut [u8]) -> Result<(), CryptoError>,
) -> Result<Zeroizing<[u8; N]>, CryptoError> {
    let mut bytes = Zeroizing::new([0; N]);
    fill(bytes.as_mut())?;
    Ok(bytes)
}

/// Derive the master wrapping key with exactly the accepted version 1 profile.
///
/// Password bytes are borrowed and preserved exactly; the caller owns cleanup.
/// There is no alternate profile, pepper or associated-data input. This does not
/// validate a password or authorize access; a later slot authentication must do so.
pub fn derive_master_key(password: &[u8], salt: &[u8; 16]) -> Result<SecretKey, CryptoError> {
    let _guard = match MASTER_DERIVATION.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::WouldBlock) => return Err(CryptoError::DerivationBusy),
        Err(std::sync::TryLockError::Poisoned(_)) => return Err(CryptoError::DerivationFailed),
    };
    let params = Params::new(65_536, 3, 4, Some(32)).map_err(|_| CryptoError::DerivationFailed)?;
    let context = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    derive_with_memory(&context, password, salt)
}

fn derive_with_memory(
    context: &Argon2<'_>,
    password: &[u8],
    salt: &[u8],
) -> Result<SecretKey, CryptoError> {
    let mut memory = Zeroizing::new(Vec::<Block>::new());
    memory
        .try_reserve_exact(context.params().block_count())
        .map_err(|_| CryptoError::AllocationFailed)?;
    memory.resize(context.params().block_count(), Block::default());
    let mut output = Zeroizing::new([0; 32]);
    context
        .hash_password_into_with_memory(password, salt, output.as_mut(), &mut *memory)
        .map_err(|_| CryptoError::DerivationFailed)?;
    Ok(SecretKey(output))
}

/// Derive the recovery wrapping key from decoded high-entropy secret bytes.
///
/// Performs full HKDF-SHA256 extract/expand and binds format/suite, vault ID and
/// recovery purpose exactly as specified. This is not password hardening. Paper
/// decoding/checksum verification must precede this call for transcribed input.
pub fn derive_recovery_key(
    secret: &[u8; 32],
    salt: &[u8; 32],
    vault_id: &[u8; 16],
) -> Result<SecretKey, CryptoError> {
    let mut info = [0; 46];
    info[..26].copy_from_slice(b"Codeward/recovery-wrap/v1\0");
    info[26..28].copy_from_slice(&1_u16.to_le_bytes());
    info[28..30].copy_from_slice(&1_u16.to_le_bytes());
    info[30..].copy_from_slice(vault_id);
    hkdf_key(secret, salt, &info)
}

fn hkdf_key(secret: &[u8], salt: &[u8], info: &[u8]) -> Result<SecretKey, CryptoError> {
    let mut output = Zeroizing::new([0; 32]);
    Hkdf::<Sha256>::new(Some(salt), secret)
        .expand(info, output.as_mut())
        .map_err(|_| CryptoError::DerivationFailed)?;
    Ok(SecretKey(output))
}

/// Encrypt with full XChaCha20-Poly1305 and a fresh, consumed nonce.
///
/// Returns ciphertext followed by its full 16-byte tag. Bounds are checked before
/// copying plaintext. Neither a file save nor credential-schema validation occurs.
pub fn seal(
    key: &SecretKey,
    nonce: EncryptionNonce,
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if plaintext.len() > MAX_PLAINTEXT_LENGTH || aad.len() > MAX_FILE_LENGTH {
        return Err(CryptoError::InvalidLength);
    }
    let mut buffer = secret_buffer(plaintext, plaintext.len() + TAG_LENGTH)?;
    XChaCha20Poly1305::new(key.bytes().into())
        .encrypt_in_place((&nonce.0).into(), aad, &mut *buffer)
        .map_err(|_| CryptoError::EncryptionFailed)?;
    Ok(std::mem::take(&mut *buffer))
}

/// Authenticate and decrypt into an owned buffer cleared on drop.
///
/// Returns no plaintext on error and leaves ciphertext unchanged. Successful
/// output is authenticated bytes, not validated credentials or an unlocked session.
pub fn open(
    key: &SecretKey,
    nonce: &[u8; 24],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    if !(TAG_LENGTH..=MAX_PLAINTEXT_LENGTH + TAG_LENGTH).contains(&ciphertext.len())
        || aad.len() > MAX_FILE_LENGTH
    {
        return Err(CryptoError::InvalidLength);
    }
    let mut buffer = secret_buffer(ciphertext, ciphertext.len())?;
    XChaCha20Poly1305::new(key.bytes().into())
        .decrypt_in_place(nonce.into(), aad, &mut *buffer)
        .map_err(|_| CryptoError::AuthenticationFailed)?;
    Ok(buffer)
}

fn secret_buffer(input: &[u8], capacity: usize) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    let mut buffer = Zeroizing::new(Vec::new());
    buffer
        .try_reserve_exact(capacity)
        .map_err(|_| CryptoError::AllocationFailed)?;
    buffer.extend_from_slice(input);
    Ok(buffer)
}

#[cfg(test)]
mod tests;
