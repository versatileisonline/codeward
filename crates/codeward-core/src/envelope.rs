//! Version 1 structural parsing, before cryptographic work.
//!
//! Byte offsets and domain separators follow `docs/vault-format-v1.md`.
//! All returned metadata is unauthenticated. The parser borrows an existing byte
//! slice without allocating; a future file reader must also bound reads before
//! allocating that slice. No filesystem access, KDF or decryption occurs here.

use std::{error::Error, fmt};

/// Whole-file cap, including header, encrypted payload and tag.
pub const MAX_FILE_LENGTH: usize = 16 * 1024 * 1024;
/// Fixed version 1 header, including both protected slots and the payload nonce.
pub const HEADER_LENGTH: usize = 280;
/// Required full AEAD authentication tag length.
pub const TAG_LENGTH: usize = 16;

const SLOT_DOMAIN: &[u8] = b"Codeward/slot/v1\0";
const PAYLOAD_DOMAIN: &[u8] = b"Codeward/payload/v1\0";
const RECOVERY_DOMAIN: &[u8] = b"Codeward/recovery-wrap/v1\0";

/// Independent protected copy of the same vault data key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Slot {
    /// Argon2id-derived master-password wrapping key.
    MasterPassword,
    /// Generated-secret HKDF-SHA256 wrapping key.
    Recovery,
}

/// Structural rejection without input bytes or secret-bearing diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatError {
    /// Actual input exceeds the whole-file cap.
    TooLarge,
    /// Input cannot hold the complete fixed header and a full payload tag.
    Truncated,
    /// Input does not start with the Codeward file identifier.
    InvalidMagic,
    /// Format version is not exactly version 1.
    UnsupportedVersion,
    /// Cryptographic suite identifier is not exactly suite 1.
    UnsupportedSuite,
    /// Zero is not a committed revision.
    InvalidRevision,
    /// Declared header length is not the fixed version 1 length.
    InvalidHeaderLength,
    /// Declared payload length cannot contain a full tag or exceeds the cap.
    InvalidPayloadLength,
    /// Actual input length differs from the declared complete envelope length.
    LengthMismatch,
    /// Master-password slot role or KDF identifier is incorrect.
    InvalidMasterSlot,
    /// Argon2id version, memory, passes or lanes differs from the approved profile.
    UnsupportedMasterProfile,
    /// Recovery slot role or KDF identifier is incorrect.
    InvalidRecoverySlot,
    /// A reserved field is nonzero.
    ReservedField,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::TooLarge => "vault exceeds the whole-file limit",
            Self::Truncated => "vault is incomplete",
            Self::InvalidMagic => "unrecognized vault identifier",
            Self::UnsupportedVersion => "unsupported vault version",
            Self::UnsupportedSuite => "unsupported cryptographic suite",
            Self::InvalidRevision => "invalid vault revision",
            Self::InvalidHeaderLength => "invalid header length",
            Self::InvalidPayloadLength => "invalid encrypted payload length",
            Self::LengthMismatch => "vault length does not match its header",
            Self::InvalidMasterSlot => "invalid master-password slot",
            Self::UnsupportedMasterProfile => "unsupported master-password profile",
            Self::InvalidRecoverySlot => "invalid recovery slot",
            Self::ReservedField => "unsupported reserved field",
        };
        f.write_str(message)
    }
}

impl Error for FormatError {}

/// Borrowed, structurally valid version 1 envelope, still unauthenticated.
///
/// There is deliberately no `Debug` implementation that could dump its bytes.
/// Neither this type nor any metadata getter establishes successful unlock.
pub struct UnauthenticatedEnvelope<'a> {
    header: &'a [u8; HEADER_LENGTH],
    encrypted_payload: &'a [u8],
    revision: u64,
}

