//! Single-component file operations bound to a retained NTFS directory handle.

use std::{
    fs::File,
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle},
    ptr::{null, null_mut},
};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_NON_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
            NtCreateFile,
        },
    },
    Win32::{
        Foundation::{
            OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, RtlNtStatusToDosError,
            STATUS_FILE_IS_A_DIRECTORY, STATUS_IO_REPARSE_TAG_NOT_HANDLED,
            STATUS_REPARSE_POINT_ENCOUNTERED, STATUS_STOPPED_ON_SYMLINK, UNICODE_STRING,
        },
        Security::SECURITY_ATTRIBUTES,
        Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, SYNCHRONIZE},
        System::IO::IO_STATUS_BLOCK,
    },
};

use crate::{COMPANION, VAULT};

pub(crate) fn open_child(
    parent: &File,
    name: &str,
    access: u32,
    sharing: u32,
    disposition: u32,
    security: Option<&SECURITY_ATTRIBUTES>,
) -> io::Result<File> {
    let stage = name.strip_prefix(".codeward.stage-");
    let stage = stage.is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    });
    if name != VAULT && name != COMPANION && !stage {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    // All accepted names are fixed-length ASCII and contain no separator,
    // stream, device syntax, dot component or trailing dot/space.
    let mut units: Vec<u16> = name.encode_utf16().collect();
    let length = (units.len() * size_of::<u16>()) as u16;
    let unicode = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: units.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &unicode,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        SecurityDescriptor: security.map_or(null(), |value| value.lpSecurityDescriptor.cast()),
        SecurityQualityOfService: null(),
    };
    let mut handle = null_mut();
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: parent is live; unicode references a bounded initialized name.
    // The optional descriptor is owned by the caller for the call's duration.
    // Outputs are writable, and synchronous I/O prevents a pending request
    // from retaining stack/name buffers beyond this call.
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            access | SYNCHRONIZE,
            &attributes,
            &mut status,
            null(),
            FILE_ATTRIBUTE_NORMAL,
            sharing,
            disposition,
            FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            null(),
            0,
        )
    };
    if result < 0 {
        if matches!(
            result,
            STATUS_FILE_IS_A_DIRECTORY
                | STATUS_REPARSE_POINT_ENCOUNTERED
                | STATUS_IO_REPARSE_TAG_NOT_HANDLED
                | STATUS_STOPPED_ON_SYMLINK
        ) {
            return Err(io::ErrorKind::Unsupported.into());
        }
        // SAFETY: converting the returned native status has no pointer inputs.
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ));
    }
    // SAFETY: successful NtCreateFile returned a new owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}
