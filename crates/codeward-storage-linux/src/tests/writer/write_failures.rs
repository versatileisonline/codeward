//! Kernel refusals confined to exact-filtered child processes on synthetic ext4.

use super::*;
use codeward_core::{
    transaction::{OperationRef, Session},
    vault::{begin_recovery, unlock_with_master},
};
use std::time::{Duration, Instant};

const CHILD_CASE: &str = "CODEWARD_LINUX_WRITE_FAILURE_CASE";
const CHILD_LIMIT: &str = "CODEWARD_LINUX_WRITE_FAILURE_LIMIT";
const REPLACEMENT: &str = "synthetic replacement Linux password";
const FIXTURE_PASSWORD: &str = "synthetic password";

fn limits() -> libc::rlimit {
    let mut value = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: value is aligned writable storage for the native rlimit type.
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_FSIZE, &mut value) },
        0
    );
    value
}

struct FileLimit {
    original: libc::rlimit,
    handler: libc::sighandler_t,
}

impl FileLimit {
    // Process-wide controls are changed only inside the dedicated child.
    fn install(bytes: u64) -> Self {
        let original = limits();
        assert!(bytes < original.rlim_cur);
        // SAFETY: SIG_IGN is the native disposition. This child must observe
        // EFBIG instead of terminating at SIGXFSZ.
        let handler = unsafe { libc::signal(libc::SIGXFSZ, libc::SIG_IGN) };
        assert_ne!(handler, libc::SIG_ERR);
        let limit = libc::rlimit {
            rlim_cur: bytes,
            rlim_max: original.rlim_max,
        };
        // SAFETY: limit is initialized; only the child's soft limit is lowered.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &limit) }, 0);
        assert_eq!(limits().rlim_cur, bytes);
        Self { original, handler }
    }
}

impl Drop for FileLimit {
    fn drop(&mut self) {
        // SAFETY: original came from getrlimit; the hard limit was unchanged.
        assert_eq!(
            unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &self.original) },
            0
        );
        assert_eq!(limits().rlim_cur, self.original.rlim_cur);
        // SAFETY: handler is the disposition returned by signal in this child.
        assert_ne!(
            unsafe { libc::signal(libc::SIGXFSZ, self.handler) },
            libc::SIG_ERR
        );
    }
}

fn stages(parent: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .as_encoded_bytes()
                .starts_with(b".codeward.stage-")
        })
        .collect();
    paths.sort();
    paths
}

fn fixture_bytes() -> Vec<u8> {
    include_str!("../../../../codeward-core/tests/fixtures/authenticated-v1.hex")
        .split_ascii_whitespace()
        .flat_map(|line| {
            (0..line.len())
                .step_by(2)
                .map(|index| u8::from_str_radix(&line[index..index + 2], 16).unwrap())
        })
        .collect()
}

fn fixture_paper() -> &'static str {
    include_str!("../../../../codeward-core/tests/fixtures/authenticated-recovery.txt").trim()
}

fn stage_refusal(home: &Path, limit: u64) {
    let lease = ReadLease::acquire_synthetic(home).unwrap();
    let bytes = fixture_bytes();
    let guard = FileLimit::install(limit);
    let mut writes = Vec::new();
    let result = lease.prepare_stage_with(&bytes, |file, mut remaining| {
        while !remaining.is_empty() {
            // Observe the unmodified slice through actual kernel writes.
            match file.write(remaining) {
                Ok(count) => {
                    writes.push(Ok(count));
                    assert_ne!(count, 0);
                    remaining = &remaining[count..];
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    writes.push(Err(error.raw_os_error()));
                    return Err(error);
                }
            }
        }
        Ok(())
    });
    assert_eq!(result.err(), Some(NativeError::WriteStage));
    let mut expected = Vec::new();
    if limit != 0 {
        expected.push(Ok(limit as usize));
    }
    expected.push(Err(Some(libc::EFBIG)));
    assert_eq!(writes, expected);
    let residue = stages(home);
    assert_eq!(residue.len(), 1);
    let partial = fs::read(&residue[0]).unwrap();
    assert_eq!(partial, bytes[..limit as usize]);
    assert_eq!(fs::metadata(&residue[0]).unwrap().mode() & 0o7777, 0o600);
    assert_eq!(lease.read_current().unwrap(), None);
    drop(guard);
    drop(lease.prepare_stage(&bytes).unwrap());
    assert_eq!(stages(home).len(), 2);
    assert_eq!(fs::read(&residue[0]).unwrap(), partial);
    assert_eq!(lease.read_current().unwrap(), None);
}

