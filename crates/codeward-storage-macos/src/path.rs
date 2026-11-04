use std::{
    ffi::{CStr, CString},
    fs::{File, Metadata, TryLockError},
    io,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::Path,
};

use crate::{NativeError, native};

// One stable companion per directory deliberately serializes all vaults there.
// Case/Unicode aliases of a vault name therefore cannot choose different locks.
const COMPANION: &CStr = c".codeward.writer-lock";

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct Identity(u64, u64);
impl Identity {
    pub(super) fn of(meta: &Metadata) -> Self {
        Self(meta.dev(), meta.ino())
    }
    fn of_stat(meta: &libc::stat) -> Self {
        Self(meta.st_dev as u64, meta.st_ino)
    }
}

struct Directory {
    file: File,
    name: CString,
    identity: Identity,
}

/// Handle-rooted macOS destination for synthetic storage experiments.
///
/// Only absolute paths without empty/dot/parent components are accepted. Each
/// parent is opened without following symlinks. Root/parent handles are retained;
/// later reads reject observed directory or companion substitutions.
pub struct VaultPath {
    root: File,
    directories: Vec<Directory>,
    pub(super) name: CString,
}

impl VaultPath {
    /// Resolve an existing user-owned POSIX-non-writable-by-others parent on local,
    /// writable nonremovable APFS. Does not authenticate a vault or authorize UI.
    /// ACL grants and cloud-synchronization/location policy are not established.
    pub fn resolve(path: &Path) -> Result<Self, NativeError> {
        let bytes = path.as_os_str().as_bytes();
        if bytes.len() > libc::PATH_MAX as usize || !bytes.starts_with(b"/") {
            return Err(NativeError::InvalidPath);
        }
        let mut names = Vec::new();
        for part in bytes[1..].split(|b| *b == b'/') {
            if part.is_empty() || part == b"." || part == b".." {
                return Err(NativeError::InvalidPath);
            }
            names.push(CString::new(part).map_err(|_| NativeError::InvalidPath)?);
        }
        let name = names.pop().ok_or(NativeError::InvalidPath)?;
        if crate::staging::reserved_name(name.as_bytes()) {
            return Err(NativeError::InvalidPath);
        }
        let root = File::open("/").map_err(|_| NativeError::Io)?;
        let mut result = Self {
            root,
            directories: Vec::new(),
            name,
        };
        for name in names {
            let file = native::open_at(
                result.parent(),
                &name,
                libc::O_RDONLY | libc::O_DIRECTORY,
                0,
            )
            .map_err(open_error)?;
            let identity = Identity::of(&file.metadata().map_err(|_| NativeError::Io)?);
            result.directories.push(Directory {
                file,
                name,
                identity,
            });
        }
        result.verify()?;
        if !native::local_writable_apfs(result.parent()).map_err(|_| NativeError::Io)? {
            return Err(NativeError::Unsupported);
        }
        Ok(result)
    }

    pub(super) fn parent(&self) -> &File {
        self.directories
            .last()
            .map_or(&self.root, |entry| &entry.file)
    }

    pub(super) fn verify(&self) -> Result<(), NativeError> {
        let mut parent = &self.root;
        for entry in &self.directories {
            let named =
                native::named_stat(parent, &entry.name).map_err(|_| NativeError::Conflict)?;
            if named.st_mode & libc::S_IFMT != libc::S_IFDIR
                || Identity::of_stat(&named) != entry.identity
            {
                return Err(NativeError::Conflict);
            }
            parent = &entry.file;
        }
        let metadata = self.parent().metadata().map_err(|_| NativeError::Io)?;
        if metadata.uid() != native::effective_uid() || metadata.mode() & 0o022 != 0 {
            return Err(NativeError::Permissions);
        }
        Ok(())
    }

