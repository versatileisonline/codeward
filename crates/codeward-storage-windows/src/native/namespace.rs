use std::{fs::File, io, os::windows::io::AsRawHandle, ptr::null};
use windows_sys::{
    Wdk::Storage::FileSystem::NtFlushBuffersFileEx,
    Win32::{Foundation::RtlNtStatusToDosError, System::IO::IO_STATUS_BLOCK},
};

pub(crate) fn flush(file: &File) -> io::Result<()> {
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: file owns a synchronous disk handle, status is writable, and the
    // documented normal flush uses flags zero and no parameter buffer.
    let result = unsafe { NtFlushBuffersFileEx(file.as_raw_handle(), 0, null(), 0, &mut status) };
    if result != 0 {
        // SAFETY: converting a returned NTSTATUS has no pointer inputs.
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ));
    }
    Ok(())
}
