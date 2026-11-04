use std::{
    fs::{self, Permissions},
    io::{BufRead, BufReader},
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use super::*;
use crate::{
    VaultPath,
    test_support::{TestDir, chmod_acl, fixture, stages},
};

#[test]
fn prepares_distinct_private_exact_stages_without_publishing_or_cleanup() {
    let dir = TestDir::new();
    let bytes = fixture();
    dir.write(b"synthetic live original");
    let lease = dir.lease();
    let first = lease.prepare_stage(&bytes).unwrap();
    let second = lease.prepare_stage(&bytes).unwrap();
    assert_ne!(first._name, second._name);
    for path in stages(&dir.0) {
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert!(native::no_extended_acl(&File::open(&path).unwrap()).unwrap());
    }
    drop((first, second));
    assert_eq!(stages(&dir.0).len(), 2);
    assert_eq!(fs::read(dir.vault()).unwrap(), b"synthetic live original");
    assert_eq!(
        lease.read_current().unwrap().unwrap(),
        b"synthetic live original"
    );
}

#[test]
fn checks_cap_and_random_failure_before_creating_any_stage() {
    let dir = TestDir::new();
    let lease = dir.lease();
    assert_eq!(
        lease.prepare_stage(&vec![0; MAX_FILE_LENGTH + 1]).err(),
        Some(NativeError::TooLarge)
    );
    let mut random = || Err(io::Error::from_raw_os_error(libc::EIO));
    assert_eq!(
        lease
            .prepare_with(b"synthetic encrypted", &mut SystemIo, &mut random)
            .err(),
        Some(NativeError::RandomnessUnavailable)
    );
    assert!(stages(&dir.0).is_empty());
    assert!(!dir.vault().exists());
    let stage = lease.prepare_stage(&vec![0; MAX_FILE_LENGTH]).unwrap();
    assert_eq!(
        fs::metadata(dir.0.join(stage._name.to_str().unwrap()))
            .unwrap()
            .len(),
        MAX_FILE_LENGTH as u64
    );
}

#[test]
fn collisions_never_follow_or_truncate_existing_objects_and_attempts_are_bounded() {
    for kind in 0..5 {
        let dir = TestDir::new();
        dir.write(b"original");
        let lease = dir.lease();
        let collision = dir
            .0
            .join(".codeward.stage-00000000000000000000000000000000");
        match kind {
            0 => fs::write(&collision, b"occupied").unwrap(),
            1 => symlink(dir.vault(), &collision).unwrap(),
            2 => fs::hard_link(dir.vault(), &collision).unwrap(),
            3 => fs::create_dir(&collision).unwrap(),
            _ => {
                assert!(
                    Command::new("/usr/bin/mkfifo")
                        .arg(&collision)
                        .status()
                        .unwrap()
                        .success()
                );
            }
        }
        let mut calls = 0;
        let mut random = || {
            calls += 1;
            Ok([0; 16])
        };
        assert_eq!(
            lease
                .prepare_with(b"candidate", &mut SystemIo, &mut random)
                .err(),
            Some(NativeError::StageUnavailable)
        );
        assert_eq!(calls, ATTEMPTS);
        assert_eq!(fs::read(dir.vault()).unwrap(), b"original");
        assert!(fs::symlink_metadata(&collision).is_ok());
        let mut calls = 0;
        let mut random = || {
            let bytes = [calls; 16];
            calls += 1;
            Ok(bytes)
        };
        let stage = lease
            .prepare_with(b"candidate", &mut SystemIo, &mut random)
            .unwrap();
        assert_eq!(calls, 2);
        assert!(
            stage
                ._name
                .to_bytes()
                .ends_with(b"01010101010101010101010101010101")
        );
    }
    let dir = TestDir::new();
    for name in [
        ".codeward.stage-00",
        ".CODEWARD.STAGE-vault",
        ".Codeward.Stage-",
        ".codeward.ſtage-00000000000000000000000000000000",
        ".codeward.writer-locK",
        ".Codeward.other-internal-name",
    ] {
        assert_eq!(
            VaultPath::resolve(&dir.0.join(name)).err(),
            Some(NativeError::InvalidPath)
        );
    }
}

#[test]
fn refuses_parent_acl_without_modifying_it_or_writing_a_stage() {
    for acl in [
        "everyone allow read,write,file_inherit,directory_inherit",
        "everyone deny delete",
    ] {
        let dir = TestDir::new();
        let lease = dir.lease();
        chmod_acl(&dir.0, acl);
        assert!(!native::no_extended_acl(lease.path.parent()).unwrap());
        assert_eq!(
            lease.prepare_stage(b"candidate").err(),
            Some(NativeError::Permissions)
        );
        assert!(stages(&dir.0).is_empty());
        assert!(!native::no_extended_acl(lease.path.parent()).unwrap());
        // Remove only our synthetic ACL after proving staging preserved it.
        assert!(
            Command::new("/bin/chmod")
                .arg("-N")
                .arg(&dir.0)
                .status()
                .unwrap()
                .success()
        );
    }
}

#[test]
fn refuses_inherited_acl_before_payload_bytes_even_when_posix_mode_is_private() {
    let dir = TestDir::new();
    let lease = dir.lease();
    let mut random = || {
        chmod_acl(
            &dir.0,
            "everyone allow read,write,file_inherit,directory_inherit",
        );
        Ok([7; 16])
    };
    assert_eq!(
        lease
            .prepare_with(b"candidate", &mut SystemIo, &mut random)
            .err(),
        Some(NativeError::Permissions)
    );
    let files = stages(&dir.0);
    assert_eq!(files.len(), 1);
    assert_eq!(fs::metadata(&files[0]).unwrap().len(), 0);
    assert_eq!(fs::metadata(&files[0]).unwrap().mode() & 0o777, 0o600);
    assert!(!native::no_extended_acl(&File::open(&files[0]).unwrap()).unwrap());
}

#[test]
fn stage_permission_gate_refuses_acl_grants_even_with_mode_0600() {
    let dir = TestDir::new();
    dir.write(b"synthetic");
    let file = File::open(dir.vault()).unwrap();
    assert!(stage_permissions(&file).is_ok());
    chmod_acl(&dir.vault(), "everyone allow read,write");
    assert_eq!(file.metadata().unwrap().mode() & 0o777, 0o600);
    assert_eq!(stage_permissions(&file), Err(NativeError::Permissions));
    assert!(!native::no_extended_acl(&file).unwrap());
}

#[derive(Clone, Copy)]
enum Fault {
    InterruptedAndShort,
    Zero,
    PartialThenError,
    Sync,
    Close(i32),
    Parent,
    Companion,
    Stage,
}
struct FaultIo<'a> {
    fault: Fault,
    dir: &'a TestDir,
    writes: usize,
    closes: usize,
}
impl StageIo for FaultIo<'_> {
    fn write(&mut self, file: &mut File, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        match self.fault {
            Fault::InterruptedAndShort if self.writes == 1 => {
                Err(io::ErrorKind::Interrupted.into())
            }
            Fault::InterruptedAndShort => file.write(&bytes[..bytes.len().min(7)]),
            Fault::Zero => Ok(0),
            Fault::PartialThenError if self.writes == 1 => file.write(&bytes[..3]),
            Fault::PartialThenError => Err(io::Error::from_raw_os_error(libc::ENOSPC)),
            Fault::Parent => {
                fs::set_permissions(&self.dir.0, Permissions::from_mode(0o777)).unwrap();
                file.write(bytes)
            }
            Fault::Companion => {
                fs::rename(self.dir.companion(), self.dir.0.join("old-lock")).unwrap();
                fs::write(self.dir.companion(), b"").unwrap();
                file.write(bytes)
            }
            Fault::Stage => {
                let path = stages(&self.dir.0).pop().unwrap();
                fs::rename(&path, self.dir.0.join("known-original-stage")).unwrap();
                fs::write(path, b"replacement").unwrap();
                file.write(bytes)
            }
            _ => file.write(bytes),
        }
    }
    fn sync(&mut self, file: &File) -> io::Result<()> {
        if matches!(self.fault, Fault::Sync) {
            return Err(io::Error::from_raw_os_error(libc::EIO));
        }
        native::sync_stage(file)
    }
    fn close(&mut self, file: File) -> io::Result<()> {
        self.closes += 1;
        native::close_checked(file)?;
        if let Fault::Close(code) = self.fault {
            return Err(io::Error::from_raw_os_error(code));
        }
        Ok(())
    }
}

