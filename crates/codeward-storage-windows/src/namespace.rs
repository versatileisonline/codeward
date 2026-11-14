//! Expected-state namespace barriers for disposable synthetic NTFS storage.

use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek},
    path::Path,
};
use windows_sys::{
    Wdk::Storage::FileSystem::FILE_OPEN,
    Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE},
        Storage::FileSystem::{
            FILE_ADD_FILE, FILE_READ_ATTRIBUTES, FILE_TRAVERSE, READ_CONTROL, SYNCHRONIZE,
        },
    },
};

use crate::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    MAX_FILE_LENGTH, NativeError, ReadLease, VAULT, leaf_missing, managed, native,
};
use std::os::windows::fs::OpenOptionsExt;

impl ReadLease {
    /// Synchronize the complete expected encrypted file, or confirmed absence,
    /// then the namespace from the leaf through the existing fixture/AppData anchor.
    ///
    /// This synthetic NTFS experiment does not authenticate bytes or admit real
    /// vaults. Every failure preserves the visible file and transaction residue.
    /// After any earlier publication the caller must retain `OutcomeUnknown` on
    /// failure; this operation never rolls back, cleans up or replays a write.
    pub fn synchronize_namespace(&self, expected: Option<&[u8]>) -> Result<(), NativeError> {
        self.synchronize_with(expected, &mut SystemIo)
    }

