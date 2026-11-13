use std::{
    ffi::{OsString, c_void},
    fs::File,
    io,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr::null_mut,
};

use windows_sys::{
    Win32::{
        Foundation::{
            ERROR_ALREADY_EXISTS, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree,
        },
        Security::{
            ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                GetSecurityInfo, SE_FILE_OBJECT,
            },
            DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation, GetTokenInformation,
            IsWellKnownSid, OWNER_SECURITY_INFORMATION, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY,
            TOKEN_USER, TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid,
        },
        Storage::FileSystem::{CreateDirectoryW, CreateFileW, OPEN_ALWAYS},
        System::{
            Com::CoTaskMemFree,
            SystemServices::{ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE},
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
        UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath},
    },
    core::PWSTR,
};

use crate::{
    COMPANION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, NativeError,
    companion_error, native, open_directory,
};

pub(super) struct Admission {
    user: String,
    first_private: usize,
}

impl Admission {
    pub(super) fn new() -> Result<Self, NativeError> {
        Ok(Self {
            user: process_user_string()?,
            first_private: 0,
        })
    }

    pub(super) fn prepare(
        &mut self,
        parent: &mut PathBuf,
        directories: &mut Vec<(PathBuf, File)>,
    ) -> Result<File, NativeError> {
        self.first_private = directories.len() - 1;
        let owner = &self.user;
        let descriptor = security_descriptor(&format!(
            "O:{owner}D:P(A;;FA;;;{owner})(A;;FA;;;SY)(A;;FA;;;BA)"
        ))?;
        let attributes = attributes(&descriptor);

        for component in ["Codeward", "Vaults"] {
            self.verify_directories(directories)?;
            let path = parent.join(component);
            create_directory(&path, &attributes)?;
            let directory = open_directory(&path)?;
            let actual = native::final_path(&directory)?;
            if actual != path {
                return Err(NativeError::Unsupported);
            }
            self.verify_object(&directory)?;
            directories.push((actual.clone(), directory));
            *parent = actual;
        }
        self.verify_directories(directories)?;

        let path = wide(&parent.join(COMPANION));
        // SAFETY: path and attributes live through the call; the descriptor is
        // retained above. OPEN_ALWAYS leaves an existing object unchanged.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                &attributes,
                OPEN_ALWAYS,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(companion_error(io::Error::last_os_error()));
        }
        // SAFETY: CreateFileW returned a fresh owned handle.
        Ok(unsafe { File::from_raw_handle(handle) })
    }

    pub(super) fn verify_directories(
        &self,
        directories: &[(PathBuf, File)],
    ) -> Result<(), NativeError> {
        crate::verify_directory_chain(directories)?;
        for (_, directory) in &directories[self.first_private..] {
            self.verify_object(directory)?;
        }
        Ok(())
    }

    pub(super) fn verify_object(&self, file: &File) -> Result<(), NativeError> {
        let mut owner = null_mut();
        let mut acl = null_mut();
        let mut descriptor = LocalAllocation(null_mut());
        // SAFETY: file is live and all outputs point to writable storage.
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut acl,
                null_mut(),
                &mut descriptor.0,
            )
        };
        if status != 0 || owner.is_null() || acl.is_null() {
            return Err(NativeError::Permissions);
        }
        if sid_string(owner)?.text()? != self.user {
            return Err(NativeError::Permissions);
        }
        check_acl(acl, owner)
    }
}

fn check_acl(acl: *const ACL, owner: PSID) -> Result<(), NativeError> {
    let mut size = ACL_SIZE_INFORMATION::default();
    // SAFETY: the security descriptor owns acl and size is a valid output.
    if unsafe {
        GetAclInformation(
            acl,
            (&mut size as *mut ACL_SIZE_INFORMATION).cast(),
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
    {
        return Err(NativeError::Permissions);
    }
    for index in 0..size.AceCount {
        let mut ace = null_mut();
        // SAFETY: index is within the reported ACL count and ace is writable.
        if unsafe { GetAce(acl, index, &mut ace) } == 0 || ace.is_null() {
            return Err(NativeError::Permissions);
        }
        // SAFETY: the OS returned an ACL entry with a readable header.
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        match u32::from(header.AceType) {
            ACCESS_DENIED_ACE_TYPE => continue,
            ACCESS_ALLOWED_ACE_TYPE => {
                let offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
                if usize::from(header.AceSize) < offset + 8 {
                    return Err(NativeError::Permissions);
                }
                // SAFETY: checked space for the fixed SID header inside the ACE.
                let sid = unsafe { ace.cast::<u8>().add(offset) };
                let count = usize::from(unsafe { *sid.add(1) });
                if count > 15 || usize::from(header.AceSize) != offset + 8 + 4 * count {
                    return Err(NativeError::Permissions);
                }
                // SAFETY: the SID is wholly contained in the live ACE.
                let trusted = unsafe {
                    EqualSid(sid.cast(), owner) != 0
                        || IsWellKnownSid(sid.cast(), WinLocalSystemSid) != 0
                        || IsWellKnownSid(sid.cast(), WinBuiltinAdministratorsSid) != 0
                };
                if !trusted {
                    return Err(NativeError::Permissions);
                }
            }
            _ => return Err(NativeError::Permissions),
        }
    }
    Ok(())
}

pub(super) fn local_app_data() -> Result<PathBuf, NativeError> {
    let mut path: PWSTR = null_mut();
    // SAFETY: the known-folder identifier is valid and path is a writable output.
    let result = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, null_mut(), &mut path) };
    let value = if result >= 0 && !path.is_null() {
        read_wide(path).map(|units| PathBuf::from(OsString::from_wide(&units)))
    } else {
        Err(NativeError::Open)
    };
    // SAFETY: the shell API's output uses the COM task allocator.
    unsafe { CoTaskMemFree(path.cast()) };
    value
}

