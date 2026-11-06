use std::{
    fs::{File, Metadata},
    io::{self, Read},
    os::unix::fs::MetadataExt,
};

use codeward_core::envelope::MAX_FILE_LENGTH;

use crate::{
    NativeError, ReadLease, native,
    path::{open_error, private_acl, validate_regular},
};

impl ReadLease {
    /// Read bounded, unauthenticated encrypted input under retained exclusion.
    ///
    /// Absence is distinct from failure. Descriptor metadata is checked before
    /// buffer allocation, every growth is capped, and a one-byte stack probe
    /// rejects growth beyond 16 MiB. Changed file/directory/companion identity or
    /// observed metadata change fails closed. Parent, companion and vault extended
    /// ACLs are refused; the vault is checked before buffering and after reading.
    /// Returned bytes still need the core's
    /// structural, authentication and schema gates; a read never grants access.
    pub fn read_current(&self) -> Result<Option<Vec<u8>>, NativeError> {
        self.read_with_hook(|| {})
    }

    fn read_with_hook(&self, after_open: impl FnOnce()) -> Result<Option<Vec<u8>>, NativeError> {
        self.verify()?;
        let mut file =
            match native::open_at(self.path().parent(), &self.path().name, libc::O_RDONLY, 0) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    self.verify()?;
                    // Confirm absence at the still-validated name; a racing creation
                    // is a conflict, not evidence that the destination is empty.
                    return match native::named_stat(self.path().parent(), &self.path().name) {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                        _ => Err(NativeError::Conflict),
                    };
                }
                Err(error) => return Err(open_error(error)),
            };
        let before = file.metadata().map_err(|_| NativeError::Io)?;
        validate_regular(&before)?;
        private_acl(&file)?;
        if self.is_companion(&before) {
            return Err(NativeError::Unsupported);
        }
        if before.len() > MAX_FILE_LENGTH as u64 {
            return Err(NativeError::TooLarge);
        }
        after_open();
        let bytes = read_capped(&mut file, before.len() as usize)?;
        let after = file.metadata().map_err(|_| NativeError::Io)?;
        validate_regular(&after)?;
        private_acl(&file)?;
        if stamp(&before) != stamp(&after) || after.len() != bytes.len() as u64 {
            return Err(NativeError::Conflict);
        }
        self.verify()?;
        self.match_named(&self.path().name, &after)?;
        Ok(Some(bytes))
    }
}

fn read_capped(file: &mut File, expected_length: usize) -> Result<Vec<u8>, NativeError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(expected_length)
        .map_err(|_| NativeError::AllocationFailed)?;
    let mut chunk = [0; 8192];
    loop {
        let remaining = MAX_FILE_LENGTH - bytes.len();
        // Even at the exact cap, only one extra byte is read into stack storage.
        let limit = remaining.min(chunk.len()).max(1);
        let count = match file.read(&mut chunk[..limit]) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(NativeError::Io),
            Ok(count) => count,
        };
        if count == 0 {
            return Ok(bytes);
        }
        if count > remaining {
            return Err(NativeError::TooLarge);
        }
        if count > expected_length - bytes.len() {
            return Err(NativeError::Conflict);
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

pub(super) fn stamp(meta: &Metadata) -> (u64, u64, u64, u32, u32, u64, i64, i64, i64, i64) {
    (
        meta.dev(),
        meta.ino(),
        meta.nlink(),
        meta.uid(),
        meta.mode(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
    )
}

#[cfg(test)]
mod tests;
