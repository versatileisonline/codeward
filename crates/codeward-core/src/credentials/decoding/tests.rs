use super::*;

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/credentials-v1.json"
));
const ESCAPED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/credentials-escaped.json"
));
const RECORD: &str = r#"{"id":"00000000000000000000000000000001","title":"Synthetic","username":"","password":"","url":"","notes":"","created_at":0,"updated_at":0}"#;

fn snapshot(record: &str) -> String {
    format!(r#"{{"schema":1,"credentials":[{record}]}}"#)
}

#[test]
fn independent_python_fixtures_preserve_exact_unicode_and_control_characters() {
    for fixture in [FIXTURE, ESCAPED] {
        let decoded = CredentialSnapshot::decode(fixture.as_bytes()).unwrap();
        let fields = decoded.credentials()[0].fields();
        assert_eq!(decoded.credentials().len(), 2);
        assert_eq!(fields.username, "  résumé  ");
        assert_eq!(fields.password, " A\0\"\\🔒e\u{301} ");
        assert_eq!(fields.notes, "line one\r\nline two\t[]{}");
        assert_eq!(fields.created_at, 0);
        assert_eq!(fields.updated_at, super::super::MAX_TIMESTAMP);
    }
    let empty = CredentialSnapshot::decode(br#"{"credentials":[],"schema":1}"#).unwrap();
    assert!(empty.credentials().is_empty());
    let escaped_keys = snapshot(RECORD)
        .replace("schema", r"sch\u0065ma")
        .replace("title", r"\u0074itle");
    assert!(CredentialSnapshot::decode(escaped_keys.as_bytes()).is_ok());
}

#[test]
fn rejects_missing_unknown_duplicate_and_wrong_context_fields() {
    for text in [
        "{}",
        r#"{"schema":1}"#,
        r#"{"credentials":[]}"#,
        r#"{"schema":1,"schema":1,"credentials":[]}"#,
        r#"{"schema":1,"credentials":[],"credentials":[]}"#,
        r#"{"schema":1,"credentials":[],"title":"x"}"#,
        r#"{"schema":1,"credentials":[],"extra":0}"#,
        r#"{"schema":1,"credentials":null}"#,
    ] {
        assert!(
            CredentialSnapshot::decode(text.as_bytes()).is_err(),
            "{text}"
        );
    }
    let fields = [
        "id",
        "title",
        "username",
        "password",
        "url",
        "notes",
        "created_at",
        "updated_at",
    ];
    let values = [
        r#""00000000000000000000000000000001""#,
        r#""Synthetic""#,
        r#""""#,
        r#""""#,
        r#""""#,
        r#""""#,
        "0",
        "0",
    ];
    for (field, value) in fields.into_iter().zip(values) {
        let duplicate = format!(
            r#"{},"{field}":{value}}}"#,
            RECORD.strip_suffix('}').unwrap()
        );
        assert!(CredentialSnapshot::decode(snapshot(&duplicate).as_bytes()).is_err());
        let mut missing: serde_json::Value = serde_json::from_str(RECORD).unwrap();
        missing.as_object_mut().unwrap().remove(field);
        let missing = serde_json::to_string(&missing).unwrap();
        assert!(
            CredentialSnapshot::decode(snapshot(&missing).as_bytes()).is_err(),
            "missing {field}"
        );
    }
    for extra in [
        r#", "schema":1}"#,
        r#", "extra":0}"#,
        r#", "\u0074itle":"other"}"#,
    ] {
        let record = RECORD.strip_suffix('}').unwrap().to_owned() + extra;
        assert!(CredentialSnapshot::decode(snapshot(&record).as_bytes()).is_err());
    }
}

#[test]
fn rejects_noninteger_numbers_and_invalid_record_relationships() {
    for number in [
        "1.0",
        "1e0",
        "1E+0",
        "-0",
        "-1",
        "18446744073709551616",
        "null",
        "true",
        r#""1""#,
        "[]",
        "{}",
        "01",
        "+1",
    ] {
        let text = snapshot(RECORD).replace("\"schema\":1", &format!("\"schema\":{number}"));
        assert!(CredentialSnapshot::decode(text.as_bytes()).is_err());
        let record = RECORD.replace("\"created_at\":0", &format!("\"created_at\":{number}"));
        assert!(CredentialSnapshot::decode(snapshot(&record).as_bytes()).is_err());
    }
    for record in [
        RECORD.replace("\"created_at\":0", "\"created_at\":1"),
        RECORD.replace("\"updated_at\":0", "\"updated_at\":253402300800000"),
        RECORD.replace("Synthetic", ""),
        RECORD.replace(
            "00000000000000000000000000000001",
            "0000000000000000000000000000000A",
        ),
    ] {
        assert!(CredentialSnapshot::decode(snapshot(&record).as_bytes()).is_err());
    }
    let duplicate_ids = format!("{RECORD},{RECORD}");
    assert!(CredentialSnapshot::decode(snapshot(&duplicate_ids).as_bytes()).is_err());
}

#[test]
fn rejects_truncation_trailing_data_invalid_utf8_and_bad_string_escapes() {
    let input = snapshot(RECORD);
    for length in 0..input.len() {
        assert!(CredentialSnapshot::decode(&input.as_bytes()[..length]).is_err());
    }
    for suffix in ["{}", "null", "0", ",", "x"] {
        assert!(CredentialSnapshot::decode((input.clone() + suffix).as_bytes()).is_err());
    }
    assert!(CredentialSnapshot::decode((input.clone() + " \r\n\t").as_bytes()).is_ok());
    assert!(CredentialSnapshot::decode(("\u{FEFF}".to_owned() + &input).as_bytes()).is_err());
    for bad in [
        r"\uD800",
        r"\uDC00",
        r"\uD800\u0041",
        r"\uGGGG",
        r"\q",
        "\n",
    ] {
        let record = RECORD.replace("Synthetic", bad);
        assert!(CredentialSnapshot::decode(snapshot(&record).as_bytes()).is_err());
    }
    let mut invalid_utf8 = input.into_bytes();
    let position = invalid_utf8.iter().position(|b| *b == b'S').unwrap();
    invalid_utf8[position] = 0xFF;
    let before = invalid_utf8.clone();
    assert!(CredentialSnapshot::decode(&invalid_utf8).is_err());
    assert_eq!(invalid_utf8, before);
}
