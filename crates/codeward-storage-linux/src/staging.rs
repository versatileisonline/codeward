use std::{
    ffi::CString,
    fmt::Write as _,
    fs::{File, Metadata},
    io::{self, Write},
};

use crate::{
    MAX_FILE_LENGTH, NativeError, ReadLease, changed, metadata, native, open_error,
    same_observation, validate_regular,
};

const ATTEMPTS: usize = 8;

/// A completed synthetic encrypted stage tied to its writer lease.
///
/// The private file remains in place when the marker is dropped. A stage
/// establishes neither publication nor a committed revision.
pub struct PreparedStage<'lease> {
    lease: &'lease ReadLease,
    name: CString,
    identity: File,
    stamp: Metadata,
}

impl PreparedStage<'_> {
    /// Recheck the lease and the staged file's observed identity and metadata.
    pub fn verify(&self) -> Result<(), NativeError> {
        self.lease.verify()?;
        let named = self.lease.open_stage_identity(&self.name)?;
        if !same_observation(&self.stamp, &metadata(&self.identity)?)
            || !same_observation(&self.stamp, &metadata(&named)?)
        {
            return Err(NativeError::Conflict);
        }
        Ok(())
    }
}

impl ReadLease {
    /// Prepare already-encrypted synthetic bytes in a new private sibling.
    ///
    /// The caller owns encryption and authentication. The live vault is never
    /// modified by preparation. An incomplete stage is preserved on failure.
    pub fn prepare_stage(&self, encrypted: &[u8]) -> Result<PreparedStage<'_>, NativeError> {
        self.prepare_stage_with(encrypted, File::write_all)
    }

    pub(super) fn prepare_stage_with(
        &self,
        encrypted: &[u8],
        write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
    ) -> Result<PreparedStage<'_>, NativeError> {
        if encrypted.len() > MAX_FILE_LENGTH {
            return Err(NativeError::TooLarge);
        }
        self.verify()?;
        let (mut file, name) = self.create_stage()?;
        let prepared = (|| {
            self.verify()?;
            let initial = validate_regular(&file, self.location.owner)?;
            let identity = self.open_stage_identity(&name)?;
            if !same_observation(&initial, &metadata(&identity)?) {
                return Err(NativeError::Conflict);
            }

            write(&mut file, encrypted).map_err(|_| NativeError::WriteStage)?;
            let stamp = validate_regular(&file, self.location.owner)?;
            if stamp.len() != encrypted.len() as u64 {
                return Err(NativeError::Conflict);
            }
            file.sync_all().map_err(|_| NativeError::SyncStage)?;

            Ok(PreparedStage {
                lease: self,
                name,
                identity,
                stamp,
            })
        })();

        let closed = native::close_checked(file).map_err(|_| NativeError::CloseStage);
        let stage = prepared?;
        closed?;
        stage.verify()?;
        Ok(stage)
    }

    fn create_stage(&self) -> Result<(File, CString), NativeError> {
        for _ in 0..ATTEMPTS {
            let random = native::stage_random().map_err(|_| NativeError::RandomnessUnavailable)?;
            let mut name = String::from(".codeward.stage-");
            for byte in random {
                write!(&mut name, "{byte:02x}").expect("String formatting");
            }
            let name = CString::new(name).expect("ASCII stage name");
            match native::open_at(
                self.location.parent(),
                &name,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                true,
            ) {
                Ok(file) => return Ok((file, name)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(open_error(error)),
            }
        }
        Err(NativeError::StageUnavailable)
    }

    fn open_stage_identity(&self, name: &std::ffi::CStr) -> Result<File, NativeError> {
        native::open_at(self.location.parent(), name, libc::O_PATH, true).map_err(changed)
    }
}
