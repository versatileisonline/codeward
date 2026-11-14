//! Consuming synthetic publication; native visibility is not persistence.

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
};
use windows_sys::{
    Wdk::Storage::FileSystem::FILE_OPEN,
    Win32::{
        Foundation::GENERIC_READ,
        Storage::FileSystem::{DELETE, FILE_SHARE_DELETE},
    },
};

use super::PreparedStage;
use crate::{
    FILE_SHARE_READ, FILE_SHARE_WRITE, MAX_FILE_LENGTH, NativeError, ReadLease, VAULT, native,
};

/// Synthetic publication outcome. Success establishes visibility only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationError {
    /// Refused before the native rename; the live vault was not changed by this operation.
    NotPublished(NativeError),
    /// Rename was attempted and its complete outcome could not be confirmed.
    OutcomeUnknown,
}

impl PreparedStage {
    /// Consume this stage and atomically refuse an existing destination.
    ///
    /// A racing creator cannot be overwritten. Success is visibility, not a
    /// saved/committed result; namespace/storage barriers remain separate.
    pub fn publish_create(self) -> Result<(), PublicationError> {
        self.publish_with(None, &mut SystemIo)
    }

    /// Consume this stage and replace an exactly matching current vault.
    ///
    /// The caller supplies its authenticated base's complete encrypted bytes.
    /// The retained lease coordinates cooperating writers. An external writer
    /// ignoring that lease can race the last comparison and native rename.
    pub fn publish_replace(self, expected: &[u8]) -> Result<(), PublicationError> {
        self.publish_with(Some(expected), &mut SystemIo)
    }

    fn publish_with(
        mut self,
        expected: Option<&[u8]>,
        operations: &mut impl PublicationIo,
    ) -> Result<(), PublicationError> {
        use PublicationError::{NotPublished, OutcomeUnknown};

        if expected.is_some_and(|bytes| bytes.len() > MAX_FILE_LENGTH) {
            return Err(NotPublished(NativeError::TooLarge));
        }
        let owner = ReadLease {
            inner: Arc::clone(&self.lease.inner),
        };
        let _publication = owner
            .inner
            .publication
            .try_lock()
            .map_err(|_| NotPublished(NativeError::WriterUnavailable))?;
        self.verify().map_err(NotPublished)?;
        expect_current(&owner, expected).map_err(NotPublished)?;

        // Snapshot only encrypted bytes, bounded and fallibly allocated. The
        // existing read handle excludes writes/deletes throughout this read.
        let mut candidate = Vec::new();
        candidate
            .try_reserve_exact(self.stamp.len as usize)
            .map_err(|_| NotPublished(NativeError::AllocationFailed))?;
        candidate.resize(self.stamp.len as usize, 0);
        self.file
            .rewind()
            .map_err(|_| NotPublished(NativeError::Read))?;
        self.file
            .read_exact(&mut candidate)
            .map_err(|_| NotPublished(NativeError::Read))?;
        self.verify().map_err(NotPublished)?;

        // Pin the original object while releasing the restrictive read handle
        // to acquire DELETE access. This guard permits rename sharing, so a
        // substitution is refused by identity comparison rather than by name.
        let identity = owner
            .open_stage(
                &self.name,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            )
            .map_err(NotPublished)?;
        if native::regular_info(&identity).map_err(NotPublished)? != self.stamp {
            return Err(NotPublished(NativeError::Conflict));
        }
        let PreparedStage {
            lease: _,
            name,
            file,
            stamp,
            admission,
        } = self;
        operations
            .close(file)
            .map_err(|_| NotPublished(NativeError::CloseStage))?;

        let parent = &owner.inner.directories.last().expect("retained parent").1;
        let mut source = native::open_child(
            parent,
            &name,
            GENERIC_READ | DELETE,
            FILE_SHARE_READ,
            FILE_OPEN,
            None,
        )
        .map_err(|_| NotPublished(NativeError::Open))?;
        let before = (|| {
            owner.verify()?;
            admission.verify_object(&source)?;
            owner.verify_leaf(&source, &name)?;
            let actual = native::regular_info(&source)?;
            if !actual.same_file(&native::regular_info(&identity)?) || actual != stamp {
                return Err(NativeError::Conflict);
            }
            matches_bytes(&mut source, &candidate)?;
            expect_current(&owner, expected)?;
            owner.verify()?;
            admission.verify_object(&source)?;
            Ok(())
        })();
        drop(identity);
        if let Err(error) = before {
            let _ = operations.close(source);
            return Err(NotPublished(error));
        }

        // No attempted native rename error is treated as permission to replay.
        let renamed = operations.rename(&source, parent, expected.is_some());
        let observed = if renamed.is_ok() {
            (|| {
                owner.verify()?;
                admission.verify_object(&source)?;
                owner.verify_leaf(&source, VAULT)?;
                let mut visible = native::open_child(
                    parent,
                    VAULT,
                    GENERIC_READ,
                    FILE_SHARE_READ | FILE_SHARE_DELETE,
                    FILE_OPEN,
                    None,
                )
                .map_err(|_| NativeError::Open)?;
                admission.verify_object(&visible)?;
                owner.verify_leaf(&visible, VAULT)?;
                if !native::regular_info(&visible)?.same_file(&stamp) {
                    return Err(NativeError::Conflict);
                }
                matches_bytes(&mut visible, &candidate)?;
                owner.verify()
            })()
        } else {
            Err(NativeError::Open)
        };
        // The source handle remains exclusive against writes/deletes through
        // postchecks. Check its final close even when rename/postcheck failed.
        let closed = operations.close(source);
        if renamed.is_err() || observed.is_err() || closed.is_err() {
            return Err(OutcomeUnknown);
        }
        Ok(())
    }
}

fn expect_current(lease: &ReadLease, expected: Option<&[u8]>) -> Result<(), NativeError> {
    if lease.read_current()?.as_deref() != expected {
        return Err(NativeError::Conflict);
    }
    Ok(())
}

fn matches_bytes(file: &mut File, expected: &[u8]) -> Result<(), NativeError> {
    let before = native::regular_info(file)?;
    if before.len != expected.len() as u64 {
        return Err(NativeError::Conflict);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| NativeError::Read)?;
    let mut buffer = [0; 8192];
    for chunk in expected.chunks(buffer.len()) {
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
    Ok(())
}

trait PublicationIo {
    fn close(&mut self, file: File) -> io::Result<()> {
        native::close_checked(file)
    }
    fn rename(&mut self, source: &File, parent: &File, replace: bool) -> io::Result<()> {
        native::publish(source, parent, replace)
    }
}
struct SystemIo;
impl PublicationIo for SystemIo {}

#[cfg(test)]
mod tests;