#[test]
fn short_interrupted_writes_complete_but_zero_partial_sync_and_close_failures_do_not() {
    let cases = [
        (Fault::InterruptedAndShort, None),
        (Fault::Zero, Some(NativeError::Write)),
        (Fault::PartialThenError, Some(NativeError::Write)),
        (Fault::Sync, Some(NativeError::SyncStage)),
        (Fault::Close(libc::EIO), Some(NativeError::Close)),
        (Fault::Close(libc::EINTR), Some(NativeError::Close)),
        (Fault::Parent, Some(NativeError::Permissions)),
        (Fault::Companion, Some(NativeError::Conflict)),
        (Fault::Stage, Some(NativeError::Conflict)),
    ];
    for (fault, expected) in cases {
        let dir = TestDir::new();
        dir.write(b"original");
        let lease = dir.lease();
        let bytes = fixture();
        let mut operations = FaultIo {
            fault,
            dir: &dir,
            writes: 0,
            closes: 0,
        };
        let result = lease.prepare_with(&bytes, &mut operations, &mut native::stage_random);
        assert_eq!(result.err(), expected);
        assert_eq!(operations.closes, 1);
        assert!(!stages(&dir.0).is_empty());
        assert_eq!(fs::read(dir.vault()).unwrap(), b"original");
        if expected.is_none() {
            assert_eq!(fs::read(stages(&dir.0).pop().unwrap()).unwrap(), bytes);
        }
    }
}

