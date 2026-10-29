//! Published primitive vectors plus independent synthetic Codeward-profile vectors.
//! Fixed test keys/nonces are never available through production nonce generation.

use super::*;
use argon2::{AssociatedData, ParamsBuilder};

fn hex(text: &str) -> Vec<u8> {
    let text: String = text.split_ascii_whitespace().collect();
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn key() -> SecretKey {
    SecretKey::from_bytes(Zeroizing::new(std::array::from_fn(|i| 0x80 + i as u8)))
}

#[test]
fn checks_argon2_password_length_bound_without_allocating_a_multigigabyte_input() {
    // Test length metadata rather than allocating >4 GiB to exercise rejection.
    // Source review also checks that this guard precedes locking/work allocation.
    for length in [0, 1, 0xffff_ffff] {
        assert_eq!(validate_master_password_length(length), Ok(()));
    }
    if let Ok(over_limit) = usize::try_from(0x1_0000_0000_u64) {
        assert_eq!(
            validate_master_password_length(over_limit),
            Err(CryptoError::InvalidLength)
        );
        assert_eq!(
            validate_master_password_length(usize::MAX),
            Err(CryptoError::InvalidLength)
        );
    }
}

#[test]
fn rfc9106_argon2id_vector_uses_the_same_owned_memory_path() {
    // RFC 9106 section 5.3. Alternate costs/secret/AD are test-only, not the app profile.
    let params = ParamsBuilder::new()
        .m_cost(32)
        .t_cost(3)
        .p_cost(4)
        .data(AssociatedData::new(&[4; 12]).unwrap())
        .build()
        .unwrap();
    let context =
        Argon2::new_with_secret(&[3; 8], Algorithm::Argon2id, Version::V0x13, params).unwrap();
    let actual = derive_with_memory(&context, &[1; 32], &[2; 16]).unwrap();
    assert_eq!(
        actual.bytes().as_slice(),
        hex("0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659")
    );
}

#[test]
fn accepted_master_profile_matches_independent_c_implementation_without_normalizing() {
    let salt = std::array::from_fn(|i| i as u8);
    let passwords = [
        "synthetic password",
        " synthetic password ",
        "Synthetic password",
        "é",
        "e\u{0301}",
    ];
    let expected = include_str!("../../tests/fixtures/master-keys.hex");
    assert_eq!(expected.lines().count(), passwords.len());
    for (password, expected) in passwords.into_iter().zip(expected.lines()) {
        let actual = derive_master_key(password.as_bytes(), &salt).unwrap();
        assert_eq!(actual.bytes().as_slice(), hex(expected));
        assert_eq!(salt, std::array::from_fn(|i| i as u8));
    }
    // Try-lock refuses concurrent work before allocating or deriving.
    let _guard = MASTER_DERIVATION.lock().unwrap();
    assert_eq!(
        derive_master_key(b"synthetic", &salt).err(),
        Some(CryptoError::DerivationBusy)
    );
}

#[test]
fn rfc5869_hkdf_vector_uses_full_extract_and_expand() {
    // RFC 5869 appendix A.1, first 32 bytes of the 42-byte output.
    let actual = hkdf_key(
        &[0x0b; 22],
        &hex("000102030405060708090a0b0c"),
        &hex("f0f1f2f3f4f5f6f7f8f9"),
    )
    .unwrap();
    assert_eq!(
        actual.bytes().as_slice(),
        hex("3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf")
    );
}

#[test]
fn recovery_derivation_matches_python_and_binds_secret_salt_and_vault() {
    let secret = std::array::from_fn(|i| i as u8);
    let salt = std::array::from_fn(|i| 32 + i as u8);
    let id = std::array::from_fn(|i| i as u8);
    let actual = derive_recovery_key(&secret, &salt, &id).unwrap();
    assert_eq!(
        actual.bytes().as_slice(),
        hex(include_str!("../../tests/fixtures/recovery-key.hex"))
    );
    let mut changed_secret = secret;
    changed_secret[0] ^= 1;
    let mut changed_salt = salt;
    changed_salt[0] ^= 1;
    let mut changed_id = id;
    changed_id[0] ^= 1;
    for changed in [
        derive_recovery_key(&changed_secret, &salt, &id).unwrap(),
        derive_recovery_key(&secret, &changed_salt, &id).unwrap(),
        derive_recovery_key(&secret, &salt, &changed_id).unwrap(),
    ] {
        assert_ne!(actual.bytes(), changed.bytes());
    }
}

#[test]
fn xchacha_draft_vector_matches_ciphertext_and_full_tag() {
    // draft-irtf-cfrg-xchacha-03 appendix A.3.1, not an RFC.
    let nonce = std::array::from_fn(|i| 0x40 + i as u8);
    let aad = hex("50515253c0c1c2c3c4c5c6c7");
    let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let expected = hex(
        "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb
        731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452
        2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9
        21f9664c97637da9768812f615c68b13b52ec0875924c1c7987947deafd8780acf49",
    );
    assert_eq!(
        seal(&key(), EncryptionNonce(nonce), &aad, plaintext).unwrap(),
        expected
    );
    assert_eq!(
        open(&key(), &nonce, &aad, &expected).unwrap().as_slice(),
        plaintext
    );
}

#[test]
fn rejects_every_changed_ciphertext_tag_nonce_aad_and_wrong_key_without_modifying_input() {
    let nonce = [5; 24];
    let aad = b"synthetic metadata";
    let encrypted = seal(&key(), EncryptionNonce(nonce), aad, b"synthetic plaintext").unwrap();
    let original = encrypted.clone();
    for end in 0..encrypted.len() {
        assert!(open(&key(), &nonce, aad, &encrypted[..end]).is_err());
    }
    let mut appended = encrypted.clone();
    appended.push(0);
    assert_eq!(
        open(&key(), &nonce, aad, &appended).err(),
        Some(CryptoError::AuthenticationFailed)
    );
    for i in 0..encrypted.len() {
        let mut changed = encrypted.clone();
        changed[i] ^= 1;
        let before = changed.clone();
        assert_eq!(
            open(&key(), &nonce, aad, &changed).err(),
            Some(CryptoError::AuthenticationFailed)
        );
        assert_eq!(changed, before);
    }
    for i in 0..nonce.len() {
        let mut changed = nonce;
        changed[i] ^= 1;
        assert_eq!(
            open(&key(), &changed, aad, &encrypted).err(),
            Some(CryptoError::AuthenticationFailed)
        );
    }
    for i in 0..aad.len() {
        let mut changed = *aad;
        changed[i] ^= 1;
        assert_eq!(
            open(&key(), &nonce, &changed, &encrypted).err(),
            Some(CryptoError::AuthenticationFailed)
        );
    }
    let wrong_key = SecretKey::from_bytes(Zeroizing::new([0; 32]));
    assert_eq!(
        open(&wrong_key, &nonce, aad, &encrypted).err(),
        Some(CryptoError::AuthenticationFailed)
    );
    assert_eq!(encrypted, original);
}

#[test]
fn enforces_crypto_length_bounds_and_accepts_empty_and_maximum_plaintext() {
    for length in [0, MAX_PLAINTEXT_LENGTH] {
        let plaintext = vec![0xA5; length];
        let ciphertext = seal(&key(), EncryptionNonce([9; 24]), b"", &plaintext).unwrap();
        assert_eq!(ciphertext.len(), length + TAG_LENGTH);
        assert_eq!(
            open(&key(), &[9; 24], b"", &ciphertext).unwrap().as_slice(),
            plaintext
        );
    }
    let too_large = vec![0; MAX_PLAINTEXT_LENGTH + 1];
    assert_eq!(
        seal(&key(), EncryptionNonce([9; 24]), b"", &too_large).err(),
        Some(CryptoError::InvalidLength)
    );
    let large_ciphertext = vec![0; MAX_PLAINTEXT_LENGTH + TAG_LENGTH + 1];
    assert_eq!(
        open(&key(), &[9; 24], b"", &large_ciphertext).err(),
        Some(CryptoError::InvalidLength)
    );
    for length in 0..TAG_LENGTH {
        assert_eq!(
            open(&key(), &[9; 24], b"", &[0; 16][..length]).err(),
            Some(CryptoError::InvalidLength)
        );
    }
    let large_aad = vec![0; MAX_FILE_LENGTH + 1];
    assert_eq!(
        seal(&key(), EncryptionNonce([9; 24]), &large_aad, b"").err(),
        Some(CryptoError::InvalidLength)
    );
    assert_eq!(
        open(&key(), &[9; 24], &large_aad, &[0; 16]).err(),
        Some(CryptoError::InvalidLength)
    );
}

#[test]
fn randomness_failure_returns_no_partial_secret_or_fallback() {
    let mut calls = 0;
    let result = random_secret_with::<32>(|buffer| {
        calls += 1;
        buffer[..8].fill(0xA5);
        Err(CryptoError::RandomnessUnavailable)
    });
    assert_eq!(result.err(), Some(CryptoError::RandomnessUnavailable));
    assert_eq!(calls, 1);
}

#[test]
fn native_randomness_smoke_test_generates_keys_and_independent_nonces() {
    // This exercises the OS provider; it is not an entropy/security proof.
    let key = SecretKey::generate().unwrap();
    let first = EncryptionNonce::generate().unwrap();
    let first_bytes = *first.as_bytes();
    let second = EncryptionNonce::generate().unwrap();
    let second_bytes = *second.as_bytes();
    assert_ne!(first_bytes, second_bytes);
    let a = seal(&key, first, b"", b"synthetic").unwrap();
    let b = seal(&key, second, b"", b"synthetic").unwrap();
    assert_ne!(a, b);
    assert_eq!(
        open(&key, &first_bytes, b"", &a).unwrap().as_slice(),
        b"synthetic"
    );
}
