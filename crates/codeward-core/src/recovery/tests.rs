use super::*;

fn codes() -> Vec<&'static str> {
    include_str!("../../tests/fixtures/recovery-codes.txt")
        .lines()
        .collect()
}

fn synthetic() -> RecoverySecret {
    RecoverySecret(Zeroizing::new(std::array::from_fn(|i| i as u8)))
}

#[test]
fn canonical_codes_match_independent_python_base32_and_sha256_fixtures() {
    let secrets = [[0; 32], std::array::from_fn(|i| i as u8), [255; 32]];
    assert_eq!(codes().len(), secrets.len());
    for (bytes, expected) in secrets.into_iter().zip(codes()) {
        let secret = RecoverySecret(Zeroizing::new(bytes));
        let text = secret.format_for_enrollment();
        assert_eq!(text.as_str(), expected);
        assert_eq!(text.len(), 74);
        assert_eq!(text.split('-').count(), 15);
        assert!(text.split('-').all(|group| group.len() == 4));
        let decoded = RecoverySecret::decode(expected).unwrap();
        assert_eq!(*decoded.0, bytes);
    }
}

#[test]
fn accepts_only_the_agreed_ascii_normalization_and_preserves_input() {
    let original = codes()[1];
    let compact = original.replace('-', "");
    let variants = [
        original.to_lowercase(),
        compact.clone(),
        format!(" \t\r\n{} \n", compact.to_lowercase()),
        original.replace('-', " \t\r\n-"),
    ];
    for input in variants {
        let before = input.clone();
        let decoded = RecoverySecret::decode(&input).unwrap();
        assert_eq!(*decoded.0, *synthetic().0);
        assert_eq!(input, before);
        assert_eq!(decoded.format_for_enrollment().as_str(), original);
    }
}

#[test]
fn rejects_wrong_lengths_padding_lookalikes_and_disallowed_characters() {
    let compact = codes()[1].replace('-', "");
    let mut invalid = vec![
        String::new(),
        compact[..59].into(),
        format!("{compact}A"),
        "- \t\r\n".into(),
    ];
    for bad in [
        "0", "1", "=", "_", "8", "9", "!", "\0", "\u{00a0}", "Ａ", "а",
    ] {
        invalid.push(format!("{bad}{}", &compact[1..]));
    }
    for input in invalid {
        assert_eq!(
            RecoverySecret::decode(&input).err(),
            Some(RecoveryError::InvalidCode)
        );
    }
}

#[test]
fn caps_raw_utf8_before_normalization_including_allowed_whitespace() {
    let compact = codes()[1].replace('-', "");
    let exact = format!("{}{compact}", " ".repeat(256 - compact.len()));
    assert_eq!(exact.len(), 256);
    assert!(RecoverySecret::decode(&exact).is_ok());
    assert_eq!(
        RecoverySecret::decode(&(exact + " ")).err(),
        Some(RecoveryError::InvalidCode)
    );
    assert_eq!(
        RecoverySecret::decode(&"é".repeat(129)).err(),
        Some(RecoveryError::InvalidCode)
    );
}

#[test]
fn rejects_every_single_character_substitution_and_unequal_adjacent_transposition() {
    let compact = codes()[1].replace('-', "").into_bytes();
    for i in 0..compact.len() {
        for byte in ALPHABET {
            if *byte == compact[i] {
                continue;
            }
            let mut changed = compact.clone();
            changed[i] = *byte;
            let input = String::from_utf8(changed).unwrap();
            assert_eq!(
                RecoverySecret::decode(&input).err(),
                Some(RecoveryError::InvalidCode)
            );
        }
        if i + 1 < compact.len() && compact[i] != compact[i + 1] {
            let mut changed = compact.clone();
            changed.swap(i, i + 1);
            assert_eq!(
                RecoverySecret::decode(std::str::from_utf8(&changed).unwrap()).err(),
                Some(RecoveryError::InvalidCode)
            );
        }
    }
}

#[test]
fn rejects_noncanonical_unused_bits_even_with_the_original_valid_checksum() {
    let compact = codes()[1].replace('-', "").into_bytes();
    let canonical = digit(compact[51]).unwrap();
    assert_eq!(canonical & 15, 0);
    for unused in 1..16 {
        let mut changed = compact.clone();
        changed[51] = ALPHABET[usize::from(canonical | unused)];
        assert_eq!(
            RecoverySecret::decode(std::str::from_utf8(&changed).unwrap()).err(),
            Some(RecoveryError::InvalidCode)
        );
    }
}

#[test]
fn confirmation_rejects_another_valid_secret_and_invalid_transcription() {
    let secret = synthetic();
    assert_eq!(secret.confirm_transcription(codes()[1]), Ok(()));
    assert_eq!(
        secret.confirm_transcription(codes()[0]),
        Err(RecoveryError::ConfirmationMismatch)
    );
    assert_eq!(
        secret.confirm_transcription("invalid"),
        Err(RecoveryError::InvalidCode)
    );
    // Encoding/confirmation do not consume the reusable paper secret.
    assert_eq!(secret.confirm_transcription(codes()[1]), Ok(()));
}

#[test]
fn normalized_transcription_derives_from_decoded_bytes_only() {
    let salt = std::array::from_fn(|i| 32 + i as u8);
    let id = std::array::from_fn(|i| i as u8);
    let text = codes()[1].to_lowercase().replace('-', " \n");
    let decoded = RecoverySecret::decode(&text).unwrap();
    let generated_key = synthetic().derive_key(&salt, &id).unwrap();
    let decoded_key = decoded.derive_key(&salt, &id).unwrap();
    assert_eq!(generated_key.bytes(), decoded_key.bytes());
}

#[test]
fn varied_secrets_round_trip_and_native_generation_exercises_the_os_provider() {
    for value in 0..=255 {
        let secret = RecoverySecret(Zeroizing::new([value; 32]));
        let decoded = RecoverySecret::decode(secret.format_for_enrollment().as_str()).unwrap();
        assert_eq!(*decoded.0, *secret.0);
    }
    let generated = RecoverySecret::generate().unwrap();
    assert_eq!(
        generated.confirm_transcription(generated.format_for_enrollment().as_str()),
        Ok(())
    );
}
