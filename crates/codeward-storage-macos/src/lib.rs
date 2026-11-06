//! Experimental macOS reads, exclusion, staging and publication for synthetic data.
//!
//! Default builds do not export a writable core adapter. The explicit
//! `experimental-writer` feature adds one for synthetic development only, not a
//! supported credential store. Location and persistence acceptance remain pending.
//! Staging/publication refuse extended ACLs on the parent and candidate; nothing
//! here reports a committed save. Replacement also refuses destination ACLs.

#![cfg(target_os = "macos")]
#![deny(missing_docs, unsafe_op_in_unsafe_fn)]

// Native calls stay in this adapter crate; codeward-core remains unsafe-free.

use std::{error::Error, fmt};

mod namespace;
mod native;
mod path;
mod reading;
mod staging;
#[cfg(test)]
mod test_support;
#[cfg(any(test, feature = "experimental-writer"))]
mod writer;
pub use path::{ReadLease, VaultPath};
pub use staging::{PreparedStage, PublicationError};
#[cfg(feature = "experimental-writer")]
pub use writer::{ExperimentalStorage, ExperimentalWriterLease};

/// Fixed diagnostic categories; paths and OS error text never enter these values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeError {
    /// Path is not an absolute, unambiguous sequence of ordinary components.
    InvalidPath,
    /// Unsupported filesystem, linked file, file type or path resolution.
    Unsupported,
    /// Effective user ownership or restrictive POSIX mode check failed.
    Permissions,
    /// Companion ownership or same-lease publication exclusion is unavailable.
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
    /// Candidate file/parent namespace synchronization failed; no rollback proof.
    SyncNamespace,
    /// Explicit staging close reported an error; no publication was attempted.
    Close,
}

impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "native storage error: {self:?}")
    }
}
impl Error for NativeError {}
