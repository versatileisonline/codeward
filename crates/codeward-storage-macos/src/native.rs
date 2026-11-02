//! Small descriptor-relative system-call boundary. No borrowed raw fd escapes.

use std::{
    ffi::CStr,
    fs::File,
    io,
    mem::MaybeUninit,
    os::fd::{AsRawFd, FromRawFd},
};

// macOS 26.3.1 SDK sys/mount.h; libc 0.2.191 does not bind MNT_REMOVABLE.
const MNT_REMOVABLE: u32 = 0x0000_0200;

pub(super) fn open_at(parent: &File, name: &CStr, flags: i32, mode: u16) -> io::Result<File> {
    loop {
        // SAFETY: parent owns a live directory descriptor for this call, name is
        // NUL-terminated, and openat's variadic mode has C's promoted int type.
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                libc::c_int::from(mode),
            )
        };
        if fd >= 0 {
            // SAFETY: successful openat returned a fresh owned fd. File takes
            // sole ownership; it is never separately closed or exposed to callers.
            return Ok(unsafe { File::from_raw_fd(fd) });
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

pub(super) fn named_stat(parent: &File, name: &CStr) -> io::Result<libc::stat> {
    let mut output = MaybeUninit::uninit();
    // SAFETY: valid owned directory fd/name and writable correctly aligned stat
    // storage. AT_SYMLINK_NOFOLLOW examines the entry itself, not its target.
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            output.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstatat initializes the complete output structure.
    Ok(unsafe { output.assume_init() })
}

pub(super) fn local_writable_apfs(file: &File) -> io::Result<bool> {
    let mut output = MaybeUninit::uninit();
    // SAFETY: valid owned fd and writable correctly aligned statfs storage.
    if unsafe { libc::fstatfs(file.as_raw_fd(), output.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstatfs initialized the output. Use a bounded array
    // comparison rather than assuming its filesystem-name buffer terminates.
    let output = unsafe { output.assume_init() };
    let name = output.f_fstypename.map(|byte| byte as u8);
    Ok(filesystem_allowed(output.f_flags, &name))
}

fn filesystem_allowed(flags: u32, name: &[u8]) -> bool {
    flags & libc::MNT_LOCAL as u32 != 0
        && flags & (libc::MNT_RDONLY as u32 | MNT_REMOVABLE) == 0
        && name.starts_with(b"apfs\0")
}

pub(super) fn effective_uid() -> u32 {
    // SAFETY: geteuid takes no pointers and cannot invalidate process memory.
    unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filesystem_filter_refuses_network_readonly_removable_and_other_formats() {
        let local = libc::MNT_LOCAL as u32;
        assert!(filesystem_allowed(local, b"apfs\0"));
        for flags in [0, local | libc::MNT_RDONLY as u32, local | MNT_REMOVABLE] {
            assert!(!filesystem_allowed(flags, b"apfs\0"));
        }
        for name in [b"hfs\0".as_slice(), b"apfs-extra\0", b"apfs"] {
            assert!(!filesystem_allowed(local, name));
        }
    }
}
