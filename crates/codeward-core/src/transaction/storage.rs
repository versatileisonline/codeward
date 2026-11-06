//! Trusted storage boundary. Native implementations require separate acceptance;
//! the macOS crate offers only a default-off experiment for synthetic development.

/// Controlled storage failure, with no paths, secret input or OS diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageError {
    /// Exclusive writer ownership is unavailable.
    WriterUnavailable,
    /// Unsupported destination, path identity, filesystem or adapter.
    Unsupported,
    /// Bounded read failed.
    Read,
    /// Exclusive restrictive stage creation failed.
    CreateStage,
    /// Whole encrypted write failed, including a short write.
    Write,
    /// Stage close failed.
    Close,
    /// Required staging-file synchronization failed.
    SyncStage,
    /// Required namespace/storage persistence barrier failed.
    Barrier,
}

/// Publication mode; creation must never replace an existing destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishMode {
    /// Atomically refuse any existing destination.
    Create,
    /// Atomically replace the checked existing destination.
    Replace,
}

/// Publication failure classification. Ambiguity must never become rollback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishError {
    /// The adapter proves publication did not occur.
    NotPublished,
    /// Publication may have occurred, including interrupted acknowledgement.
    OutcomeUnknown,
}

/// A validated destination capable of acquiring stable OS-managed writer exclusion.
///
/// This is a trusted core adapter, never a UI interface. Implementations must bind
/// validated directory/path identity, reject unsupported aliases/storage, and
/// acquire a stable companion lock rather than locking the replaceable file.
pub trait Storage {
    /// Owned lease retaining exclusion until dropped, including while locked.
    type Lease: WriterLease;

    /// Refuse a second participating writer. Failure must not modify the vault.
    fn acquire(self) -> Result<Self::Lease, StorageError>;
}

/// Exclusive writer lease and encrypted-only operations required by ADR-0003.
///
/// Every method acts on the same validated destination under the retained lease.
/// A production implementation requires separate native/platform evidence. The
/// core cannot establish atomicity or durability when an implementation lies.
pub trait WriterLease {
    /// Read the actual destination, enforcing the 16 MiB cap before allocation.
    /// Absence is distinct from unreadable, malformed or oversized input.
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, StorageError>;

    /// Exclusively create a fresh unpredictable restrictive same-directory stage,
    /// write all encrypted bytes, check close errors and synchronize it. Never
    /// truncate/patch the live file. Each call uses a new owned stage; errors are
    /// definite pre-publication failures. Preserve ambiguous residue; no promotion
    /// or speculative cleanup is authorized by this interface.
    fn stage(&mut self, encrypted: &[u8]) -> Result<(), StorageError>;

    /// Atomically publish the completed stage; never delete-first/copy-and-delete.
    /// If publication cannot be ruled out, return OutcomeUnknown.
    fn publish(&mut self, mode: PublishMode) -> Result<(), PublishError>;

    /// Complete the declared namespace/storage barriers for the actual destination.
    /// A visible file/read-back alone is insufficient. Also used on reconciliation.
    fn persist(&mut self) -> Result<(), StorageError>;
}
