use super::*;
use std::{
    io::{self, BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

const CHILD_BASE: &str = "CODEWARD_WINDOWS_WRITER_BASE";
const CHILD_CASE: &str = "CODEWARD_WINDOWS_WRITER_CASE";
const CHILD_PHASE: &str = "CODEWARD_WINDOWS_WRITER_PHASE";
const CHECKPOINT: &str = "codeward-windows-writer-checkpoint";
const FIXTURE_PASSWORD: &str = "synthetic password";

fn initial() -> Vec<u8> {
    include_str!("../../../../codeward-core/tests/fixtures/authenticated-v1.hex")
        .split_ascii_whitespace()
        .flat_map(|line| {
            (0..line.len())
                .step_by(2)
                .map(|index| u8::from_str_radix(&line[index..index + 2], 16).unwrap())
        })
        .collect()
}

fn paper() -> &'static str {
    include_str!("../../../../codeward-core/tests/fixtures/authenticated-recovery.txt").trim()
}

#[test]
fn ntfs_core_process_termination_preserves_content_and_access_metadata() {
    let _authentication = AUTHENTICATION.lock().unwrap();
    let _restricted = RestrictedContext::enter();
    let original = initial();
    let access = unlock_with_master(&original, FIXTURE_PASSWORD).unwrap();
    let original_snapshot = access.snapshot().encode().unwrap();
    access.lock();
    let next_snapshot = snapshot().encode().unwrap();
    for case in ["contents", "password"] {
        for phase in ["staged", "published", "committed"] {
            let fixture = fixture();
            let vault = fixture.0.join("Codeward/Vaults/vault.cw");
            let mut writer = storage(&fixture.0).acquire().unwrap();
            assert_eq!(writer.read_current(), Ok(None));
            writer.stage(&original).unwrap();
            writer.publish(PublishMode::Create).unwrap();
            writer.persist().unwrap();
            drop(writer);
            kill_writer(&fixture.0, case, phase);
            let reopened = crate::ReadLease::acquire_managed_at(&fixture.0).unwrap();
            let actual = reopened.read_current().unwrap().unwrap();
            let published = phase != "staged";
            if !published {
                assert_eq!(actual, original);
            }
            let revision = if published { 2 } else { 1 };
            let password = if published && case == "password" {
                REPLACEMENT
            } else {
                FIXTURE_PASSWORD
            };
            let access = unlock_with_master(&actual, password).unwrap();
            assert_eq!(access.revision(), revision);
            assert_eq!(
                access.snapshot().encode().unwrap().as_slice(),
                if published && case == "contents" {
                    next_snapshot.as_slice()
                } else {
                    original_snapshot.as_slice()
                }
            );
            access.lock();
            assert_eq!(
                begin_recovery(&actual, paper()).unwrap().revision(),
                revision
            );
            let residue = stages(vault.parent().unwrap());
            assert_eq!(residue.len(), if published { 0 } else { 1 });
            if !published {
                assert_eq!(
                    begin_recovery(&residue[0].1, paper()).unwrap().revision(),
                    2
                );
            }
            drop(reopened);
            let (_session, worker) =
                Committer::open_master(storage(&fixture.0), OWNER, password).unwrap();
            assert_eq!(worker.committed_revision(), Some(revision));
            assert_eq!(stages(vault.parent().unwrap()), residue);
        }
    }
}

fn kill_writer(base: &Path, case: &str, phase: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "writer::tests::termination::transaction_child",
            "--ignored",
            "--nocapture",
        ])
        .env(CHILD_BASE, base)
        .env(CHILD_CASE, case)
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
    let ready = receive.recv_timeout(Duration::from_secs(60));
    let contender = ready
        .is_ok()
        .then(|| Committer::open_locked(storage(base), OWNER).err());
    let killed = child.kill();
    let status = child.wait().unwrap();
    reader.join().unwrap();
    assert!(ready.is_ok(), "child failed to reach {case}/{phase}");
    killed.unwrap();
    assert!(!status.success());
    assert_eq!(
        contender,
        Some(Some(TransactionError::Storage(
            StorageError::WriterUnavailable
        )))
    );
}

#[test]
#[ignore = "exact-filtered child of Windows core termination tests"]
fn transaction_child() {
    let _restricted = RestrictedContext::enter();
    let base = PathBuf::from(std::env::var_os(CHILD_BASE).unwrap());
    assert_eq!(base.parent(), Some(std::env::temp_dir().as_path()));
    let case = std::env::var(CHILD_CASE).unwrap();
    let phase = std::env::var(CHILD_PHASE).unwrap();
    assert!(matches!(
        phase.as_str(),
        "staged" | "published" | "committed"
    ));
    let (mut session, mut worker) = match case.as_str() {
        "contents" => Committer::open_master(storage(&base), OWNER, FIXTURE_PASSWORD).unwrap(),
        "password" => Committer::open_recovery(storage(&base), OWNER, paper()).unwrap(),
        _ => panic!("unknown synthetic case"),
    };
    match case.as_str() {
        "contents" => session
            .prepare_contents(&mut worker, OWNER, request(1, 1), &snapshot())
            .unwrap(),
        "password" => session
            .prepare_recovery(&mut worker, OWNER, request(1, 1), REPLACEMENT, REPLACEMENT)
            .unwrap(),
        _ => unreachable!(),
    };
    assert_eq!(
        worker.advance(),
        Some(OperationStatus::Pending(Phase::Staged))
    );
    if phase != "staged" {
        for state in [Phase::Admitted, Phase::Published] {
            assert_eq!(worker.advance(), Some(OperationStatus::Pending(state)));
            assert_eq!(worker.committed_revision(), Some(1));
        }
    }
    if phase == "committed" {
        assert_eq!(
            worker.advance(),
            Some(OperationStatus::Committed { revision: 2 })
        );
    }
    println!("{CHECKPOINT}");
    io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}
