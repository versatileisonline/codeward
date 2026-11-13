use std::{ffi::CStr, fs::File, io, os::unix::fs::MetadataExt};

use super::PreparedStage;
use crate::{
    NativeError, VAULT, changed, named_metadata, native, open_regular, same_inode,
    same_observation, validate_regular,
};

/// Synthetic publication outcome. Success establishes visibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationError {
    /// The operation was refused before calling the native rename.
    NotPublished(NativeError),
    /// Rename was attempted and the observed outcome could not be confirmed.
    OutcomeUnknown,
}

impl PreparedStage {
    /// Consume the synthetic stage and publish only at an absent vault name.
    ///
    /// A racing creator cannot be overwritten. Namespace synchronization is
    /// separate from publication.
    pub fn publish_create(self) -> Result<(), PublicationError> {
        self.publish_with(None, native::publish_at)
    }

    /// Consume the synthetic stage and replace a byte-for-byte matching vault.
    ///
    /// The caller supplies the authenticated base's complete encrypted bytes.
    /// The lease excludes cooperating writers. An external writer that ignores
    /// the lease can race the comparison and rename.
    pub fn publish_replace(self, expected: &[u8]) -> Result<(), PublicationError> {
        self.publish_with(Some(expected), native::publish_at)
    }

    pub(crate) fn publish_with(
        self,
        expected: Option<&[u8]>,
        publish: impl FnOnce(&File, &CStr, &CStr, bool) -> io::Result<()>,
    ) -> Result<(), PublicationError> {
        use PublicationError::{NotPublished, OutcomeUnknown};

        let lease = &self.lease;
        let _guard = lease
            .inner
            .publication
            .try_lock()
            .map_err(|_| NotPublished(NativeError::WriterUnavailable))?;
        self.verify().map_err(NotPublished)?;
        let current = lease.expect_current(expected).map_err(NotPublished)?;
        self.verify().map_err(NotPublished)?;
        lease
            .verify_current(current.as_ref())
            .map_err(NotPublished)?;

        let parent = lease.inner.location.parent();
        publish(parent, &self.name, VAULT, expected.is_none()).map_err(|_| OutcomeUnknown)?;

        let after = || -> Result<(), NativeError> {
            lease.verify()?;
            let visible = open_regular(parent, VAULT, libc::O_RDONLY, lease.inner.location.owner)?
                .ok_or(NativeError::Conflict)?;
            let stamp = validate_regular(&visible, lease.inner.location.owner)?;
            // Rename can update ctime. Identity, size and mtime must match the stage.
            if !same_inode(&self.stamp, &stamp)
                || self.stamp.len() != stamp.len()
                || self.stamp.mtime() != stamp.mtime()
                || self.stamp.mtime_nsec() != stamp.mtime_nsec()
                || !same_observation(&stamp, &named_metadata(parent, VAULT).map_err(changed)?)
            {
                return Err(NativeError::Conflict);
            }
            lease.verify()
        };
        after().map_err(|_| OutcomeUnknown)
    }
}
