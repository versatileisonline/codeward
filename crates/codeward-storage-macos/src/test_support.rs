use crate::{ReadLease, VaultPath};
use std::{
    fs::{self, OpenOptions, Permissions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

// The core deliberately refuses concurrent master derivations. Native test
// modules share this guard only around their synthetic derivation calls.
pub(crate) static MASTER_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) struct TestDir(pub(crate) PathBuf);
impl TestDir {
    pub(crate) fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap();
        let mut path = root.join(format!(
            "codeward-native-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // Processes can reuse PIDs, and ambiguous prior artifacts must not be
        // deleted just to acquire a test location. Try a fresh name instead.
        while path.exists() {
            path.set_extension(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
        }
        fs::DirBuilder::new().create(&path).unwrap();
        fs::set_permissions(&path, Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    pub(crate) fn vault(&self) -> PathBuf {
        self.0.join("vault.cw")
    }
    pub(crate) fn companion(&self) -> PathBuf {
        self.0.join(".codeward.writer-lock")
    }
    pub(crate) fn lease(&self) -> ReadLease {
        VaultPath::resolve(&self.vault())
            .unwrap()
            .acquire()
            .unwrap()
    }
    pub(crate) fn write(&self, bytes: &[u8]) {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.vault())
            .unwrap();
        file.write_all(bytes).unwrap();
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        // Only this exclusively created synthetic test directory is removed.
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn fixture() -> Vec<u8> {
    include_str!("../../codeward-core/tests/fixtures/authenticated-v1.hex")
        .split_ascii_whitespace()
        .flat_map(|s| {
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        })
        .collect()
}

pub(crate) fn stages(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .as_encoded_bytes()
                .starts_with(b".codeward.stage-")
        })
        .collect()
}

pub(crate) fn chmod_acl(path: &Path, text: &str) {
    assert!(
        Command::new("/bin/chmod")
            .arg("+a")
            .arg(text)
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}
