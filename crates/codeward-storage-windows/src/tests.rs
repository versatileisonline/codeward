use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

#[test]
fn windows_distinguishes_missing_leaf_from_failed_parent_lookup() {
    use windows_sys::Win32::Foundation::ERROR_PATH_NOT_FOUND;
    assert!(leaf_missing(&io::Error::from_raw_os_error(
        ERROR_FILE_NOT_FOUND as i32
    )));
    assert!(!leaf_missing(&io::Error::from_raw_os_error(
        ERROR_PATH_NOT_FOUND as i32
    )));
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "codeward-ntfs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn ntfs_reads_absence_bytes_and_cap_under_exclusion() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    assert_eq!(lease.read_current().unwrap(), None);
    assert_eq!(
        ReadLease::acquire_synthetic(&fixture.0).err(),
        Some(NativeError::WriterUnavailable)
    );
    let vault = fixture.0.join("vault.cw");
    fs::write(&vault, b"synthetic").unwrap();
    assert_eq!(lease.read_current().unwrap().unwrap(), b"synthetic");
    let file = OpenOptions::new().write(true).open(&vault).unwrap();
    file.set_len(MAX_FILE_LENGTH as u64).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Read));
    drop(file);
    assert_eq!(
        lease.read_current().unwrap().unwrap().len(),
        MAX_FILE_LENGTH
    );
    OpenOptions::new()
        .write(true)
        .open(&vault)
        .unwrap()
        .set_len(MAX_FILE_LENGTH as u64 + 1)
        .unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::TooLarge));
    drop(lease);
    assert!(ReadLease::acquire_synthetic(&fixture.0).is_ok());
}

#[test]
fn ntfs_rejects_hard_links_and_nonempty_companion() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    let other = fixture.0.join("other");
    fs::write(&other, b"preserve").unwrap();
    fs::hard_link(&other, fixture.0.join("vault.cw")).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Unsupported));
    drop(lease);
    let companion = fixture.0.join(".codeward.writer-lock");
    fs::write(&companion, b"preserve").unwrap();
    assert_eq!(
        ReadLease::acquire_synthetic(&fixture.0).err(),
        Some(NativeError::Unsupported)
    );
    assert_eq!(fs::read(companion).unwrap(), b"preserve");
}

#[test]
fn windows_rejects_non_disk_and_ambiguous_paths() {
    for path in [
        r"relative",
        r"C:relative",
        r"C:\safe\..\other",
        r"C:\safe\alias.",
        r"C:\safe\COM¹.txt",
        r"C:\safe\NUL",
        r"C:\safe\entry:stream",
        r"\\server\share\fixture",
        r"\\?\C:\fixture",
    ] {
        assert_eq!(
            ReadLease::acquire_synthetic(Path::new(path)).err(),
            Some(NativeError::InvalidPath)
        );
    }
}

#[test]
fn ntfs_excludes_an_independent_process() {
    let fixture = Fixture::new();
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
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    child("yes");
    drop(lease);
    child("no");
}

#[test]
#[ignore = "subprocess of ntfs_excludes_an_independent_process"]
fn companion_child() {
    let path = PathBuf::from(std::env::var_os("CODEWARD_NTFS_FIXTURE").unwrap());
    let result = ReadLease::acquire_synthetic(&path);
    match std::env::var("CODEWARD_NTFS_EXPECT_LOCKED")
        .unwrap()
        .as_str()
    {
        "yes" => assert_eq!(result.err(), Some(NativeError::WriterUnavailable)),
        "no" => assert_eq!(result.unwrap().read_current().unwrap(), None),
        _ => panic!("missing expected companion state"),
    }
}

#[test]
fn ntfs_refuses_junction_as_directory_or_vault() {
    let fixture = Fixture::new();
    let target = fixture.0.join("target");
    let junction = fixture.0.join("vault.cw");
    fs::create_dir(&target).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J", "vault.cw", "target"])
        .current_dir(&fixture.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        ReadLease::acquire_synthetic(&junction).err(),
        Some(NativeError::Unsupported)
    );
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    assert_eq!(lease.read_current(), Err(NativeError::Unsupported));
}
