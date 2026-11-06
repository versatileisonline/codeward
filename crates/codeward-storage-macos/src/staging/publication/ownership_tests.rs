use super::*;
use crate::{
    PublicationError, VaultPath,
    test_support::{TestDir, stages},
};
use std::{
    fs,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

fn unavailable(dir: &TestDir) {
    assert_eq!(
        VaultPath::resolve(&dir.vault()).unwrap().acquire().err(),
        Some(NativeError::WriterUnavailable)
    );
}

fn child_probe(dir: &TestDir, expected: &str) {
    // Reuse the existing read/exclusion helper. EOF on null stdin releases an
    // acquired probe immediately, so neither outcome waits for parent input.
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "reading::tests::subprocess_lock_helper",
            "--nocapture",
        ])
        .env("CODEWARD_SYNTHETIC_LOCK_PATH", dir.vault())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout).unwrap().contains(expected));
}

#[test]
fn owned_stages_retain_exclusion_until_last_owner_without_removing_residue() {
    let dir = TestDir::new();
    let lease = dir.lease();
    let inode = fs::metadata(dir.companion()).unwrap().ino();
    let first = lease.prepare_stage(b"synthetic first").unwrap();
    let second = lease.prepare_stage(b"synthetic second").unwrap();
    let first_path = dir.0.join(first._name.to_str().unwrap());
    let second_path = dir.0.join(second._name.to_str().unwrap());
    drop(lease);
    unavailable(&dir);
    child_probe(&dir, "LEASE_BUSY");
    drop(first);
    unavailable(&dir);
    drop(second);
    child_probe(&dir, "LEASE_ACQUIRED");
    drop(dir.lease());
    assert_eq!(fs::metadata(dir.companion()).unwrap().ino(), inode);
    assert_eq!(fs::read(first_path).unwrap(), b"synthetic first");
    assert_eq!(fs::read(second_path).unwrap(), b"synthetic second");
    assert!(!dir.vault().exists());
}

#[test]
fn owned_stage_moves_to_an_unscoped_thread_and_publishes_after_caller_drop() {
    let dir = TestDir::new();
    let lease = dir.lease();
    let stage = lease
        .prepare_stage(b"synthetic complete candidate")
        .unwrap();
    drop(lease);
    let (entered, ready) = mpsc::channel();
    let (release, proceed) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        entered.send(()).unwrap();
        proceed.recv_timeout(Duration::from_secs(15)).unwrap();
        stage.publish_create()
    });
    ready.recv_timeout(Duration::from_secs(15)).unwrap();
    unavailable(&dir);
    release.send(()).unwrap();
    assert_eq!(worker.join().unwrap(), Ok(()));
    let lease = dir.lease();
    assert_eq!(
        lease.read_current().unwrap().unwrap(),
        b"synthetic complete candidate"
    );
    assert!(stages(&dir.0).is_empty());
}

#[test]
fn owned_stage_errors_release_only_their_share_and_preserve_visibility_or_residue() {
    for published in [false, true] {
        for keep_caller in [false, true] {
            let dir = TestDir::new();
            let lease = dir.lease();
            let stage = lease.prepare_stage(b"synthetic candidate").unwrap();
            let path = dir.0.join(stage._name.to_str().unwrap());
            let caller = keep_caller.then_some(lease);
            let result = if published {
                stage.publish_with(None, |parent, from, to, create| {
                    native::publish_at(parent, from, to, create)?;
                    Err(io::Error::from_raw_os_error(libc::EIO))
                })
            } else {
                dir.write(b"synthetic conflicting original");
                stage.publish_create()
            };
            if published {
                assert_eq!(result, Err(PublicationError::OutcomeUnknown));
                assert_eq!(fs::read(dir.vault()).unwrap(), b"synthetic candidate");
                assert!(!path.exists());
            } else {
                assert_eq!(
                    result,
                    Err(PublicationError::NotPublished(NativeError::Conflict))
                );
                assert_eq!(
                    fs::read(dir.vault()).unwrap(),
                    b"synthetic conflicting original"
                );
                assert_eq!(fs::read(path).unwrap(), b"synthetic candidate");
            }
            if caller.is_some() {
                unavailable(&dir);
            }
            drop(caller);
            drop(dir.lease());
        }
    }
}

#[test]
fn owned_stage_unwind_releases_last_owner_and_preserves_stage() {
    let dir = TestDir::new();
    let lease = dir.lease();
    let stage = lease.prepare_stage(b"synthetic unwind residue").unwrap();
    let path = dir.0.join(stage._name.to_str().unwrap());
    drop(lease);
    assert!(
        std::thread::spawn(move || {
            let _stage = stage;
            panic!("synthetic stage-owner unwind");
        })
        .join()
        .is_err()
    );
    drop(dir.lease());
    assert_eq!(fs::read(path).unwrap(), b"synthetic unwind residue");
    assert!(!dir.vault().exists());
}
