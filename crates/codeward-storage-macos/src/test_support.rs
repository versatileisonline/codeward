use crate::{ReadLease, VaultPath};
use std::{
    cell::Cell,
    fs::{self, OpenOptions, Permissions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub(crate) enum MountFault {
    Refused,
    QueryFailed,
}

thread_local! {
    static MOUNT_FAULT: Cell<Option<MountFault>> = const { Cell::new(None) };
}

// A scoped current-thread query result, never a real remount. Other test threads
// and spawned child processes continue to query their actual native filesystem.
pub(crate) struct MountProbe {
    previous: Option<MountFault>,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl MountProbe {
    pub(crate) fn new() -> Self {
        Self {
            previous: MOUNT_FAULT.replace(None),
            _thread: std::marker::PhantomData,
        }
    }
    pub(crate) fn set(&self, fault: Option<MountFault>) {
        MOUNT_FAULT.set(fault);
    }
}
impl Drop for MountProbe {
    fn drop(&mut self) {
        MOUNT_FAULT.set(self.previous);
    }
}

pub(crate) fn mount_result() -> Option<io::Result<bool>> {
    MOUNT_FAULT.get().map(|fault| match fault {
        MountFault::Refused => Ok(false),
        MountFault::QueryFailed => Err(io::Error::from_raw_os_error(libc::EIO)),
    })
}

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
    decode_hex(include_str!(
        "../../codeward-core/tests/fixtures/authenticated-v1.hex"
    ))
}

pub(crate) fn maximum_revision_fixture() -> Vec<u8> {
    decode_hex(include_str!(
        "../../codeward-core/tests/fixtures/authenticated-max-revision.hex"
    ))
}

fn decode_hex(text: &str) -> Vec<u8> {
    text.split_ascii_whitespace()
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
