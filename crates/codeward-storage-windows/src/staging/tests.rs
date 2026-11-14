use super::*;
use crate::tests::Fixture;
use std::fs::{self, OpenOptions};

#[test]
fn ntfs_stage_guard_prevents_substitution_across_checked_close() {
    struct Probe;
    impl StageIo for Probe {
        fn close(&mut self, file: File) -> io::Result<()> {
            let path = native::final_path(&file).unwrap();
            native::close_checked(file)?;
            // Only the read guard now pins the name, not the closed writer.
            assert!(fs::remove_file(&path).is_err());
            assert!(fs::rename(&path, path.with_extension("substitution")).is_err());
            Ok(())
        }
    }
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    let stage = lease
        .prepare_with(b"encrypted", &mut Probe, &mut native::stage_random)
        .unwrap();
    stage.verify().unwrap();
    assert_eq!(fs::read(&stages(&fixture.0)[0]).unwrap(), b"encrypted");
}

#[test]
fn ntfs_stage_refuses_a_writer_racing_the_close_reopen_boundary() {
    use std::os::windows::fs::OpenOptionsExt;
    struct WriterProbe {
        hold: bool,
        writer: Option<File>,
    }
    impl StageIo for WriterProbe {
        fn close(&mut self, file: File) -> io::Result<()> {
            let path = native::final_path(&file).unwrap();
            native::close_checked(file)?;
            let mut writer = OpenOptions::new()
                .write(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .open(path)?;
            if self.hold {
                self.writer = Some(writer);
            } else {
                // Same-size content changes must be detected independently of
                // timestamps finalized by the original write-handle close.
                writer.write_all(b"different")?;
                writer.sync_all()?;
                native::close_checked(writer)?;
            }
            Ok(())
        }
    }
    for (hold, expected) in [(true, NativeError::Open), (false, NativeError::Conflict)] {
        let fixture = Fixture::new();
        let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
        let mut probe = WriterProbe { hold, writer: None };
        assert_eq!(
            lease
                .prepare_with(b"encrypted", &mut probe, &mut native::stage_random)
                .err(),
            Some(expected)
        );
        drop(probe);
        let paths = stages(&fixture.0);
        assert_eq!(paths.len(), 1);
        assert_eq!(
            fs::read(&paths[0]).unwrap(),
            if hold {
                &b"encrypted"[..]
            } else {
                &b"different"[..]
            }
        );
        assert_eq!(lease.read_current().unwrap(), None);
    }
}

#[test]
fn ntfs_stage_retains_exclusion_in_another_process() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    let stage = lease.prepare_stage(b"encrypted").unwrap();
    drop(lease);
    let child = |expected| {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::companion_child", "--ignored"])
            .env("CODEWARD_NTFS_FIXTURE", &fixture.0)
            .env("CODEWARD_NTFS_EXPECT_LOCKED", expected)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    };
    child("yes");
    drop(stage);
    child("no");
    assert_eq!(fs::read(&stages(&fixture.0)[0]).unwrap(), b"encrypted");
}

fn stages(parent: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".codeward.stage-")
        })
        .collect();
    paths.sort();
    paths
}

#[test]
fn ntfs_prepared_stage_survives_process_termination_without_publication() {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
        sync::mpsc,
        time::Duration,
    };
    let fixture = Fixture::new();
    fs::write(fixture.0.join(crate::VAULT), b"old synthetic vault").unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "staging::tests::stage_child",
            "--ignored",
            "--nocapture",
        ])
        .env("CODEWARD_WINDOWS_STAGE_CHILD", &fixture.0)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line == "codeward-windows-stage-ready" {
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
    assert!(ready.is_ok(), "stage child did not reach checkpoint");
    killed.unwrap();
    assert!(!exit.success());
    assert_eq!(contender, Some(Some(NativeError::WriterUnavailable)));
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    assert_eq!(
        lease.read_current().unwrap().unwrap(),
        b"old synthetic vault"
    );
    let paths = stages(&fixture.0);
    assert_eq!(paths.len(), 1);
    assert_eq!(fs::read(&paths[0]).unwrap(), b"encrypted child candidate");
    drop(lease.prepare_stage(b"explicit new candidate").unwrap());
    assert_eq!(stages(&fixture.0).len(), 2);
    assert_eq!(fs::read(&paths[0]).unwrap(), b"encrypted child candidate");
}

#[test]
#[ignore = "exact-filtered child of native stage termination test"]
fn stage_child() {
    let path = std::path::PathBuf::from(std::env::var_os("CODEWARD_WINDOWS_STAGE_CHILD").unwrap());
    assert_eq!(path.parent(), Some(std::env::temp_dir().as_path()));
    let lease = ReadLease::acquire_synthetic(&path).unwrap();
    let stage = lease.prepare_stage(b"encrypted child candidate").unwrap();
    drop(lease);
    stage.verify().unwrap();
    println!("codeward-windows-stage-ready");
    io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}

#[derive(Default)]
struct FaultIo {
    fault: Option<&'static str>,
    writes: usize,
    syncs: usize,
    closes: usize,
}

impl StageIo for FaultIo {
    fn write(&mut self, file: &mut File, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        match self.fault {
            Some("zero") => Ok(0),
            Some("partial") if self.writes == 1 => file.write(&bytes[..3.min(bytes.len())]),
            Some("partial") => Err(io::ErrorKind::StorageFull.into()),
            Some("short") => file.write(&bytes[..3.min(bytes.len())]),
            Some("interrupted") if self.writes == 1 => Err(io::ErrorKind::Interrupted.into()),
            _ => file.write(bytes),
        }
    }
    fn sync(&mut self, file: &File) -> io::Result<()> {
        self.syncs += 1;
        if self.fault == Some("sync") {
            return Err(io::ErrorKind::Other.into());
        }
        file.sync_all()
    }
    fn close(&mut self, file: File) -> io::Result<()> {
        self.closes += 1;
        native::close_checked(file)?;
        if self.fault == Some("close") {
            return Err(io::ErrorKind::Other.into());
        }
        Ok(())
    }
}

