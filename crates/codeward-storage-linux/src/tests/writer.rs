use super::*;
use crate::writer::ExperimentalStorage;
use codeward_core::{
    credentials::{Credential, CredentialInput, CredentialSnapshot},
    transaction::{
        CallerId, Committer, OperationStatus, Phase, PublishError, PublishMode, Request, RequestId,
        Storage, StorageError, TransactionError, WriterLease,
    },
};

const OWNER: CallerId = CallerId(7);
const PASSWORD: &str = "synthetic managed Linux password";
const CHILD_HOME: &str = "CODEWARD_LINUX_TRANSACTION_HOME";
const CHILD_PHASE: &str = "CODEWARD_LINUX_TRANSACTION_PHASE";
const CHECKPOINT: &str = "codeward-linux-transaction-checkpoint";

fn storage(home: &Path) -> ExperimentalStorage {
    ExperimentalStorage::from_location(
        managed::Location::prepare(home, true, test_owner()).unwrap(),
    )
}

fn request(revision: u64) -> Request {
    Request {
        id: RequestId([revision as u8; 16]),
        expected_revision: revision,
    }
}

fn snapshot() -> CredentialSnapshot {
    let record = Credential::new(CredentialInput {
        id: "00000000000000000000000000000001",
        title: "Synthetic Linux record",
        username: "synthetic user",
        password: "synthetic credential",
        url: "",
        notes: "",
        created_at: 0,
        updated_at: 0,
    })
    .unwrap();
    CredentialSnapshot::new(vec![record]).unwrap()
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_writer_refuses_stale_stage_and_barrier_observation() {
    let fixture = Fixture::new();
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
    let vault = fixture.0.join(".local/share/codeward/vaults/vault.cw");
    fs::write(&vault, b"changed").unwrap();
    assert_eq!(writer.persist(), Err(StorageError::Barrier));
    fs::write(&vault, b"candidate").unwrap();
    assert_eq!(writer.persist(), Err(StorageError::Barrier));
    assert_eq!(writer.read_current(), Ok(Some(b"candidate".to_vec())));
    writer.persist().unwrap();
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_core_workflow_reconciles_and_survives_process_termination() {
    let fixture = Fixture::new();
    let home = &fixture.0;
    let vault = home.join(".local/share/codeward/vaults/vault.cw");
    let (mut session, mut worker) = Committer::begin_create(storage(home), OWNER).unwrap();
    let paper = session.recovery_code(OWNER).unwrap();
    let operation = session
        .prepare_creation(&mut worker, OWNER, request(0), PASSWORD, PASSWORD, &paper)
        .unwrap();
    for phase in [Phase::Staged, Phase::Admitted, Phase::Published] {
        assert_eq!(worker.advance(), Some(OperationStatus::Pending(phase)));
        assert_eq!(worker.committed_revision(), None);
    }
    // Force a native admission failure at the barrier after rename.
    let parent = vault.parent().unwrap();
    fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(worker.advance(), Some(OperationStatus::OutcomeUnknown));
    assert_eq!(worker.committed_revision(), None);
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
    session.lock();
    assert!(
        worker
            .reconcile_recovery(OWNER, &operation, "invalid")
            .is_err()
    );
    assert_eq!(worker.committed_revision(), None);
    assert_eq!(
        worker.reconcile_recovery(OWNER, &operation, &paper),
        Ok(OperationStatus::Committed { revision: 1 })
    );
    assert!(session.is_locked());
    drop((session, worker));

    let original = fs::read(&vault).unwrap();
    kill_writer(home, "staged");
    assert_eq!(fs::read(&vault).unwrap(), original);
    let residue: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .as_encoded_bytes()
                .starts_with(b".codeward.stage-")
        })
        .collect();
    assert_eq!(residue.len(), 1);
    let abandoned = fs::read(&residue[0]).unwrap();
    kill_writer(home, "committed");
    assert_eq!(fs::read(&residue[0]).unwrap(), abandoned);

    let (mut session, worker) = Committer::open_master(storage(home), OWNER, PASSWORD).unwrap();
    assert_eq!(worker.committed_revision(), Some(2));
    let page = session
        .list_credentials(&worker, OWNER, session.generation(), 10, None)
        .unwrap();
    assert_eq!(page.summaries.len(), 1);
    assert_eq!(page.summaries[0].title.as_str(), "Synthetic Linux record");
}

#[test]
#[ignore = "subprocess of the dedicated ext4 transaction workflow"]
fn transaction_child() {
    let home = PathBuf::from(env::var_os(CHILD_HOME).unwrap());
    let fixture = PathBuf::from(env::var_os(FIXTURE).unwrap());
    assert_eq!(home.parent(), Some(fixture.as_path()));
    let (mut session, mut worker) =
        Committer::open_master(storage(&home), OWNER, PASSWORD).unwrap();
    session
        .prepare_contents(&mut worker, OWNER, request(1), &snapshot())
        .unwrap();
    assert_eq!(
        worker.advance(),
        Some(OperationStatus::Pending(Phase::Staged))
    );
    match env::var(CHILD_PHASE).unwrap().as_str() {
        "staged" => assert_eq!(worker.committed_revision(), Some(1)),
        "committed" => {
            for phase in [Phase::Admitted, Phase::Published] {
                assert_eq!(worker.advance(), Some(OperationStatus::Pending(phase)));
                assert_eq!(worker.committed_revision(), Some(1));
            }
            assert_eq!(
                worker.advance(),
                Some(OperationStatus::Committed { revision: 2 })
            );
        }
        _ => panic!("unknown checkpoint"),
    }
    println!("{CHECKPOINT}");
    io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_owned_stage_retains_exclusion_after_lease_drop() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    let stage = lease.prepare_stage(b"synthetic candidate").unwrap();
    drop(lease);
    assert_eq!(
        ReadLease::acquire_synthetic(&fixture.0).err(),
        Some(NativeError::WriterUnavailable)
    );
    stage.publish_create().unwrap();
    let reopened = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    assert_eq!(
        reopened.read_current().unwrap().unwrap(),
        b"synthetic candidate"
    );
}

fn kill_writer(home: &Path, phase: &str) {
    use std::{
        io::{BufRead, BufReader},
        os::unix::process::ExitStatusExt,
        process::Stdio,
        sync::mpsc,
        time::Duration,
    };
    let contender_storage = storage(home);
    let mut child = Command::new(env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::writer::transaction_child",
            "--ignored",
            "--nocapture",
        ])
        .env(CHILD_HOME, home)
        .env(CHILD_PHASE, phase)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line == CHECKPOINT {
                let _ = send.send(());
                break;
            }
        }
    });
    let ready = receive.recv_timeout(Duration::from_secs(45));
    let contender = ready
        .is_ok()
        .then(|| Committer::open_locked(contender_storage, OWNER).err());
    let killed = child.kill();
    let exit = child.wait().unwrap();
    reader.join().unwrap();
    assert!(ready.is_ok(), "child did not reach {phase}");
    killed.unwrap();
    assert_eq!(exit.signal(), Some(libc::SIGKILL));
    assert_eq!(
        contender,
        Some(Some(TransactionError::Storage(
            StorageError::WriterUnavailable
        )))
    );
}
