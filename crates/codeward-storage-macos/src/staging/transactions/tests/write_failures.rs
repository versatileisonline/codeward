//! Actual kernel write refusal, confined to a dedicated synthetic child process.

use super::*;
use codeward_core::transaction::{OperationRef, Session};
use std::os::unix::fs::OpenOptionsExt;
use std::time::Instant;

fn limits() -> libc::rlimit {
    let mut value = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: value is live, aligned writable storage of the SDK's rlimit type.
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
    // Only called by the exact-filtered child helper, never a parent test or
    // runtime operation. A panic exits that child without affecting its parent.
    fn install(bytes: u64) -> Self {
        let original = limits();
        assert!(bytes < original.rlim_cur);
        // SAFETY: SIG_IGN is the SDK-defined disposition, not a Rust callback.
        // Ignore only in this dedicated child so write returns EFBIG to inspect.
        let handler = unsafe { libc::signal(libc::SIGXFSZ, libc::SIG_IGN) };
        assert_ne!(handler, libc::SIG_ERR);
        let limit = libc::rlimit {
            rlim_cur: bytes,
            rlim_max: original.rlim_max,
        };
        // SAFETY: limit is live initialized storage. Only the soft limit lowers;
        // retaining the hard limit permits explicit restoration without privilege.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &limit) }, 0);
        assert_eq!(limits().rlim_cur, bytes);
        assert_eq!(limits().rlim_max, original.rlim_max);
        Self { original, handler }
    }

    fn restore(self) {
        // SAFETY: original is the initialized value returned by getrlimit in
        // this child; the hard limit was never changed.
        assert_eq!(
            unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &self.original) },
            0
        );
        assert_eq!(limits().rlim_cur, self.original.rlim_cur);
        assert_eq!(limits().rlim_max, self.original.rlim_max);
        // SAFETY: handler is the unchanged disposition returned by signal above.
        assert_ne!(
            unsafe { libc::signal(libc::SIGXFSZ, self.handler) },
            libc::SIG_ERR
        );
    }
}

