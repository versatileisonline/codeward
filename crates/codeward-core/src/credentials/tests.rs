use super::*;

fn input() -> CredentialInput<'static> {
    CredentialInput {
        id: "00000000000000000000000000000001",
        title: "Synthetic service",
        username: " test user ",
        password: " synthetic\0🔒e\u{301} ",
        url: " https://example.invalid/ ",
        notes: "line one\r\nline two",
        created_at: 0,
        updated_at: MAX_TIMESTAMP,
    }
}

#[test]
fn owns_validated_fields_without_normalization() {
    let record = Credential::new(input()).unwrap();
    let actual = record.fields();
    let expected = input();
    assert_eq!(actual.id, expected.id);
    assert_eq!(actual.title, expected.title);
    assert_eq!(actual.username, expected.username);
    assert_eq!(actual.password, expected.password);
    assert_eq!(actual.url, expected.url);
    assert_eq!(actual.notes, expected.notes);
    assert_eq!(actual.created_at, expected.created_at);
    assert_eq!(actual.updated_at, expected.updated_at);
}

#[test]
fn validates_id_syntax_and_timestamp_range_and_order() {
    for id in [
        "",
        "0",
        "0000000000000000000000000000000A",
        "0000000000000000000000000000000g",
        "000000000000000000000000000000001",
    ] {
        assert!(Credential::new(CredentialInput { id, ..input() }).is_err());
    }
    for (created_at, updated_at) in [(1, 0), (0, MAX_TIMESTAMP + 1), (u64::MAX, u64::MAX)] {
        assert!(
            Credential::new(CredentialInput {
                created_at,
                updated_at,
                ..input()
            })
            .is_err()
        );
    }
    assert!(
        Credential::new(CredentialInput {
            created_at: MAX_TIMESTAMP,
            updated_at: MAX_TIMESTAMP,
            ..input()
        })
        .is_ok()
    );
}

#[test]
fn enforces_each_field_limit_in_utf8_bytes() {
    for (index, max) in TEXT_LIMITS.into_iter().enumerate() {
        for extra in [0, 1] {
            let text = "é".repeat(max / 2) + &"x".repeat(extra);
            let mut fields = input();
            match index {
                0 => fields.title = &text,
                1 => fields.username = &text,
                2 => fields.password = &text,
                3 => fields.url = &text,
                4 => fields.notes = &text,
                _ => unreachable!(),
            }
            assert_eq!(Credential::new(fields).is_ok(), extra == 0);
        }
    }
    assert!(
        Credential::new(CredentialInput {
            title: "",
            ..input()
        })
        .is_err()
    );
    assert!(
        Credential::new(CredentialInput {
            username: "",
            password: "",
            url: "",
            notes: "",
            ..input()
        })
        .is_ok()
    );
}

#[test]
fn snapshots_sort_ids_and_reject_duplicates() {
    let later = Credential::new(CredentialInput {
        id: "ffffffffffffffffffffffffffffffff",
        ..input()
    })
    .unwrap();
    let earlier = Credential::new(input()).unwrap();
    let snapshot = CredentialSnapshot::new(vec![later, earlier]).unwrap();
    assert_eq!(snapshot.credentials()[0].fields().id, input().id);
    assert!(
        CredentialSnapshot::new(vec![])
            .unwrap()
            .credentials()
            .is_empty()
    );
    assert!(matches!(
        CredentialSnapshot::new(vec![
            Credential::new(input()).unwrap(),
            Credential::new(input()).unwrap()
        ]),
        Err(CredentialError::DuplicateId)
    ));
}

#[test]
fn rejects_excess_records_before_duplicate_check() {
    let records = (0..=MAX_CREDENTIALS)
        .map(|_| Credential::new(input()).unwrap())
        .collect();
    assert!(matches!(
        CredentialSnapshot::new(records),
        Err(CredentialError::LimitExceeded)
    ));
}
