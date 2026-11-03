use std::{
    ffi::CString,
    fs::File,
    io::{self, Write},
    os::unix::fs::MetadataExt,
};

use codeward_core::envelope::MAX_FILE_LENGTH;

use crate::{NativeError, ReadLease, native, path::validate_regular};

const PREFIX: &[u8] = b".codeward.stage-";
const RESERVED: &[u8] = b".codeward.";
const ATTEMPTS: usize = 8;

pub(super) fn reserved_name(name: &[u8]) -> bool {
    name.get(..RESERVED.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(RESERVED))
}

/// Opaque completed synthetic stage, borrowing its writer-exclusion lease.
///
/// Dropping this marker preserves the file. There is no publication, cleanup,
/// committed-revision or save-success operation. Stage names and handles are not
/// exposed. Preparation alone never establishes namespace persistence.
///
/// ```compile_fail
/// use codeward_storage_macos::ReadLease;
/// fn cannot_release_writer_early(lease: ReadLease) {
///     let stage = lease.prepare_stage(b"synthetic encrypted bytes").unwrap();
///     drop(lease);
///     drop(stage);
/// }
/// ```
pub struct PreparedStage<'lease> {
    _lease: &'lease ReadLease,
    _name: CString,
}

impl ReadLease {
    /// Write an already-encrypted candidate to a fresh restrictive sibling file.
    ///
    /// Trusted core callers must supply encrypted bytes only; this method does
    /// not authenticate or encrypt its input. Synthetic data only while location
    /// policy, publication and native commit acceptance remain unfinished.
    /// The cap is checked before creation. Parent/stage ACLs must be empty;
    /// complete writes, fsync, F_FULLFSYNC and explicit close must all succeed.
    /// Every failure leaves the live destination untouched and preserves residue.
    pub fn prepare_stage(&self, encrypted: &[u8]) -> Result<PreparedStage<'_>, NativeError> {
        self.prepare_with(encrypted, &mut SystemIo, &mut native::stage_random)
    }

    fn prepare_with(
        &self,
        encrypted: &[u8],
        operations: &mut impl StageIo,
        random: &mut impl FnMut() -> io::Result<[u8; 16]>,
    ) -> Result<PreparedStage<'_>, NativeError> {
        if encrypted.len() > MAX_FILE_LENGTH {
            return Err(NativeError::TooLarge);
        }
        self.verify()?;
        private_acl(self.path.parent())?;
        let (mut file, name) = self.create_stage(random)?;
        // All exits retain any created stage. No speculative cleanup or promotion.
        let prepared = (|| {
            self.verify()?;
            private_acl(self.path.parent())?;
            let before = file.metadata().map_err(|_| NativeError::Io)?;
            stage_permissions(&file)?;
            self.match_named(&name, &before)?;
            let mut remaining = encrypted;
            while !remaining.is_empty() {
                match operations.write(&mut file, remaining) {
                    Ok(0) => return Err(NativeError::Write),
                    Ok(count) if count <= remaining.len() => remaining = &remaining[count..],
                    Ok(_) => return Err(NativeError::Write),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => return Err(NativeError::Write),
                }
            }
            operations.sync(&file).map_err(|_| NativeError::SyncStage)?;
            stage_permissions(&file)?;
            let after = file.metadata().map_err(|_| NativeError::Io)?;
            if after.dev() != before.dev()
                || after.ino() != before.ino()
                || after.len() != encrypted.len() as u64
            {
                return Err(NativeError::Conflict);
            }
            self.verify()?;
            private_acl(self.path.parent())?;
            self.match_named(&name, &after)?;
            Ok(after)
        })();
        // Even failed writes receive a checked, ownership-consuming close. The
        // earlier failure remains authoritative if both operations fail.
        let closed = operations.close(file).map_err(|_| NativeError::Close);
        let metadata = prepared?;
        closed?;
        self.verify()?;
        private_acl(self.path.parent())?;
        self.match_named(&name, &metadata)?;
        Ok(PreparedStage {
            _lease: self,
            _name: name,
        })
    }

    fn create_stage(
        &self,
        random: &mut impl FnMut() -> io::Result<[u8; 16]>,
    ) -> Result<(File, CString), NativeError> {
        for _ in 0..ATTEMPTS {
            let bytes = random().map_err(|_| NativeError::RandomnessUnavailable)?;
            let mut name = String::from_utf8(PREFIX.to_vec()).expect("ASCII prefix");
            for byte in bytes {
                use std::fmt::Write;
                write!(&mut name, "{byte:02x}").expect("String formatting");
            }
            let name = CString::new(name).expect("hex name");
            match native::open_at(
                self.path.parent(),
                &name,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                0o600,
            ) {
                Ok(file) => return Ok((file, name)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(NativeError::Io),
            }
        }
        Err(NativeError::StageUnavailable)
    }
}

fn private_acl(file: &File) -> Result<(), NativeError> {
    match native::no_extended_acl(file) {
        Ok(true) => Ok(()),
        Ok(false) => Err(NativeError::Permissions),
        Err(_) => Err(NativeError::Unsupported),
    }
}

fn stage_permissions(file: &File) -> Result<(), NativeError> {
    let metadata = file.metadata().map_err(|_| NativeError::Io)?;
    validate_regular(&metadata)?;
    if metadata.mode() & 0o777 != 0o600 {
        return Err(NativeError::Permissions);
    }
    private_acl(file)
}

trait StageIo {
    fn write(&mut self, file: &mut File, bytes: &[u8]) -> io::Result<usize> {
        file.write(bytes)
    }
    fn sync(&mut self, file: &File) -> io::Result<()> {
        native::sync_stage(file)
    }
    fn close(&mut self, file: File) -> io::Result<()> {
        native::close_checked(file)
    }
}
struct SystemIo;
impl StageIo for SystemIo {}

#[cfg(test)]
mod tests;
