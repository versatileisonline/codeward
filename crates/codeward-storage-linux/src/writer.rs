use codeward_core::transaction::{PublishError, PublishMode, Storage, StorageError, WriterLease};

use crate::{MAX_FILE_LENGTH, NativeError, PreparedStage, PublicationError, ReadLease, managed};

/// Opt-in Linux managed storage for synthetic development.
pub struct ExperimentalStorage {
    location: managed::Location,
}

impl ExperimentalStorage {
    /// Prepare the fixed managed destination using the native Linux admission policy.
    #[cfg(feature = "experimental-writer")]
    pub fn managed() -> Result<Self, NativeError> {
        Ok(Self {
            location: managed::Location::managed()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_location(location: managed::Location) -> Self {
        Self { location }
    }
}

struct PendingStage {
    prepared: PreparedStage,
    encrypted: Vec<u8>,
}

/// Synthetic encrypted-only storage lease retained through reconciliation.
pub struct ExperimentalWriterLease {
    native: ReadLease,
    // None indicates no selected observation. Some(None) is confirmed absence.
    observed: Option<Option<Vec<u8>>>,
    pending: Option<PendingStage>,
}

impl Storage for ExperimentalStorage {
    type Lease = ExperimentalWriterLease;

    fn acquire(self) -> Result<Self::Lease, StorageError> {
        let native = ReadLease::acquire(self.location).map_err(|error| match error {
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
        // Revoke the previous marker while leaving its residue intact.
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
        let expected = self.observed.as_ref().ok_or(StorageError::Barrier)?;
        let result = self.native.synchronize_namespace(expected.as_deref());
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