    /// Acquire the stable directory companion without truncating or unlinking it.
    /// A second descriptor/process is refused. All vaults in this directory share
    /// exclusion, including aliases; dropping the lease releases OS ownership.
    pub fn acquire(self) -> Result<ReadLease, NativeError> {
        self.verify()?;
        let companion = match native::open_at(
            self.parent(),
            COMPANION,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o600,
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                native::open_at(self.parent(), COMPANION, libc::O_RDWR, 0).map_err(open_error)?
            }
            Err(error) => return Err(open_error(error)),
        };
        let metadata = companion.metadata().map_err(|_| NativeError::Io)?;
        validate_regular(&metadata)?;
        if metadata.mode() & 0o777 != 0o600 || metadata.len() != 0 {
            return Err(NativeError::Permissions);
        }
        match companion.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(NativeError::WriterUnavailable),
            Err(TryLockError::Error(_)) => return Err(NativeError::Io),
        }
        let lease = ReadLease {
            path: self,
            identity: Identity::of(&metadata),
            companion,
            publication: std::sync::Mutex::new(()),
        };
        lease.verify()?;
        Ok(lease)
    }
}

/// Native writer-exclusion ownership with bounded reads and synthetic staging.
///
/// Does not implement the core's writable `Storage`/`WriterLease` interface. There
/// is no raw handle, clone, unlock, cleanup, plaintext or save-acknowledgement API.
pub struct ReadLease {
    pub(super) path: VaultPath,
    identity: Identity,
    companion: File,
    pub(super) publication: std::sync::Mutex<()>,
}

impl Drop for ReadLease {
    fn drop(&mut self) {
        // No intentional duplicate descriptor escapes this lease. Explicitly
        // release the lock before closing so a transient inherited descriptor
        // during an unrelated concurrent process spawn cannot prolong ownership.
        // File drop still closes our fd if unlocking reports an OS error.
        let _ = self.companion.unlock();
    }
}

impl ReadLease {
    pub(super) fn verify(&self) -> Result<(), NativeError> {
        self.path.verify()?;
        let meta = self.companion.metadata().map_err(|_| NativeError::Io)?;
        validate_regular(&meta)?;
        if Identity::of(&meta) != self.identity || meta.mode() & 0o777 != 0o600 || meta.len() != 0 {
            return Err(NativeError::Conflict);
        }
        self.match_named(COMPANION, &meta)?;
        Ok(())
    }

    pub(super) fn match_named(&self, name: &CStr, expected: &Metadata) -> Result<(), NativeError> {
        let meta =
            native::named_stat(self.path.parent(), name).map_err(|_| NativeError::Conflict)?;
        if meta.st_mode & libc::S_IFMT != libc::S_IFREG
            || u64::from(meta.st_nlink) != expected.nlink()
            || Identity::of_stat(&meta) != Identity::of(expected)
            || meta.st_uid != expected.uid()
            || u32::from(meta.st_mode) != expected.mode()
            || meta.st_size < 0
            || meta.st_size as u64 != expected.len()
            || (meta.st_mtime, meta.st_mtime_nsec) != (expected.mtime(), expected.mtime_nsec())
            || (meta.st_ctime, meta.st_ctime_nsec) != (expected.ctime(), expected.ctime_nsec())
        {
            return Err(NativeError::Conflict);
        }
        Ok(())
    }

    pub(super) fn is_companion(&self, file: &Metadata) -> bool {
        Identity::of(file) == self.identity
    }
}

pub(super) fn validate_regular(meta: &Metadata) -> Result<(), NativeError> {
    if !meta.is_file() || meta.nlink() != 1 {
        return Err(NativeError::Unsupported);
    }
    if meta.uid() != native::effective_uid() || meta.mode() & 0o177 != 0 || meta.mode() & 0o400 == 0
    {
        return Err(NativeError::Permissions);
    }
    Ok(())
}

pub(super) fn open_error(error: io::Error) -> NativeError {
    match error.raw_os_error() {
        Some(libc::ELOOP | libc::ENOTDIR) => NativeError::Unsupported,
        _ if error.kind() == io::ErrorKind::PermissionDenied => NativeError::Permissions,
        _ => NativeError::Io,
    }
}
