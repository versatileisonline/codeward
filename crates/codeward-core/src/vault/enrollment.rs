//! Paper confirmation gates preparation; a candidate still requires a commit.

use super::{
    CredentialSnapshot, CryptoError, EncryptionNonce, HeaderParts, RecoverySecret, SecretKey,
    VaultCandidate, VaultError, crypto, seal_snapshot, validate_new_password,
};
use crate::envelope::{Slot, TAG_LENGTH, slot_aad};
use zeroize::Zeroizing;

/// Volatile paper-enrollment state. Drop it to cancel; no plaintext is persisted.
///
/// Successful preparation releases the paper secret and prevents later redisplay
/// or repeated creation through this context. Password inputs are borrowed only.
pub struct Enrollment {
    recovery: Option<RecoverySecret>,
}

impl Enrollment {
    /// Generate a fresh independent paper secret using the reviewed OS provider.
    pub fn begin() -> Result<Self, VaultError> {
        Ok(Self {
            recovery: Some(RecoverySecret::generate()?),
        })
    }

    /// Return a temporary code display while this enrollment is still pending.
    ///
    /// The caller owns clearing UI/display copies; successful preparation revokes
    /// this method but cannot erase a copy already returned to the caller.
    pub fn recovery_code(&self) -> Result<Zeroizing<String>, VaultError> {
        Ok(self
            .recovery
            .as_ref()
            .ok_or(VaultError::EnrollmentFinished)?
            .format_for_enrollment())
    }

    /// Confirm both secrets and prepare an encrypted empty version 1 vault.
    ///
    /// Validation/crypto failures preserve enrollment for an explicit retry. Each
    /// retry generates new key/salts/nonces. Success returns unsaved bytes only;
    /// it does not mark a file ready or establish a browsing session.
    pub fn prepare(
        &mut self,
        password: &str,
        password_confirmation: &str,
        paper_transcription: &str,
    ) -> Result<VaultCandidate, VaultError> {
        let recovery = self
            .recovery
            .as_ref()
            .ok_or(VaultError::EnrollmentFinished)?;
        validate_new_password(password, password_confirmation)?;
        recovery.confirm_transcription(paper_transcription)?;
        let candidate = create(password, recovery)?;
        self.recovery = None;
        Ok(candidate)
    }
}

fn create(password: &str, recovery: &RecoverySecret) -> Result<VaultCandidate, VaultError> {
    let data_key = SecretKey::generate()?;
    let master_nonce = EncryptionNonce::generate()?;
    let recovery_nonce = EncryptionNonce::generate()?;
    let mut header = HeaderParts {
        vault_id: *crypto::random_secret::<16>()?,
        revision: 1,
        master_salt: *crypto::random_secret::<16>()?,
        master_nonce: *master_nonce.as_bytes(),
        master_protected_key: [0; 48],
        recovery_salt: *crypto::random_secret::<32>()?,
        recovery_nonce: *recovery_nonce.as_bytes(),
        recovery_protected_key: [0; 48],
        payload_nonce: [0; 24],
    };
    // Slots exclude ciphertext, payload length/nonce and revision from their AAD.
    // A temporary minimum-length header supplies only their exact metadata; it
    // is never returned as a candidate. Final payload encryption binds both slots.
    let slot_header = header.encode(TAG_LENGTH)?;
    let master_key = crypto::derive_master_key(password.as_bytes(), &header.master_salt)?;
    let protected = crypto::seal(
        &master_key,
        master_nonce,
        &slot_aad(&slot_header, Slot::MasterPassword),
        data_key.bytes(),
    )?;
    header.master_protected_key = protected
        .try_into()
        .map_err(|_| CryptoError::EncryptionFailed)?;
    drop(master_key);
    let recovery_key = recovery.derive_key(&header.recovery_salt, &header.vault_id)?;
    let protected = crypto::seal(
        &recovery_key,
        recovery_nonce,
        &slot_aad(&slot_header, Slot::Recovery),
        data_key.bytes(),
    )?;
    header.recovery_protected_key = protected
        .try_into()
        .map_err(|_| CryptoError::EncryptionFailed)?;
    drop(recovery_key);
    seal_snapshot(header, &data_key, &CredentialSnapshot::new(Vec::new())?)
}
