//! Nonallocating checks before Serde can build string scratch buffers.

use super::CredentialError;
use crate::crypto::MAX_PLAINTEXT_LENGTH;

// "credentials" is the longest accepted field name. Every ASCII letter can be
// encoded as a six-byte Unicode escape; valid names need no larger raw token.
const MAX_ENCODED_KEY_LENGTH: usize = 11 * 6;

pub(super) fn preflight(input: &[u8]) -> Result<&str, CredentialError> {
    if input.len() > MAX_PLAINTEXT_LENGTH {
        return Err(CredentialError::LimitExceeded);
    }
    let text = std::str::from_utf8(input).map_err(|_| CredentialError::InvalidSnapshot)?;
    let mut stack = [0; 3];
    let mut depth = 0;
    let mut i = 0;
    while let Some(&byte) = input.get(i) {
        match byte {
            b'"' => {
                let start = i + 1;
                i = start;
                loop {
                    match input.get(i) {
                        Some(b'"') => break,
                        Some(b'\\') => i += 2,
                        Some(_) => i += 1,
                        None => return Err(CredentialError::InvalidSnapshot),
                    }
                }
                let end = i;
                i += 1;
                while input
                    .get(i)
                    .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                {
                    i += 1;
                }
                if input.get(i) == Some(&b':') && end - start > MAX_ENCODED_KEY_LENGTH {
                    return Err(CredentialError::LimitExceeded);
                }
                continue;
            }
            b'[' | b'{' => {
                if depth == stack.len() {
                    return Err(CredentialError::LimitExceeded);
                }
                stack[depth] = byte;
                depth += 1;
            }
            b']' | b'}' => {
                let expected = if byte == b']' { b'[' } else { b'{' };
                if depth == 0 || stack[depth - 1] != expected {
                    return Err(CredentialError::InvalidSnapshot);
                }
                depth -= 1;
            }
            _ => {}
        }
        i += 1;
    }
    if depth != 0 {
        return Err(CredentialError::InvalidSnapshot);
    }
    // This is only a resource preflight. Serde still checks the complete grammar.
    Ok(text)
}

pub(super) fn string_length(raw: &str, max: usize) -> Result<usize, CredentialError> {
    let invalid = CredentialError::InvalidSnapshot;
    let bytes = raw.as_bytes();
    if bytes.len() < 2 || bytes.first() != Some(&b'"') || bytes.last() != Some(&b'"') {
        return Err(invalid);
    }
    let mut i = 1;
    let mut length = 0;
    while i < bytes.len() - 1 {
        if bytes[i] == b'\\' {
            let escape = *bytes.get(i + 1).ok_or(invalid)?;
            i += 2;
            length += match escape {
                b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => 1,
                b'u' => {
                    let high = hex_quad(bytes, i)?;
                    i += 4;
                    let codepoint = if (0xD800..=0xDBFF).contains(&high) {
                        if bytes.get(i..i + 2) != Some(b"\\u") {
                            return Err(invalid);
                        }
                        let low = hex_quad(bytes, i + 2)?;
                        i += 6;
                        if !(0xDC00..=0xDFFF).contains(&low) {
                            return Err(invalid);
                        }
                        0x10000 + ((u32::from(high) - 0xD800) << 10) + u32::from(low) - 0xDC00
                    } else {
                        u32::from(high)
                    };
                    char::from_u32(codepoint).ok_or(invalid)?.len_utf8()
                }
                _ => return Err(invalid),
            };
        } else {
            if bytes[i] < 0x20 || bytes[i] == b'"' {
                return Err(invalid);
            }
            // Raw UTF-8 bytes already passed preflight. Counting bytes preserves
            // the specified limits rather than treating code points as bytes.
            length += 1;
            i += 1;
        }
        if length > max {
            return Err(CredentialError::LimitExceeded);
        }
    }
    if i != bytes.len() - 1 {
        return Err(invalid);
    }
    Ok(length)
}

fn hex_quad(bytes: &[u8], start: usize) -> Result<u16, CredentialError> {
    let invalid = CredentialError::InvalidSnapshot;
    let digits = bytes.get(start..start + 4).ok_or(invalid)?;
    let text = std::str::from_utf8(digits).map_err(|_| invalid)?;
    if !digits.iter().all(u8::is_ascii_hexdigit) {
        return Err(invalid);
    }
    u16::from_str_radix(text, 16).map_err(|_| invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_bounds_depth_and_keys_while_ignoring_quoted_delimiters() {
        assert!(preflight(br#"{"credentials":[{"notes":"[{}]\\\""}]}"#).is_ok());
        assert!(matches!(
            preflight(b"[[[[]]]]"),
            Err(CredentialError::LimitExceeded)
        ));
        for invalid in [b"[}".as_slice(), b"[", b"\"unterminated", b"\"trailing\\"] {
            assert!(preflight(invalid).is_err());
        }
        let escaped = r"\u0063\u0072\u0065\u0064\u0065\u006e\u0074\u0069\u0061\u006c\u0073";
        assert!(preflight(format!("{{\"{escaped}\":[]}}").as_bytes()).is_ok());
        let oversized = "x".repeat(MAX_ENCODED_KEY_LENGTH + 1);
        assert!(matches!(
            preflight(format!("{{\"{oversized}\":0}}").as_bytes()),
            Err(CredentialError::LimitExceeded)
        ));
    }

    #[test]
    fn preflight_caps_plaintext_before_parsing() {
        let mut bytes = vec![b' '; MAX_PLAINTEXT_LENGTH];
        assert!(preflight(&bytes).is_ok());
        bytes.push(b' ');
        assert!(matches!(
            preflight(&bytes),
            Err(CredentialError::LimitExceeded)
        ));
    }

    #[test]
    fn unicode_escape_lengths_match_scalar_utf8_lengths() {
        for scalar in 0..=0xFFFF {
            if let Some(character) = char::from_u32(scalar) {
                let raw = format!(r#""\u{scalar:04x}""#);
                assert_eq!(string_length(&raw, 4).unwrap(), character.len_utf8());
            }
        }
        for scalar in [0x10000, 0x1F512, 0x10FFFF] {
            let high = 0xD800 + ((scalar - 0x10000) >> 10);
            let low = 0xDC00 + ((scalar - 0x10000) & 0x3FF);
            let raw = format!(r#""\u{high:04x}\u{low:04x}""#);
            assert_eq!(string_length(&raw, 4).unwrap(), 4);
            assert!(string_length(&raw, 3).is_err());
        }
        for text in ["", "a\0\n\t\r\u{8}\u{c}\"\\/é€🔒", "e\u{301}"] {
            let raw = serde_json::to_string(text).unwrap();
            assert_eq!(string_length(&raw, text.len()).unwrap(), text.len());
        }
    }

    #[test]
    fn rejects_bad_escapes_and_lengths_without_decoding_a_string() {
        for raw in [
            r#""\uD800""#,
            r#""\uDC00""#,
            r#""\uD800\u0000""#,
            r#""\uGGGG""#,
            r#""\q""#,
            r#""\u🔒""#,
            "not a string",
        ] {
            assert!(string_length(raw, 100).is_err());
        }
        assert_eq!(string_length(r#""\u0061\u0062""#, 2).unwrap(), 2);
        assert!(matches!(
            string_length(r#""\u0061\u0062""#, 1),
            Err(CredentialError::LimitExceeded)
        ));
    }
}