fn prepare<L: WriterLease>(
    case: &str,
    session: &mut Session,
    worker: &mut Committer<L>,
    id: u8,
    paper: &str,
) -> Result<OperationRef, TransactionError> {
    let request = Request {
        id: RequestId([id; 16]),
        expected_revision: if case == "create" { 0 } else { 1 },
    };
    match case {
        "create" => session.prepare_creation(worker, OWNER, request, PASSWORD, PASSWORD, paper),
        "update" => session.prepare_contents(worker, OWNER, request, &snapshot()),
        "recover" => session.prepare_recovery(worker, OWNER, request, REPLACEMENT, REPLACEMENT),
        _ => panic!("unknown write failure case"),
    }
}

fn transaction_refusal(home: &Path, case: &str, limit: u64) {
    let vault = home.join(".local/share/codeward/vaults/vault.cw");
    let store = storage(home);
    let creating = case == "create";
    if !creating {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&vault)
            .unwrap()
            .write_all(&fixture_bytes())
            .unwrap();
    }
    let (mut session, mut worker) = match case {
        "create" => Committer::begin_create(store, OWNER).unwrap(),
        "update" => Committer::open_master(store, OWNER, FIXTURE_PASSWORD).unwrap(),
        "recover" => Committer::open_recovery(store, OWNER, fixture_paper()).unwrap(),
        _ => unreachable!(),
    };
    let mut enrollment = creating.then(|| session.recovery_code(OWNER).unwrap());
    let paper = enrollment
        .as_ref()
        .map_or(fixture_paper(), |code| code.as_str());
    let operation = prepare(case, &mut session, &mut worker, 101, paper).unwrap();
    let mut guard = Some(FileLimit::install(limit));
    let failed = OperationStatus::Failed(StorageError::Write);
    assert_eq!(worker.advance(), Some(failed));
    assert_eq!(
        worker.committed_revision(),
        if creating { None } else { Some(1) }
    );
    let parent = vault.parent().unwrap();
    let residue = stages(parent);
    assert_eq!(residue.len(), 1);
    let partial = fs::read(&residue[0]).unwrap();
    assert_eq!(partial.len(), limit as usize);
    assert_eq!(fs::metadata(&residue[0]).unwrap().mode() & 0o7777, 0o600);
    if creating {
        assert!(!vault.exists());
    } else {
        let original = fs::read(&vault).unwrap();
        assert_eq!(original, fixture_bytes());
        assert_eq!(
            unlock_with_master(&original, FIXTURE_PASSWORD)
                .unwrap()
                .revision(),
            1
        );
        assert_eq!(
            begin_recovery(&original, fixture_paper())
                .unwrap()
                .revision(),
            1
        );
        assert!(unlock_with_master(&original, REPLACEMENT).is_err());
    }

    // A known failed request stays failed both before and after lifting the limit.
    for restore in [false, true] {
        if restore {
            drop(guard.take().unwrap());
        }
        let same = prepare(case, &mut session, &mut worker, 101, paper).unwrap();
        assert_eq!(worker.status(OWNER, &operation).unwrap(), failed);
        assert_eq!(worker.status(OWNER, &same).unwrap(), failed);
        assert_eq!(worker.advance(), None);
        assert_eq!(stages(parent), residue);
        assert_eq!(fs::read(&residue[0]).unwrap(), partial);
    }

    if creating {
        assert_eq!(
            prepare(case, &mut session, &mut worker, 102, paper).err(),
            Some(TransactionError::Vault(
                codeward_core::vault::VaultError::EnrollmentFinished
            ))
        );
        session.restart_enrollment(&worker, OWNER).unwrap();
        enrollment = Some(session.recovery_code(OWNER).unwrap());
    }
    let paper = enrollment
        .as_ref()
        .map_or(fixture_paper(), |code| code.as_str());
    prepare(case, &mut session, &mut worker, 102, paper).unwrap();
    for phase in [Phase::Staged, Phase::Admitted, Phase::Published] {
        assert_eq!(worker.advance(), Some(OperationStatus::Pending(phase)));
        assert_eq!(
            worker.committed_revision(),
            if creating { None } else { Some(1) }
        );
    }
    let revision = if creating { 1 } else { 2 };
    assert_eq!(
        worker.advance(),
        Some(OperationStatus::Committed { revision })
    );
    assert_eq!(worker.committed_revision(), Some(revision));
    let actual = fs::read(&vault).unwrap();
    let password = match case {
        "create" => PASSWORD,
        "update" => FIXTURE_PASSWORD,
        "recover" => REPLACEMENT,
        _ => unreachable!(),
    };
    let unlocked = unlock_with_master(&actual, password).unwrap();
    assert_eq!(unlocked.revision(), revision);
    let expected = match case {
        "create" => CredentialSnapshot::new(Vec::new()).unwrap(),
        "update" => snapshot(),
        "recover" => CredentialSnapshot::decode(include_bytes!(
            "../../../../codeward-core/tests/fixtures/credentials-v1.json"
        ))
        .unwrap(),
        _ => unreachable!(),
    };
    assert_eq!(
        unlocked.snapshot().encode().unwrap().as_slice(),
        expected.encode().unwrap().as_slice()
    );
    assert_eq!(begin_recovery(&actual, paper).unwrap().revision(), revision);
    if case == "recover" {
        assert!(unlock_with_master(&actual, FIXTURE_PASSWORD).is_err());
    }
    assert_eq!(stages(parent), residue);
    assert_eq!(fs::read(&residue[0]).unwrap(), partial);
    drop((session, worker));
    let (_, reopened) = Committer::open_recovery(storage(home), OWNER, paper).unwrap();
    assert_eq!(reopened.committed_revision(), Some(revision));
}

