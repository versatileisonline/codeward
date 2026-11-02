use super::*;

fn fixture(name: &str) -> Vec<u8> {
    let text = match name {
        "valid" => include_str!("../../tests/fixtures/authenticated-v1.hex"),
        "schema" => include_str!("../../tests/fixtures/authenticated-invalid-schema.hex"),
        _ => unreachable!(),
    };
    text.split_ascii_whitespace()
        .flat_map(|line| {
            (0..line.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
        })
        .collect()
}

fn paper_code() -> &'static str {
    include_str!("../../tests/fixtures/authenticated-recovery.txt").trim()
}

#[test]
fn independent_complete_fixture_opens_through_both_routes() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let bytes = fixture("valid");
    let before = bytes.clone();
    let vault = unlock_with_master(&bytes, "synthetic password").unwrap();
    assert_eq!(
        &*vault.snapshot().encode().unwrap(),
        include_bytes!("../../tests/fixtures/credentials-v1.json")
    );
    assert_eq!(vault.revision(), 1);
    assert_eq!(vault.vault_id(), &std::array::from_fn(|i| i as u8));
    let recovery = begin_recovery(&bytes, paper_code()).unwrap();
    assert_eq!(recovery.revision(), vault.revision());
    vault.lock();
    assert_eq!(bytes, before);
}

#[test]
fn authenticated_invalid_schema_never_returns_either_access_type() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let bytes = fixture("schema");
    assert_eq!(
        unlock_with_master(&bytes, "synthetic password").err(),
        Some(VaultError::Credentials(CredentialError::InvalidSnapshot))
    );
    assert_eq!(
        begin_recovery(&bytes, paper_code()).err(),
        Some(VaultError::Credentials(CredentialError::InvalidSnapshot))
    );
}

#[test]
fn rejects_wrong_secrets_and_bounds_password_input_without_modifying_bytes() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let bytes = fixture("valid");
    let before = bytes.clone();
    for password in ["wrong synthetic password", " synthetic password "] {
        assert_eq!(
            unlock_with_master(&bytes, password).err(),
            Some(VaultError::Crypto(CryptoError::AuthenticationFailed))
        );
    }
    for password in ["", &"x".repeat(MAX_MASTER_PASSWORD_BYTES + 1)] {
        assert_eq!(
            unlock_with_master(&bytes, password).err(),
            Some(VaultError::InvalidPassword)
        );
    }
    let other = include_str!("../../tests/fixtures/recovery-codes.txt")
        .lines()
        .next()
        .unwrap();
    assert_eq!(
        begin_recovery(&bytes, other).err(),
        Some(VaultError::Crypto(CryptoError::AuthenticationFailed))
    );
    assert_eq!(
        begin_recovery(&bytes, "invalid").err(),
        Some(VaultError::Recovery(RecoveryError::InvalidCode))
    );
    assert_eq!(bytes, before);
}

#[test]
fn every_changed_envelope_byte_is_rejected_through_recovery() {
    let bytes = fixture("valid");
    for i in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[i] ^= 1;
        let before = changed.clone();
        assert!(
            begin_recovery(&changed, paper_code()).is_err(),
            "offset {i}"
        );
        assert_eq!(changed, before);
    }
}

#[test]
fn master_route_also_rejects_damage_to_either_slot_or_payload() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let bytes = fixture("valid");
    for i in [100, 147, 208, 255, 256, 280, bytes.len() - 1] {
        let mut changed = bytes.clone();
        changed[i] ^= 1;
        let before = changed.clone();
        assert_eq!(
            unlock_with_master(&changed, "synthetic password").err(),
            Some(VaultError::Crypto(CryptoError::AuthenticationFailed))
        );
        assert_eq!(changed, before);
    }
}

#[test]
fn unsupported_header_is_rejected_before_derivation_or_paper_decoding() {
    let bytes = fixture("valid");
    let mut changed = bytes.clone();
    changed[8] = 2;
    assert_eq!(
        unlock_with_master(&changed, "").err(),
        Some(VaultError::Format(FormatError::UnsupportedVersion))
    );
    assert_eq!(
        begin_recovery(&changed, "invalid").err(),
        Some(VaultError::Format(FormatError::UnsupportedVersion))
    );
}
