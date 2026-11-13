//! Native Linux read, writer-exclusion and staging experiments for synthetic data.
#![cfg(target_os = "linux")]
#![deny(missing_docs, unsafe_op_in_unsafe_fn)]

use std::{
    error::Error,
    ffi::{CStr, CString},
    fmt,
    fs::{File, Metadata, TryLockError},
    io::{self, Read},
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::Path,
};

use codeward_core::envelope::MAX_FILE_LENGTH;

mod managed;
mod native;
mod staging;
pub use staging::PreparedStage;
#[cfg(test)]
mod tests;

const COMPANION: &CStr = c".codeward.writer-lock";
const VAULT: &CStr = c"vault.cw";

/// Fixed diagnostic categories with no path or OS error text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeError {
    /// The path is not absolute or contains ambiguous components.
    InvalidPath,
    /// The filesystem, mount, link or entry type is unsupported.
    Unsupported,
    /// Ownership, modes, ACLs or companion length do not satisfy the experiment.
    Permissions,
    /// Another open description holds the companion lock.
    WriterUnavailable,
    /// An observed directory, companion or vault changed.
    Conflict,
    /// A native operation failed.
    Io,
    /// The input is larger than the whole-file cap.
    TooLarge,
    /// A fallible buffer reservation failed.
    AllocationFailed,
    /// Native secure randomness was unavailable before creating a stage.
    RandomnessUnavailable,
    /// All bounded exclusive stage-name attempts collided.
    StageUnavailable,
    /// Writing the complete encrypted candidate failed.
    WriteStage,
    /// Synchronizing the stage file failed.
    SyncStage,
    /// Explicitly closing the stage writer failed.
    CloseStage,
}

impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "native storage error: {self:?}")
    }
}
impl Error for NativeError {}

/// Retains advisory writer exclusion for a synthetic vault on Linux.
pub struct ReadLease {
    location: managed::Location,
    companion: File,
}

impl ReadLease {
    /// Open an existing private synthetic directory on a writable ext4 nodev mount.
    ///
    /// This experiment takes a fixture path. The application must use its own
    /// managed-location resolver once Linux runtime storage is implemented.
    pub fn acquire_synthetic(path: &Path) -> Result<Self, NativeError> {
        // SAFETY: geteuid has no pointer arguments or preconditions.
        let owner = unsafe { libc::geteuid() };
        Self::acquire(managed::Location::fixture(path, owner)?)
    }

    /// Prepare the fixed per-account Linux location and acquire a synthetic read lease.
    ///
    /// Uses an existing absolute XDG data directory or the OS account home for the
    /// default path. The admitted location must be on a writable ext4 nodev mount.
    pub fn acquire_managed_synthetic() -> Result<Self, NativeError> {
        Self::acquire(managed::Location::managed()?)
    }

    fn acquire(location: managed::Location) -> Result<Self, NativeError> {
        location.verify()?;
        let parent = location.parent();
        let owner = location.owner;
        let companion = match native::open_at(
            parent,
            COMPANION,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            true,
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                open_regular(parent, COMPANION, libc::O_RDWR, owner)?
                    .ok_or(NativeError::Conflict)?
            }
            Err(error) => return Err(open_error(error)),
        };
        if validate_regular(&companion, owner)?.len() != 0 {
            return Err(NativeError::Permissions);
        }
        match companion.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(NativeError::WriterUnavailable),
            Err(TryLockError::Error(_)) => return Err(NativeError::Io),
        }
        let lease = Self {
            location,
            companion,
        };
        lease.verify()?;
        Ok(lease)
    }

    /// Return the named input, bounded by the core cap, or confirmed absence.
    ///
    /// The bytes are unauthenticated and still require core validation.
    pub fn read_current(&self) -> Result<Option<Vec<u8>>, NativeError> {
        self.read_with(|| {})
    }

    fn read_with(&self, after_open: impl FnOnce()) -> Result<Option<Vec<u8>>, NativeError> {
        self.verify()?;
        let mut file = match open_regular(
            self.location.parent(),
            VAULT,
            libc::O_RDONLY,
            self.location.owner,
        )? {
            Some(file) => file,
            None => {
                self.verify()?;
                return match named_metadata(self.location.parent(), VAULT) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                    _ => Err(NativeError::Conflict),
                };
            }
        };
        let before = validate_regular(&file, self.location.owner)?;
        if same_inode(&before, &metadata(&self.companion)?) {
            return Err(NativeError::Conflict);
        }
        after_open();
        let bytes = read_bounded(&mut file, before.len())?;
        let after = metadata(&file)?;
        if !same_observation(&before, &after) {
            return Err(NativeError::Conflict);
        }
        native::refuse_acl(&file, c"system.posix_acl_access")?;
        self.verify()?;
        let named = named_metadata(self.location.parent(), VAULT).map_err(changed)?;
        if !same_observation(&after, &named) {
            return Err(NativeError::Conflict);
        }
        Ok(Some(bytes))
    }

    fn verify(&self) -> Result<(), NativeError> {
        self.location.verify()?;
        let companion = validate_regular(&self.companion, self.location.owner)?;
        let named = named_metadata(self.location.parent(), COMPANION).map_err(changed)?;
        if !same_observation(&companion, &named) {
            return Err(NativeError::Conflict);
        }
        if companion.len() != 0 {
            return Err(NativeError::Permissions);
        }
        Ok(())
    }
}

