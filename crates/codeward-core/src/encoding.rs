//! Assemble the accepted envelope layout from public metadata and ciphertext.
//!
//! Encoding establishes structure only. It does not encrypt, authenticate,
//! generate randomness, advance revisions or commit anything to disk.

use crate::envelope::{FormatError, HEADER_LENGTH, MAX_FILE_LENGTH, TAG_LENGTH};

/// Borrowed inputs for an unauthenticated version 1 envelope.
///
/// Fixed-size arrays enforce metadata and protected-key lengths. Callers must
/// eventually supply cryptographically produced slots/payload and fresh random
/// nonces for each new encryption. This encoder cannot verify those properties.
/// There is no `Debug` implementation that could dump input bytes.
pub struct EnvelopeParts<'a> {
    /// Stable public vault identifier.
    pub vault_id: &'a [u8; 16],
    /// Nonzero revision; authorization and overflow handling belong to mutations.
    pub revision: u64,
    /// Public Argon2id salt.
    pub master_salt: &'a [u8; 16],
    /// Public master-slot nonce.
    pub master_nonce: &'a [u8; 24],
    /// Encrypted data key plus the complete master-slot tag.
    pub master_protected_key: &'a [u8; 48],
    /// Public recovery HKDF salt.
    pub recovery_salt: &'a [u8; 32],
    /// Public recovery-slot nonce.
    pub recovery_nonce: &'a [u8; 24],
    /// Encrypted data key plus the complete recovery-slot tag.
    pub recovery_protected_key: &'a [u8; 48],
    /// Public payload nonce.
    pub payload_nonce: &'a [u8; 24],
    /// Encrypted snapshot including the complete payload tag.
    pub encrypted_payload: &'a [u8],
}

impl EnvelopeParts<'_> {
    /// Encode the exact version 1 layout with the single accepted crypto profile.
    ///
    /// Reject zero revision and out-of-bounds payload lengths before allocating
    /// output. Inputs are preserved; ciphertext and tags remain unchecked. Output
    /// is only an in-memory byte buffer, never evidence of a saved/unlocked vault.
    pub fn encode(&self) -> Result<Vec<u8>, FormatError> {
        if self.revision == 0 {
            return Err(FormatError::InvalidRevision);
        }
        let payload_length = self.encrypted_payload.len();
        if payload_length > MAX_FILE_LENGTH - HEADER_LENGTH {
            return Err(FormatError::TooLarge);
        }
        if payload_length < TAG_LENGTH {
            return Err(FormatError::InvalidPayloadLength);
        }
        // The cap above bounds both arithmetic and the conversion to u32.
        let mut header = [0_u8; HEADER_LENGTH];
        header[..8].copy_from_slice(b"CODEWARD");
        header[8..10].copy_from_slice(&1_u16.to_le_bytes());
        header[10..12].copy_from_slice(&1_u16.to_le_bytes());
        header[12..28].copy_from_slice(self.vault_id);
        header[28..36].copy_from_slice(&self.revision.to_le_bytes());
        header[36..40].copy_from_slice(&(HEADER_LENGTH as u32).to_le_bytes());
        header[40..44].copy_from_slice(&(payload_length as u32).to_le_bytes());
        header[44..48].copy_from_slice(&[1, 1, 0x13, 0]);
        header[48..52].copy_from_slice(&65_536_u32.to_le_bytes());
        header[52..56].copy_from_slice(&3_u32.to_le_bytes());
        header[56..60].copy_from_slice(&4_u32.to_le_bytes());
        header[60..76].copy_from_slice(self.master_salt);
        header[76..100].copy_from_slice(self.master_nonce);
        header[100..148].copy_from_slice(self.master_protected_key);
        header[148..152].copy_from_slice(&[2, 2, 0, 0]);
        header[152..184].copy_from_slice(self.recovery_salt);
        header[184..208].copy_from_slice(self.recovery_nonce);
        header[208..256].copy_from_slice(self.recovery_protected_key);
        header[256..280].copy_from_slice(self.payload_nonce);

        let mut bytes = Vec::with_capacity(HEADER_LENGTH + payload_length);
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(self.encrypted_payload);
        Ok(bytes)
    }
}
