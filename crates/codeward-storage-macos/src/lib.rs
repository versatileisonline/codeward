//! Experimental macOS path, reads, exclusion and staging for synthetic data.
//!
//! This is not a writable storage adapter or a supported credential store. Native
//! publication, namespace barriers and cloud-location validation remain separate
//! work. Staging refuses extended ACLs; nothing here reports a committed save.

#![cfg(target_os = "macos")]
#![deny(missing_docs, unsafe_op_in_unsafe_fn)]

// Native calls stay in this adapter crate; codeward-core remains unsafe-free.

use std::{error::Error, fmt};

mod native;
mod path;
mod reading;
mod staging;
#[cfg(test)]
mod test_support;
pub use path::{ReadLease, VaultPath};
pub use staging::PreparedStage;

/// Fixed diagnostic categories; paths and OS error text never enter these values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeError {
    /// Path is not an absolute, unambiguous sequence of ordinary components.
    InvalidPath,
    /// Unsupported filesystem, linked file, file type or path resolution.
    Unsupported,
    /// Effective user ownership or restrictive POSIX mode check failed.
    Permissions,
    /// Companion writer lease is already held by another descriptor/process.
    WriterUnavailable,
    /// A directory, companion or vault identity changed during the operation.
    Conflict,
    /// Native open/stat/read/create failed; source text is intentionally omitted.
    Io,
    /// Whole-file length exceeds the accepted 16 MiB cap.
    TooLarge,
    /// Fallible input-buffer reservation failed.
    AllocationFailed,
    /// Native secure randomness was unavailable; no stage was created.
    RandomnessUnavailable,
    /// Exclusive stage creation exhausted its bounded collision attempts.
    StageUnavailable,
    /// Complete staging write failed.
    Write,
    /// Staging-file synchronization failed; no publication was attempted.
    SyncStage,
    /// Explicit staging close reported an error; no publication was attempted.
    Close,
}

impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "native storage error: {self:?}")
    }
}
impl Error for NativeError {}
