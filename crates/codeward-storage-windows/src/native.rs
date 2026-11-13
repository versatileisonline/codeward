use std::{
    ffi::OsString,
    fs::File,
    mem::{MaybeUninit, size_of},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::AsRawHandle,
    },
    path::{Path, PathBuf},
    ptr::null_mut,
};

use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FILE_TYPE_DISK, FileBasicInfo, GetDriveTypeW,
    GetFileInformationByHandle, GetFileInformationByHandleEx, GetFileType,
    GetFinalPathNameByHandleW, GetVolumeInformationByHandleW, VOLUME_NAME_GUID,
};

use windows_sys::Win32::System::SystemServices::FILE_READ_ONLY_VOLUME;

use crate::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, NativeError};

pub(super) fn require_fixed_drive(path: &Path) -> Result<(), NativeError> {
    const DRIVE_FIXED: u32 = 3;
    let root: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: root contains a validated drive root followed by a NUL.
    if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
        return Err(NativeError::Unsupported);
    }
    Ok(())
}

pub(super) fn require_ntfs(root: &File) -> Result<(), NativeError> {
    let mut name = [0; 16];
    let mut flags = 0;
    // SAFETY: root owns the handle. Output buffers are writable at their declared
    // sizes. Unused outputs can be null.
    let result = unsafe {
        GetVolumeInformationByHandleW(
            root.as_raw_handle(),
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            &mut flags,
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if result == 0 {
        return Err(NativeError::Open);
    }
    if name[..5]
        .iter()
        .copied()
        .ne("NTFS".encode_utf16().chain([0]))
        || flags & FILE_READ_ONLY_VOLUME != 0
    {
        return Err(NativeError::Unsupported);
    }
    Ok(())
}

pub(super) fn final_path(file: &File) -> Result<PathBuf, NativeError> {
    let mut name = [0; 512];
    // SAFETY: file owns the handle and name is writable for its declared length.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            name.as_mut_ptr(),
            name.len() as u32,
            VOLUME_NAME_GUID,
        )
    } as usize;
    if length == 0 || length >= name.len() {
        return Err(NativeError::Unsupported);
    }
    Ok(PathBuf::from(OsString::from_wide(&name[..length])))
}

pub(super) fn require_directory(file: &File) -> Result<(), NativeError> {
    let attrs = file_info(file)?.dwFileAttributes;
    if attrs & FILE_ATTRIBUTE_DIRECTORY == 0 || attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(NativeError::Unsupported);
    }
    Ok(())
}

#[derive(PartialEq, Eq)]
pub(super) struct Observation {
    pub(super) len: u64,
    write_time: i64,
    change_time: i64,
}

pub(super) fn regular_info(file: &File) -> Result<Observation, NativeError> {
    let info = file_info(file)?;
    if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
        || info.nNumberOfLinks != 1
    {
        return Err(NativeError::Unsupported);
    }
    let mut basic = MaybeUninit::<FILE_BASIC_INFO>::uninit();
    // SAFETY: file owns the live handle. The buffer size and type match FileBasicInfo.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            basic.as_mut_ptr().cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return Err(NativeError::Read);
    }
    // SAFETY: the native query succeeded.
    let basic = unsafe { basic.assume_init() };
    Ok(Observation {
        len: (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow),
        write_time: basic.LastWriteTime,
        change_time: basic.ChangeTime,
    })
}

fn file_info(file: &File) -> Result<BY_HANDLE_FILE_INFORMATION, NativeError> {
    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: file owns the handle and info points to the correct writable type.
    unsafe {
        if GetFileType(file.as_raw_handle()) != FILE_TYPE_DISK {
            return Err(NativeError::Unsupported);
        }
        if GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) == 0 {
            return Err(NativeError::Open);
        }
        Ok(info.assume_init())
    }
}
