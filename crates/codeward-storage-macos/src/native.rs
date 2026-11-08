//! Small descriptor-relative system-call boundary. No borrowed raw fd escapes.

use std::{
    ffi::CStr,
    ffi::OsStr,
    fs::File,
    io,
    mem::MaybeUninit,
    os::fd::{AsRawFd, FromRawFd, IntoRawFd},
    os::unix::ffi::OsStrExt,
    path::PathBuf,
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
    #[cfg(test)]
    if let Some(result) = crate::test_support::mount_result() {
        return result;
    }
    let output = filesystem_stats(file)?;
    let name = output.f_fstypename.map(|byte| byte as u8);
    let flags = output.f_flags;
    // Disk images are removable. The opt-in capacity test exempts only its
    // own device, leaving the kernel I/O and all other checks active.
    #[cfg(test)]
    let flags = {
        use std::os::unix::fs::MetadataExt;
        match crate::test_support::capacity_image_device() {
            Some(device) if file.metadata()?.dev() == device => flags & !MNT_REMOVABLE,
            _ => flags,
        }
    };
    Ok(filesystem_allowed(flags, &name))
}

pub(super) fn filesystem_stats(file: &File) -> io::Result<libc::statfs> {
    let mut output = MaybeUninit::uninit();
    // SAFETY: valid owned fd and writable correctly aligned statfs storage.
    if unsafe { libc::fstatfs(file.as_raw_fd(), output.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstatfs initialized the output structure.
    Ok(unsafe { output.assume_init() })
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

pub(super) fn account_home() -> io::Result<PathBuf> {
    // A fixed bound avoids unbounded account-service ERANGE retries. The call
    // itself may block in the OS account service; this is not a latency bound.
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(64 * 1024)
        .map_err(|_| io::ErrorKind::OutOfMemory)?;
    buffer.resize(64 * 1024, 0u8);
    let uid = effective_uid();
    let mut record = MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    // SAFETY: live writable record, byte buffer and result pointer with the
    // declared lengths. getpwuid_r retains no ownership of caller storage.
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 {
        // getpwuid_r returns its error number directly, not through errno.
        return Err(io::Error::from_raw_os_error(status));
    }
    if result.is_null() {
        return Err(io::ErrorKind::NotFound.into());
    }
    if result != record.as_mut_ptr() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    // SAFETY: a successful query returned this supplied record as its result.
    let record = unsafe { record.assume_init() };
    account_home_bytes(&buffer, uid, record.pw_uid, record.pw_dir)
        .map(|bytes| PathBuf::from(OsStr::from_bytes(bytes)))
}

fn account_home_bytes(
    buffer: &[u8],
    expected_uid: u32,
    uid: u32,
    home: *const libc::c_char,
) -> io::Result<&[u8]> {
    if expected_uid != uid {
        return Err(io::ErrorKind::InvalidData.into());
    }
    // Validate the returned pointer against caller-owned storage before reading
    // it. No CStr walk can escape the fixed buffer on a malformed OS result.
    let offset = (home as usize)
        .checked_sub(buffer.as_ptr() as usize)
        .filter(|offset| *offset < buffer.len())
        .ok_or(io::ErrorKind::InvalidData)?;
    let tail = &buffer[offset..];
    let length = tail
        .iter()
        .position(|b| *b == 0)
        .ok_or(io::ErrorKind::InvalidData)?;
    if length == 0 || length > libc::PATH_MAX as usize || tail[0] != b'/' {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(&tail[..length])
}

pub(super) fn mkdir_at(parent: &File, name: &CStr) -> io::Result<()> {
    // SAFETY: retained live directory descriptor and a NUL-terminated single
    // component. One call only: errors preserve any possibly-created directory.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
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
    fn acl_get_tag_type(entry: *mut libc::c_void, tag: *mut libc::c_int) -> libc::c_int;
}

pub(super) fn no_extended_acl(file: &File) -> io::Result<bool> {
    restrictive_acl(file, false)
}

pub(super) fn no_acl_grants(file: &File) -> io::Result<bool> {
    restrictive_acl(file, true)
}

fn restrictive_acl(file: &File, allow_denies: bool) -> io::Result<bool> {
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
    let checked = (|| {
        // SDK ACL_MAX_ENTRIES=128. One final probe recognizes a full ACL's end.
        for index in 0..=128 {
            let mut entry = std::ptr::null_mut();
            // SAFETY: the copied ACL stays live; entry is writable. Darwin uses
            // 0/FIRST then -1/NEXT, returning -1/EINVAL at the end of a valid ACL.
            let result = unsafe { acl_get_entry(acl, if index == 0 { 0 } else { -1 }, &mut entry) };
            if result == -1 {
                let error = io::Error::last_os_error();
                return if error.raw_os_error() == Some(libc::EINVAL) {
                    Ok(true)
                } else {
                    Err(error)
                };
            }
            if result != 0 || index == 128 {
                return Err(io::ErrorKind::InvalidData.into());
            }
            if !allow_denies {
                return Ok(false);
            }
            let mut tag = 0;
            // SAFETY: retrieval succeeded and the ACL/entry remain live; tag is
            // writable. SDK acl_tag_t is a C enum; EXTENDED_DENY=2.
            if unsafe { acl_get_tag_type(entry, &mut tag) } != 0 {
                return Err(io::Error::last_os_error());
            }
            if tag != 2 {
                return Ok(false);
            }
        }
        Err(io::ErrorKind::InvalidData.into())
    })();
    // SAFETY: this allocation is owned once and no entry is used after release.
    let freed = unsafe { acl_free(acl) };
    if freed != 0 {
        return Err(io::Error::last_os_error());
    }
    checked
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
    use crate::test_support::{MountFault, MountProbe, TestDir, chmod_acl};

    #[test]
    fn account_home_results_are_bounded_account_bound_and_preserve_os_bytes() {
        let bytes = b"prefix/home/\xff\0unused";
        assert_eq!(
            account_home_bytes(bytes, 7, 7, bytes.as_ptr().wrapping_add(6).cast()).unwrap(),
            b"/home/\xff"
        );
        assert!(account_home_bytes(bytes, 7, 8, bytes.as_ptr().wrapping_add(6).cast()).is_err());
        for value in [b"\0".as_slice(), b"relative\0", b"/unterminated"] {
            assert!(account_home_bytes(value, 7, 7, value.as_ptr().cast()).is_err());
        }
        for pointer in [
            std::ptr::null(),
            bytes.as_ptr().wrapping_sub(1).cast(),
            bytes.as_ptr().wrapping_add(bytes.len()).cast(),
        ] {
            assert!(account_home_bytes(bytes, 7, 7, pointer).is_err());
        }
        let mut over = vec![b'x'; libc::PATH_MAX as usize + 2];
        over[0] = b'/';
        *over.last_mut().unwrap() = 0;
        assert!(account_home_bytes(&over, 7, 7, over.as_ptr().cast()).is_err());
    }

    #[test]
    fn current_account_home_lookup_is_read_only_and_absolute() {
        let home = account_home().unwrap();
        assert!(home.is_absolute());
        assert!(home.as_os_str().as_bytes().len() <= libc::PATH_MAX as usize);
    }

    #[test]
    fn deny_only_ancestor_acls_do_not_relax_private_directory_policy() {
        let dir = TestDir::new();
        let file = File::open(&dir.0).unwrap();
        assert!(no_acl_grants(&file).unwrap());
        assert!(no_extended_acl(&file).unwrap());
        chmod_acl(&dir.0, "everyone deny delete");
        assert!(no_acl_grants(&file).unwrap());
        assert!(!no_extended_acl(&file).unwrap());
        chmod_acl(&dir.0, "everyone deny writeextattr");
        assert!(no_acl_grants(&file).unwrap());
        chmod_acl(&dir.0, "everyone allow read");
        assert!(!no_acl_grants(&file).unwrap());
        assert!(!no_extended_acl(&file).unwrap());
        // Clear only this synthetic fixture's ACL so its owner can remove it.
        assert!(
            std::process::Command::new("/bin/chmod")
                .arg("-N")
                .arg(&dir.0)
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn managed_mkdir_is_exclusive_and_preserves_existing_entries() {
        let dir = TestDir::new();
        let parent = File::open(&dir.0).unwrap();
        std::fs::write(dir.0.join("occupied"), b"synthetic untouched").unwrap();
        assert_eq!(
            mkdir_at(&parent, c"occupied").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            std::fs::read(dir.0.join("occupied")).unwrap(),
            b"synthetic untouched"
        );
        mkdir_at(&parent, c"fresh").unwrap();
        let child = open_at(&parent, c"fresh", libc::O_RDONLY | libc::O_DIRECTORY, 0).unwrap();
        assert!(child.metadata().unwrap().is_dir());
        assert_eq!(
            mkdir_at(&parent, c"fresh").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
    }

    #[test]
    fn mount_probe_is_scoped_and_isolated_to_the_current_test_thread() {
        let dir = TestDir::new();
        let parent = File::open(&dir.0).unwrap();
        assert!(local_writable_apfs(&parent).unwrap());
        {
            let probe = MountProbe::new();
            probe.set(Some(MountFault::Refused));
            assert!(!local_writable_apfs(&parent).unwrap());
            std::thread::scope(|scope| {
                assert!(
                    scope
                        .spawn(|| local_writable_apfs(&parent).unwrap())
                        .join()
                        .unwrap()
                );
            });
            {
                let nested = MountProbe::new();
                assert!(local_writable_apfs(&parent).unwrap());
                nested.set(Some(MountFault::QueryFailed));
                assert_eq!(
                    local_writable_apfs(&parent).unwrap_err().raw_os_error(),
                    Some(libc::EIO)
                );
            }
            assert!(!local_writable_apfs(&parent).unwrap());
        }
        assert!(local_writable_apfs(&parent).unwrap());
    }

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
