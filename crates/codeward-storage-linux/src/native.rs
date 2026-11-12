use std::{
    ffi::CStr,
    fs::File,
    io::{self, BufRead, BufReader},
    mem::{MaybeUninit, size_of_val},
    os::fd::{AsRawFd, FromRawFd},
};

use crate::NativeError;

pub(super) fn open_at(
    parent: &File,
    name: &CStr,
    flags: libc::c_int,
    stay_on_mount: bool,
) -> io::Result<File> {
    // SAFETY: open_how contains only unsigned integer fields.
    let mut how: libc::open_how = unsafe { std::mem::zeroed() };
    how.flags = (flags | libc::O_CLOEXEC) as u64;
    how.resolve = libc::RESOLVE_BENEATH | libc::RESOLVE_NO_SYMLINKS;
    if stay_on_mount {
        how.resolve |= libc::RESOLVE_NO_XDEV;
    }
    if flags & libc::O_CREAT != 0 {
        how.mode = 0o600;
    }
    // SAFETY: The directory is live; name is NUL-terminated and how is initialized.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            parent.as_raw_fd(),
            name.as_ptr(),
            &raw const how,
            size_of_val(&how),
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: A successful openat2 returned a new, uniquely owned file descriptor.
    Ok(unsafe { File::from_raw_fd(fd as libc::c_int) })
}

pub(super) fn refuse_acl(file: &File, name: &CStr) -> Result<(), NativeError> {
    // SAFETY: The file and name are live. A null output with length zero queries size.
    let result =
        unsafe { libc::fgetxattr(file.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0) };
    if result >= 0 {
        return Err(NativeError::Permissions);
    }
    match io::Error::last_os_error().raw_os_error() {
        // ENOTSUP is also valid on ancestors whose filesystem has no xattr support.
        Some(libc::ENODATA | libc::ENOTSUP) => Ok(()),
        _ => Err(NativeError::Io),
    }
}

pub(super) fn mount_id(file: &File) -> Result<u64, NativeError> {
    let mut stat = MaybeUninit::<libc::statx>::zeroed();
    // SAFETY: AT_EMPTY_PATH queries the live fd. stat points to writable storage.
    let result = unsafe {
        libc::statx(
            file.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH | libc::AT_STATX_DONT_SYNC,
            libc::STATX_MNT_ID,
            stat.as_mut_ptr(),
        )
    };
    if result != 0 {
        return Err(NativeError::Io);
    }
    // SAFETY: statx initialized the result on success.
    let stat = unsafe { stat.assume_init() };
    if stat.stx_mask & libc::STATX_MNT_ID == 0 {
        return Err(NativeError::Unsupported);
    }
    Ok(stat.stx_mnt_id)
}

pub(super) fn verify_mount(id: u64) -> Result<(), NativeError> {
    let mounts = File::open("/proc/self/mountinfo").map_err(|_| NativeError::Io)?;
    for line in BufReader::new(mounts).split(b'\n') {
        let line = line.map_err(|_| NativeError::Io)?;
        if let Some(accepted) = mount_allowed(&line, id) {
            return if accepted {
                Ok(())
            } else {
                Err(NativeError::Unsupported)
            };
        }
    }
    Err(NativeError::Unsupported)
}

pub(super) fn mount_allowed(line: &[u8], id: u64) -> Option<bool> {
    let mut fields = line.split(|&byte| byte == b' ');
    let mount_id: u64 = std::str::from_utf8(fields.next()?).ok()?.parse().ok()?;
    if id != mount_id {
        return None;
    }
    let options = fields.nth(4)?;
    let mut options = options.split(|&byte| byte == b',');
    // nodev also protects the checked open if a special entry races the preflight.
    let writable = options.clone().any(|option| option == b"rw");
    let nodev = options.any(|option| option == b"nodev");
    fields.find(|&field| field == b"-")?;
    let ext4 = fields.next()? == b"ext4";
    fields.next()?; // Source
    let super_options = fields.next()?;
    let super_writable = super_options
        .split(|&byte| byte == b',')
        .any(|option| option == b"rw");
    Some(writable && nodev && ext4 && super_writable)
}