fn run_case(case: &str, limit: u64) {
    let dir = TestDir::new();
    let before = limits();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "staging::transactions::tests::write_failures::kernel_write_limit_subprocess_helper",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("CODEWARD_KERNEL_LIMIT_HOME", &dir.0)
        .env("CODEWARD_KERNEL_LIMIT_CASE", case)
        .env("CODEWARD_KERNEL_LIMIT_BYTES", limit.to_string())
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

#[derive(Default)]
struct ObservedIo {
    writes: Vec<Result<usize, Option<i32>>>,
    syncs: usize,
    closes: Vec<Result<(), Option<i32>>>,
}
impl StageIo for ObservedIo {
    fn write(&mut self, file: &mut File, bytes: &[u8]) -> io::Result<usize> {
        // Delegate the unmodified whole remaining slice to the real OS writer.
        let result = file.write(bytes);
        self.writes.push(match &result {
            Ok(count) => Ok(*count),
            Err(error) => Err(error.raw_os_error()),
        });
        result
    }
    fn sync(&mut self, file: &File) -> io::Result<()> {
        self.syncs += 1;
        native::sync_stage(file)
    }
    fn close(&mut self, file: File) -> io::Result<()> {
        let result = native::close_checked(file);
        self.closes.push(match &result {
            Ok(()) => Ok(()),
            Err(error) => Err(error.raw_os_error()),
        });
        result
    }
}

fn stage_refusal(home: &Path, limit: u64) {
    let path = VaultPath::managed_for_test(home).unwrap();
    let vault = home.join("Library/Application Support/Codeward/Vaults/vault.cw");
    let lease = path.acquire().unwrap();
    let bytes = fixture();
    let limit_guard = FileLimit::install(limit);
    let mut observed = ObservedIo::default();
    assert_eq!(
        lease
            .prepare_with(&bytes, &mut observed, &mut native::stage_random)
            .err(),
        Some(NativeError::Write)
    );
    let mut expected = Vec::new();
    if limit != 0 {
        expected.push(Ok(limit as usize));
    }
    expected.push(Err(Some(libc::EFBIG)));
    assert_eq!(observed.writes, expected);
    assert_eq!(observed.syncs, 0);
    assert_eq!(observed.closes, [Ok(())]);
    assert!(!vault.exists());
    let residue = stages(vault.parent().unwrap());
    assert_eq!(residue.len(), 1);
    assert_eq!(fs::read(&residue[0]).unwrap(), bytes[..limit as usize]);
    assert_eq!(fs::metadata(&residue[0]).unwrap().mode() & 0o777, 0o600);
    limit_guard.restore();
    drop(lease.prepare_stage(&bytes).unwrap());
    assert_eq!(stages(vault.parent().unwrap()).len(), 2);
    assert_eq!(fs::read(&residue[0]).unwrap(), bytes[..limit as usize]);
    assert!(!vault.exists());
}

fn prepare(
    case: &str,
    session: &mut Session,
    worker: &mut Committer<SyntheticLease>,
    id: u8,
    code: &str,
) -> Result<OperationRef, TransactionError> {
    match case {
        "create" => {
            session.prepare_creation(worker, OWNER, request(id, 0), PASSWORD, PASSWORD, code)
        }
        "recover" => {
            session.prepare_recovery(worker, OWNER, request(id, 1), REPLACEMENT, REPLACEMENT)
        }
        "update" => session.prepare_contents(worker, OWNER, request(id, 1), &empty()),
        _ => panic!("unknown synthetic transaction case"),
    }
}

fn transaction_refusal(home: &Path, case: &str, limit: u64) {
    let path = VaultPath::managed_for_test(home).unwrap();
    let vault = home.join("Library/Application Support/Codeward/Vaults/vault.cw");
    let creating = case == "create";
    if !creating {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&vault)
            .unwrap()
            .write_all(&fixture())
            .unwrap();
    }
    let (storage, control) = SyntheticStorage::from_path(path);
    let (mut session, mut worker) = match case {
        "create" => Committer::begin_create(storage, OWNER).unwrap(),
        "recover" => Committer::open_recovery(storage, OWNER, paper()).unwrap(),
        "update" => Committer::open_master(storage, OWNER, PASSWORD).unwrap(),
        _ => unreachable!(),
    };
    let mut enrolled = creating.then(|| session.recovery_code(OWNER).unwrap());
    let code = enrolled.as_ref().map_or(paper(), |code| code.as_str());
    let operation = prepare(case, &mut session, &mut worker, 1, code).unwrap();
    let mut limit_guard = Some(FileLimit::install(limit));
    let failed = OperationStatus::Failed(StorageError::Write);
    assert_eq!(worker.advance(), Some(failed));
    assert_eq!(
        worker.committed_revision(),
        if creating { None } else { Some(1) }
    );
    assert_eq!(control.lock().unwrap().events, [Event::Read, Event::Stage]);
    let residue = stages(vault.parent().unwrap());
    assert_eq!(residue.len(), 1);
    let partial = fs::read(&residue[0]).unwrap();
    assert_eq!(partial.len(), limit as usize);
    assert_eq!(fs::metadata(&residue[0]).unwrap().mode() & 0o777, 0o600);
    if creating {
        assert!(!vault.exists());
    } else {
        let actual = fs::read(&vault).unwrap();
        assert_eq!(actual, fixture());
        assert_eq!(unlock_with_master(&actual, PASSWORD).unwrap().revision(), 1);
        assert_eq!(begin_recovery(&actual, paper()).unwrap().revision(), 1);
        assert!(unlock_with_master(&actual, REPLACEMENT).is_err());
    }

    // Neither the still-active kernel refusal nor its removal authorizes replay.
    // Known IDs retain the definite failed result without any further native work.
    for restore in [false, true] {
        if restore {
            limit_guard.take().unwrap().restore();
        }
        let same = prepare(case, &mut session, &mut worker, 1, code).unwrap();
        assert_eq!(worker.status(OWNER, &operation).unwrap(), failed);
        assert_eq!(worker.status(OWNER, &same).unwrap(), failed);
        assert!(worker.advance().is_none());
        assert_eq!(control.lock().unwrap().events, [Event::Read, Event::Stage]);
        assert_eq!(stages(vault.parent().unwrap()), residue);
        assert_eq!(fs::read(&residue[0]).unwrap(), partial);
    }
    if creating {
        // Creation consumed its original enrollment during encryption. A new
        // explicit enrollment/code is required, rather than replaying that state.
        assert_eq!(
            prepare(case, &mut session, &mut worker, 2, code).err(),
            Some(TransactionError::Vault(
                codeward_core::vault::VaultError::EnrollmentFinished
            ))
        );
        session.restart_enrollment(&worker, OWNER).unwrap();
        enrolled = Some(session.recovery_code(OWNER).unwrap());
    }
    let code = enrolled.as_ref().map_or(paper(), |code| code.as_str());
    prepare(case, &mut session, &mut worker, 2, code).unwrap();
    let revision = if creating { 1 } else { 2 };
    assert_eq!(
        complete(&mut worker),
        OperationStatus::Committed { revision }
    );
    assert_eq!(worker.committed_revision(), Some(revision));
    let actual = fs::read(&vault).unwrap();
    let password = if case == "recover" {
        REPLACEMENT
    } else {
        PASSWORD
    };
    assert_eq!(
        unlock_with_master(&actual, password).unwrap().revision(),
        revision
    );
    assert_eq!(begin_recovery(&actual, code).unwrap().revision(), revision);
    if case == "recover" {
        assert!(unlock_with_master(&actual, PASSWORD).is_err());
    }
    assert_eq!(stages(vault.parent().unwrap()), residue);
    assert_eq!(fs::read(&residue[0]).unwrap(), partial);
    assert_eq!(
        control.lock().unwrap().events,
        [
            Event::Read,
            Event::Stage,
            Event::Stage,
            Event::Read,
            Event::Publish(if creating {
                PublishMode::Create
            } else {
                PublishMode::Replace
            }),
            Event::Barrier
        ]
    );
    drop((session, worker));
    let (storage, _) = SyntheticStorage::from_path(VaultPath::managed_for_test(home).unwrap());
    let (_, reopened) = Committer::open_recovery(storage, OWNER, code).unwrap();
    assert_eq!(reopened.committed_revision(), Some(revision));
}

#[test]
fn kernel_write_limit_subprocess_helper() {
    let Some(home) = std::env::var_os("CODEWARD_KERNEL_LIMIT_HOME") else {
        return;
    };
    let case = std::env::var("CODEWARD_KERNEL_LIMIT_CASE").unwrap();
    let limit = std::env::var("CODEWARD_KERNEL_LIMIT_BYTES")
        .unwrap()
        .parse()
        .unwrap();
    match case.as_str() {
        "stage" => stage_refusal(Path::new(&home), limit),
        "create" | "update" | "recover" => transaction_refusal(Path::new(&home), &case, limit),
        _ => panic!("unknown synthetic kernel write case"),
    }
}

#[test]
fn kernel_write_refusal_closes_partial_stages_without_sync_or_publication() {
    for limit in [0, 17] {
        run_case("stage", limit);
    }
}

#[test]
fn managed_kernel_write_failures_preserve_creation_contents_and_recovery_until_explicit_retry() {
    for case in ["create", "update", "recover"] {
        for limit in [0, 17] {
            run_case(case, limit);
        }
    }
}