#[test]
fn ntfs_stage_caps_input_before_creation_and_supports_exact_cap() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    assert_eq!(
        lease.prepare_stage(&vec![0; MAX_FILE_LENGTH + 1]).err(),
        Some(NativeError::TooLarge)
    );
    assert!(stages(&fixture.0).is_empty());
    let bytes = vec![42; MAX_FILE_LENGTH];
    let stage = lease.prepare_stage(&bytes).unwrap();
    stage.verify().unwrap();
    assert_eq!(fs::read(&stages(&fixture.0)[0]).unwrap(), bytes);
    drop(stage);
    drop(lease.prepare_stage(b"").unwrap());
    assert_eq!(stages(&fixture.0).len(), 2);
    assert_eq!(lease.read_current().unwrap(), None);
}

#[test]
fn ntfs_stage_bounds_collisions_and_refuses_failed_randomness() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    let mut io = FaultIo::default();
    drop(
        lease
            .prepare_with(b"preserved", &mut io, &mut || Ok([0; 16]))
            .unwrap(),
    );
    let collision = stages(&fixture.0);
    let mut attempts = 0;
    assert_eq!(
        lease
            .prepare_with(b"overwrite", &mut io, &mut || {
                attempts += 1;
                Ok([0; 16])
            })
            .err(),
        Some(NativeError::StageUnavailable)
    );
    assert_eq!(attempts, 8);
    assert_eq!(fs::read(&collision[0]).unwrap(), b"preserved");
    assert_eq!(
        lease
            .prepare_with(b"candidate", &mut io, &mut || Err(
                io::ErrorKind::Other.into()
            ))
            .err(),
        Some(NativeError::RandomnessUnavailable)
    );
    assert_eq!(stages(&fixture.0), collision);
    assert_eq!(lease.read_current().unwrap(), None);
}

#[test]
fn ntfs_stage_failures_preserve_residue_and_never_change_vault() {
    for (fault, expected, partial, syncs) in [
        ("zero", NativeError::WriteStage, &b""[..], 0),
        ("partial", NativeError::WriteStage, &b"enc"[..], 0),
        ("sync", NativeError::SyncStage, &b"encrypted"[..], 1),
        ("close", NativeError::CloseStage, &b"encrypted"[..], 1),
    ] {
        let fixture = Fixture::new();
        let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
        fs::write(fixture.0.join(crate::VAULT), b"old synthetic vault").unwrap();
        let mut io = FaultIo {
            fault: Some(fault),
            ..Default::default()
        };
        assert_eq!(
            lease
                .prepare_with(b"encrypted", &mut io, &mut native::stage_random)
                .err(),
            Some(expected)
        );
        assert_eq!(io.syncs, syncs);
        assert_eq!(io.closes, 1);
        let residue = stages(&fixture.0);
        assert_eq!(residue.len(), 1);
        assert_eq!(fs::read(&residue[0]).unwrap(), partial);
        assert_eq!(
            lease.read_current().unwrap().unwrap(),
            b"old synthetic vault"
        );
        drop(lease.prepare_stage(b"new candidate").unwrap());
        assert_eq!(stages(&fixture.0).len(), 2);
        assert_eq!(fs::read(&residue[0]).unwrap(), partial);
    }
}

#[test]
fn ntfs_stage_completes_short_and_interrupted_writes() {
    for fault in ["short", "interrupted"] {
        let fixture = Fixture::new();
        let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
        let mut io = FaultIo {
            fault: Some(fault),
            ..Default::default()
        };
        let stage = lease
            .prepare_with(b"encrypted", &mut io, &mut native::stage_random)
            .unwrap();
        stage.verify().unwrap();
        assert_eq!(fs::read(&stages(&fixture.0)[0]).unwrap(), b"encrypted");
        assert_eq!(io.syncs, 1);
        assert_eq!(io.closes, 1);
    }
}

#[test]
fn ntfs_stage_readback_refuses_changed_encrypted_bytes() {
    struct Changed;
    impl StageIo for Changed {
        fn write(&mut self, file: &mut File, bytes: &[u8]) -> io::Result<usize> {
            let changed = vec![0; bytes.len()];
            file.write(&changed)
        }
    }
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    assert_eq!(
        lease
            .prepare_with(b"encrypted", &mut Changed, &mut native::stage_random)
            .err(),
        Some(NativeError::Conflict)
    );
    assert_eq!(lease.read_current().unwrap(), None);
    let paths = stages(&fixture.0);
    assert_eq!(paths.len(), 1);
    assert_eq!(fs::read(&paths[0]).unwrap(), vec![0; 9]);
    assert!(OpenOptions::new().write(true).open(&paths[0]).is_ok());
}

#[test]
fn ntfs_managed_stage_uses_private_acl_even_with_noninheritable_parent() {
    let fixture = Fixture::new();
    managed::protect_fixture(&fixture.0);
    let lease = ReadLease::acquire_managed_at(&fixture.0).unwrap();
    let stage = lease.prepare_stage(b"encrypted").unwrap();
    stage.verify().unwrap();
    managed::Admission::new()
        .unwrap()
        .verify_object(&stage.file)
        .unwrap();
    assert_eq!(lease.read_current().unwrap(), None);
}
