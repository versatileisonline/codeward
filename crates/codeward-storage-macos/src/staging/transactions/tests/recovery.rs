//! M3 acceptance through the headless host and shared managed APFS writer.

use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt, time::Instant};

use codeward_core::{
    envelope::{Slot, UnauthenticatedEnvelope},
    recovery::{RecoveryError, RecoverySecret},
    transaction::{AccessProof, HeadlessVault, HostCompletion, HostError},
    vault::VaultError,
};

use super::*;

type Host = HeadlessVault<SyntheticLease>;
const SECOND: &str = "second synthetic recovery replacement password";
const RECORD_PASSWORD: &str = "synthetic M3 credential password";

fn managed(dir: &TestDir) -> (SyntheticStorage, Controls) {
    SyntheticStorage::from_path(VaultPath::managed_for_test(&dir.0).unwrap())
}

fn write_private(path: &Path, bytes: &[u8]) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

fn seeded(dir: &TestDir) -> (SyntheticStorage, Controls) {
    let storage = managed(dir);
    write_private(&dir.managed_vault(), &fixture());
    storage
}

fn next(host: &mut Host) -> HostCompletion {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(result) = host.poll().unwrap() {
            return result;
        }
        assert!(host.has_pending_work(), "expected a recovery completion");
        assert!(Instant::now() < deadline, "recovery worker timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn authenticate(host: &mut Host, proof: AccessProof<'_>) {
    host.begin_authentication(proof).unwrap();
    assert!(matches!(next(host), HostCompletion::Authentication(Ok(()))));
}

fn committed(host: &mut Host, expected: u64) {
    assert!(matches!(
        next(host),
        HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision }), ..
        } if revision == expected
    ));
}

fn preparation_failed(host: &mut Host, expected: TransactionError) {
    assert!(matches!(
        next(host),
        HostCompletion::PreparationFailed { error, .. } if error == expected
    ));
}

fn artifacts(dir: &TestDir) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut entries: Vec<_> = fs::read_dir(dir.managed_vault().parent().unwrap())
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

#[test]
fn confirmed_enrollment_and_two_recoveries_preserve_records_and_paper_slot() {
    let _master = MASTER_TESTS.lock().unwrap();
    let dir = TestDir::new();
    let (storage, _) = managed(&dir);
    let mut host = Host::begin_create(storage, OWNER).unwrap();
    let code = host.recovery_code().unwrap();
    let other = RecoverySecret::generate().unwrap().format_for_enrollment();
    host.create(request(1, 0), PASSWORD, PASSWORD, &other)
        .unwrap();
    preparation_failed(
        &mut host,
        VaultError::Recovery(RecoveryError::ConfirmationMismatch).into(),
    );
    assert!(!dir.managed_vault().exists());
    assert!(artifacts(&dir).iter().all(|(_, bytes)| bytes.is_empty()));
    host.create(request(2, 0), PASSWORD, PASSWORD, &code)
        .unwrap();
    committed(&mut host, 1);
    assert!(host.recovery_code().is_err());
    authenticate(&mut host, AccessProof::Master(PASSWORD));
    let record = Credential::new(CredentialInput {
        id: "00000000000000000000000000000003",
        title: "Synthetic M3 service",
        username: "synthetic M3 username",
        password: RECORD_PASSWORD,
        url: "https://example.invalid/m3",
        notes: "Synthetic M3 note",
        created_at: 4,
        updated_at: 5,
    })
    .unwrap();
    let snapshot = CredentialSnapshot::new(vec![record]).unwrap();
    let expected = snapshot.encode().unwrap();
    host.save(request(3, 1), snapshot).unwrap();
    committed(&mut host, 2);
    host.close();
    let original = fs::read(dir.managed_vault()).unwrap();
    let mut previous_password = PASSWORD;
    for (index, replacement) in [REPLACEMENT, SECOND].into_iter().enumerate() {
        let revision = index as u64 + 2;
        let (storage, _) = managed(&dir);
        let mut host = Host::open(storage, OWNER).unwrap();
        assert!(host.is_locked());
        // Model a handwritten transcription using accepted ASCII normalization.
        let transcription = code.replace('-', " ").to_ascii_lowercase();
        authenticate(&mut host, AccessProof::Paper(&transcription));
        assert!(host.list_credentials(host.generation(), 10, None).is_err());
        host.replace_master_password(request(4, revision), replacement, replacement)
            .unwrap();
        committed(&mut host, revision + 1);
        host.close();
        let bytes = fs::read(dir.managed_vault()).unwrap();
        let updated = unlock_with_master(&bytes, replacement).unwrap();
        assert_eq!(updated.revision(), revision + 1);
        assert_eq!(updated.snapshot().encode().unwrap(), expected);
        updated.lock();
        assert!(unlock_with_master(&bytes, previous_password).is_err());
        assert_eq!(
            begin_recovery(&bytes, &code).unwrap().revision(),
            revision + 1
        );
        assert_eq!(
            UnauthenticatedEnvelope::parse(&bytes)
                .unwrap()
                .protected_data_key(Slot::Recovery),
            UnauthenticatedEnvelope::parse(&original)
                .unwrap()
                .protected_data_key(Slot::Recovery)
        );
        let (storage, _) = managed(&dir);
        let mut reopened = Host::open(storage, OWNER).unwrap();
        authenticate(&mut reopened, AccessProof::Master(replacement));
        let page = reopened
            .list_credentials(reopened.generation(), 10, None)
            .unwrap();
        assert_eq!(page.revision, revision + 1);
        assert_eq!(page.summaries.len(), 1);
        reopened.close();
        previous_password = replacement;
    }
    for (_, bytes) in artifacts(&dir) {
        for secret in [
            PASSWORD,
            REPLACEMENT,
            SECOND,
            RECORD_PASSWORD,
            code.as_str(),
        ] {
            assert!(
                !bytes
                    .windows(secret.len())
                    .any(|window| window == secret.as_bytes())
            );
        }
    }
}