fn validate_path(path: &Path) -> Result<&[u8], NativeError> {
    let path = path.as_os_str().as_bytes();
    let path = path.strip_prefix(b"/").ok_or(NativeError::InvalidPath)?;
    let segments = path.split(|&byte| byte == b'/');
    if segments
        .clone()
        .any(|segment| matches!(segment, b"" | b"." | b"..") || segment.contains(&0))
    {
        return Err(NativeError::InvalidPath);
    }
    Ok(path)
}

fn open_path(path: &Path, owner: u32, private: bool) -> Result<File, NativeError> {
    let path = validate_path(path)?;
    let mut parent = File::open("/").map_err(|_| NativeError::Io)?;
    validate_directory(&parent, owner, false)?;
    let mut segments = path.split(|&byte| byte == b'/').peekable();
    while let Some(segment) = segments.next() {
        let segment = CString::new(segment).map_err(|_| NativeError::InvalidPath)?;
        parent = native::open_at(&parent, &segment, libc::O_RDONLY | libc::O_DIRECTORY, false)
            .map_err(open_error)?;
        validate_directory(&parent, owner, private && segments.peek().is_none())?;
    }
    Ok(parent)
}

fn validate_directory(file: &File, owner: u32, private: bool) -> Result<(), NativeError> {
    let meta = metadata(file)?;
    let allowed = if private {
        meta.uid() == owner && meta.mode() & 0o7777 == 0o700
    } else {
        (meta.uid() == 0 || meta.uid() == owner) && meta.mode() & 0o022 == 0
    };
    if !allowed {
        return Err(NativeError::Permissions);
    }
    native::refuse_acl(file, c"system.posix_acl_access")?;
    if private {
        native::refuse_acl(file, c"system.posix_acl_default")?;
    }
    Ok(())
}

fn open_regular(
    parent: &File,
    name: &CStr,
    flags: libc::c_int,
    owner: u32,
) -> Result<Option<File>, NativeError> {
    let before = match named_metadata(parent, name) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(open_error(error)),
    };
    check_regular(&before, owner)?;
    let file = native::open_at(
        parent,
        name,
        flags | libc::O_NONBLOCK | libc::O_NOCTTY,
        true,
    )
    .map_err(changed)?;
    if !same_observation(&before, &metadata(&file)?) {
        return Err(NativeError::Conflict);
    }
    Ok(Some(file))
}

fn named_metadata(parent: &File, name: &CStr) -> io::Result<Metadata> {
    native::open_at(parent, name, libc::O_PATH, true)?.metadata()
}

fn metadata(file: &File) -> Result<Metadata, NativeError> {
    file.metadata().map_err(|_| NativeError::Io)
}

fn check_regular(meta: &Metadata, owner: u32) -> Result<(), NativeError> {
    if !meta.is_file() || meta.nlink() != 1 {
        return Err(NativeError::Unsupported);
    }
    if meta.uid() != owner || meta.mode() & 0o7777 != 0o600 {
        return Err(NativeError::Permissions);
    }
    Ok(())
}

fn validate_regular(file: &File, owner: u32) -> Result<Metadata, NativeError> {
    let meta = metadata(file)?;
    check_regular(&meta, owner)?;
    native::refuse_acl(file, c"system.posix_acl_access")?;
    Ok(meta)
}

fn same_inode(left: &Metadata, right: &Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

fn same_observation(left: &Metadata, right: &Metadata) -> bool {
    same_inode(left, right)
        && left.nlink() == right.nlink()
        && left.uid() == right.uid()
        && left.mode() == right.mode()
        && left.len() == right.len()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
}

fn read_bounded(file: &mut File, length: u64) -> Result<Vec<u8>, NativeError> {
    if length > MAX_FILE_LENGTH as u64 {
        return Err(NativeError::TooLarge);
    }
    let length = length as usize;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| NativeError::AllocationFailed)?;
    bytes.resize(length, 0);
    file.read_exact(&mut bytes).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            NativeError::Conflict
        } else {
            NativeError::Io
        }
    })?;
    let mut extra = [0];
    loop {
        match file.read(&mut extra) {
            Ok(0) => return Ok(bytes),
            Ok(_) if length == MAX_FILE_LENGTH => return Err(NativeError::TooLarge),
            Ok(_) => return Err(NativeError::Conflict),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Err(NativeError::Io),
        }
    }
}

fn changed(error: io::Error) -> NativeError {
    if error.kind() == io::ErrorKind::NotFound {
        NativeError::Conflict
    } else {
        open_error(error)
    }
}

fn open_error(error: io::Error) -> NativeError {
    match error.raw_os_error() {
        Some(libc::ELOOP | libc::EXDEV | libc::ENOTDIR | libc::ENOSYS) => NativeError::Unsupported,
        Some(libc::EACCES | libc::EPERM) => NativeError::Permissions,
        _ => NativeError::Io,
    }
}
