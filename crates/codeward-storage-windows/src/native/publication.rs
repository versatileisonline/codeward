use std::{
    fs::File,
    io,
    mem::{offset_of, size_of},
    os::windows::io::AsRawHandle,
    ptr::{addr_of_mut, copy_nonoverlapping},
};
use windows_sys::{
    Wdk::Storage::FileSystem::{
        FILE_RENAME_INFORMATION, FILE_RENAME_INFORMATION_0, FILE_RENAME_REPLACE_IF_EXISTS,
        FileRenameInformationEx, NtSetInformationFile,
    },
    Win32::{Foundation::RtlNtStatusToDosError, System::IO::IO_STATUS_BLOCK},
};

use crate::VAULT;

pub(crate) fn rename(source: &File, parent: &File, replace: bool) -> io::Result<()> {
    // The only destination is the fixed ASCII vault name plus a trailing NUL.
    const _: () = assert!(VAULT.len() == 8);
    const _: () = assert!(size_of::<FILE_RENAME_INFORMATION>() + 16 <= size_of::<[usize; 8]>());
    const _: () =
        assert!(offset_of!(FILE_RENAME_INFORMATION, FileName) + 18 <= size_of::<[usize; 8]>());
    let mut name = [0_u16; 9];
    for (slot, unit) in name.iter_mut().zip(VAULT.encode_utf16()) {
        *slot = unit;
    }
    // usize alignment meets the generated structure's pointer alignment. The
    // flexible name tail is contiguous, including across header tail padding.
    let mut buffer = [0_usize; 8];
    let pointer = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    let information = FILE_RENAME_INFORMATION {
        Anonymous: FILE_RENAME_INFORMATION_0 {
            Flags: if replace {
                FILE_RENAME_REPLACE_IF_EXISTS
            } else {
                0
            },
        },
        RootDirectory: parent.as_raw_handle(),
        FileNameLength: 16,
        FileName: [0],
    };
    let mut completion = IO_STATUS_BLOCK::default();
    // SAFETY: the stack buffer is aligned and the compile-time bounds cover
    // both the generated header and nine UTF-16 name units. Both handles are
    // live; source has DELETE access. This synchronous call retains no pointers.
    let result = unsafe {
        pointer.write(information);
        copy_nonoverlapping(
            name.as_ptr(),
            addr_of_mut!((*pointer).FileName).cast(),
            name.len(),
        );
        NtSetInformationFile(
            source.as_raw_handle(),
            &mut completion,
            pointer.cast(),
            (size_of::<FILE_RENAME_INFORMATION>() + 16) as u32,
            FileRenameInformationEx,
        )
    };
    if result != 0 {
        // SAFETY: converting the returned NTSTATUS has no pointer arguments.
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ));
    }
    Ok(())
}
