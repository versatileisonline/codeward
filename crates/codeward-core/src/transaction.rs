//! Session-bound admission and encrypted committed-write protocol, not UI commands.
//!
//! Storage implementations are trusted, separately validated adapters. The test
//! model is not a native durability guarantee. Session and worker are separate so
//! a blocked storage call need not retain session keys or prevent security lock.

use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

use sha2::{Digest, Sha256};

use crate::{envelope::UnauthenticatedEnvelope, vault::VaultError};

mod session;
mod storage;
mod worker;
pub use session::{
    ActivityRef, AuthenticationJob, AuthenticationResult, CredentialCursor, CredentialPage,
    CredentialSummary, INACTIVITY_TIMEOUT, MAX_CREDENTIAL_SUMMARIES, Session, SessionActivity,
    SessionEvent,
};
pub use storage::{PublishError, PublishMode, Storage, StorageError, WriterLease};
pub use worker::Committer;

/// Bounded request history per opened writer. Capacity exhaustion fails closed;
/// reopen/review actual state explicitly rather than automatically replaying IDs.
pub const MAX_REQUESTS: usize = 64;

/// Caller/window identity supplied by a trusted host, never claimed by UI input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallerId(pub u64);

/// Caller-selected correlation ID; an identifier alone grants no authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestId(pub [u8; 16]);

/// Save intent correlation and optimistic revision. Creation expects zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Request {
    /// ID retained in bounded memory for this opened writer.
    pub id: RequestId,
    /// Authenticated source revision, or zero for initial creation.
    pub expected_revision: u64,
}

/// Opaque operation reference scoped to caller, opened vault and generation.
#[derive(Clone)]
pub struct OperationRef {
    scope: Arc<()>,
    caller: CallerId,
    generation: u64,
    id: RequestId,
}

/// Nonsecret worker progress. None of these stages establishes a committed save.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// Complete encrypted candidate held only in memory.
    Prepared,
    /// Restrictive encrypted stage written, closed and synchronized.
    Staged,
    /// Base rechecked and publication atomically admitted against session lock.
    Admitted,
    /// Atomic publication returned success; final barriers remain.
    Published,
}

/// Safe targeted status; never contains keys, credentials or input diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationStatus {
    /// Transaction is still in progress.
    Pending(Phase),
    /// Security lock won before publication admission; candidate cannot publish.
    Canceled,
    /// Observed destination differs from the exact authenticated source.
    Conflict,
    /// Definite pre-publication storage failure.
    Failed(StorageError),
    /// Adapter proved the atomic publication did not happen.
    NotPublished,
    /// Publication/persistence is uncertain; new mutations are blocked.
    OutcomeUnknown,
    /// Required persistence barriers completed for the full selected envelope.
    Committed {
        /// Revision whose full envelope completed the persistence barriers.
        revision: u64,
    },
    /// Reconciliation proved the original base (or creation absence), not a save.
    Unchanged,
    /// This bounded process has no status; it does not authorize replay.
    Unavailable,
}

/// Controlled transaction and session errors, without sensitive inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionError {
    /// Caller or opened-vault scope differs.
    Unauthorized,
    /// Session is locked or lacks the requested access mode.
    Locked,
    /// Current access needs refresh, reload or reopen before using the worker's base.
    ReloadRequired,
    /// The requested summary page size is outside the supported range.
    InvalidPageSize,
    /// The list cursor belongs to an older session generation or committed revision.
    StaleCursor,
    /// Another transaction is active, including an uncertain outcome.
    Busy,
    /// A known ID was reused with different arguments or session generation.
    RequestReused,
    /// No request IDs are evicted to make room for blind replays.
    HistoryFull,
    /// Session generation is exhausted; it cannot be reused.
    GenerationExhausted,
    /// Authentication belongs to a canceled, replaced or otherwise stale attempt.
    AuthenticationCanceled,
    /// A monotonic observation regressed or a deadline could not be represented.
    ClockUnavailable,
    /// Allocation failed while preparing a core operation or response.
    AllocationFailed,
    /// Actual bytes cannot be established as the known base/candidate.
    OutcomeUnknown,
    /// Authentication, schema, revision or candidate preparation failed.
    Vault(VaultError),
    /// Controlled storage failure.
    Storage(StorageError),
}

impl fmt::Display for TransactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // All variants and nested errors contain only fixed public categories.
        write!(f, "transaction error: {self:?}")
    }
}
impl Error for TransactionError {}
impl From<VaultError> for TransactionError {
    fn from(error: VaultError) -> Self {
        Self::Vault(error)
    }
}
impl From<StorageError> for TransactionError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct Fingerprint {
    vault_id: [u8; 16],
    revision: u64,
    sha256: [u8; 32],
}

impl Fingerprint {
    fn of(bytes: &[u8]) -> Result<Self, TransactionError> {
        let parsed = UnauthenticatedEnvelope::parse(bytes).map_err(VaultError::from)?;
        Ok(Self {
            vault_id: parsed.vault_id().try_into().expect("parsed fixed vault ID"),
            revision: parsed.revision(),
            sha256: Sha256::digest(bytes).into(),
        })
    }
}

const WAITING: u8 = 0;
const ADMITTED: u8 = 1;
const CANCELED: u8 = 2;

// One atomic decision shared by the secret-owning session and encrypted worker.
struct Permit(AtomicU8);
impl Permit {
    fn new() -> Self {
        Self(AtomicU8::new(WAITING))
    }
    fn cancel(&self) {
        let _ = self
            .0
            .compare_exchange(WAITING, CANCELED, Ordering::SeqCst, Ordering::SeqCst);
    }
    fn admit(&self) -> bool {
        self.0
            .compare_exchange(WAITING, ADMITTED, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
    fn canceled(&self) -> bool {
        self.0.load(Ordering::SeqCst) == CANCELED
    }
}

#[cfg(test)]
mod tests;