    fn synchronize_with(
        &self,
        expected: Option<&[u8]>,
        operations: &mut impl NamespaceIo,
    ) -> Result<(), NativeError> {
        if expected.is_some_and(|bytes| bytes.len() > MAX_FILE_LENGTH) {
            return Err(NativeError::TooLarge);
        }
        let _publication = self
            .inner
            .publication
            .try_lock()
            .map_err(|_| NativeError::WriterUnavailable)?;
        self.verify()?;
        let anchor = self.inner.managed.as_ref().map_or(
            self.inner.directories.len() - 1,
            managed::Admission::namespace_anchor,
        );
        // A drive-root directory flush could become a volume-wide operation.
        // The existing anchor is the final boundary, never its parent.
        if anchor == 0 {
            return Err(NativeError::Unsupported);
        }
        let admission = managed::Admission::new()?;
        let parent = &self.inner.directories.last().expect("retained parent").1;
        let mut candidate = match operations.open_file(parent) {
            Ok(file) => Some(file),
            Err(error) if leaf_missing(&error) => None,
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                return Err(NativeError::Unsupported);
            }
            Err(_) => return Err(NativeError::Open),
        };
        // Consume every temporary handle through checked close, including on an
        // earlier validation/flush error. Preserve the earlier failure as primary.
        let result = (|| {
            self.verify_expected(candidate.as_mut(), expected, &admission)?;
            if let Some(file) = candidate.as_mut() {
                let before = native::regular_info(file)?;
                operations
                    .flush(file)
                    .map_err(|_| NativeError::SyncNamespace)?;
                self.verify_expected(Some(file), expected, &admission)?;
                if native::regular_info(file)? != before {
                    return Err(NativeError::Conflict);
                }
            }
            for index in (anchor..self.inner.directories.len()).rev() {
                self.verify_expected(candidate.as_mut(), expected, &admission)?;
                let (path, retained) = &self.inner.directories[index];
                let file = operations
                    .open_directory(path)
                    .map_err(|_| NativeError::Open)?;
                let synchronized = (|| {
                    self.verify_namespace_directory(&file, retained, path, &admission)?;
                    operations
                        .flush(&file)
                        .map_err(|_| NativeError::SyncNamespace)?;
                    self.verify_namespace_directory(&file, retained, path, &admission)?;
                    self.verify_expected(candidate.as_mut(), expected, &admission)
                })();
                let closed = operations
                    .close(file)
                    .map_err(|_| NativeError::SyncNamespace);
                synchronized?;
                closed?;
                self.verify_expected(candidate.as_mut(), expected, &admission)?;
            }
            Ok(())
        })();
        // Keep the flushed object pinned and deny deletion while its writable
        // handle is closed. The read guard shares writes only to coexist with
        // that handle; a subsequent restrictive reader closes the sharing gap.
        let protected = if result.is_ok() {
            candidate
                .as_ref()
                .map(|file| {
                    let stamp = native::regular_info(file)?;
                    let guard = native::open_child(
                        parent,
                        VAULT,
                        GENERIC_READ,
                        FILE_SHARE_READ | FILE_SHARE_WRITE,
                        FILE_OPEN,
                        None,
                    )
                    .map_err(|_| NativeError::Open)?;
                    let checked = (|| {
                        admission.verify_object(&guard)?;
                        self.verify_leaf(&guard, VAULT)?;
                        if native::regular_info(&guard)? != stamp {
                            return Err(NativeError::Conflict);
                        }
                        Ok(())
                    })();
                    if let Err(error) = checked {
                        let _ = operations.close(guard);
                        return Err(error);
                    }
                    Ok((guard, stamp))
                })
                .transpose()
        } else {
            Ok(None)
        };
        let closed = candidate.map_or(Ok(()), |file| {
            operations
                .close(file)
                .map_err(|_| NativeError::SyncNamespace)
        });
        result?;
        let Some((guard, stamp)) = protected? else {
            closed?;
            self.verify()?;
            return if self.read_current()?.is_none() {
                Ok(())
            } else {
                Err(NativeError::Conflict)
            };
        };
        let mut final_reader = None;
        let checked = (|| {
            closed?;
            // This reader denies write/delete sharing after the writable handle
            // has closed. Check the original identity, metadata and full bytes,
            // rather than accepting a same-byte replacement through read_current.
            final_reader = Some(self.open_vault().map_err(|_| NativeError::Open)?);
            self.verify_expected(final_reader.as_mut(), expected, &admission)?;
            if native::regular_info(final_reader.as_ref().unwrap())? != stamp {
                return Err(NativeError::Conflict);
            }
            Ok(())
        })();
        // Consume both temporary handles even when an earlier check/close failed.
        // The final reader keeps write/delete exclusion while the guard closes.
        let guard_closed = operations
            .close(guard)
            .map_err(|_| NativeError::SyncNamespace);
        let verified = (|| {
            checked?;
            guard_closed?;
            self.verify_expected(final_reader.as_mut(), expected, &admission)?;
            if native::regular_info(final_reader.as_ref().unwrap())? != stamp {
                return Err(NativeError::Conflict);
            }
            Ok(())
        })();
        let final_closed = final_reader.map_or(Ok(()), |file| {
            operations
                .close(file)
                .map_err(|_| NativeError::SyncNamespace)
        });
        verified?;
        final_closed?;
        // An arbitrary writer ignoring the companion can still race after the
        // last protected handle closes; no external-writer snapshot is promised.
        self.verify()
    }

    fn verify_expected(
        &self,
        candidate: Option<&mut File>,
        expected: Option<&[u8]>,
        admission: &managed::Admission,
    ) -> Result<(), NativeError> {
        self.verify()?;
        match (candidate, expected) {
            (Some(file), Some(bytes)) => {
                admission.verify_object(file)?;
                self.verify_leaf(file, VAULT)?;
                let before = native::regular_info(file)?;
                if before.len > MAX_FILE_LENGTH as u64 {
                    return Err(NativeError::TooLarge);
                }
                if before.len != bytes.len() as u64 {
                    return Err(NativeError::Conflict);
                }
                file.rewind().map_err(|_| NativeError::Read)?;
                let mut buffer = [0; 8192];
                for chunk in bytes.chunks(buffer.len()) {
                    file.read_exact(&mut buffer[..chunk.len()])
                        .map_err(|_| NativeError::Read)?;
                    if &buffer[..chunk.len()] != chunk {
                        return Err(NativeError::Conflict);
                    }
                }
                if file.read(&mut buffer[..1]).map_err(|_| NativeError::Read)? != 0
                    || native::regular_info(file)? != before
                {
                    return Err(NativeError::Conflict);
                }
                admission.verify_object(file)?;
                self.verify_leaf(file, VAULT)?;
            }
            (None, None) if self.read_current()?.is_none() => {}
            _ => return Err(NativeError::Conflict),
        }
        self.verify()
    }

    fn verify_namespace_directory(
        &self,
        file: &File,
        retained: &File,
        path: &Path,
        admission: &managed::Admission,
    ) -> Result<(), NativeError> {
        self.verify()?;
        native::require_ntfs(file)?;
        if !native::same_directory(file, retained)? || native::final_path(file)? != path {
            return Err(NativeError::Conflict);
        }
        admission.verify_object(file)
    }
}

trait NamespaceIo {
    fn open_file(&mut self, parent: &File) -> io::Result<File> {
        native::open_child(
            parent,
            VAULT,
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ,
            FILE_OPEN,
            None,
        )
    }
    fn open_directory(&mut self, path: &Path) -> io::Result<File> {
        // Temporary measured directory-specific write rights. The long-lived
        // retained handles keep their existing metadata/traverse-only access.
        OpenOptions::new()
            .access_mode(
                FILE_ADD_FILE | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            )
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
    }
    fn flush(&mut self, file: &File) -> io::Result<()> {
        native::flush(file)
    }
    fn close(&mut self, file: File) -> io::Result<()> {
        native::close_checked(file)
    }
}
struct SystemIo;
impl NamespaceIo for SystemIo {}

#[cfg(test)]
mod tests;