fn process_user_string() -> Result<String, NativeError> {
    let mut token = null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle; token is writable.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(NativeError::Permissions);
    }
    // SAFETY: OpenProcessToken returned a fresh owned handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut buffer = [0_usize; 32];
    let mut length = 0;
    // SAFETY: buffer is aligned and writable for the supplied size.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size_of_val(&buffer) as u32,
            &mut length,
        )
    } == 0
    {
        return Err(NativeError::Permissions);
    }
    // SAFETY: the successful TokenUser query initialized the structure and SID.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    sid_string(user.User.Sid)?.text()
}

fn sid_string(sid: PSID) -> Result<LocalAllocation, NativeError> {
    let mut value = null_mut();
    // SAFETY: callers supply a live SID from a successful native query.
    if unsafe { ConvertSidToStringSidW(sid, &mut value) } == 0 || value.is_null() {
        return Err(NativeError::Permissions);
    }
    Ok(LocalAllocation(value.cast()))
}

fn security_descriptor(sddl: &str) -> Result<LocalAllocation, NativeError> {
    let sddl: Vec<_> = sddl.encode_utf16().chain([0]).collect();
    let mut descriptor = LocalAllocation(null_mut());
    // SAFETY: the string is NUL-terminated and descriptor is writable.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor.0,
            null_mut(),
        )
    } == 0
    {
        return Err(NativeError::Permissions);
    }
    Ok(descriptor)
}

fn attributes(descriptor: &LocalAllocation) -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    }
}

fn create_directory(path: &Path, attributes: &SECURITY_ATTRIBUTES) -> Result<(), NativeError> {
    let path = wide(path);
    // SAFETY: path and the caller's descriptor live through the call.
    if unsafe { CreateDirectoryW(path.as_ptr(), attributes) } == 0
        && io::Error::last_os_error().raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32)
    {
        return Err(NativeError::Open);
    }
    Ok(())
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

struct LocalAllocation(*mut c_void);

impl LocalAllocation {
    fn text(&self) -> Result<String, NativeError> {
        String::from_utf16(&read_wide(self.0.cast())?).map_err(|_| NativeError::Permissions)
    }
}

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this allocation belongs to the Windows local allocator.
            unsafe { LocalFree(self.0) };
        }
    }
}

fn read_wide(value: *const u16) -> Result<Vec<u16>, NativeError> {
    if value.is_null() {
        return Err(NativeError::Open);
    }
    let mut units = Vec::new();
    for index in 0..32_768 {
        // SAFETY: callers supply an OS-allocated NUL-terminated UTF-16 string.
        let unit = unsafe { *value.add(index) };
        if unit == 0 {
            return Ok(units);
        }
        units.push(unit);
    }
    Err(NativeError::InvalidPath)
}

#[cfg(test)]
pub(super) fn protect_fixture(path: &Path) {
    // Recreate the empty fixture with the same protected descriptor used by
    // the managed path. This only touches the disposable test directory.
    std::fs::remove_dir(path).unwrap();
    let owner = process_user_string().unwrap();
    let descriptor = security_descriptor(&format!("O:{owner}D:P(A;;FA;;;{owner})")).unwrap();
    create_directory(path, &attributes(&descriptor)).unwrap();
}

#[cfg(test)]
pub(super) fn create_broad_fixture(path: &Path) {
    let owner = process_user_string().unwrap();
    let descriptor =
        security_descriptor(&format!("O:{owner}D:P(A;;FA;;;{owner})(A;;FR;;;WD)")).unwrap();
    create_directory(path, &attributes(&descriptor)).unwrap();
}

#[cfg(test)]
pub(super) fn create_broad_vault(path: &Path) {
    use windows_sys::Win32::Storage::FileSystem::CREATE_NEW;

    let owner = process_user_string().unwrap();
    let descriptor =
        security_descriptor(&format!("O:{owner}D:P(A;;FA;;;{owner})(A;;FR;;;WD)")).unwrap();
    let path = wide(path);
    // SAFETY: the path and descriptor remain live during creation.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_WRITE,
            0,
            &attributes(&descriptor),
            CREATE_NEW,
            0,
            null_mut(),
        )
    };
    assert_ne!(handle, INVALID_HANDLE_VALUE);
    // SAFETY: the successful call returned a fresh owned handle.
    drop(unsafe { File::from_raw_handle(handle) });
}
