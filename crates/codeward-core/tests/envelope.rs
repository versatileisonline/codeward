use codeward_core::envelope::{
    FormatError, HEADER_LENGTH, MAX_FILE_LENGTH, Slot, TAG_LENGTH, UnauthenticatedEnvelope,
};

fn decode_hex(text: &str) -> Vec<u8> {
    let digits: String = text.split_ascii_whitespace().collect();
    assert_eq!(digits.len() % 2, 0);
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).expect("valid fixture hex"))
        .collect()
}

fn fixture() -> Vec<u8> {
    decode_hex(include_str!("fixtures/structural-v1.hex"))
}

fn reject(bytes: &[u8], expected: FormatError) {
    assert_eq!(UnauthenticatedEnvelope::parse(bytes).err(), Some(expected));
}

fn metadata(bytes: &[u8]) -> [Vec<u8>; 4] {
    let envelope = UnauthenticatedEnvelope::parse(bytes).expect("structural fixture");
    [
        envelope.slot_aad(Slot::MasterPassword),
        envelope.slot_aad(Slot::Recovery),
        envelope.payload_aad(),
        envelope.recovery_hkdf_info(),
    ]
}

#[test]
fn parses_independent_fixture_with_little_endian_fields_and_exact_views() {
    let bytes = fixture();
    let original = bytes.clone();
    let envelope = UnauthenticatedEnvelope::parse(&bytes).expect("structural fixture");
    assert_eq!(envelope.revision(), 0x0102_0304_0506_0708);
    assert_eq!(envelope.vault_id(), (0_u8..16).collect::<Vec<_>>());
    assert_eq!(envelope.header_bytes().len(), 280);
    assert_eq!(envelope.encrypted_payload(), [0xA5; 32]);
    assert_eq!(
        envelope.slot_salt(Slot::MasterPassword),
        (0x10..0x20).collect::<Vec<_>>()
    );
    assert_eq!(
        envelope.slot_salt(Slot::Recovery),
        (0x70..0x90).collect::<Vec<_>>()
    );
    assert_eq!(
        envelope.slot_nonce(Slot::MasterPassword),
        (0x20..0x38).collect::<Vec<_>>()
    );
    assert_eq!(
        envelope.slot_nonce(Slot::Recovery),
        (0x90..0xA8).collect::<Vec<_>>()
    );
    assert_eq!(envelope.payload_nonce(), (0xE0..0xF8).collect::<Vec<_>>());
    assert_eq!(
        envelope.protected_data_key(Slot::MasterPassword),
        (0x40..0x70).collect::<Vec<_>>()
    );
    assert_eq!(
        envelope.protected_data_key(Slot::Recovery),
        (0xB0..0xE0).collect::<Vec<_>>()
    );
    assert_eq!(bytes, original);
    assert_eq!(envelope.encrypted_payload().as_ptr(), bytes[280..].as_ptr());
}

#[test]
fn exact_aad_and_hkdf_info_match_independent_python_fixtures() {
    let actual = metadata(&fixture());
    let expected = [
        include_str!("fixtures/master-aad.hex"),
        include_str!("fixtures/recovery-aad.hex"),
        include_str!("fixtures/payload-aad.hex"),
        include_str!("fixtures/recovery-info.hex"),
    ];
    for (bytes, hex) in actual.iter().zip(expected) {
        assert_eq!(bytes, &decode_hex(hex));
    }
    assert_eq!(actual.each_ref().map(Vec::len), [101, 105, 300, 46]);
}

#[test]
fn rejects_every_truncated_prefix_and_preserves_input() {
    let bytes = fixture();
    for end in 0..bytes.len() {
        let prefix = &bytes[..end];
        let original = prefix.to_vec();
        assert!(
            UnauthenticatedEnvelope::parse(prefix).is_err(),
            "prefix {end}"
        );
        assert_eq!(prefix, original);
    }
}

#[test]
fn rejects_trailing_data_even_when_it_looks_like_another_header() {
    let mut bytes = fixture();
    bytes.extend_from_slice(b"CODEWARD");
    reject(&bytes, FormatError::LengthMismatch);
}

#[test]
fn enforces_actual_whole_file_cap_before_header_processing() {
    let bytes = vec![0; MAX_FILE_LENGTH + 1];
    reject(&bytes, FormatError::TooLarge);
}

#[test]
fn accepts_exact_cap_without_trusting_ciphertext_or_allocating_from_a_length() {
    let small = fixture();
    let mut bytes = vec![0; MAX_FILE_LENGTH];
    bytes[..HEADER_LENGTH].copy_from_slice(&small[..HEADER_LENGTH]);
    bytes[40..44].copy_from_slice(&((MAX_FILE_LENGTH - HEADER_LENGTH) as u32).to_le_bytes());
    let envelope = UnauthenticatedEnvelope::parse(&bytes).expect("structural cap");
    assert_eq!(
        envelope.encrypted_payload().len(),
        MAX_FILE_LENGTH - HEADER_LENGTH
    );
}

#[test]
fn rejects_invalid_declared_payload_lengths_before_length_based_work() {
    for length in [
        0,
        1,
        15,
        (MAX_FILE_LENGTH - HEADER_LENGTH + 1) as u32,
        u32::MAX,
    ] {
        let mut bytes = fixture();
        bytes[40..44].copy_from_slice(&length.to_le_bytes());
        reject(&bytes, FormatError::InvalidPayloadLength);
    }
}