fn run_case(case: &str, limit: u64) {
    use std::process::Stdio;
    let fixture = Fixture::new();
    let before = limits();
    let mut child = Command::new(env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::writer::write_failures::write_limit_child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_HOME, &fixture.0)
        .env(CHILD_CASE, case)
        .env(CHILD_LIMIT, limit.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let finished = loop {
        if child.try_wait().unwrap().is_some() {
            break true;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = child.wait_with_output().unwrap();
    assert!(finished, "kernel write child timed out: {case}/{limit}");
    assert!(
        output.status.success(),
        "kernel write child failed: {case}/{limit}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(limits().rlim_cur, before.rlim_cur);
    assert_eq!(limits().rlim_max, before.rlim_max);
}

#[test]
#[ignore = "exact-filtered child of the ext4 kernel write tests"]
fn write_limit_child() {
    let home = PathBuf::from(env::var_os(CHILD_HOME).unwrap());
    let root = PathBuf::from(env::var_os(FIXTURE).unwrap());
    assert_eq!(home.parent(), Some(root.as_path()));
    let case = env::var(CHILD_CASE).unwrap();
    let limit = env::var(CHILD_LIMIT).unwrap().parse().unwrap();
    match case.as_str() {
        "stage" => stage_refusal(&home, limit),
        "create" | "update" | "recover" => transaction_refusal(&home, &case, limit),
        _ => panic!("unknown kernel write case"),
    }
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_kernel_write_refusal_preserves_partial_stages() {
    for limit in [0, 17] {
        run_case("stage", limit);
    }
}

#[test]
#[ignore = "requires the dedicated ext4 nodev test mount"]
fn ext4_kernel_write_failures_require_explicit_transaction_retry() {
    for case in ["create", "update", "recover"] {
        for limit in [0, 17] {
            run_case(case, limit);
        }
    }
}
