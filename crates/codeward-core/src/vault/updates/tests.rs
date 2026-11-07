use super::*;
use crate::{
    credentials::{Credential, CredentialError, CredentialInput},
    encoding::EnvelopeParts,
    envelope::{UnauthenticatedEnvelope, payload_aad},
    vault::{
        begin_recovery,
        tests::{fixture, paper_code},
        unlock_with_master,
    },
};

fn edited_snapshot() -> CredentialSnapshot {
    CredentialSnapshot::new(vec![
        Credential::new(CredentialInput {
            id: "00000000000000000000000000000001",
            title: "Edited synthetic service",
            username: "test",
            password: "synthetic edited password",
            url: "",
            notes: "updated",
            created_at: 0,
            updated_at: 2,
        })
        .unwrap(),
    ])
    .unwrap()
}

#[test]
fn content_candidates_retain_slots_and_do_not_rerun_argon2_or_change_source() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let bytes = fixture("valid");
    let before = bytes.clone();
    let vault = unlock_with_master(&bytes, "synthetic password").unwrap();
    let snapshot = edited_snapshot();
    let guard = crypto::occupy_master_derivation();
    assert_eq!(
        vault.prepare_contents(0, &snapshot).err(),
        Some(VaultError::RevisionConflict)
    );
    let candidate = vault.prepare_contents(1, &snapshot).unwrap();
    let retry = vault.prepare_contents(1, &snapshot).unwrap();
    drop(guard);
    assert_eq!(&candidate.as_bytes()[44..256], &bytes[44..256]);
    assert_ne!(&candidate.as_bytes()[256..280], &bytes[256..280]);
    assert_ne!(&candidate.as_bytes()[256..280], &retry.as_bytes()[256..280]);
    assert_eq!(vault.revision(), 1);
    assert_eq!(vault.snapshot().credentials().len(), 2);
    let updated = unlock_with_master(candidate.as_bytes(), "synthetic password").unwrap();
    assert_eq!(updated.revision(), 2);
    assert_eq!(
        updated.snapshot().credentials()[0].fields().title,
        "Edited synthetic service"
    );
    assert_eq!(
        begin_recovery(candidate.as_bytes(), paper_code())
            .unwrap()
            .revision(),
        2
    );
    assert_eq!(bytes, before);
}

#[test]
fn committed_contents_refresh_validates_payload_and_schema_before_replacing_access() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let mut vault = unlock_with_master(&fixture("valid"), "synthetic password").unwrap();
    let before = vault.snapshot().encode().unwrap();
    let guard = crypto::occupy_master_derivation();
    let candidate = vault.prepare_contents(1, &edited_snapshot()).unwrap();
    let mut damaged = candidate.as_bytes().to_vec();
    *damaged.last_mut().unwrap() ^= 1;
    assert_eq!(
        vault.refresh_contents(&damaged),
        Err(VaultError::Crypto(CryptoError::AuthenticationFailed))
    );

    let mut header = vault.next_header(1).unwrap();
    let nonce = EncryptionNonce::generate().unwrap();
    header.payload_nonce = *nonce.as_bytes();
    let malformed = br#"{"schema":2,"credentials":[]}"#;
    let aad = payload_aad(&header.encode(malformed.len() + TAG_LENGTH).unwrap());
    let payload = crypto::seal(&vault.data_key, nonce, &aad, malformed).unwrap();
    let invalid = EnvelopeParts {
        header,
        encrypted_payload: &payload,
    }
    .encode()
    .unwrap();
    assert_eq!(
        vault.refresh_contents(&invalid),
        Err(VaultError::Credentials(CredentialError::InvalidSnapshot))
    );
    assert_eq!(vault.revision(), 1);
    assert_eq!(&*vault.snapshot().encode().unwrap(), &*before);
    vault.refresh_contents(candidate.as_bytes()).unwrap();
    assert_eq!(vault.revision(), 2);
    assert_eq!(vault.snapshot().credentials().len(), 1);
    drop(guard);
}

