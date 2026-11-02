use super::*;
use crate::credentials::CredentialInput;

const FIXTURE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/credentials-v1.json"
));
const ESCAPED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/credentials-escaped.json"
));

#[test]
fn serialization_matches_independent_python_bytes_and_preserves_text() {
    for bytes in [FIXTURE, ESCAPED] {
        let snapshot = CredentialSnapshot::decode(bytes).unwrap();
        assert_eq!(&*snapshot.encode().unwrap(), FIXTURE);
    }
    let mut value: serde_json::Value = serde_json::from_slice(FIXTURE).unwrap();
    value["credentials"].as_array_mut().unwrap().reverse();
    let reordered = serde_json::to_vec(&value).unwrap();
    let snapshot = CredentialSnapshot::decode(&reordered).unwrap();
    assert_eq!(&*snapshot.encode().unwrap(), FIXTURE);
    let empty = CredentialSnapshot::new(vec![]).unwrap();
    assert_eq!(
        &*empty.encode().unwrap(),
        br#"{"schema":1,"credentials":[]}"#
    );
}

#[test]
fn writer_rejects_excess_before_copying_and_preserves_existing_bytes() {
    let mut writer = PlaintextWriter::default();
    let exact = vec![b'x'; MAX_PLAINTEXT_LENGTH];
    writer.write_all(&exact).unwrap();
    assert!(writer.write_all(b"x").is_err());
    assert_eq!(writer.failure, Some(CredentialError::LimitExceeded));
    assert_eq!(&*writer.bytes, &exact);
}

fn large_snapshot(last_notes_length: usize) -> CredentialSnapshot {
    let notes = "a".repeat(65_536);
    let last_notes = "a".repeat(last_notes_length);
    let records = (0..256)
        .map(|i| {
            Credential::new(CredentialInput {
                id: &format!("{i:032x}"),
                title: "Synthetic",
                username: "",
                password: "",
                url: "",
                notes: if i == 255 { &last_notes } else { &notes },
                created_at: 0,
                updated_at: 0,
            })
            .unwrap()
        })
        .collect();
    CredentialSnapshot::new(records).unwrap()
}

#[test]
fn serialized_snapshot_accepts_exact_cap_and_rejects_one_byte_over() {
    let baseline_length = large_snapshot(0).encode().unwrap().len();
    let remaining = MAX_PLAINTEXT_LENGTH - baseline_length;
    assert!(remaining < 65_536);
    let exact = large_snapshot(remaining).encode().unwrap();
    assert_eq!(exact.len(), MAX_PLAINTEXT_LENGTH);
    assert_eq!(
        CredentialSnapshot::decode(&exact)
            .unwrap()
            .credentials()
            .len(),
        256
    );
    assert!(matches!(
        large_snapshot(remaining + 1).encode(),
        Err(CredentialError::LimitExceeded)
    ));
}
