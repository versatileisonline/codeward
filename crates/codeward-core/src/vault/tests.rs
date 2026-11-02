use super::*;

pub(super) fn fixture(name: &str) -> Vec<u8> {
    let text = match name {
        "valid" => include_str!("../../tests/fixtures/authenticated-v1.hex"),
        "schema" => include_str!("../../tests/fixtures/authenticated-invalid-schema.hex"),
        "max" => include_str!("../../tests/fixtures/authenticated-max-revision.hex"),
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

pub(super) fn paper_code() -> &'static str {
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

#[test]
fn enrollment_confirms_then_prepares_an_empty_vault_without_redisplay() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let mut enrollment = Enrollment::begin().unwrap();
    let code = enrollment.recovery_code().unwrap();
    let password = " synthetic e\u{301} password ";
    let candidate = enrollment.prepare(password, password, &code).unwrap();
    assert_eq!(
        enrollment.recovery_code().err(),
        Some(VaultError::EnrollmentFinished)
    );
    assert_eq!(
        enrollment.prepare(password, password, &code).err(),
        Some(VaultError::EnrollmentFinished)
    );
    let vault = unlock_with_master(candidate.as_bytes(), password).unwrap();
    assert_eq!(vault.revision(), 1);
    assert!(vault.snapshot().credentials().is_empty());
    assert_eq!(
        begin_recovery(candidate.as_bytes(), &code)
            .unwrap()
            .revision(),
        1
    );
    assert!(unlock_with_master(candidate.as_bytes(), password.trim()).is_err());
}

#[test]
fn invalid_confirmation_preserves_enrollment_and_does_not_derive() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let mut enrollment = Enrollment::begin().unwrap();
    let code = enrollment.recovery_code().unwrap();
    // Holding the production guard proves invalid inputs are rejected before KDF
    // admission. The existing primitive tests cover the no-allocation busy path.
    let guard = crypto::occupy_master_derivation();
    assert!(validate_password(&"é".repeat(2_048)).is_ok());
    assert_eq!(
        validate_password(&"é".repeat(2_049)),
        Err(VaultError::InvalidPassword)
    );
    let other = include_str!("../../tests/fixtures/recovery-codes.txt")
        .lines()
        .next()
        .unwrap();
    assert_eq!(
        enrollment.prepare("synthetic", "synthetic", other).err(),
        Some(VaultError::Recovery(RecoveryError::ConfirmationMismatch))
    );
    assert_eq!(
        enrollment.prepare("", "", &code).err(),
        Some(VaultError::InvalidPassword)
    );
    assert_eq!(
        enrollment.prepare("é", "e\u{301}", &code).err(),
        Some(VaultError::PasswordConfirmationMismatch)
    );
    assert_eq!(
        enrollment
            .prepare("synthetic", "synthetic", "invalid")
            .err(),
        Some(VaultError::Recovery(RecoveryError::InvalidCode))
    );
    assert_eq!(
        enrollment.prepare("synthetic", "synthetic", &code).err(),
        Some(VaultError::Crypto(CryptoError::DerivationBusy))
    );
    assert_eq!(&*enrollment.recovery_code().unwrap(), &*code);
    drop(guard);
    assert!(enrollment.prepare("synthetic", "synthetic", &code).is_ok());
}

#[test]
fn transplanting_valid_slots_from_another_vault_never_grants_access() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let bytes = fixture("valid");
    let mut enrollment = Enrollment::begin().unwrap();
    let other_code = enrollment.recovery_code().unwrap();
    let other = enrollment
        .prepare("synthetic password", "synthetic password", &other_code)
        .unwrap();
    for (start, end) in [(44, 148), (148, 256), (44, 256)] {
        let mut spliced = bytes.clone();
        spliced[start..end].copy_from_slice(&other.as_bytes()[start..end]);
        let before = spliced.clone();
        for code in [paper_code(), &other_code] {
            assert!(begin_recovery(&spliced, code).is_err());
        }
        assert!(unlock_with_master(&spliced, "synthetic password").is_err());
        assert_eq!(spliced, before);
    }
}