impl<'a> UnauthenticatedEnvelope<'a> {
    /// Validate actual size, exact lengths, identifiers and the single KDF profile.
    ///
    /// Does not allocate, run a KDF, authenticate, decrypt or modify the input.
    /// Trailing bytes are rejected. Nonces/salts are not interpreted as proof of
    /// valid randomness; ciphertext and tags remain cryptographically unchecked.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FormatError> {
        if bytes.len() > MAX_FILE_LENGTH {
            return Err(FormatError::TooLarge);
        }
        if bytes.len() < HEADER_LENGTH + TAG_LENGTH {
            return Err(FormatError::Truncated);
        }
        let header: &[u8; HEADER_LENGTH] = bytes[..HEADER_LENGTH]
            .try_into()
            .map_err(|_| FormatError::Truncated)?;
        if &header[..8] != b"CODEWARD" {
            return Err(FormatError::InvalidMagic);
        }
        if u16_at(header, 8) != 1 {
            return Err(FormatError::UnsupportedVersion);
        }
        if u16_at(header, 10) != 1 {
            return Err(FormatError::UnsupportedSuite);
        }
        let revision = u64::from_le_bytes([
            header[28], header[29], header[30], header[31], header[32], header[33], header[34],
            header[35],
        ]);
        if revision == 0 {
            return Err(FormatError::InvalidRevision);
        }
        if u32_at(header, 36) != HEADER_LENGTH as u32 {
            return Err(FormatError::InvalidHeaderLength);
        }
        let payload_length =
            usize::try_from(u32_at(header, 40)).map_err(|_| FormatError::InvalidPayloadLength)?;
        if !(TAG_LENGTH..=MAX_FILE_LENGTH - HEADER_LENGTH).contains(&payload_length) {
            return Err(FormatError::InvalidPayloadLength);
        }
        let complete_length = HEADER_LENGTH
            .checked_add(payload_length)
            .ok_or(FormatError::InvalidPayloadLength)?;
        if bytes.len() != complete_length {
            return Err(FormatError::LengthMismatch);
        }
        if header[44] != 1 || header[45] != 1 {
            return Err(FormatError::InvalidMasterSlot);
        }
        if header[47] != 0 || header[150] != 0 || header[151] != 0 {
            return Err(FormatError::ReservedField);
        }
        if header[46] != 0x13
            || u32_at(header, 48) != 65_536
            || u32_at(header, 52) != 3
            || u32_at(header, 56) != 4
        {
            return Err(FormatError::UnsupportedMasterProfile);
        }
        if header[148] != 2 || header[149] != 2 {
            return Err(FormatError::InvalidRecoverySlot);
        }
        Ok(Self {
            header,
            encrypted_payload: &bytes[HEADER_LENGTH..],
            revision,
        })
    }

    /// Unauthenticated revision; not a trusted conflict or rollback decision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The 16-byte public vault identifier, still unauthenticated.
    pub fn vault_id(&self) -> &[u8] {
        &self.header[12..28]
    }

    /// Exact original public header bytes; never reconstruct AAD from defaults.
    pub fn header_bytes(&self) -> &[u8; HEADER_LENGTH] {
        self.header
    }

    /// Encrypted snapshot followed by its unchecked 16-byte tag.
    pub fn encrypted_payload(&self) -> &'a [u8] {
        self.encrypted_payload
    }

    /// Public 24-byte payload nonce, still unauthenticated.
    pub fn payload_nonce(&self) -> &[u8] {
        &self.header[256..280]
    }

    /// Public slot salt: 16 bytes for the master slot, 32 for recovery.
    pub fn slot_salt(&self, slot: Slot) -> &[u8] {
        match slot {
            Slot::MasterPassword => &self.header[60..76],
            Slot::Recovery => &self.header[152..184],
        }
    }

    /// Public 24-byte nonce of the requested slot, still unauthenticated.
    pub fn slot_nonce(&self, slot: Slot) -> &[u8] {
        match slot {
            Slot::MasterPassword => &self.header[76..100],
            Slot::Recovery => &self.header[184..208],
        }
    }

    /// Encrypted 32-byte data key followed by the slot's unchecked 16-byte tag.
    pub fn protected_data_key(&self, slot: Slot) -> &[u8] {
        match slot {
            Slot::MasterPassword => &self.header[100..148],
            Slot::Recovery => &self.header[208..256],
        }
    }

    /// Exact AAD for one slot, excluding revision, protected key and other slot.
    pub fn slot_aad(&self, slot: Slot) -> Vec<u8> {
        let metadata = match slot {
            Slot::MasterPassword => &self.header[44..100],
            Slot::Recovery => &self.header[148..208],
        };
        let mut aad = Vec::with_capacity(SLOT_DOMAIN.len() + 28 + metadata.len());
        aad.extend_from_slice(SLOT_DOMAIN);
        aad.extend_from_slice(&self.header[..28]);
        aad.extend_from_slice(metadata);
        aad
    }

    /// Exact payload AAD, binding the full header including both encrypted slots.
    pub fn payload_aad(&self) -> Vec<u8> {
        let mut aad = Vec::with_capacity(PAYLOAD_DOMAIN.len() + HEADER_LENGTH);
        aad.extend_from_slice(PAYLOAD_DOMAIN);
        aad.extend_from_slice(self.header);
        aad
    }

    /// HKDF info binding the recovery purpose, format/suite and stable vault ID.
    ///
    /// This is public context, not the HKDF secret, salt, PRK or wrapping key.
    pub fn recovery_hkdf_info(&self) -> Vec<u8> {
        let mut info = Vec::with_capacity(RECOVERY_DOMAIN.len() + 20);
        info.extend_from_slice(RECOVERY_DOMAIN);
        info.extend_from_slice(&self.header[8..28]);
        info
    }
}

// Only fixed, internally selected offsets into a validated 280-byte header call
// these helpers. No untrusted offset or declared length reaches an index here.
fn u16_at(header: &[u8; HEADER_LENGTH], offset: usize) -> u16 {
    u16::from_le_bytes([header[offset], header[offset + 1]])
}

fn u32_at(header: &[u8; HEADER_LENGTH], offset: usize) -> u32 {
    u32::from_le_bytes([
        header[offset],
        header[offset + 1],
        header[offset + 2],
        header[offset + 3],
    ])
}
