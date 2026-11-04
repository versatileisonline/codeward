use std::{ffi::CStr, fs::File, io, os::unix::fs::MetadataExt};

use super::{PreparedStage, private_acl, stage_permissions};
use crate::{NativeError, native, path::open_error, reading::stamp};
use codeward_core::envelope::MAX_FILE_LENGTH;

/// Visibility failure only; even successful publication is not a committed save.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationError {
    /// Validation refused before invoking the native publication operation.
    /// The destination was not modified by this API and residue is preserved.
    NotPublished(NativeError),
    /// The native operation was invoked; its outcome cannot be confirmed.
    /// Preserve artifacts, block mutations and reconcile; never blindly retry.
    OutcomeUnknown,
}

impl PreparedStage<'_> {
    /// Consume this synthetic stage and atomically publish only at an absent name.
    ///
    /// A racing creation cannot be overwritten. Success confirms visibility only,
    /// not namespace persistence, readiness, recovery completion or a saved state.
    /// No cleanup or rollback is performed. Synthetic encrypted inputs only.
    ///
    /// ```compile_fail
    /// use codeward_storage_macos::PreparedStage;
    /// fn cannot_publish_twice(stage: PreparedStage<'_>) {
    ///     let _ = stage.publish_create();
    ///     let _ = stage.publish_create();
    /// }
    /// ```
    pub fn publish_create(self) -> Result<(), PublicationError> {
        self.publish_with(None, native::publish_at)
    }

    /// Consume this synthetic stage and replace an exactly matching current file.
    ///
    /// The trusted caller supplies the complete original encrypted bytes from its
    /// authenticated base. This method compares bytes, never authenticates them.
    /// Cooperating writers remain excluded; last-moment nonparticipating writers
    /// are outside the advisory lock guarantee. Success is visibility only; the
    /// namespace barrier and core commit acknowledgement remain separate work.
    pub fn publish_replace(self, expected: &[u8]) -> Result<(), PublicationError> {
        self.publish_with(Some(expected), native::publish_at)
    }

    fn publish_with(
        self,
        expected: Option<&[u8]>,
        publish: impl FnOnce(&File, &CStr, &CStr, bool) -> io::Result<()>,
    ) -> Result<(), PublicationError> {
        let lease = self._lease;
        // The OS companion excludes other leases, not threads sharing this one.
        // Serialize the full base-check/rename/postcheck interval. Contention or
        // poisoning fails before publication rather than trusting a stale base.
        let _publication = lease
            .publication
            .try_lock()
            .map_err(|_| PublicationError::NotPublished(NativeError::WriterUnavailable))?;
        let before = || -> Result<File, NativeError> {
            if expected.is_some_and(|bytes| bytes.len() > MAX_FILE_LENGTH) {
                return Err(NativeError::TooLarge);
            }
            lease.verify()?;
            private_acl(lease.path.parent())?;
            let stage = native::open_at(lease.path.parent(), &self._name, libc::O_RDONLY, 0)
                .map_err(open_error)?;
            self.verify_stage(&stage)?;

            // Hold the current descriptor through the bounded comparison and
            // recheck its name, so an observed substitution/ACL change refuses.
            let current = if expected.is_some() {
                let file =
                    native::open_at(lease.path.parent(), &lease.path.name, libc::O_RDONLY, 0)
                        .map_err(open_error)?;
                private_acl(&file)?;
                let metadata = file.metadata().map_err(|_| NativeError::Io)?;
                Some((file, metadata))
            } else {
                None
            };
            let actual = lease.read_current()?;
            if actual.as_deref() != expected {
                return Err(NativeError::Conflict);
            }
            if let Some((file, metadata)) = current {
                private_acl(&file)?;
                lease.match_named(&lease.path.name, &metadata)?;
            }
            self.verify_stage(&stage)?;
            lease.verify()?;
            private_acl(lease.path.parent())?;
            Ok(stage)
        };
        let stage = before().map_err(PublicationError::NotPublished)?;

        // The marker is consumed even on error. Conservatively treat every
        // syscall error (including EINTR/EIO/EEXIST) as unknown; no retry or
        // speculative restore. An injected lost reply after success is identical.
        publish(
            lease.path.parent(),
            &self._name,
            &lease.path.name,
            expected.is_none(),
        )
        .map_err(|_| PublicationError::OutcomeUnknown)?;
        let after = || -> Result<(), NativeError> {
            stage_permissions(&stage)?;
            let metadata = stage.metadata().map_err(|_| NativeError::Io)?;
            // Rename can change ctime, but must preserve the candidate's object,
            // complete length and mtime. The name must now identify that object.
            if metadata.dev() != self.metadata.dev()
                || metadata.ino() != self.metadata.ino()
                || metadata.len() != self.metadata.len()
                || metadata.mtime() != self.metadata.mtime()
                || metadata.mtime_nsec() != self.metadata.mtime_nsec()
            {
                return Err(NativeError::Conflict);
            }
            lease.match_named(&lease.path.name, &metadata)?;
            lease.verify()?;
            private_acl(lease.path.parent())
        };
        after().map_err(|_| PublicationError::OutcomeUnknown)
    }

    fn verify_stage(&self, file: &File) -> Result<(), NativeError> {
        stage_permissions(file)?;
        let metadata = file.metadata().map_err(|_| NativeError::Io)?;
        if stamp(&metadata) != stamp(&self.metadata) {
            return Err(NativeError::Conflict);
        }
        self._lease.match_named(&self._name, &metadata)
    }
}

#[cfg(test)]
mod tests;
