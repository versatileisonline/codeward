//! Authenticated in-memory vault operations, not UI commands or file commits.
//!
//! Parse before derivation; authenticate the selected slot and complete payload;
//! validate the credential schema before returning any contents. These objects
//! do not establish caller authorization, session generations or storage durability.

use std::{error::Error, fmt};

use zeroize::Zeroizing;

use crate::{
    credentials::{CredentialError, CredentialSnapshot},
    crypto::{self, CryptoError, SecretKey},
    encoding::HeaderParts,
    envelope::{FormatError, Slot, UnauthenticatedEnvelope},
    recovery::{RecoveryError, RecoverySecret},
};

/// Application master-password bound in exact UTF-8 bytes, per ADR-0005.
pub const MAX_MASTER_PASSWORD_BYTES: usize = 4_096;

/// Controlled failures containing no input, credentials or secret material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultError {
    /// Unsupported or malformed envelope, rejected before derivation.
    Format(FormatError),
    /// Randomness, derivation, allocation or authentication failure.
    Crypto(CryptoError),
    /// Authenticated bytes did not pass the credential schema.
    Credentials(CredentialError),
    /// Invalid or mismatched paper-code transcription.
    Recovery(RecoveryError),
    /// Empty or excessive master-password input.
    InvalidPassword,
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(error) => error.fmt(f),
            Self::Crypto(error) => error.fmt(f),
            Self::Credentials(error) => error.fmt(f),
            Self::Recovery(error) => error.fmt(f),
            Self::InvalidPassword => f.write_str("invalid master-password length"),
        }
    }
}

impl Error for VaultError {}

impl From<FormatError> for VaultError {
    fn from(error: FormatError) -> Self {
        Self::Format(error)
    }
}
impl From<CryptoError> for VaultError {
    fn from(error: CryptoError) -> Self {
        Self::Crypto(error)
    }
}
impl From<CredentialError> for VaultError {
    fn from(error: CredentialError) -> Self {
        Self::Credentials(error)
    }
}
impl From<RecoveryError> for VaultError {
    fn from(error: RecoveryError) -> Self {
        Self::Recovery(error)
    }
}

/// Authenticated, schema-valid contents and their owned data key inside Rust.
///
/// Deliberately not Debug/Clone/Copy or serializable. This is not a UI reply or
/// an application session; future callers must still enforce access permissions.
pub struct AuthenticatedVault {
    header: HeaderParts,
    data_key: SecretKey,
    snapshot: CredentialSnapshot,
}

impl AuthenticatedVault {
    /// Borrow validated contents for authorized core work, never bulk UI replies.
    pub fn snapshot(&self) -> &CredentialSnapshot {
        &self.snapshot
    }

    /// Authenticated revision of the input, not a commit/rollback-detection proof.
    pub fn revision(&self) -> u64 {
        self.header.revision
    }

    /// Authenticated public identifier of this vault.
    pub fn vault_id(&self) -> &[u8; 16] {
        &self.header.vault_id
    }

    /// Consume this object and release its owned key and credential strings.
    ///
    /// Copies held elsewhere are not revoked; app timers/UI redaction are separate.
    /// ```compile_fail
    /// use codeward_core::vault::unlock_with_master;
    /// let vault = unlock_with_master(&[], "synthetic").unwrap();
    /// vault.lock();
    /// let _ = vault.snapshot(); // consumed by lock
    /// ```
    pub fn lock(self) {
        let Self {
            data_key, snapshot, ..
        } = self;
        drop(snapshot);
        drop(data_key);
    }
}

/// Restricted recovery context: validates contents without exposing records.
///
/// Future recovery preparation uses this key internally. Dropping the context
/// releases its owned key/contents; there is no credential-browsing method.
/// ```compile_fail
/// use codeward_core::vault::begin_recovery;
/// let recovery = begin_recovery(&[], "synthetic").unwrap();
/// let _ = recovery.snapshot(); // recovery is not ordinary browsing access
/// ```
pub struct RecoveryAccess {
    vault: AuthenticatedVault,
}

impl RecoveryAccess {
    /// Authenticated revision for recovery preparation/conflict checks.
    pub fn revision(&self) -> u64 {
        self.vault.revision()
    }
}

/// Authenticate an independent master-password slot, payload and complete schema.
///
/// Input is borrowed and unchanged on every outcome. The entered password is
/// preserved exactly; callers own its cleanup. No file is opened or modified.
pub fn unlock_with_master(bytes: &[u8], password: &str) -> Result<AuthenticatedVault, VaultError> {
    authenticate(bytes, Slot::MasterPassword, |header| {
        validate_password(password)?;
        Ok(crypto::derive_master_key(
            password.as_bytes(),
            &header.master_salt,
        )?)
    })
}

/// Validate paper transcription, authenticate the recovery route and gate schema.
///
/// Does not require a master password or expose records, and never reports a
/// completed recovery: replacing the password must later commit coherently.
pub fn begin_recovery(bytes: &[u8], code: &str) -> Result<RecoveryAccess, VaultError> {
    let vault = authenticate(bytes, Slot::Recovery, |header| {
        Ok(RecoverySecret::decode(code)?.derive_key(&header.recovery_salt, &header.vault_id)?)
    })?;
    Ok(RecoveryAccess { vault })
}

fn authenticate(
    bytes: &[u8],
    slot: Slot,
    derive: impl FnOnce(&HeaderParts) -> Result<SecretKey, VaultError>,
) -> Result<AuthenticatedVault, VaultError> {
    let envelope = UnauthenticatedEnvelope::parse(bytes)?;
    let header = HeaderParts::from_envelope(&envelope);
    let wrapping_key = derive(&header)?;
    let nonce = match slot {
        Slot::MasterPassword => &header.master_nonce,
        Slot::Recovery => &header.recovery_nonce,
    };
    let unwrapped = crypto::open(
        &wrapping_key,
        nonce,
        &envelope.slot_aad(slot),
        envelope.protected_data_key(slot),
    )?;
    drop(wrapping_key);
    if unwrapped.len() != 32 {
        return Err(CryptoError::AuthenticationFailed.into());
    }
    let mut key_bytes = Zeroizing::new([0; 32]);
    key_bytes.copy_from_slice(&unwrapped);
    drop(unwrapped);
    let data_key = SecretKey::from_bytes(key_bytes);
    // Always use original parsed header bytes for authentication, including both
    // encrypted slots. Successfully opening one slot never waives this check.
    let plaintext = crypto::open(
        &data_key,
        &header.payload_nonce,
        &envelope.payload_aad(),
        envelope.encrypted_payload(),
    )?;
    let snapshot = CredentialSnapshot::decode(&plaintext)?;
    drop(plaintext);
    Ok(AuthenticatedVault {
        header,
        data_key,
        snapshot,
    })
}

fn validate_password(password: &str) -> Result<(), VaultError> {
    if password.is_empty() || password.len() > MAX_MASTER_PASSWORD_BYTES {
        return Err(VaultError::InvalidPassword);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
