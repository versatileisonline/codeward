use codeward_core::{
    encoding::EnvelopeParts,
    envelope::{FormatError, HEADER_LENGTH, MAX_FILE_LENGTH, Slot, UnauthenticatedEnvelope},
};

const fn sequence<const N: usize>(start: u8) -> [u8; N] {
    let mut bytes = [0; N];
    let mut i = 0;
    while i < N {
        bytes[i] = start.wrapping_add(i as u8);
        i += 1;
    }
    bytes
}

fn parts(payload: &[u8]) -> EnvelopeParts<'_> {
    EnvelopeParts {
        vault_id: &const { sequence::<16>(0) },
        revision: 0x0102_0304_0506_0708,
        master_salt: &const { sequence::<16>(0x10) },
        master_nonce: &const { sequence::<24>(0x20) },
        master_protected_key: &const { sequence::<48>(0x40) },
        recovery_salt: &const { sequence::<32>(0x70) },
        recovery_nonce: &const { sequence::<24>(0x90) },
        recovery_protected_key: &const { sequence::<48>(0xB0) },
        payload_nonce: &const { sequence::<24>(0xE0) },
        encrypted_payload: payload,
    }
}

#[test]
fn encoding_matches_independent_python_fixture_byte_for_byte() {
    let expected: Vec<u8> = include_str!("fixtures/structural-v1.hex")
        .split_ascii_whitespace()
        .flat_map(|line| {
            (0..line.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
        })
        .collect();
    assert_eq!(parts(&[0xA5; 32]).encode().unwrap(), expected);
}

#[test]
fn rejects_zero_revision_and_every_short_tag_length() {
    let mut input = parts(&[0xA5; 16]);
    input.revision = 0;
    assert_eq!(input.encode(), Err(FormatError::InvalidRevision));
    for length in 0..16 {
        assert_eq!(
            parts(&[0xA5; 16][..length]).encode(),
            Err(FormatError::InvalidPayloadLength)
        );
    }
}

#[test]
fn accepts_minimum_and_exact_cap_and_rejects_one_byte_over_cap() {
    for length in [16, MAX_FILE_LENGTH - HEADER_LENGTH] {
        let payload = vec![0xA5; length];
        let encoded = parts(&payload).encode().unwrap();
        assert_eq!(encoded.len(), HEADER_LENGTH + length);
        let parsed = UnauthenticatedEnvelope::parse(&encoded).unwrap();
        assert_eq!(parsed.encrypted_payload(), payload);
    }
    let payload = vec![0xA5; MAX_FILE_LENGTH - HEADER_LENGTH + 1];
    assert_eq!(parts(&payload).encode(), Err(FormatError::TooLarge));
}

#[test]
fn preserves_all_supplied_fields_and_inputs_across_varied_encodings() {
    for seed in [0_u8, 1, 127, 255] {
        let id = sequence::<16>(seed);
        let salt = sequence::<16>(seed.wrapping_add(1));
        let nonce = sequence::<24>(seed.wrapping_add(2));
        let key = sequence::<48>(seed.wrapping_add(3));
        let recovery_salt = sequence::<32>(seed.wrapping_add(4));
        let recovery_nonce = sequence::<24>(seed.wrapping_add(5));
        let recovery_key = sequence::<48>(seed.wrapping_add(6));
        let payload_nonce = sequence::<24>(seed.wrapping_add(7));
        let payload = vec![seed; 17 + usize::from(seed)];
        let original = payload.clone();
        for revision in [1, 256, u64::MAX] {
            let input = EnvelopeParts {
                vault_id: &id,
                revision,
                master_salt: &salt,
                master_nonce: &nonce,
                master_protected_key: &key,
                recovery_salt: &recovery_salt,
                recovery_nonce: &recovery_nonce,
                recovery_protected_key: &recovery_key,
                payload_nonce: &payload_nonce,
                encrypted_payload: &payload,
            };
            let encoded = input.encode().unwrap();
            let parsed = UnauthenticatedEnvelope::parse(&encoded).unwrap();
            assert_eq!(parsed.vault_id(), id);
            assert_eq!(parsed.revision(), revision);
            assert_eq!(parsed.slot_salt(Slot::MasterPassword), salt);
            assert_eq!(parsed.slot_nonce(Slot::MasterPassword), nonce);
            assert_eq!(parsed.protected_data_key(Slot::MasterPassword), key);
            assert_eq!(parsed.slot_salt(Slot::Recovery), recovery_salt);
            assert_eq!(parsed.slot_nonce(Slot::Recovery), recovery_nonce);
            assert_eq!(parsed.protected_data_key(Slot::Recovery), recovery_key);
            assert_eq!(parsed.payload_nonce(), payload_nonce);
            assert_eq!(parsed.encrypted_payload(), original);
            assert_eq!(payload, original);
        }
    }
}

#[test]
fn ciphertext_is_copied_without_claiming_authentication_or_changing_slots() {
    let payload = [0xA5; 32];
    let original = parts(&payload).encode().unwrap();
    let mut changed_payload = payload;
    changed_payload[0] ^= 1;
    let changed = parts(&changed_payload).encode().unwrap();
    assert_eq!(&original[..HEADER_LENGTH], &changed[..HEADER_LENGTH]);
    assert_ne!(&original[HEADER_LENGTH..], &changed[HEADER_LENGTH..]);
    assert!(UnauthenticatedEnvelope::parse(&changed).is_ok());
}