#[test]
fn refuses_changed_parent_and_companion_before_creating_a_stage() {
    for companion in [false, true] {
        let dir = TestDir::new();
        let lease = dir.lease();
        if companion {
            fs::rename(dir.companion(), dir.0.join("old-lock")).unwrap();
            fs::write(dir.companion(), b"").unwrap();
        } else {
            fs::set_permissions(&dir.0, Permissions::from_mode(0o777)).unwrap();
        }
        assert!(lease.prepare_stage(b"candidate").is_err());
        assert!(stages(&dir.0).is_empty());
    }
}

fn checkpoint() -> ! {
    println!("stage-checkpoint");
    io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}

struct PauseIo(String);
impl StageIo for PauseIo {
    fn write(&mut self, file: &mut File, bytes: &[u8]) -> io::Result<usize> {
        if self.0 == "created" {
            checkpoint();
        }
        if self.0 == "partial" {
            file.write_all(&bytes[..bytes.len() / 2])?;
            checkpoint();
        }
        file.write(bytes)
    }
    fn sync(&mut self, file: &File) -> io::Result<()> {
        native::sync_stage(file)?;
        checkpoint();
    }
}

#[test]
fn staging_subprocess_helper() {
    let Ok(path) = std::env::var("CODEWARD_STAGING_TEST_PATH") else {
        return;
    };
    // umask is process-wide, so permission outcomes are tested only in a child.
    if let Ok(mask) = std::env::var("CODEWARD_STAGING_TEST_UMASK") {
        let mask = u16::from_str_radix(&mask, 8).unwrap();
        // SAFETY: this isolated test process owns its umask; no pointers involved.
        unsafe { libc::umask(mask) };
        let lease = VaultPath::resolve(Path::new(&path))
            .unwrap()
            .acquire()
            .unwrap();
        let result = lease.prepare_stage(&fixture());
        if mask == 0o077 {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.err(), Some(NativeError::Permissions));
            let files = stages(Path::new(&path).parent().unwrap());
            assert_eq!(files.len(), 1);
            assert_eq!(fs::metadata(&files[0]).unwrap().len(), 0);
        }
        return;
    }
    let lease = VaultPath::resolve(Path::new(&path))
        .unwrap()
        .acquire()
        .unwrap();
    let mut operations = PauseIo(std::env::var("CODEWARD_STAGING_TEST_PHASE").unwrap());
    let _ = lease.prepare_with(&fixture(), &mut operations, &mut native::stage_random);
    panic!("subprocess should stop at checkpoint");
}

#[test]
fn child_umask_preserves_restrictive_modes_and_never_chmods_to_force_progress() {
    for mask in ["077", "277"] {
        let dir = TestDir::new();
        drop(dir.lease());
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "staging::tests::staging_subprocess_helper",
                "--nocapture",
            ])
            .env("CODEWARD_STAGING_TEST_PATH", dir.vault())
            .env("CODEWARD_STAGING_TEST_UMASK", mask)
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stdout)
        );
        let file = stages(&dir.0).pop().unwrap();
        assert_eq!(
            fs::metadata(file).unwrap().mode() & 0o777,
            if mask == "077" { 0o600 } else { 0o400 }
        );
        assert!(!dir.vault().exists());
    }
}

#[test]
fn killing_staging_process_preserves_live_vault_and_unpromoted_residue() {
    for phase in ["created", "partial", "synced"] {
        let dir = TestDir::new();
        let original = fixture();
        dir.write(&original);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "staging::tests::staging_subprocess_helper",
                "--nocapture",
            ])
            .env("CODEWARD_STAGING_TEST_PATH", dir.vault())
            .env("CODEWARD_STAGING_TEST_PHASE", phase)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if line.unwrap() == "stage-checkpoint" {
                    let _ = sender.send(());
                    break;
                }
            }
        });
        let ready = receiver.recv_timeout(Duration::from_secs(15));
        child.kill().unwrap();
        child.wait().unwrap();
        reader.join().unwrap();
        assert!(ready.is_ok(), "child did not reach {phase} checkpoint");
        assert_eq!(fs::read(dir.vault()).unwrap(), original);
        let files = stages(&dir.0);
        assert_eq!(files.len(), 1);
        let expected_length = match phase {
            "created" => 0,
            "partial" => original.len() / 2,
            _ => original.len(),
        };
        assert_eq!(
            fs::metadata(&files[0]).unwrap().len(),
            expected_length as u64
        );
        if phase == "synced" {
            assert_eq!(fs::read(&files[0]).unwrap(), original);
        }
        let lease = dir.lease();
        assert_eq!(lease.read_current().unwrap(), Some(original));
        drop(lease);
        assert!(files[0].exists());
    }
}
