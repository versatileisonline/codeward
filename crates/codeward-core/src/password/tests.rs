use super::*;

fn options(mask: u8, length: usize) -> PasswordOptions {
    PasswordOptions {
        length,
        lowercase: mask & 1 != 0,
        uppercase: mask & 2 != 0,
        digits: mask & 4 != 0,
        symbols: mask & 8 != 0,
    }
}

fn alphabet(mask: u8) -> Vec<u8> {
    [
        b"abcdefghijklmnopqrstuvwxyz".as_slice(),
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ".as_slice(),
        b"0123456789".as_slice(),
        b"!@#$%^&*()-_=+[]{}:,.?".as_slice(),
    ]
    .into_iter()
    .enumerate()
    .filter(|(index, _)| mask & (1 << index) != 0)
    .flat_map(|(_, bytes)| bytes.iter().copied())
    .collect()
}

#[test]
fn default_generates_24_characters_from_all_four_classes() {
    let mut next = 0u8;
    let password = generate_with(PasswordOptions::default(), |bytes| {
        for byte in bytes {
            *byte = next;
            next = next.wrapping_add(1);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(password.as_str(), "abcdefghijklmnopqrstuvwx");
    // A full pass through the default alphabet includes the exact 22 symbols.
    let password = generate_with(
        PasswordOptions {
            length: 84,
            ..PasswordOptions::default()
        },
        |bytes| {
            for byte in bytes {
                *byte = next;
                next = next.wrapping_add(1);
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        password.as_str(),
        "#$%^&*()-_=+[]{}:,.?abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@"
    );
}

#[test]
fn every_allowed_length_and_class_union_uses_only_the_exact_selected_alphabet() {
    for mask in 1..16 {
        let expected = alphabet(mask);
        let mut distinct = expected.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), expected.len());
        for length in 12..=128 {
            let mut next = 0usize;
            let password = generate_with(options(mask, length), |bytes| {
                for byte in bytes {
                    *byte = (next % expected.len()) as u8;
                    next += 1;
                }
                Ok(())
            })
            .unwrap();
            let bytes = password.as_str().as_bytes();
            assert_eq!(bytes.len(), length);
            assert!(bytes.iter().all(|byte| expected.contains(byte)));
            if length >= expected.len() {
                assert_eq!(&bytes[..expected.len()], expected);
            }
        }
    }
}

#[test]
fn invalid_preferences_fail_before_entropy_is_requested() {
    for length in [0, 1, 11, 129, usize::MAX] {
        assert_eq!(
            generate_with(options(15, length), |_| panic!(
                "invalid length used entropy"
            ))
            .err(),
            Some(GenerationError::InvalidOptions)
        );
    }
    assert_eq!(
        generate_with(options(0, 24), |_| panic!("empty alphabet used entropy")).err(),
        Some(GenerationError::InvalidOptions)
    );
}

#[test]
fn every_byte_has_equal_accepted_preimages_for_each_selected_union() {
    for mask in 1..16 {
        let expected = alphabet(mask);
        let mut counts = vec![0usize; expected.len()];
        let mut rejected = 0;
        for value in 0..=255u8 {
            let mut calls = 0;
            let password = generate_with(options(mask, 12), |bytes| {
                calls += 1;
                // Rejected values must trigger another entropy request, never map
                // into the alphabet. Zero supplies valid bytes after rejection.
                bytes.fill(if calls == 1 { value } else { 0 });
                Ok(())
            })
            .unwrap();
            let bytes = password.as_str().as_bytes();
            assert!(bytes.iter().all(|byte| *byte == bytes[0]));
            if calls == 1 {
                let index = expected.iter().position(|byte| *byte == bytes[0]).unwrap();
                counts[index] += 1;
            } else {
                assert_eq!(calls, 2);
                assert_eq!(bytes[0], expected[0]);
                rejected += 1;
            }
        }
        // Exactly floor(256/n) preimages per character, with only the remainder
        // rejected. Includes the 32-character alphabet whose threshold is 256.
        assert!(counts.iter().all(|count| *count == 256 / expected.len()));
        assert_eq!(rejected, 256 % expected.len());
    }
}

#[test]
fn rejected_bytes_are_skipped_without_losing_accepted_bytes_or_forcing_classes() {
    let mut calls = 0;
    let password = generate_with(PasswordOptions::default(), |bytes| {
        calls += 1;
        bytes.fill(255);
        // For 84 characters, 251 is accepted and 252 is the first rejected byte.
        bytes[..4].copy_from_slice(&[252, 251, 0, 255]);
        Ok(())
    })
    .unwrap();
    assert_eq!(password.as_str(), "?a?a?a?a?a?a?a?a?a?a?a?a");
    assert_eq!(calls, 12);

    let password = generate_with(PasswordOptions::default(), |bytes| {
        bytes.fill(0);
        Ok(())
    })
    .unwrap();
    assert_eq!(password.as_str(), "aaaaaaaaaaaaaaaaaaaaaaaa");
}

#[test]
fn entropy_failure_never_returns_an_initial_or_partial_password() {
    for successful_fills in [0, 1] {
        let mut calls = 0;
        let result = generate_with(options(15, 128), |bytes| {
            calls += 1;
            bytes.fill(0);
            if calls > successful_fills {
                Err(GenerationError::RandomnessUnavailable)
            } else {
                Ok(())
            }
        });
        assert_eq!(result.err(), Some(GenerationError::RandomnessUnavailable));
        assert_eq!(calls, successful_fills + 1);
    }
}

#[test]
fn public_entry_point_uses_native_randomness_with_valid_output() {
    for mask in 1..16 {
        let password = generate_password(options(mask, 128)).unwrap();
        assert_eq!(password.as_str().len(), 128);
        assert!(
            password
                .as_str()
                .bytes()
                .all(|byte| alphabet(mask).contains(&byte))
        );
    }
    assert_eq!(MIN_PASSWORD_LENGTH, 12);
    assert_eq!(DEFAULT_PASSWORD_LENGTH, 24);
    assert_eq!(MAX_PASSWORD_LENGTH, 128);
    assert_eq!(
        generate_password(options(0, 24)).err(),
        Some(GenerationError::InvalidOptions)
    );
}
