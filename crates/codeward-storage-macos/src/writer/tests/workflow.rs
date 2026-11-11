use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::process::ExitStatusExt,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use codeward_core::{
    credentials::{Credential, CredentialInput, CredentialSnapshot},
    transaction::{
        AccessProof, CallerId, Committer, HeadlessVault, HostCompletion, OperationStatus, Phase,
        Request, RequestId, Session, TransactionError,
    },
    vault::unlock_with_master,
};

use super::*;
use crate::test_support::MASTER_TESTS;

const OWNER: CallerId = CallerId(7);
const PASSWORD: &str = "synthetic managed workflow password";
const REPLACEMENT: &str = "synthetic managed workflow replacement";
const TITLE: &str = "Synthetic managed credential";
const CHECKPOINT: &str = "codeward-managed-workflow-checkpoint";
type Worker = Committer<ExperimentalWriterLease>;

fn managed(home: &Path) -> ExperimentalStorage {
    // Supply a disposable account home to the real managed resolver. Only
    // account lookup is substituted; the writer and native calls are unchanged.
    ExperimentalStorage::from_path(VaultPath::managed_for_test(home).unwrap())
}

fn request(revision: u64) -> Request {
    Request {
        id: RequestId([revision as u8; 16]),
        expected_revision: revision,
    }
}

fn authenticate(
    session: &mut Session,
    worker: &Worker,
    password: &str,
) -> Result<(), TransactionError> {
    let job = session.begin_master_authentication(worker, OWNER, password)?;
    let result = std::thread::spawn(move || job.run()).join().unwrap();
    session.complete_authentication(worker, OWNER, result)
}

fn publish(worker: &mut Worker, previous: Option<u64>) {
    for phase in [Phase::Staged, Phase::Admitted, Phase::Published] {
        assert_eq!(worker.advance(), Some(OperationStatus::Pending(phase)));
        assert_eq!(worker.committed_revision(), previous);
    }
}

fn snapshot() -> CredentialSnapshot {
    let record = Credential::new(CredentialInput {
        id: "00000000000000000000000000000001",
        title: TITLE,
        username: "synthetic-user",
        password: "synthetic-record-password",
        url: "",
        notes: "",
        created_at: 0,
        updated_at: 0,
    })
    .unwrap();
    CredentialSnapshot::new(vec![record]).unwrap()
}

#[test]
fn managed_workflow_child() {
    let Some(home) = std::env::var_os("CODEWARD_MANAGED_WORKFLOW_HOME") else {
        return;
    };
    let home = Path::new(&home);
    assert_eq!(
        home.parent(),
        Some(std::env::temp_dir().canonicalize().unwrap().as_path())
    );
    assert!(home.file_name().is_some_and(|name| {
        name.as_encoded_bytes()
            .starts_with(b"codeward-native-test-")
    }));
    let (mut session, mut worker) = Committer::open_locked(managed(home), OWNER).unwrap();
    authenticate(&mut session, &worker, PASSWORD).unwrap();
    let operation = session
        .prepare_contents(&mut worker, OWNER, request(1), &snapshot())
        .unwrap();
    match std::env::var("CODEWARD_MANAGED_WORKFLOW_PHASE")
        .unwrap()
        .as_str()
    {
        "staged" => assert_eq!(
            worker.advance(),
            Some(OperationStatus::Pending(Phase::Staged))
        ),
        "committed" => {
            publish(&mut worker, Some(1));
            assert_eq!(
                worker.advance(),
                Some(OperationStatus::Committed { revision: 2 })
            );
            assert_eq!(
                worker.status(OWNER, &operation),
                Ok(OperationStatus::Committed { revision: 2 })
            );
        }
        _ => panic!("unknown synthetic checkpoint"),
    }
    println!("{CHECKPOINT}");
    io::stdout().flush().unwrap();
    // Keep the live writer lease open until the parent kills this process.
    loop {
        std::thread::park();
    }
}

fn kill_save_at(home: &Path, phase: &str) {
    let contender = managed(home);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "writer::tests::workflow::managed_workflow_child",
            "--nocapture",
        ])
        .env("CODEWARD_MANAGED_WORKFLOW_HOME", home)
        .env("CODEWARD_MANAGED_WORKFLOW_PHASE", phase)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line == CHECKPOINT {
                let _ = sender.send(());
                break;
            }
        }
    });
    let ready = receiver.recv_timeout(Duration::from_secs(45));
    // Compare after waiting on the child so a failed assertion cannot strand it.
    let contention = ready
        .is_ok()
        .then(|| Committer::open_locked(contender, OWNER).err());
    let killed = child.kill();
    let exit = child.wait().unwrap();
    reader.join().unwrap();
    assert!(ready.is_ok(), "managed child did not reach {phase}");
    killed.unwrap();
    assert_eq!(exit.signal(), Some(libc::SIGKILL));
    assert_eq!(
        contention,
        Some(Some(TransactionError::Storage(
            StorageError::WriterUnavailable
        )))
    );
}

