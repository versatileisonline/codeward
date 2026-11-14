use super::*;
use crate::{
    MAX_FILE_LENGTH,
    tests::{AUTHENTICATION, Fixture},
};
use codeward_core::{
    credentials::{Credential, CredentialInput, CredentialSnapshot},
    transaction::{
        CallerId, Committer, OperationStatus, Phase, PublishError, PublishMode, Request, RequestId,
        Storage, StorageError, TransactionError, WriterLease,
    },
    vault::{begin_recovery, unlock_with_master},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const OWNER: CallerId = CallerId(9);
const PASSWORD: &str = "synthetic Windows managed password";
const REPLACEMENT: &str = "synthetic Windows replacement password";

fn storage(base: &Path) -> ExperimentalStorage {
    ExperimentalStorage {
        base: base.to_owned(),
    }
}

fn fixture() -> Fixture {
    let fixture = Fixture::new();
    crate::managed::protect_fixture(&fixture.0);
    fixture
}

fn request(id: u8, revision: u64) -> Request {
    Request {
        id: RequestId([id; 16]),
        expected_revision: revision,
    }
}

fn snapshot() -> CredentialSnapshot {
    CredentialSnapshot::new(vec![
        Credential::new(CredentialInput {
            id: "00000000000000000000000000000001",
            title: "Synthetic Windows record",
            username: "synthetic user",
            password: "synthetic credential",
            url: "",
            notes: "",
            created_at: 0,
            updated_at: 0,
        })
        .unwrap(),
    ])
    .unwrap()
}

fn committed<L: WriterLease>(worker: &mut Committer<L>, previous: Option<u64>, revision: u64) {
    for phase in [Phase::Staged, Phase::Admitted, Phase::Published] {
        assert_eq!(worker.advance(), Some(OperationStatus::Pending(phase)));
        assert_eq!(worker.committed_revision(), previous);
    }
    assert_eq!(
        worker.advance(),
        Some(OperationStatus::Committed { revision })
    );
    assert_eq!(worker.committed_revision(), Some(revision));
}

#[test]
fn ntfs_writer_revokes_failed_stages_and_invalid_barrier_observations() {
    let fixture = fixture();
    let mut writer = storage(&fixture.0).acquire().unwrap();
    assert_eq!(writer.persist(), Err(StorageError::Barrier));
    assert_eq!(writer.read_current(), Ok(None));
    writer.stage(b"abandoned").unwrap();
    assert_eq!(
        writer.stage(&vec![0; MAX_FILE_LENGTH + 1]),
        Err(StorageError::CreateStage)
    );
    assert_eq!(
        writer.publish(PublishMode::Create),
        Err(PublishError::NotPublished)
    );
    assert_eq!(writer.persist(), Err(StorageError::Barrier));
    assert_eq!(writer.read_current(), Ok(None));
    writer.stage(b"candidate").unwrap();
    writer.publish(PublishMode::Create).unwrap();
    let vault = fixture.0.join("Codeward/Vaults/vault.cw");
    fs::write(&vault, b"changed").unwrap();
    assert_eq!(writer.persist(), Err(StorageError::Barrier));
    fs::write(&vault, b"candidate").unwrap();
    assert_eq!(writer.persist(), Err(StorageError::Barrier));
    assert_eq!(writer.read_current(), Ok(Some(b"candidate".to_vec())));
    writer.persist().unwrap();
}

#[test]
fn ntfs_core_managed_create_content_delete_password_lock_and_reopen() {
    let _authentication = AUTHENTICATION.lock().unwrap();
    let fixture = fixture();
    let vault = fixture.0.join("Codeward/Vaults/vault.cw");
    let (mut session, mut worker) = Committer::begin_create(storage(&fixture.0), OWNER).unwrap();
    let paper = session.recovery_code(OWNER).unwrap();
    session
        .prepare_creation(
            &mut worker,
            OWNER,
            request(1, 0),
            PASSWORD,
            PASSWORD,
            &paper,
        )
        .unwrap();
    assert!(!vault.exists());
    committed(&mut worker, None, 1);
    drop((session, worker));

    let (mut session, mut worker) =
        Committer::open_master(storage(&fixture.0), OWNER, PASSWORD).unwrap();
    session
        .prepare_contents(&mut worker, OWNER, request(2, 1), &snapshot())
        .unwrap();
    committed(&mut worker, Some(1), 2);
    session.reload_master(&worker, OWNER, PASSWORD).unwrap();
    let page = session
        .list_credentials(&worker, OWNER, session.generation(), 10, None)
        .unwrap();
    assert_eq!(page.summaries.len(), 1);
    assert_eq!(page.summaries[0].title.as_str(), "Synthetic Windows record");
    let empty = CredentialSnapshot::new(Vec::new()).unwrap();
    session
        .prepare_contents(&mut worker, OWNER, request(3, 2), &empty)
        .unwrap();
    committed(&mut worker, Some(2), 3);
    session.lock();
    assert!(
        session
            .list_credentials(&worker, OWNER, session.generation(), 10, None)
            .is_err()
    );
    assert!(
        session
            .prepare_contents(&mut worker, OWNER, request(4, 3), &empty)
            .is_err()
    );
    drop((session, worker));

    let (mut recovery, mut worker) =
        Committer::open_recovery(storage(&fixture.0), OWNER, &paper).unwrap();
    recovery
        .prepare_recovery(&mut worker, OWNER, request(5, 3), REPLACEMENT, REPLACEMENT)
        .unwrap();
    committed(&mut worker, Some(3), 4);
    drop((recovery, worker));
    let bytes = fs::read(&vault).unwrap();
    let unlocked = unlock_with_master(&bytes, REPLACEMENT).unwrap();
    assert_eq!(unlocked.revision(), 4);
    assert_eq!(
        unlocked.snapshot().encode().unwrap().as_slice(),
        empty.encode().unwrap().as_slice()
    );
    unlocked.lock();
    assert_eq!(begin_recovery(&bytes, &paper).unwrap().revision(), 4);
    assert!(Committer::open_master(storage(&fixture.0), OWNER, PASSWORD).is_err());
    assert_eq!(fs::read(&vault).unwrap(), bytes);
    let (mut session, worker) =
        Committer::open_master(storage(&fixture.0), OWNER, REPLACEMENT).unwrap();
    assert_eq!(worker.committed_revision(), Some(4));
    assert_eq!(
        session
            .list_credentials(&worker, OWNER, session.generation(), 10, None)
            .unwrap()
            .summaries
            .len(),
        0
    );
}

struct BarrierStorage {
    storage: ExperimentalStorage,
    fail: Arc<AtomicBool>,
}
struct BarrierLease {
    inner: ExperimentalWriterLease,
    fail: Arc<AtomicBool>,
}
impl Storage for BarrierStorage {
    type Lease = BarrierLease;
    fn acquire(self) -> Result<Self::Lease, StorageError> {
        Ok(BarrierLease {
            inner: self.storage.acquire()?,
            fail: self.fail,
        })
    }
}
impl WriterLease for BarrierLease {
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, StorageError> {
        self.inner.read_current()
    }
    fn stage(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
        self.inner.stage(bytes)
    }
    fn publish(&mut self, mode: PublishMode) -> Result<(), PublishError> {
        self.inner.publish(mode)
    }
    fn persist(&mut self) -> Result<(), StorageError> {
        let fail = self.fail.swap(false, Ordering::SeqCst);
        self.inner.persist_with(|lease, expected| {
            if fail {
                Err(NativeError::SyncNamespace)
            } else {
                lease.synchronize_namespace(expected)
            }
        })
    }
}

#[test]
fn ntfs_core_failed_barrier_requires_fresh_authenticated_reconciliation() {
    let _authentication = AUTHENTICATION.lock().unwrap();
    let fixture = fixture();
    let fail = Arc::new(AtomicBool::new(true));
    let (mut session, mut worker) = Committer::begin_create(
        BarrierStorage {
            storage: storage(&fixture.0),
            fail: Arc::clone(&fail),
        },
        OWNER,
    )
    .unwrap();
    let paper = session.recovery_code(OWNER).unwrap();
    let operation = session
        .prepare_creation(
            &mut worker,
            OWNER,
            request(1, 0),
            PASSWORD,
            PASSWORD,
            &paper,
        )
        .unwrap();
    for phase in [Phase::Staged, Phase::Admitted, Phase::Published] {
        assert_eq!(worker.advance(), Some(OperationStatus::Pending(phase)));
    }
    assert_eq!(worker.advance(), Some(OperationStatus::OutcomeUnknown));
    assert_eq!(worker.committed_revision(), None);
    session.lock();
    assert!(
        worker
            .reconcile_recovery(OWNER, &operation, "invalid")
            .is_err()
    );
    assert_eq!(worker.committed_revision(), None);
    fail.store(true, Ordering::SeqCst);
    assert_eq!(
        worker.reconcile_recovery(OWNER, &operation, &paper),
        Err(TransactionError::Storage(StorageError::Barrier))
    );
    assert_eq!(worker.committed_revision(), None);
    assert_eq!(
        worker.reconcile_recovery(OWNER, &operation, &paper),
        Ok(OperationStatus::Committed { revision: 1 })
    );
    assert!(session.is_locked());
    drop((session, worker));
    let (session, worker) = Committer::open_master(storage(&fixture.0), OWNER, PASSWORD).unwrap();
    assert_eq!(worker.committed_revision(), Some(1));
    assert!(!session.is_locked());
}

fn stages(parent: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".codeward.stage-")
        })
        .map(|path| {
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    entries.sort();
    entries
}

#[test]
fn ntfs_core_refuses_malformed_or_tampered_named_input_without_promoting_residue() {
    let _authentication = AUTHENTICATION.lock().unwrap();
    let fixture = fixture();
    let vault = fixture.0.join("Codeward/Vaults/vault.cw");
    let (mut session, mut worker) = Committer::begin_create(storage(&fixture.0), OWNER).unwrap();
    let paper = session.recovery_code(OWNER).unwrap();
    session
        .prepare_creation(
            &mut worker,
            OWNER,
            request(1, 0),
            PASSWORD,
            PASSWORD,
            &paper,
        )
        .unwrap();
    committed(&mut worker, None, 1);
    drop((session, worker));
    let valid = fs::read(&vault).unwrap();
    let mut lease = storage(&fixture.0).acquire().unwrap();
    lease.read_current().unwrap();
    lease.stage(&valid).unwrap();
    drop(lease);
    let residue = stages(vault.parent().unwrap());
    assert_eq!(residue.len(), 1);
    let mut tampered = valid.clone();
    *tampered.last_mut().unwrap() ^= 1;
    for damaged in [b"malformed synthetic vault".to_vec(), tampered] {
        fs::write(&vault, &damaged).unwrap();
        assert!(Committer::open_master(storage(&fixture.0), OWNER, PASSWORD).is_err());
        assert!(Committer::open_recovery(storage(&fixture.0), OWNER, &paper).is_err());
        assert_eq!(fs::read(&vault).unwrap(), damaged);
        assert_eq!(stages(vault.parent().unwrap()), residue);
    }
    fs::remove_file(&vault).unwrap();
    let (mut enrollment, worker) = Committer::begin_create(storage(&fixture.0), OWNER).unwrap();
    assert!(enrollment.recovery_code(OWNER).is_ok());
    assert_eq!(worker.committed_revision(), None);
    assert!(!vault.exists());
    assert_eq!(stages(vault.parent().unwrap()), residue);
}
