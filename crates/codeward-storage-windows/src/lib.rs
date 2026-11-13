//! Synthetic Windows storage experiments.
#![cfg(windows)]
#![deny(missing_docs, unsafe_op_in_unsafe_fn)]

#[cfg(test)]
mod tests;

use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{self, Read},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::OpenOptionsExt,
    },
    path::{Path, PathBuf},
};

use codeward_core::envelope::MAX_FILE_LENGTH;
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SHARING_VIOLATION},
    Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    },
};

mod native;

const COMPANION: &str = ".codeward.writer-lock";
const VAULT: &str = "vault.cw";

/// Controlled failures without file paths or OS diagnostic text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeError {
    /// The fixture path is outside the supported drive-absolute syntax.
    InvalidPath,
    /// Unsupported volume, file type, reparse point or link count.
    Unsupported,
    /// Another handle prevents exclusive companion acquisition.
    WriterUnavailable,
    /// The fixture or companion could not be opened or verified.
    Open,
    /// The vault could not be read or changed during observation.
    Read,
    /// The vault exceeds the core input cap.
    TooLarge,
    /// Bounded input allocation failed.
    AllocationFailed,
}

/// Retained handles and writer exclusion for a synthetic NTFS fixture.
pub struct ReadLease {
    parent: PathBuf,
    directories: Vec<(PathBuf, File)>,
    companion: File,
}

impl ReadLease {
    /// Open a pre-existing synthetic directory on a fixed NTFS drive.
    ///
    /// The trusted caller supplies a disposable fixture directory. This API
    /// does not resolve app-data paths or admit real vaults.
    pub fn acquire_synthetic(path: &Path) -> Result<Self, NativeError> {
        let (root_path, components) = split_path(path)?;
        native::require_fixed_drive(&root_path)?;
        let root = open_directory(&root_path)?;
        native::require_ntfs(&root)?;
        let mut parent = native::final_path(&root)?;
        if parent.parent().is_some() || parent.file_name().is_some() {
            return Err(NativeError::Unsupported);
        }
        native::require_fixed_drive(&parent)?;
        let mut directories = vec![(parent.clone(), root)];
        for component in components {
            let directory = open_directory(&parent.join(component))?;
            let actual = native::final_path(&directory)?;
            if actual.parent() != Some(parent.as_path()) {
                return Err(NativeError::Unsupported);
            }
            directories.push((actual.clone(), directory));
            parent = actual;
        }

        let companion = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(parent.join(COMPANION))
            .map_err(|error| {
                if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) {
                    NativeError::WriterUnavailable
                } else {
                    NativeError::Open
                }
            })?;
        let lease = Self {
            parent,
            directories,
            companion,
        };
        lease.verify()?;
        Ok(lease)
    }

    /// Read the bounded named file or confirm its absence.
    ///
    /// Returned bytes require separate parsing and authentication by the core.
    pub fn read_current(&self) -> Result<Option<Vec<u8>>, NativeError> {
        self.verify()?;
        let mut file = match self.open_vault() {
            Ok(file) => file,
            Err(error) if leaf_missing(&error) => {
                self.verify()?;
                return match self.open_vault() {
                    Err(error) if leaf_missing(&error) => Ok(None),
                    _ => Err(NativeError::Read),
                };
            }
            Err(_) => return Err(NativeError::Read),
        };
        let before = native::regular_info(&file)?;
        let path = self.verify_leaf(&file)?;
        if before.len > MAX_FILE_LENGTH as u64 {
            return Err(NativeError::TooLarge);
        }
        let length = before.len as usize;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| NativeError::AllocationFailed)?;
        bytes.resize(length, 0);
        file.read_exact(&mut bytes).map_err(|_| NativeError::Read)?;
        let mut extra = [0];
        loop {
            match file.read(&mut extra) {
                Ok(0) => break,
                Ok(_) if length == MAX_FILE_LENGTH => return Err(NativeError::TooLarge),
                Ok(_) => return Err(NativeError::Read),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return Err(NativeError::Read),
            }
        }
        self.verify()?;
        if native::regular_info(&file)? != before || self.verify_leaf(&file)? != path {
            return Err(NativeError::Read);
        }
        Ok(Some(bytes))
    }

    fn open_vault(&self) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(self.parent.join(VAULT))
    }

    fn verify(&self) -> Result<(), NativeError> {
        for (path, directory) in &self.directories {
            native::require_directory(directory)?;
            if &native::final_path(directory)? != path {
                return Err(NativeError::Open);
            }
        }
        if native::regular_info(&self.companion)?.len != 0 {
            return Err(NativeError::Unsupported);
        }
        self.verify_leaf(&self.companion)?;
        Ok(())
    }

    fn verify_leaf(&self, file: &File) -> Result<PathBuf, NativeError> {
        let path = native::final_path(file)?;
        if path.parent() != Some(self.parent.as_path()) {
            return Err(NativeError::Unsupported);
        }
        Ok(path)
    }
}

fn leaf_missing(error: &io::Error) -> bool {
    error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32)
}

fn open_directory(path: &Path) -> Result<File, NativeError> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|_| NativeError::Open)?;
    native::require_directory(&file)?;
    Ok(file)
}

fn split_path(path: &Path) -> Result<(PathBuf, Vec<OsString>), NativeError> {
    let units: Vec<_> = path.as_os_str().encode_wide().collect();
    let [drive, 58, 92, rest @ ..] = units.as_slice() else {
        return Err(NativeError::InvalidPath);
    };
    if !u8::try_from(*drive).is_ok_and(|letter| letter.is_ascii_alphabetic()) {
        return Err(NativeError::InvalidPath);
    }
    let mut components = Vec::new();
    for component in rest.split(|&unit| unit == 92) {
        if component.is_empty()
            || matches!(component.last(), Some(32 | 46))
            || component
                .iter()
                .any(|&unit| matches!(unit, 0..=31 | 34 | 42 | 47 | 58 | 60 | 62 | 63 | 124))
        {
            return Err(NativeError::InvalidPath);
        }
        let name = OsString::from_wide(component);
        let stem = name
            .to_string_lossy()
            .split('.')
            .next()
            .unwrap_or_default()
            .trim_end()
            .to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || stem
                .strip_prefix("COM")
                .or_else(|| stem.strip_prefix("LPT"))
                .is_some_and(|number| {
                    matches!(
                        number,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
        {
            return Err(NativeError::InvalidPath);
        }
        components.push(name);
    }
    Ok((PathBuf::from(OsString::from_wide(&units[..3])), components))
}