#[test]
fn managed_workflow_creates_saves_survives_termination_and_recovers() {
    let _master = MASTER_TESTS.lock().unwrap();
    let dir = TestDir::new();
    let home = &dir.0;
    let vault = dir.managed_vault();
    let (mut session, mut worker) = Committer::begin_create(managed(home), OWNER).unwrap();
    let paper = session.recovery_code(OWNER).unwrap();
    session
        .prepare_creation(&mut worker, OWNER, request(0), PASSWORD, PASSWORD, &paper)
        .unwrap();
    assert!(!vault.exists());
    publish(&mut worker, None);
    assert_eq!(
        worker.advance(),
        Some(OperationStatus::Committed { revision: 1 })
    );
    session.lock();
    drop((session, worker));

    let original = fs::read(&vault).unwrap();
    kill_save_at(home, "staged");
    assert_eq!(fs::read(&vault).unwrap(), original);
    let [abandoned] = stages(vault.parent().unwrap()).try_into().ok().unwrap();
    let residue = fs::read(&abandoned).unwrap();
    kill_save_at(home, "committed");
    assert_eq!(fs::read(&abandoned).unwrap(), residue);

    let saved = fs::read(&vault).unwrap();
    let (mut session, worker) = Committer::open_locked(managed(home), OWNER).unwrap();
    assert_eq!(worker.committed_revision(), Some(2));
    assert!(authenticate(&mut session, &worker, "wrong synthetic password").is_err());
    assert!(session.is_locked());
    assert_eq!(fs::read(&vault).unwrap(), saved);
    authenticate(&mut session, &worker, PASSWORD).unwrap();
    let page = session
        .list_credentials(&worker, OWNER, session.generation(), 10, None)
        .unwrap();
    assert_eq!(page.revision, 2);
    assert_eq!(page.summaries.len(), 1);
    assert_eq!(page.summaries[0].title.as_str(), TITLE);
    session.lock();
    assert!(matches!(
        session.list_credentials(&worker, OWNER, session.generation(), 10, None),
        Err(TransactionError::Locked)
    ));
    drop((session, worker));

    let (mut recovery, mut worker) =
        Committer::open_recovery(managed(home), OWNER, &paper).unwrap();
    let replacement = recovery
        .prepare_recovery(&mut worker, OWNER, request(2), REPLACEMENT, REPLACEMENT)
        .unwrap();
    publish(&mut worker, Some(2));
    let probe = MountProbe::new();
    probe.set(Some(MountFault::Refused));
    assert_eq!(worker.advance(), Some(OperationStatus::OutcomeUnknown));
    assert_eq!(worker.committed_revision(), Some(2));
    recovery.lock();
    probe.set(None);
    assert_eq!(
        worker.reconcile_recovery(OWNER, &replacement, &paper),
        Ok(OperationStatus::Committed { revision: 3 })
    );
    assert!(recovery.is_locked());
    drop((recovery, worker));

    let (mut session, worker) = Committer::open_locked(managed(home), OWNER).unwrap();
    assert!(authenticate(&mut session, &worker, PASSWORD).is_err());
    authenticate(&mut session, &worker, REPLACEMENT).unwrap();
    let page = session
        .list_credentials(&worker, OWNER, session.generation(), 10, None)
        .unwrap();
    assert_eq!(page.revision, 3);
    assert_eq!(page.summaries.len(), 1);
    assert_eq!(page.summaries[0].title.as_str(), TITLE);
    drop((session, worker));
    let (_, worker) = Committer::open_recovery(managed(home), OWNER, &paper).unwrap();
    assert_eq!(worker.committed_revision(), Some(3));
    let recovered = unlock_with_master(&fs::read(&vault).unwrap(), REPLACEMENT).unwrap();
    assert!(*recovered.snapshot().encode().unwrap() == *snapshot().encode().unwrap());
    assert_eq!(fs::read(&abandoned).unwrap(), residue);
}

#[test]
fn managed_headless_host_creates_saves_closes_and_recovers() {
    fn completion(host: &mut HeadlessVault<ExperimentalWriterLease>) -> HostCompletion {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(result) = host.poll().unwrap() {
                return result;
            }
            assert!(host.has_pending_work());
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    let _master = MASTER_TESTS.lock().unwrap();
    let dir = TestDir::new();
    let mut host = HeadlessVault::begin_create(managed(&dir.0), OWNER).unwrap();
    let paper = host.recovery_code().unwrap();
    host.create(request(0), PASSWORD, PASSWORD, &paper).unwrap();
    assert!(matches!(
        completion(&mut host),
        HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 1 }),
            refresh: None,
            ..
        }
    ));
    host.begin_authentication(AccessProof::Master(PASSWORD))
        .unwrap();
    assert!(matches!(
        completion(&mut host),
        HostCompletion::Authentication(Ok(()))
    ));
    let generation = host.generation();
    host.save(request(1), snapshot()).unwrap();
    assert!(matches!(
        completion(&mut host),
        HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 2 }),
            refresh: Some(Ok(())),
            ..
        }
    ));
    let page = host.list_credentials(generation, 10, None).unwrap();
    assert_eq!(page.revision, 2);
    assert_eq!(page.summaries[0].title.as_str(), TITLE);
    host.close();

    let mut host = HeadlessVault::open(managed(&dir.0), OWNER).unwrap();
    assert!(host.is_locked());
    host.begin_authentication(AccessProof::Paper(&paper))
        .unwrap();
    assert!(matches!(
        completion(&mut host),
        HostCompletion::Authentication(Ok(()))
    ));
    assert!(host.list_credentials(host.generation(), 10, None).is_err());
    host.replace_master_password(request(2), REPLACEMENT, REPLACEMENT)
        .unwrap();
    assert!(matches!(
        completion(&mut host),
        HostCompletion::Transaction {
            outcome: Ok(OperationStatus::Committed { revision: 3 }),
            refresh: None,
            ..
        }
    ));
    host.begin_authentication(AccessProof::Master(REPLACEMENT))
        .unwrap();
    assert!(matches!(
        completion(&mut host),
        HostCompletion::Authentication(Ok(()))
    ));
    let page = host.list_credentials(host.generation(), 10, None).unwrap();
    assert_eq!(page.revision, 3);
    assert_eq!(page.summaries[0].title.as_str(), TITLE);
    host.lock();
    assert!(host.is_locked());
}
