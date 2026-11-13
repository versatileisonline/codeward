use std::{fs::File, io};

use crate::{NativeError, ReadLease};

impl ReadLease {
    /// Synchronize the exact expected synthetic vault and retained directories.
    ///
    /// For expected absence, synchronize only the directories. The caller must
    /// preserve uncertainty if a check or synchronization fails after publication.
    /// This operation does not authenticate the vault or acknowledge a core save.
    pub fn synchronize_namespace(&self, expected: Option<&[u8]>) -> Result<(), NativeError> {
        self.synchronize_namespace_with(expected, File::sync_all)
    }

    pub(super) fn synchronize_namespace_with(
        &self,
        expected: Option<&[u8]>,
        mut sync: impl FnMut(&File) -> io::Result<()>,
    ) -> Result<(), NativeError> {
        let _guard = self
            .publication
            .try_lock()
            .map_err(|_| NativeError::WriterUnavailable)?;
        let current = self.expect_current(expected)?;
        if let Some(current) = &current {
            sync(&current.file).map_err(|_| NativeError::SyncNamespace)?;
        }
        for directory in self.location.directories_from_leaf() {
            self.verify_current(current.as_ref())?;
            sync(directory).map_err(|_| NativeError::SyncNamespace)?;
        }
        self.verify_current(current.as_ref())
    }
}
