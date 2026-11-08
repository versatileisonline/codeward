use std::{fs::File, io};

use codeward_core::envelope::MAX_FILE_LENGTH;

use crate::{
    NativeError, ReadLease, native,
    reading::{ObservedFile, stamp},
    staging::stage_permissions,
};

impl ReadLease {
    /// Exercise strict file/parent synchronization for an expected synthetic state.
    ///
    /// `Some` supplies complete encrypted bytes from a trusted caller's already
    /// authenticated state; `None` requires the actual name to be absent. Bounds,
    /// bytes, identities, permissions and ACLs are checked under the same local
    /// guard as publication. The checked file and directory handles each require
    /// fsync then F_FULLFSYNC. Managed paths include every directory from Vaults
    /// through the account home, even on reopen or when the vault is absent.
    ///
    /// Success establishes only that this candidate sequence completed against the
    /// checked observation. It does not authenticate input, authorize access or
    /// acknowledge Saved/Ready/RecoveryComplete. Synthetic data only until native
    /// transaction, location/privacy and persistence acceptance are complete.
    /// Any failure after an earlier publication attempt must remain OutcomeUnknown
    /// in the caller; errors never authorize rollback, cleanup or blind replay.
    pub fn synchronize_namespace(&self, expected: Option<&[u8]>) -> Result<(), NativeError> {
        self.namespace_with(expected, &mut SystemIo)
    }

    fn namespace_with(
        &self,
        expected: Option<&[u8]>,
        operations: &mut impl NamespaceIo,
    ) -> Result<(), NativeError> {
        if expected.is_some_and(|bytes| bytes.len() > MAX_FILE_LENGTH) {
            return Err(NativeError::TooLarge);
        }
        // One lease can be shared by threads. Neither a replacement nor another
        // barrier observation may interleave with this check/sync/postcheck.
        let _publication = self
            .publication()
            .try_lock()
            .map_err(|_| NativeError::WriterUnavailable)?;
        let current = self.observe_current()?;
        if current.as_ref().map(|file| file.bytes.as_slice()) != expected {
            return Err(NativeError::Conflict);
        }
        self.verify_observation(current.as_ref())?;
        for file in current
            .iter()
            .map(|file| &file.file)
            .chain(self.path().namespace_directories())
        {
            operations
                .fsync(file)
                .map_err(|_| NativeError::SyncNamespace)?;
            operations
                .full_sync(file)
                .map_err(|_| NativeError::SyncNamespace)?;
        }
        self.verify_observation(current.as_ref())
    }

    fn verify_observation(&self, current: Option<&ObservedFile>) -> Result<(), NativeError> {
        self.verify()?;
        if let Some(current) = current {
            stage_permissions(&current.file)?;
            let after = current.file.metadata().map_err(|_| NativeError::Io)?;
            if stamp(&after) != stamp(&current.metadata) {
                return Err(NativeError::Conflict);
            }
            self.match_named(&self.path().name, &after)
        } else {
            match native::named_stat(self.path().parent(), &self.path().name) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                _ => Err(NativeError::Conflict),
            }
        }
    }
}

trait NamespaceIo {
    fn fsync(&mut self, file: &File) -> io::Result<()> {
        native::fsync_once(file)
    }
    fn full_sync(&mut self, file: &File) -> io::Result<()> {
        native::full_sync(file)
    }
}
struct SystemIo;
impl NamespaceIo for SystemIo {}

#[cfg(test)]
mod tests;
