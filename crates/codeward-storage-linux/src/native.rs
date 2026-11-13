use std::{
    ffi::CStr,
    fs::File,
    io::{self, BufRead, BufReader},
    mem::{MaybeUninit, size_of_val},
    os::fd::{AsRawFd, FromRawFd, IntoRawFd},
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

pub(super) fn create_private_directory(parent: &File, name: &CStr) -> io::Result<()> {
    // SAFETY: The directory descriptor is live and name is a NUL-terminated component.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) fn account_home(owner: u32) -> Result<std::path::PathBuf, NativeError> {
    use std::os::unix::ffi::OsStrExt;

    const MAX_BUFFER: usize = 1024 * 1024;
    let mut buffer = vec![0; 4096];
    let mut account = MaybeUninit::<libc::passwd>::uninit();
    loop {
        let mut result = std::ptr::null_mut();
        // SAFETY: account and result are writable, and buffer is initialized and
        // live for the specified length and for the subsequent home lookup.
        let status = unsafe {
            libc::getpwuid_r(
                owner,
                account.as_mut_ptr(),
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            )
        };
        if status == libc::ERANGE && buffer.len() < MAX_BUFFER {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if status != 0 || result.is_null() {
            return Err(NativeError::Io);
        }
        // SAFETY: Successful getpwuid_r populated the account and its string
        // pointers refer to NUL-terminated entries in the still-live buffer.
        let account = unsafe { account.assume_init() };
        if account.pw_uid != owner || account.pw_dir.is_null() {
            return Err(NativeError::InvalidPath);
        }
        // SAFETY: pw_dir is nonnull and references the live getpwuid_r buffer.
        let path = unsafe { CStr::from_ptr(account.pw_dir) };
        return Ok(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
            path.to_bytes(),
        )));
    }
}

pub(super) fn stage_random() -> io::Result<[u8; 16]> {
    let mut bytes = [0; 16];
    // SAFETY: bytes is writable for the supplied length. With flags zero, Linux
    // guarantees a complete read up to 256 bytes once the random pool is ready.
    let count = unsafe { libc::getrandom(bytes.as_mut_ptr().cast(), bytes.len(), 0) };
    if count < 0 {
        return Err(io::Error::last_os_error());
    }
    if count as usize != bytes.len() {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(bytes)
}

pub(super) fn close_checked(file: File) -> io::Result<()> {
    let fd = file.into_raw_fd();
    // SAFETY: File transferred ownership. Never retry close on Linux, where
    // the descriptor may have already been released even when close fails.
    if unsafe { libc::close(fd) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