#[test]
fn invalid_codes_and_password_confirmation_preserve_authoritative_bytes() {
    let _master = MASTER_TESTS.lock().unwrap();
    let dir = TestDir::new();
    let (storage, _) = seeded(&dir);
    let mut host = Host::open(storage, OWNER).unwrap();
    let before = artifacts(&dir);
    let other = RecoverySecret::generate().unwrap().format_for_enrollment();
    let mut typo = paper().as_bytes().to_vec();
    typo[0] = if typo[0] == b'A' { b'B' } else { b'A' };
    let typo = String::from_utf8(typo).unwrap();
    for invalid in ["invalid synthetic paper code", &typo, other.as_str()] {
        host.begin_authentication(AccessProof::Paper(invalid))
            .unwrap();
        assert!(matches!(
            next(&mut host),
            HostCompletion::Authentication(Err(_))
        ));
        assert!(host.is_locked());
        assert_eq!(artifacts(&dir), before);
    }
    authenticate(&mut host, AccessProof::Paper(paper()));
    host.replace_master_password(request(1, 1), REPLACEMENT, "different synthetic copy")
        .unwrap();
    preparation_failed(&mut host, VaultError::PasswordConfirmationMismatch.into());
    assert_eq!(artifacts(&dir), before);
    assert!(host.replace_master_password(request(2, 1), "", "").is_err());
    assert_eq!(artifacts(&dir), before);
    assert!(host.list_credentials(host.generation(), 10, None).is_err());
    host.replace_master_password(request(3, 1), REPLACEMENT, REPLACEMENT)
        .unwrap();
    committed(&mut host, 2);
}

#[test]
fn missing_and_tampered_vaults_are_never_recreated_by_recovery() {
    let _master = MASTER_TESTS.lock().unwrap();
    let dir = TestDir::new();
    let (storage, _) = seeded(&dir);
    let mut lease = storage.acquire().unwrap();
    lease.stage(&fixture()).unwrap();
    drop(lease);
    fs::remove_file(dir.managed_vault()).unwrap();
    let residue = artifacts(&dir);
    let (storage, _) = managed(&dir);
    assert!(matches!(
        Host::open(storage, OWNER),
        Err(HostError::Transaction(TransactionError::ReloadRequired))
    ));
    let (storage, _) = managed(&dir);
    assert!(Committer::open_recovery(storage, OWNER, paper()).is_err());
    assert!(!dir.managed_vault().exists());
    assert_eq!(artifacts(&dir), residue);
    write_private(&dir.managed_vault(), b"malformed synthetic vault");
    for bytes in [b"malformed synthetic vault".to_vec(), {
        let mut bytes = fixture();
        *bytes.last_mut().unwrap() ^= 1;
        bytes
    }] {
        fs::write(dir.managed_vault(), &bytes).unwrap();
        let before = artifacts(&dir);
        let (storage, _) = managed(&dir);
        assert!(Committer::open_recovery(storage, OWNER, paper()).is_err());
        assert_eq!(artifacts(&dir), before);
    }
}

#[test]
fn restarted_enrollment_and_lock_revoke_old_codes_and_queued_replacement() {
    let _master = MASTER_TESTS.lock().unwrap();
    let dir = TestDir::new();
    let (storage, _) = managed(&dir);
    let mut host = Host::begin_create(storage, OWNER).unwrap();
    let previous = host.recovery_code().unwrap();
    host.restart_enrollment().unwrap();
    let current = host.recovery_code().unwrap();
    assert_ne!(previous.as_str(), current.as_str());
    host.create(request(1, 0), PASSWORD, PASSWORD, &previous)
        .unwrap();
    preparation_failed(
        &mut host,
        VaultError::Recovery(RecoveryError::ConfirmationMismatch).into(),
    );
    host.lock();
    assert!(host.recovery_code().is_err());
    host.close();
    assert!(!dir.managed_vault().exists());
    let (storage, _) = managed(&dir);
    let mut host = Host::begin_create(storage, OWNER).unwrap();
    let fresh = host.recovery_code().unwrap();
    assert_ne!(fresh.as_str(), current.as_str());
    host.create(request(2, 0), PASSWORD, PASSWORD, &fresh)
        .unwrap();
    committed(&mut host, 1);
    authenticate(&mut host, AccessProof::Paper(&fresh));
    let before = artifacts(&dir);
    host.replace_master_password(request(3, 1), REPLACEMENT, REPLACEMENT)
        .unwrap();
    host.lock();
    assert!(matches!(
        next(&mut host),
        HostCompletion::PreparationFailed {
            error: TransactionError::Locked,
            ..
        } | HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Canceled),
            ..
        }
    ));
    assert!(host.is_locked());
    assert_eq!(artifacts(&dir), before);
    authenticate(&mut host, AccessProof::Paper(&fresh));
    host.replace_master_password(request(4, 1), REPLACEMENT, REPLACEMENT)
        .unwrap();
    committed(&mut host, 2);
}

