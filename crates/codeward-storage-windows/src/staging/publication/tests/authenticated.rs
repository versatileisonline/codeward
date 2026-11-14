use super::*;
use codeward_core::{
    credentials::CredentialSnapshot,
    vault::{begin_recovery, unlock_with_master},
};
use std::{path::PathBuf, sync::Mutex};

const PASSWORD: &str = "synthetic password";
const REPLACEMENT: &str = "synthetic Windows replacement password";
static AUTHENTICATION: Mutex<()> = Mutex::new(());

fn fixture_bytes() -> Vec<u8> {
    include_str!("../../../../../codeward-core/tests/fixtures/authenticated-v1.hex")
        .split_ascii_whitespace()
        .flat_map(|line| {
            (0..line.len())
                .step_by(2)
                .map(|index| u8::from_str_radix(&line[index..index + 2], 16).unwrap())
        })
        .collect()
}
fn paper() -> &'static str {
    include_str!("../../../../../codeward-core/tests/fixtures/authenticated-recovery.txt").trim()
}

#[test]
fn ntfs_publication_preserves_authenticated_content_and_coherent_access_slots() {
    let _authentication = AUTHENTICATION.lock().unwrap();
    let fixture = Fixture::new();
    crate::managed::protect_fixture(&fixture.0);
    let lease = ReadLease::acquire_managed_at(&fixture.0).unwrap();
    let initial = fixture_bytes();
    lease
        .prepare_stage(&initial)
        .unwrap()
        .publish_create()
        .unwrap();
    let current = lease.read_current().unwrap().unwrap();
    assert_eq!(current, initial);
    let unlocked = unlock_with_master(&current, PASSWORD).unwrap();
    assert_eq!(unlocked.revision(), 1);
    assert_eq!(begin_recovery(&current, paper()).unwrap().revision(), 1);
    let empty = CredentialSnapshot::new(Vec::new()).unwrap();
    let update = unlocked.prepare_contents(1, &empty).unwrap().into_bytes();
    unlocked.lock();
    lease
        .prepare_stage(&update)
        .unwrap()
        .publish_replace(&current)
        .unwrap();
    let current = lease.read_current().unwrap().unwrap();
    assert_eq!(current, update);
    let updated = unlock_with_master(&current, PASSWORD).unwrap();
    assert_eq!(updated.revision(), 2);
    assert_eq!(
        updated.snapshot().encode().unwrap().as_slice(),
        empty.encode().unwrap().as_slice()
    );
    updated.lock();
    let recovery = begin_recovery(&current, paper()).unwrap();
    let replacement = recovery
        .prepare_master_password(2, REPLACEMENT, REPLACEMENT)
        .unwrap()
        .into_bytes();
    lease
        .prepare_stage(&replacement)
        .unwrap()
        .publish_replace(&current)
        .unwrap();
    drop(lease);
    let reopened = ReadLease::acquire_managed_at(&fixture.0).unwrap();
    let actual = reopened.read_current().unwrap().unwrap();
    assert_eq!(actual, replacement);
    let changed = unlock_with_master(&actual, REPLACEMENT).unwrap();
    assert_eq!(changed.revision(), 3);
    assert_eq!(
        changed.snapshot().encode().unwrap().as_slice(),
        empty.encode().unwrap().as_slice()
    );
    changed.lock();
    assert!(unlock_with_master(&actual, PASSWORD).is_err());
    assert_eq!(begin_recovery(&actual, paper()).unwrap().revision(), 3);
    assert!(stages(&fixture.0.join("Codeward/Vaults")).is_empty());
}

#[test]
fn ntfs_process_kills_around_creation_and_replacement_preserve_complete_states() {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
        sync::mpsc,
        time::Duration,
    };
    let _authentication = AUTHENTICATION.lock().unwrap();
    let base = fixture_bytes();
    let unlocked = unlock_with_master(&base, PASSWORD).unwrap();
    let update = unlocked
        .prepare_contents(1, &CredentialSnapshot::new(Vec::new()).unwrap())
        .unwrap()
        .into_bytes();
    unlocked.lock();
    for case in ["create", "replace"] {
        for phase in ["before", "after"] {
            let fixture = Fixture::new();
            if case == "replace" {
                fs::write(fixture.0.join(VAULT), &base).unwrap();
            }
            let candidate = if case == "create" { &base } else { &update };
            fs::write(fixture.0.join("candidate.bin"), candidate).unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "staging::publication::tests::authenticated::publication_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("CODEWARD_WINDOWS_PUBLICATION_CHILD", &fixture.0)
                .env("CODEWARD_WINDOWS_PUBLICATION_CASE", case)
                .env("CODEWARD_WINDOWS_PUBLICATION_PHASE", phase)
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let stdout = child.stdout.take().unwrap();
            let (send, receive) = mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if line == "codeward-windows-publication-checkpoint" {
                        let _ = send.send(());
                        break;
                    }
                }
            });
            let ready = receive.recv_timeout(Duration::from_secs(45));
            let contender = ready
                .is_ok()
                .then(|| ReadLease::acquire_synthetic(&fixture.0).err());
            let killed = child.kill();
            let exit = child.wait().unwrap();
            reader.join().unwrap();
            assert!(ready.is_ok(), "child did not reach {case}/{phase}");
            killed.unwrap();
            assert!(!exit.success());
            assert_eq!(contender, Some(Some(NativeError::WriterUnavailable)));
            let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
            let actual = lease.read_current().unwrap();
            let expected = match (case, phase) {
                ("create", "before") => None,
                (_, "after") => Some(candidate),
                ("replace", "before") => Some(&base),
                _ => unreachable!(),
            };
            assert_eq!(actual.as_ref(), expected);
            if let Some(bytes) = actual {
                assert_eq!(
                    begin_recovery(&bytes, paper()).unwrap().revision(),
                    if case == "replace" && phase == "after" {
                        2
                    } else {
                        1
                    }
                );
            }
            let residue = stages(&fixture.0);
            assert_eq!(residue.len(), if phase == "before" { 1 } else { 0 });
            if phase == "before" {
                assert_eq!(fs::read(&residue[0]).unwrap(), *candidate);
            }
        }
    }
}

#[test]
#[ignore = "exact-filtered child of publication interruption tests"]
fn publication_child() {
    let path = PathBuf::from(std::env::var_os("CODEWARD_WINDOWS_PUBLICATION_CHILD").unwrap());
    assert_eq!(path.parent(), Some(std::env::temp_dir().as_path()));
    let case = std::env::var("CODEWARD_WINDOWS_PUBLICATION_CASE").unwrap();
    let phase = std::env::var("CODEWARD_WINDOWS_PUBLICATION_PHASE").unwrap();
    assert!(matches!(case.as_str(), "create" | "replace"));
    assert!(matches!(phase.as_str(), "before" | "after"));
    let candidate = fs::read(path.join("candidate.bin")).unwrap();
    let expected = (case == "replace").then(fixture_bytes);
    let lease = ReadLease::acquire_synthetic(&path).unwrap();
    let stage = lease.prepare_stage(&candidate).unwrap();
    drop(lease);
    let mut hooks = Hooks {
        rename: Some(Box::new(|source, parent, replace| {
            if phase == "before" {
                checkpoint();
            }
            native::publish(source, parent, replace)?;
            checkpoint()
        })),
        ..Default::default()
    };
    stage.publish_with(expected.as_deref(), &mut hooks).unwrap();
    panic!("publication child must wait at its checkpoint");
}

fn checkpoint() -> ! {
    println!("codeward-windows-publication-checkpoint");
    io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}
