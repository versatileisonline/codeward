//! Encrypted-only synthetic staging; no publication or save acknowledgement.

use std::{
    fmt::Write as _,
    fs::File,
    io::{self, Read, Write},
    sync::Arc,
};
use windows_sys::{Wdk::Storage::FileSystem::FILE_OPEN, Win32::Foundation::GENERIC_READ};

use crate::{
    FILE_SHARE_READ, FILE_SHARE_WRITE, MAX_FILE_LENGTH, NativeError, ReadLease, managed, native,
};

/// A complete private encrypted sibling retaining the original writer lease.
///
/// The read handle denies writes and deletes. Dropping the stage preserves
/// its file; it never promotes or cleans up residue. There is no save result.
///
/// ```compile_fail
/// use codeward_storage_windows::PreparedStage;
/// fn cannot_duplicate(stage: PreparedStage) { let _copy = stage.clone(); }
/// ```
pub struct PreparedStage {
    lease: ReadLease,
    name: String,
    file: File,
    stamp: native::Observation,
    admission: managed::Admission,
}

impl PreparedStage {
    /// Recheck admission, retained identity and the completed file observation.
    pub fn verify(&self) -> Result<(), NativeError> {
        self.lease.verify()?;
        self.admission.verify_object(&self.file)?;
        self.lease.verify_leaf(&self.file, &self.name)?;
        if native::regular_info(&self.file)? != self.stamp {
            return Err(NativeError::Conflict);
        }
        Ok(())
    }
}

impl ReadLease {
    /// Prepare a complete already-encrypted synthetic candidate in a fresh file.
    ///
    /// The cap precedes creation. The stage receives a protected per-user DACL
    /// at exclusive creation relative to the retained directory. Checked writes,
    /// file flush, explicit write-handle close and encrypted readback precede
    /// success. Failures preserve residue and never modify the named vault.
    /// This does not encrypt/authenticate input, publish it, persist a namespace
    /// change or acknowledge a save. Synthetic NTFS experiments only.
    pub fn prepare_stage(&self, encrypted: &[u8]) -> Result<PreparedStage, NativeError> {
        self.prepare_with(encrypted, &mut SystemIo, &mut native::stage_random)
    }

    fn prepare_with(
        &self,
        encrypted: &[u8],
        operations: &mut impl StageIo,
        random: &mut impl FnMut() -> io::Result<[u8; 16]>,
    ) -> Result<PreparedStage, NativeError> {
        if encrypted.len() > MAX_FILE_LENGTH {
            return Err(NativeError::TooLarge);
        }
        self.verify()?;
        let admission = managed::Admission::new()?;
        let (mut file, name) = self.create_stage(&admission, random)?;
        // All exits preserve the exclusively created file. Always attempt the
        // ownership-consuming close, preserving any earlier error as primary.
        let prepared = (|| {
            self.verify()?;
            admission.verify_object(&file)?;
            self.verify_leaf(&file, &name)?;
            let mut remaining = encrypted;
            while !remaining.is_empty() {
                match operations.write(&mut file, remaining) {
                    Ok(0) => return Err(NativeError::WriteStage),
                    Ok(count) if count <= remaining.len() => remaining = &remaining[count..],
                    Ok(_) => return Err(NativeError::WriteStage),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => return Err(NativeError::WriteStage),
                }
            }
            operations.sync(&file).map_err(|_| NativeError::SyncStage)?;
            let before = native::regular_info(&file)?;
            if before.len != encrypted.len() as u64 {
                return Err(NativeError::Conflict);
            }
            admission.verify_object(&file)?;
            self.verify()?;
            // The read guard prevents deletion/substitution across checked
            // write-handle close. It shares writes to coexist with that handle;
            // the original handle itself still excludes other writers.
            let guard = self.open_stage(&name, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
            if !before.same_file(&native::regular_info(&guard)?) {
                return Err(NativeError::Conflict);
            }
            Ok(guard)
        })();
        let closed = operations.close(file).map_err(|_| NativeError::CloseStage);
        let guard = prepared?;
        closed?;

        // Opening without write/delete sharing refuses any concurrent writer
        // before selecting the final observation. Closing the last write handle
        // may finalize NTFS timestamps, so compare identity and complete bytes
        // rather than expecting pre-close timestamps to remain unchanged.
        let mut file = self.open_stage(&name, FILE_SHARE_READ)?;
        if !native::regular_info(&guard)?.same_file(&native::regular_info(&file)?) {
            return Err(NativeError::Conflict);
        }
        drop(guard);
        admission.verify_object(&file)?;
        let before = native::regular_info(&file)?;
        if before.len != encrypted.len() as u64 {
            return Err(NativeError::Conflict);
        }
        let mut buffer = [0; 8192];
        for chunk in encrypted.chunks(buffer.len()) {
            file.read_exact(&mut buffer[..chunk.len()])
                .map_err(|_| NativeError::Read)?;
            if &buffer[..chunk.len()] != chunk {
                return Err(NativeError::Conflict);
            }
        }
        if file.read(&mut buffer[..1]).map_err(|_| NativeError::Read)? != 0 {
            return Err(NativeError::Conflict);
        }
        let stage = PreparedStage {
            lease: ReadLease {
                inner: Arc::clone(&self.inner),
            },
            name,
            file,
            stamp: before,
            admission,
        };
        stage.verify()?;
        Ok(stage)
    }

    fn create_stage(
        &self,
        admission: &managed::Admission,
        random: &mut impl FnMut() -> io::Result<[u8; 16]>,
    ) -> Result<(File, String), NativeError> {
        for _ in 0..8 {
            let bytes = random().map_err(|_| NativeError::RandomnessUnavailable)?;
            let mut name = String::from(".codeward.stage-");
            for byte in bytes {
                write!(&mut name, "{byte:02x}").expect("String formatting");
            }
            self.verify()?;
            match admission.create_stage(
                &self.inner.directories.last().expect("retained parent").1,
                &name,
            ) {
                Ok(file) => return Ok((file, name)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                    return Err(NativeError::Permissions);
                }
                Err(_) => return Err(NativeError::Open),
            }
        }
        Err(NativeError::StageUnavailable)
    }

    fn open_stage(&self, name: &str, sharing: u32) -> Result<File, NativeError> {
        native::open_child(
            &self.inner.directories.last().expect("retained parent").1,
            name,
            GENERIC_READ,
            sharing,
            FILE_OPEN,
            None,
        )
        .map_err(|_| NativeError::Open)
    }
}

trait StageIo {
    fn write(&mut self, file: &mut File, bytes: &[u8]) -> io::Result<usize> {
        file.write(bytes)
    }
    fn sync(&mut self, file: &File) -> io::Result<()> {
        file.sync_all()
    }
    fn close(&mut self, file: File) -> io::Result<()> {
        native::close_checked(file)
    }
}

struct SystemIo;
impl StageIo for SystemIo {}

#[cfg(test)]
mod tests;
