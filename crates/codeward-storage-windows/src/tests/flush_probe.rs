//! Explicit capability diagnostic; no successful flush acknowledges a save.

use super::*;
use std::{
    fs::File,
    marker::PhantomData,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr::{null, null_mut},
    rc::Rc,
};
use windows_sys::{
    Wdk::Storage::FileSystem::{
        FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
        NtFlushBuffersFile, NtFlushBuffersFileEx,
    },
    Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE},
        Security::{
            CheckTokenMembership, CreateRestrictedToken, CreateWellKnownSid, DISABLE_MAX_PRIVILEGE,
            GetTokenInformation, ImpersonateLoggedOnUser, LUA_TOKEN, RevertToSelf,
            SECURITY_MAX_SID_SIZE, TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
            WinBuiltinAdministratorsSid,
        },
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_READ_ATTRIBUTES,
            FILE_TRAVERSE, FILE_WRITE_ATTRIBUTES, FlushFileBuffers, GetFileInformationByHandle,
            READ_CONTROL, SYNCHRONIZE,
        },
        System::{
            IO::IO_STATUS_BLOCK,
            Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken},
        },
    },
};

#[test]
#[ignore = "explicit NTFS capability diagnostic; CI runs with --nocapture"]
fn ntfs_flush_capability_probe() {
    let version = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "ver"])
        .output()
        .unwrap();
    assert!(version.status.success());
    eprintln!(
        "flush-probe os={} filesystem=NTFS (validated by lease)",
        String::from_utf8_lossy(&version.stdout).trim()
    );
    let _restricted = RestrictedContext::enter();
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    for (phase, expected, base) in [
        ("create", &b"synthetic first candidate"[..], None),
        (
            "replace",
            &b"synthetic second candidate"[..],
            Some(&b"synthetic first candidate"[..]),
        ),
    ] {
        let stage = lease.prepare_stage(expected).unwrap();
        match base {
            None => stage.publish_create().unwrap(),
            Some(base) => stage.publish_replace(base).unwrap(),
        }
        let residue = lease.prepare_stage(b"synthetic abandoned stage").unwrap();
        drop(residue);
        let before = snapshot(&fixture.0);
        assert_eq!(lease.read_current().unwrap().as_deref(), Some(expected));
        probe_files(&lease, phase);
        probe_directories(&lease, phase);
        assert_eq!(lease.read_current().unwrap().as_deref(), Some(expected));
        assert_eq!(snapshot(&fixture.0), before);
        assert_eq!(
            ReadLease::acquire_synthetic(&fixture.0).err(),
            Some(NativeError::WriterUnavailable)
        );
    }
    drop(lease);
    let reopened = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    assert_eq!(
        reopened.read_current().unwrap().unwrap(),
        b"synthetic second candidate"
    );
}

fn probe_files(lease: &ReadLease, phase: &str) {
    let parent = &lease.inner.directories.last().unwrap().1;
    for (label, access) in [
        ("file-read", GENERIC_READ),
        ("file-write-control", GENERIC_READ | GENERIC_WRITE),
    ] {
        let access = access | SYNCHRONIZE;
        let file =
            native::open_child(parent, VAULT, access, FILE_SHARE_READ, FILE_OPEN, None).unwrap();
        let result = flushes(&file, phase, label, access);
        native::close_checked(file).unwrap();
        if label == "file-write-control" {
            assert_eq!(result, (0, 0, 0), "writable control failed");
        }
    }
}

fn probe_directories(lease: &ReadLease, phase: &str) {
    let (path, parent) = lease.inner.directories.last().unwrap();
    let identity = directory_identity(parent);
    const RETAINED: u32 = FILE_TRAVERSE | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE;
    flushes(parent, phase, "directory-retained", RETAINED);
    // Directory-specific rights share numeric bits with file write/append rights.
    // These are hypotheses to measure, never changes to runtime admission.
    for (label, extra) in [
        ("directory-write-attributes", FILE_WRITE_ATTRIBUTES),
        ("directory-add-file", FILE_ADD_FILE),
        ("directory-add-subdirectory", FILE_ADD_SUBDIRECTORY),
    ] {
        let access = RETAINED | extra;
        let opened = OpenOptions::new()
            .access_mode(access)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path);
        match opened {
            Ok(file) => {
                native::require_directory(&file).unwrap();
                assert_eq!(native::final_path(&file).unwrap(), *path);
                assert_eq!(directory_identity(&file), identity);
                flushes(&file, phase, label, access);
                assert_eq!(directory_identity(&file), identity);
                native::close_checked(file).unwrap();
            }
            Err(error) => eprintln!(
                "flush-probe phase={phase} object={label} access={access:#010x} share=0x00000003 flags=0x02200000 open-win32={}",
                error.raw_os_error().unwrap_or(-1)
            ),
        }
        lease.verify().unwrap();
        assert_eq!(directory_identity(parent), identity);
    }
}

