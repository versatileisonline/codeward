//! Small descriptor-relative system-call boundary. No borrowed raw fd escapes.

use std::{
    ffi::CStr,
    fs::File,
    io,
    mem::MaybeUninit,
    os::fd::{AsRawFd, FromRawFd, IntoRawFd},
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

// macOS SDK sys/acl.h. libc 0.2.191 does not expose these Darwin ACL APIs.
unsafe extern "C" {
    fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
    fn acl_get_entry(
        acl: *mut libc::c_void,
        index: libc::c_int,
        entry: *mut *mut libc::c_void,
    ) -> libc::c_int;
    fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
}

pub(super) fn no_extended_acl(file: &File) -> io::Result<bool> {
    // SAFETY: live owned descriptor, SDK ACL_TYPE_EXTENDED = 0x100.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), 0x100) };
    if acl.is_null() {
        let error = io::Error::last_os_error();
        // Darwin's descriptor-based acl_get_fd_np reads FILESEC_ACL after
        // fstatx_np. ENOENT denotes an absent ACL property, not a missing path.
        // File owns this live fd for the whole call; other query failures fail.
        return if error.raw_os_error() == Some(libc::ENOENT) {
            Ok(true)
        } else {
            Err(error)
        };
    }
    let mut entry = std::ptr::null_mut();
    // SAFETY: successful retrieval returns a valid ACL; entry is writable.
    // Darwin returns 0 for an entry and -1/EINVAL for an empty valid ACL.
    let result = unsafe { acl_get_entry(acl, 0, &mut entry) };
    let error = io::Error::last_os_error();
    // SAFETY: this allocation is owned once and no entry is used after release.
    let freed = unsafe { acl_free(acl) };
    if freed != 0 {
        return Err(io::Error::last_os_error());
    }
    match result {
        0 => Ok(false),
        -1 if error.raw_os_error() == Some(libc::EINVAL) => Ok(true),
        _ => Err(error),
    }
}

pub(super) fn stage_random() -> io::Result<[u8; 16]> {
    let mut bytes = [0; 16];
    // SAFETY: writable initialized buffer, smaller than getentropy's 256-byte cap.
    if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(bytes)
}

pub(super) fn sync_stage(file: &File) -> io::Result<()> {
    file.sync_all()?;
    full_sync(file)
}

pub(super) fn fsync_once(file: &File) -> io::Result<()> {
    // SAFETY: live owned descriptor; fsync takes no pointers or ownership.
    // Report EINTR as an unsuccessful observation rather than retrying it.
    if unsafe { libc::fsync(file.as_raw_fd()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn full_sync(file: &File) -> io::Result<()> {
    // SAFETY: live owned descriptor; F_FULLFSYNC takes no pointer argument.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn close_checked(file: File) -> io::Result<()> {
    // Consume File first: neither its destructor nor a retry may close an fd
    // potentially reassigned after an error, including EINTR.
    let fd = file.into_raw_fd();
    // SAFETY: ownership of this descriptor was transferred exactly once above.
    if unsafe { libc::close(fd) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn publish_at(
    parent: &File,
    stage: &CStr,
    destination: &CStr,
    create: bool,
) -> io::Result<()> {
    let flags = if create { libc::RENAME_EXCL } else { 0 };
    // SAFETY: parent owns a live directory descriptor for both operands; both
    // names are NUL-terminated single components. No descriptor ownership moves.
    // RENAME_EXCL atomically refuses existing destinations. Never retry a rename
    // (even EINTR), delete first, or fall back to copy-and-delete.
    if unsafe {
        libc::renameatx_np(
            parent.as_raw_fd(),
            stage.as_ptr(),
            parent.as_raw_fd(),
            destination.as_ptr(),
            flags,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
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