#[test]
fn rejects_a_plausible_but_incorrect_declared_payload_length() {
    let mut bytes = fixture();
    bytes[40..44].copy_from_slice(&33_u32.to_le_bytes());
    reject(&bytes, FormatError::LengthMismatch);
}

#[test]
fn rejects_unsupported_version_suite_header_size_and_zero_revision() {
    let mut bytes = fixture();
    bytes[0] ^= 1;
    reject(&bytes, FormatError::InvalidMagic);
    for version in [0_u16, 2, u16::MAX] {
        let mut bytes = fixture();
        bytes[8..10].copy_from_slice(&version.to_le_bytes());
        reject(&bytes, FormatError::UnsupportedVersion);
    }
    for suite in [0_u16, 2, u16::MAX] {
        let mut bytes = fixture();
        bytes[10..12].copy_from_slice(&suite.to_le_bytes());
        reject(&bytes, FormatError::UnsupportedSuite);
    }
    for size in [0_u32, 279, 281, u32::MAX] {
        let mut bytes = fixture();
        bytes[36..40].copy_from_slice(&size.to_le_bytes());
        reject(&bytes, FormatError::InvalidHeaderLength);
    }
    let mut bytes = fixture();
    bytes[28..36].fill(0);
    reject(&bytes, FormatError::InvalidRevision);
}

#[test]
fn all_unsupported_slot_ids_versions_and_reserved_bytes_are_rejected() {
    let fields = [
        (44, 1, FormatError::InvalidMasterSlot),
        (45, 1, FormatError::InvalidMasterSlot),
        (46, 0x13, FormatError::UnsupportedMasterProfile),
        (47, 0, FormatError::ReservedField),
        (148, 2, FormatError::InvalidRecoverySlot),
        (149, 2, FormatError::InvalidRecoverySlot),
        (150, 0, FormatError::ReservedField),
        (151, 0, FormatError::ReservedField),
    ];
    for (offset, accepted, error) in fields {
        for value in 0..=u8::MAX {
            if value == accepted {
                continue;
            }
            let mut bytes = fixture();
            bytes[offset] = value;
            reject(&bytes, error);
        }
    }
}

#[test]
fn rejects_weaker_stronger_and_resource_exhausting_argon_profiles() {
    for (offset, accepted) in [(48, 65_536_u32), (52, 3), (56, 4)] {
        for value in [0, 1, accepted - 1, accepted + 1, u32::MAX] {
            let mut bytes = fixture();
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            reject(&bytes, FormatError::UnsupportedMasterProfile);
        }
    }
    let mut bytes = fixture();
    bytes[48..52].copy_from_slice(&65_536_u32.to_be_bytes());
    reject(&bytes, FormatError::UnsupportedMasterProfile);
}

#[test]
fn changing_revision_rebinds_only_payload_metadata() {
    let mut bytes = fixture();
    let before = metadata(&bytes);
    bytes[28..36].copy_from_slice(&42_u64.to_le_bytes());
    let after = metadata(&bytes);
    assert_eq!(before[0], after[0]);
    assert_eq!(before[1], after[1]);
    assert_ne!(before[2], after[2]);
    assert_eq!(before[3], after[3]);
}

#[test]
fn slot_salts_rebind_their_own_aad_and_payload_without_rebinding_other_slot() {
    for (offset, own, other) in [(60, 0, 1), (152, 1, 0)] {
        let mut bytes = fixture();
        let before = metadata(&bytes);
        bytes[offset] ^= 1;
        let after = metadata(&bytes);
        assert_ne!(before[own], after[own]);
        assert_eq!(before[other], after[other]);
        assert_ne!(before[2], after[2]);
        assert_eq!(before[3], after[3]);
    }
}

#[test]
fn both_encrypted_slots_are_bound_by_payload_but_excluded_from_slot_aad() {
    for offset in [100, 208] {
        let mut bytes = fixture();
        let before = metadata(&bytes);
        bytes[offset] ^= 1;
        let after = metadata(&bytes);
        assert_eq!(before[0], after[0]);
        assert_eq!(before[1], after[1]);
        assert_ne!(before[2], after[2]);
        assert_eq!(before[3], after[3]);
    }
}

#[test]
fn vault_identity_is_bound_to_both_slots_payload_and_recovery_context() {
    let mut bytes = fixture();
    let before = metadata(&bytes);
    bytes[12] ^= 1;
    let after = metadata(&bytes);
    for (old, new) in before.iter().zip(after) {
        assert_ne!(old, &new);
    }
}

#[test]
fn payload_nonce_and_length_are_bound_without_rebinding_slots() {
    for offset in [256, 40] {
        let mut bytes = fixture();
        let before = metadata(&bytes);
        if offset == 40 {
            bytes.push(0);
            bytes[40..44].copy_from_slice(&33_u32.to_le_bytes());
        } else {
            bytes[offset] ^= 1;
        }
        let after = metadata(&bytes);
        assert_eq!(before[0], after[0]);
        assert_eq!(before[1], after[1]);
        assert_ne!(before[2], after[2]);
        assert_eq!(before[3], after[3]);
    }
}

#[test]
fn structural_parse_does_not_claim_ciphertext_authentication() {
    let mut bytes = fixture();
    let before = metadata(&bytes);
    bytes[HEADER_LENGTH] ^= 1;
    assert!(UnauthenticatedEnvelope::parse(&bytes).is_ok());
    assert_eq!(before, metadata(&bytes));
    assert_eq!(TAG_LENGTH, 16);
    // Future crypto must reject invalid tags. This parser never returns plaintext.
}