fn flushes(file: &File, phase: &str, label: &str, access: u32) -> (u32, u32, u32) {
    // SAFETY: file owns a live synchronous file/directory handle. Each NT call
    // is synchronous and its IO_STATUS_BLOCK remains writable for the call.
    // NtEx uses the documented normal flush: flags 0, no parameter buffer.
    let win32 = unsafe { FlushFileBuffers(file.as_raw_handle()) };
    let win32 = if win32 == 0 {
        io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32
    } else {
        0
    };
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: same live synchronous handle and writable output described above.
    let nt = unsafe { NtFlushBuffersFile(file.as_raw_handle(), &mut status) } as u32;
    // SAFETY: flags/parameters and output meet the normal synchronous contract.
    let nt_ex =
        unsafe { NtFlushBuffersFileEx(file.as_raw_handle(), 0, null(), 0, &mut status) } as u32;
    let directory = label.starts_with("directory-");
    let share = if directory { 3 } else { 1 };
    let (options_kind, flags) = if directory {
        (
            "win32-flags",
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
    } else {
        (
            "nt-create-options",
            FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
        )
    };
    eprintln!(
        "flush-probe phase={phase} object={label} access={access:#010x} share={share:#010x} {options_kind}={flags:#010x} win32={win32} nt={nt:#010x} nt-ex-zero={nt_ex:#010x}"
    );
    (win32, nt, nt_ex)
}

fn directory_identity(file: &File) -> (u32, u32, u32) {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: file is live and info is the correct writable output structure.
    win32_ok(unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) });
    (
        info.dwVolumeSerialNumber,
        info.nFileIndexHigh,
        info.nFileIndexLow,
    )
}

fn snapshot(path: &Path) -> Vec<(OsString, Vec<u8>)> {
    let mut files: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            // The companion is deliberately opened with no sharing; lease
            // verification checks its identity/empty bytes through its handle.
            let bytes = if entry.file_name() == COMPANION {
                Vec::new()
            } else {
                fs::read(entry.path()).unwrap()
            };
            (entry.file_name(), bytes)
        })
        .collect();
    files.sort();
    files
}

// Thread impersonation must end on the same thread, even when assertions unwind.
pub(crate) struct RestrictedContext {
    _token: OwnedHandle,
    _thread: PhantomData<Rc<()>>,
}

impl RestrictedContext {
    pub(crate) fn enter() -> Self {
        let mut original = null_mut();
        // SAFETY: pseudo-handle is valid; original is writable for the new handle.
        win32_ok(unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY | TOKEN_DUPLICATE,
                &mut original,
            )
        });
        // SAFETY: the successful call returned sole ownership.
        let original = unsafe { OwnedHandle::from_raw_handle(original) };
        let original_elevation = elevation(original.as_raw_handle());
        let mut restricted = null_mut();
        // SAFETY: original is live with duplicate access, unused counted arrays
        // are null with zero length, and the output pointer is writable.
        win32_ok(unsafe {
            CreateRestrictedToken(
                original.as_raw_handle(),
                DISABLE_MAX_PRIVILEGE | LUA_TOKEN,
                0,
                null(),
                0,
                null(),
                0,
                null(),
                &mut restricted,
            )
        });
        // SAFETY: successful creation returned a new owned primary token.
        let restricted = unsafe { OwnedHandle::from_raw_handle(restricted) };
        // SAFETY: live same-user primary token has query/duplicate access.
        win32_ok(unsafe { ImpersonateLoggedOnUser(restricted.as_raw_handle()) });
        let context = Self {
            _token: restricted,
            _thread: PhantomData,
        };
        let mut effective = null_mut();
        // SAFETY: current-thread pseudo-handle and writable token output.
        win32_ok(unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut effective) });
        // SAFETY: successful query returned a new owned token handle.
        let effective = unsafe { OwnedHandle::from_raw_handle(effective) };
        assert_eq!(elevation(effective.as_raw_handle()), 0);
        let mut sid = [0_u32; SECURITY_MAX_SID_SIZE as usize / size_of::<u32>()];
        let mut size = size_of_val(&sid) as u32;
        // SAFETY: SID buffer is aligned and sized to SECURITY_MAX_SID_SIZE.
        win32_ok(unsafe {
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                null_mut(),
                sid.as_mut_ptr().cast(),
                &mut size,
            )
        });
        let mut administrator = 0;
        // SAFETY: null token selects the effective thread context. SID is valid
        // after the successful creation; administrator is a writable BOOL.
        win32_ok(unsafe {
            CheckTokenMembership(null_mut(), sid.as_mut_ptr().cast(), &mut administrator)
        });
        assert_eq!(administrator, 0);
        eprintln!(
            "flush-probe process-elevated={original_elevation} effective-elevated=0 effective-admin=0 token=LUA+DISABLE_MAX_PRIVILEGE"
        );
        context
    }
}

impl Drop for RestrictedContext {
    fn drop(&mut self) {
        // SAFETY: this guard stays on the impersonating thread. A failed revert
        // must not permit subsequent tests to continue under the wrong context.
        if unsafe { RevertToSelf() } == 0 {
            std::process::abort();
        }
    }
}

fn elevation(token: HANDLE) -> u32 {
    let mut elevation = TOKEN_ELEVATION::default();
    let mut length = 0;
    // SAFETY: live queryable token and correctly sized/aligned output buffer.
    win32_ok(unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        )
    });
    assert_eq!(length as usize, size_of::<TOKEN_ELEVATION>());
    elevation.TokenIsElevated
}

fn win32_ok(result: i32) {
    assert_ne!(
        result,
        0,
        "native setup error: {:?}",
        io::Error::last_os_error().raw_os_error()
    );
}