#[test]
fn failed_and_uncertain_recovery_saves_require_explicit_retry_or_fresh_reconciliation() {
    let _master = MASTER_TESTS.lock().unwrap();
    for fault in [
        Fault::Stage,
        Fault::PublishBefore,
        Fault::PublishAfter,
        Fault::Barrier,
    ] {
        let dir = TestDir::new();
        let (storage, control) = seeded(&dir);
        let original = fixture();
        let expected = unlock_with_master(&original, PASSWORD)
            .unwrap()
            .snapshot()
            .encode()
            .unwrap();
        let mut host = Host::open(storage, OWNER).unwrap();
        authenticate(&mut host, AccessProof::Paper(paper()));
        control.lock().unwrap().fault = Some(fault);
        host.replace_master_password(request(1, 1), REPLACEMENT, REPLACEMENT)
            .unwrap();
        let HostCompletion::Transaction {
            operation,
            outcome: Ok(outcome),
            ..
        } = next(&mut host)
        else {
            panic!("expected recovery storage outcome")
        };
        let events = control.lock().unwrap().events.clone();
        host.replace_master_password(request(1, 1), REPLACEMENT, REPLACEMENT)
            .unwrap();
        assert!(matches!(
            next(&mut host),
            HostCompletion::Transaction { outcome: Ok(repeated), .. } if repeated == outcome
        ));
        assert_eq!(control.lock().unwrap().events, events);
        if fault == Fault::Stage {
            assert_eq!(outcome, OperationStatus::Failed(StorageError::Write));
            assert_eq!(fs::read(dir.managed_vault()).unwrap(), original);
        } else {
            assert_eq!(outcome, OperationStatus::OutcomeUnknown);
            host.lock();
            host.reconcile(
                &operation,
                AccessProof::Paper("invalid synthetic paper code"),
            )
            .unwrap();
            assert!(matches!(
                next(&mut host),
                HostCompletion::Transaction {
                    outcome: Err(_),
                    ..
                }
            ));
            assert_eq!(host.status(&operation), Ok(OperationStatus::OutcomeUnknown));
            control.lock().unwrap().fault = Some(Fault::Barrier);
            host.reconcile(&operation, AccessProof::Paper(paper()))
                .unwrap();
            assert!(matches!(
                next(&mut host),
                HostCompletion::Transaction {
                    outcome: Err(TransactionError::Storage(StorageError::Barrier)),
                    ..
                }
            ));
            assert_eq!(host.status(&operation), Ok(OperationStatus::OutcomeUnknown));
            host.reconcile(&operation, AccessProof::Paper(paper()))
                .unwrap();
            let wanted = if fault == Fault::PublishBefore {
                OperationStatus::Unchanged
            } else {
                OperationStatus::Committed { revision: 2 }
            };
            assert!(
                matches!(next(&mut host), HostCompletion::Transaction { outcome: Ok(actual), .. } if actual == wanted)
            );
            assert!(host.is_locked());
            assert!(host.list_credentials(host.generation(), 10, None).is_err());
            if wanted == OperationStatus::Unchanged {
                assert_eq!(fs::read(dir.managed_vault()).unwrap(), original);
                authenticate(&mut host, AccessProof::Paper(paper()));
            }
        }
        if matches!(fault, Fault::Stage | Fault::PublishBefore) {
            host.replace_master_password(request(2, 1), REPLACEMENT, REPLACEMENT)
                .unwrap();
            committed(&mut host, 2);
        }
        host.close();
        let bytes = fs::read(dir.managed_vault()).unwrap();
        let recovered = unlock_with_master(&bytes, REPLACEMENT).unwrap();
        assert_eq!(recovered.snapshot().encode().unwrap(), expected);
        assert!(unlock_with_master(&bytes, PASSWORD).is_err());
        assert_eq!(begin_recovery(&bytes, paper()).unwrap().revision(), 2);
    }
}

#[test]
#[ignore = "explicitly select under the macOS network-denied sandbox"]
fn offline_network_denial_probe() {
    let error = std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, 9))
        .expect_err("offline sandbox must deny even loopback TCP");
    assert_eq!(error.raw_os_error(), Some(libc::EPERM));
}
