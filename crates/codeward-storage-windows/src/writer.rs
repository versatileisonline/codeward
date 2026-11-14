//! Opt-in encrypted-only adapter; the core owns save admission and acknowledgement.

use codeward_core::transaction::{PublishError, PublishMode, Storage, StorageError, WriterLease};
use std::path::PathBuf;

use crate::{MAX_FILE_LENGTH, NativeError, PreparedStage, PublicationError, ReadLease, managed};

/// Opt-in Windows managed storage for synthetic development.
///
/// Default builds omit this adapter. Enabling it does not admit real credentials
/// or establish Windows desktop support. Construction resolves LocalAppData;
/// acquiring the storage prepares and validates the fixed managed destination.
///
/// ```no_run
/// use codeward_storage_windows::{ExperimentalStorage, NativeError};
/// fn managed_synthetic_storage() -> Result<ExperimentalStorage, NativeError> {
///     ExperimentalStorage::managed()
/// }
/// ```
///
/// No arbitrary-path constructor is public:
/// ```compile_fail
/// use codeward_storage_windows::ExperimentalStorage;
/// let _constructor = ExperimentalStorage::from_path;
/// ```
pub struct ExperimentalStorage {
    base: PathBuf,
}

impl ExperimentalStorage {
    /// Resolve the OS account's LocalAppData base without creating a vault.
    #[cfg(feature = "experimental-writer")]
    pub fn managed() -> Result<Self, NativeError> {
        Ok(Self {
            base: managed::local_app_data()?,
        })
    }
}

struct PendingStage {
    prepared: PreparedStage,
    encrypted: Vec<u8>,
}

/// Encrypted-only lease retaining writer exclusion through uncertain outcomes.
///
/// A failed barrier discards its selected observation. Reconciliation must read
/// the actual named state again, authenticate it in the core and repeat barriers.
pub struct ExperimentalWriterLease {
    native: ReadLease,
    // Outer None means no successful observation; Some(None) confirms absence.
    observed: Option<Option<Vec<u8>>>,
    pending: Option<PendingStage>,
}

impl Storage for ExperimentalStorage {
    type Lease = ExperimentalWriterLease;

    fn acquire(self) -> Result<Self::Lease, StorageError> {
        let native = ReadLease::acquire_managed_at(&self.base).map_err(|error| match error {
            NativeError::WriterUnavailable => StorageError::WriterUnavailable,
            _ => StorageError::Unsupported,
        })?;
        Ok(ExperimentalWriterLease {
            native,
            observed: None,
            pending: None,
        })
    }
}

impl WriterLease for ExperimentalWriterLease {
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, StorageError> {
        self.observed = None;
        let bytes = self.native.read_current().map_err(|_| StorageError::Read)?;
        let selected = bytes
            .as_deref()
            .map(|bytes| copy_bytes(bytes, StorageError::Read))
            .transpose()?;
        self.observed = Some(selected);
        Ok(bytes)
    }

    fn stage(&mut self, encrypted: &[u8]) -> Result<(), StorageError> {
        // A refused new preparation cannot leave an older marker publishable.
        self.pending = None;
        let encrypted = copy_bytes(encrypted, StorageError::CreateStage)?;
        let prepared = self
            .native
            .prepare_stage(&encrypted)
            .map_err(|error| match error {
                NativeError::WriteStage => StorageError::Write,
                NativeError::SyncStage => StorageError::SyncStage,
                NativeError::CloseStage => StorageError::Close,
                _ => StorageError::CreateStage,
            })?;
        self.pending = Some(PendingStage {
            prepared,
            encrypted,
        });
        Ok(())
    }

    fn publish(&mut self, mode: PublishMode) -> Result<(), PublishError> {
        let expected = self.observed.take();
        let stage = self.pending.take().ok_or(PublishError::NotPublished)?;
        let expected = expected.ok_or(PublishError::NotPublished)?;
        if (mode == PublishMode::Create) != expected.is_none() {
            return Err(PublishError::NotPublished);
        }
        let result = match expected {
            Some(bytes) => stage.prepared.publish_replace(&bytes),
            None => stage.prepared.publish_create(),
        };
        result.map_err(|error| match error {
            PublicationError::NotPublished(_) => PublishError::NotPublished,
            PublicationError::OutcomeUnknown => PublishError::OutcomeUnknown,
        })?;
        self.observed = Some(Some(stage.encrypted));
        Ok(())
    }

    fn persist(&mut self) -> Result<(), StorageError> {
        self.persist_with(ReadLease::synchronize_namespace)
    }
}

impl ExperimentalWriterLease {
    fn persist_with(
        &mut self,
        synchronize: impl FnOnce(&ReadLease, Option<&[u8]>) -> Result<(), NativeError>,
    ) -> Result<(), StorageError> {
        let expected = self.observed.as_ref().ok_or(StorageError::Barrier)?;
        let result = synchronize(&self.native, expected.as_deref());
        if result.is_err() {
            self.observed = None;
        }
        result.map_err(|_| StorageError::Barrier)
    }
}

fn copy_bytes(bytes: &[u8], error: StorageError) -> Result<Vec<u8>, StorageError> {
    if bytes.len() > MAX_FILE_LENGTH {
        return Err(error);
    }
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len()).map_err(|_| error)?;
    copy.extend_from_slice(bytes);
    Ok(copy)
}

#[cfg(test)]
mod tests;