#[test]
fn recovery_prepares_coherent_password_replacement_and_keeps_paper_reusable() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let bytes = fixture("valid");
    let before = bytes.clone();
    let access = begin_recovery(&bytes, paper_code()).unwrap();
    let guard = crypto::occupy_master_derivation();
    assert_eq!(
        access
            .prepare_master_password(0, "new synthetic", "new synthetic")
            .err(),
        Some(VaultError::RevisionConflict)
    );
    assert_eq!(
        access
            .prepare_master_password(1, "new synthetic", "different")
            .err(),
        Some(VaultError::PasswordConfirmationMismatch)
    );
    assert_eq!(
        access
            .prepare_master_password(1, "new synthetic", "new synthetic")
            .err(),
        Some(VaultError::Crypto(CryptoError::DerivationBusy))
    );
    drop(guard);
    let candidate = access
        .prepare_master_password(1, "new synthetic", "new synthetic")
        .unwrap();
    assert_eq!(&candidate.as_bytes()[148..256], &bytes[148..256]);
    assert_ne!(&candidate.as_bytes()[60..76], &bytes[60..76]);
    assert_ne!(&candidate.as_bytes()[76..100], &bytes[76..100]);
    assert_ne!(&candidate.as_bytes()[256..280], &bytes[256..280]);
    let updated = unlock_with_master(candidate.as_bytes(), "new synthetic").unwrap();
    assert_eq!(updated.revision(), 2);
    assert_eq!(
        &*updated.snapshot().encode().unwrap(),
        include_bytes!("../../../tests/fixtures/credentials-v1.json")
    );
    assert!(unlock_with_master(candidate.as_bytes(), "synthetic password").is_err());
    let repeated = begin_recovery(candidate.as_bytes(), paper_code()).unwrap();
    let again = repeated
        .prepare_master_password(2, "third synthetic", "third synthetic")
        .unwrap();
    assert_eq!(
        unlock_with_master(again.as_bytes(), "third synthetic")
            .unwrap()
            .revision(),
        3
    );
    assert_eq!(&again.as_bytes()[148..256], &bytes[148..256]);
    // Preparing replacement does not revoke the old complete file or mutate input.
    assert_eq!(
        unlock_with_master(&bytes, "synthetic password")
            .unwrap()
            .revision(),
        1
    );
    assert_eq!(bytes, before);
}

#[test]
fn maximum_authenticated_revision_cannot_wrap_on_either_update_route() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let bytes = fixture("max");
    let parsed = UnauthenticatedEnvelope::parse(&bytes).unwrap();
    assert_eq!(parsed.revision(), u64::MAX);
    let vault = unlock_with_master(&bytes, "synthetic password").unwrap();
    let recovery = begin_recovery(&bytes, paper_code()).unwrap();
    let guard = crypto::occupy_master_derivation();
    assert_eq!(
        vault.prepare_contents(u64::MAX, &edited_snapshot()).err(),
        Some(VaultError::RevisionExhausted)
    );
    assert_eq!(
        recovery
            .prepare_master_password(u64::MAX, "new", "new")
            .err(),
        Some(VaultError::RevisionExhausted)
    );
    drop(guard);
}

#[test]
fn oversized_content_candidate_fails_without_advancing_the_source() {
    let _test = crypto::MASTER_TESTS.lock().unwrap();
    let bytes = fixture("valid");
    let vault = unlock_with_master(&bytes, "synthetic password").unwrap();
    let notes = "a".repeat(65_536);
    let records = (0..256)
        .map(|i| {
            Credential::new(CredentialInput {
                id: &format!("{i:032x}"),
                title: "Synthetic",
                username: "",
                password: "",
                url: "",
                notes: &notes,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap()
        })
        .collect();
    let oversized = CredentialSnapshot::new(records).unwrap();
    let guard = crypto::occupy_master_derivation();
    assert_eq!(
        vault.prepare_contents(1, &oversized).err(),
        Some(VaultError::Credentials(
            crate::credentials::CredentialError::LimitExceeded
        ))
    );
    drop(guard);
    assert_eq!(vault.revision(), 1);
    assert_eq!(
        &*vault.snapshot().encode().unwrap(),
        include_bytes!("../../../tests/fixtures/credentials-v1.json")
    );
}
