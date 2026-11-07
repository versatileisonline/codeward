//! Prepare coherent encrypted updates; retain the authenticated source unchanged.

use super::{
    AuthenticatedVault, CredentialSnapshot, CryptoError, EncryptionNonce, HeaderParts,
    RecoveryAccess, UnauthenticatedEnvelope, VaultCandidate, VaultError, authenticate_snapshot,
    crypto, seal_snapshot, validate_new_password,
};
use crate::envelope::{Slot, TAG_LENGTH, slot_aad};

impl AuthenticatedVault {
    /// Prepare a whole-snapshot update at the expected authenticated revision.
    ///
    /// Retains both protected slots exactly and uses fresh payload encryption,
    /// without Argon2. This does not update this object, authorize a UI request,
    /// check the live file or acknowledge a save. Transaction admission is separate.
    pub fn prepare_contents(
        &self,
        expected_revision: u64,
        snapshot: &CredentialSnapshot,
    ) -> Result<VaultCandidate, VaultError> {
        seal_snapshot(
            self.next_header(expected_revision)?,
            &self.data_key,
            snapshot,
        )
    }

    // The transaction caller proves these are the exact bytes of its committed
    // content update, which preserves both slots and the current data key.
    pub(crate) fn refresh_contents(&mut self, bytes: &[u8]) -> Result<(), VaultError> {
        let envelope = UnauthenticatedEnvelope::parse(bytes)?;
        let header = HeaderParts::from_envelope(&envelope);
        if header.vault_id != self.header.vault_id
            || Some(header.revision) != self.header.revision.checked_add(1)
        {
            return Err(VaultError::RevisionConflict);
        }
        let snapshot = authenticate_snapshot(&envelope, &header, &self.data_key)?;
        self.snapshot = snapshot;
        self.header = header;
        Ok(())
    }

    fn next_header(&self, expected_revision: u64) -> Result<HeaderParts, VaultError> {
        if expected_revision != self.header.revision {
            return Err(VaultError::RevisionConflict);
        }
        let revision = self
            .header
            .revision
            .checked_add(1)
            .ok_or(VaultError::RevisionExhausted)?;
        // Public metadata and existing encrypted slots only; no secret-key copy.
        let mut header = self.header.clone();
        header.revision = revision;
        Ok(header)
    }
}

impl RecoveryAccess {
    /// Prepare a replacement master slot and freshly encrypted payload together.
    ///
    /// Preserves the data key and recovery salt/slot. The paper secret is reusable;
    /// old file copies remain accessible through their old secrets. The candidate
    /// must commit before a future application reports recovery complete.
    pub fn prepare_master_password(
        &self,
        expected_revision: u64,
        password: &str,
        confirmation: &str,
    ) -> Result<VaultCandidate, VaultError> {
        let mut header = self.vault.next_header(expected_revision)?;
        validate_new_password(password, confirmation)?;
        header.master_salt = *crypto::random_secret::<16>()?;
        let nonce = EncryptionNonce::generate()?;
        header.master_nonce = *nonce.as_bytes();
        let metadata = header.encode(TAG_LENGTH)?;
        let wrapping_key = crypto::derive_master_key(password.as_bytes(), &header.master_salt)?;
        let protected = crypto::seal(
            &wrapping_key,
            nonce,
            &slot_aad(&metadata, Slot::MasterPassword),
            self.vault.data_key.bytes(),
        )?;
        header.master_protected_key = protected
            .try_into()
            .map_err(|_| CryptoError::EncryptionFailed)?;
        drop(wrapping_key);
        seal_snapshot(header, &self.vault.data_key, &self.vault.snapshot)
    }
}

#[cfg(test)]
mod tests;
