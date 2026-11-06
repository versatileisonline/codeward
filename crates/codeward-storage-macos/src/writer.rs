//! Shared experimental adapter. Native acceptance and real-vault use stay gated.

use codeward_core::{
    envelope::MAX_FILE_LENGTH,
    transaction::{PublishError, PublishMode, Storage, StorageError, WriterLease},
};

use crate::{NativeError, PreparedStage, PublicationError, ReadLease, VaultPath};

/// Opt-in macOS managed storage for synthetic development only.
///
/// Disabled in default builds. Enabling the feature is not real-vault admission,
/// native persistence acceptance or a supported save guarantee. No application
/// runtime should enable it for personal credentials until those gates pass.
///
/// ```no_run
/// use codeward_storage_macos::{ExperimentalStorage, NativeError};
/// fn managed_synthetic_destination() -> Result<ExperimentalStorage, NativeError> {
///     ExperimentalStorage::managed()
/// }
/// ```
///
/// Arbitrary path construction is not a public feature-enabled capability:
/// ```compile_fail
/// use codeward_storage_macos::ExperimentalStorage;
/// let _constructor = ExperimentalStorage::from_path;
/// ```
pub struct ExperimentalStorage {
    path: VaultPath,
}

impl ExperimentalStorage {
    /// Prepare the current OS account's managed location, without creating a vault.
    /// Uses the existing admission policy; there is no arbitrary-path fallback.
    #[cfg(feature = "experimental-writer")]
    pub fn managed() -> Result<Self, NativeError> {
        Ok(Self {
            path: VaultPath::managed()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_path(path: VaultPath) -> Self {
        Self { path }
    }
}

struct PendingStage {
    prepared: PreparedStage,
    encrypted: Vec<u8>,
}

/// Encrypted-only experimental lease, retained through barriers and reconciliation.
///
/// The core owns authentication, authorization, request history and unknown-outcome
/// blocking. This adapter never promotes residue or acknowledges an application save.
pub struct ExperimentalWriterLease {
    native: ReadLease,
    // Outer None means no selected successful observation. Inner None is actual
    // absence, not permission to substitute a stage or create a ready vault.
    observed: Option<Option<Vec<u8>>>,
    pending: Option<PendingStage>,
}

impl Storage for ExperimentalStorage {
    type Lease = ExperimentalWriterLease;

    fn acquire(self) -> Result<Self::Lease, StorageError> {
        let native = self.path.acquire().map_err(|error| match error {
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
        // Preserve old residue but revoke its marker before any fallible new work.
        self.pending = None;
        let encrypted = copy_bytes(encrypted, StorageError::CreateStage)?;
        let prepared = self
            .native
            .prepare_stage(&encrypted)
            .map_err(|error| match error {
                NativeError::Write => StorageError::Write,
                NativeError::SyncStage => StorageError::SyncStage,
                NativeError::Close => StorageError::Close,
                _ => StorageError::CreateStage,
            })?;
        self.pending = Some(PendingStage {
            prepared,
            encrypted,
        });
        Ok(())
    }

    fn publish(&mut self, mode: PublishMode) -> Result<(), PublishError> {
        // Invalidate even on refusal/uncertainty. A later barrier must select an
        // actual fresh read or the successfully published complete candidate.
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

#[cfg(test)]
mod tests;
