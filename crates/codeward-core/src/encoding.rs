//! Assemble the accepted envelope layout from public metadata and ciphertext.
//!
//! Encoding establishes structure only. It does not encrypt, authenticate,
//! generate randomness, advance revisions or commit anything to disk.

use crate::envelope::{
    FormatError, HEADER_LENGTH, MAX_FILE_LENGTH, Slot, TAG_LENGTH, UnauthenticatedEnvelope,
};

/// Owned public metadata and encrypted slots for a version 1 header.
///
/// This type establishes structure only; its values are not authenticated.
/// Fresh encryption nonces and cryptographically produced slots belong to callers.
/// Deliberately has no Debug implementation.
pub struct HeaderParts {
    /// Stable public vault identifier.
    pub vault_id: [u8; 16],
    /// Nonzero revision; authorization and overflow handling belong to mutations.
    pub revision: u64,
    /// Public Argon2id salt.
    pub master_salt: [u8; 16],
    /// Public master-slot nonce.
    pub master_nonce: [u8; 24],
    /// Encrypted data key plus the complete master-slot tag.
    pub master_protected_key: [u8; 48],
    /// Public recovery HKDF salt.
    pub recovery_salt: [u8; 32],
    /// Public recovery-slot nonce.
    pub recovery_nonce: [u8; 24],
    /// Encrypted data key plus the complete recovery-slot tag.
    pub recovery_protected_key: [u8; 48],
    /// Public payload nonce.
    pub payload_nonce: [u8; 24],
}

/// Header metadata and borrowed ciphertext for an unauthenticated envelope.
pub struct EnvelopeParts<'a> {
    /// Public metadata and encrypted key slots.
    pub header: HeaderParts,
    /// Encrypted snapshot including the complete payload tag.
    pub encrypted_payload: &'a [u8],
}

impl HeaderParts {
    /// Encode the fixed header with the exact accepted profile and payload length.
    ///
    /// Reject zero revision and out-of-bounds payload lengths before allocating
    /// output. Ciphertext/tags remain unchecked. Header bytes alone do not
    /// establish a complete envelope, authentication or a saved/unlocked vault.
    pub fn encode(&self, payload_length: usize) -> Result<[u8; HEADER_LENGTH], FormatError> {
        if self.revision == 0 {
            return Err(FormatError::InvalidRevision);
        }
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
        header[12..28].copy_from_slice(&self.vault_id);
        header[28..36].copy_from_slice(&self.revision.to_le_bytes());
        header[36..40].copy_from_slice(&(HEADER_LENGTH as u32).to_le_bytes());
        header[40..44].copy_from_slice(&(payload_length as u32).to_le_bytes());
        header[44..48].copy_from_slice(&[1, 1, 0x13, 0]);
        header[48..52].copy_from_slice(&65_536_u32.to_le_bytes());
        header[52..56].copy_from_slice(&3_u32.to_le_bytes());
        header[56..60].copy_from_slice(&4_u32.to_le_bytes());
        header[60..76].copy_from_slice(&self.master_salt);
        header[76..100].copy_from_slice(&self.master_nonce);
        header[100..148].copy_from_slice(&self.master_protected_key);
        header[148..152].copy_from_slice(&[2, 2, 0, 0]);
        header[152..184].copy_from_slice(&self.recovery_salt);
        header[184..208].copy_from_slice(&self.recovery_nonce);
        header[208..256].copy_from_slice(&self.recovery_protected_key);
        header[256..280].copy_from_slice(&self.payload_nonce);

        Ok(header)
    }

    /// Copy supported metadata from a structurally checked, unauthenticated input.
    ///
    /// No authentication is performed. Fixed-size destination copies follow the
    /// parser's exact-length guarantees; unsupported fields cannot be normalized.
    pub fn from_envelope(envelope: &UnauthenticatedEnvelope<'_>) -> Self {
        let mut header = Self {
            vault_id: [0; 16],
            revision: envelope.revision(),
            master_salt: [0; 16],
            master_nonce: [0; 24],
            master_protected_key: [0; 48],
            recovery_salt: [0; 32],
            recovery_nonce: [0; 24],
            recovery_protected_key: [0; 48],
            payload_nonce: [0; 24],
        };
        header.vault_id.copy_from_slice(envelope.vault_id());
        header
            .master_salt
            .copy_from_slice(envelope.slot_salt(Slot::MasterPassword));
        header
            .master_nonce
            .copy_from_slice(envelope.slot_nonce(Slot::MasterPassword));
        header
            .master_protected_key
            .copy_from_slice(envelope.protected_data_key(Slot::MasterPassword));
        header
            .recovery_salt
            .copy_from_slice(envelope.slot_salt(Slot::Recovery));
        header
            .recovery_nonce
            .copy_from_slice(envelope.slot_nonce(Slot::Recovery));
        header
            .recovery_protected_key
            .copy_from_slice(envelope.protected_data_key(Slot::Recovery));
        header
            .payload_nonce
            .copy_from_slice(envelope.payload_nonce());
        header
    }
}

impl EnvelopeParts<'_> {
    /// Encode the complete layout without encrypting, authenticating or saving it.
    ///
    /// Validates header and total length before allocating the output buffer.
    pub fn encode(&self) -> Result<Vec<u8>, FormatError> {
        let header = self.header.encode(self.encrypted_payload.len())?;
        let mut bytes = Vec::with_capacity(HEADER_LENGTH + self.encrypted_payload.len());
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(self.encrypted_payload);
        Ok(bytes)
    }
}
