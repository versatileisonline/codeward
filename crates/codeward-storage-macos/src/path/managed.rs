use super::*;

const SUFFIX: &str = "Library/Application Support/Codeward/Vaults/vault.cw";
const DIRECTORY_COUNT: usize = 4;

impl VaultPath {
    /// Prepare the current OS account's fixed managed working location.
    ///
    /// Uses the account database, never a UI path or HOME override. Missing
    /// fixed application-data directories are created privately and existing
    /// entries are validated without repair. The account home and managed tree
    /// must share one local APFS device. Recognized cloud roots are refused.
    /// Preparation leaves the vault file untouched. Synthetic inputs only until
    /// runtime storage and persistence acceptance are complete. Initial
    /// nonsandboxed macOS only.
    pub fn managed() -> Result<Self, NativeError> {
        let home = native::account_home().map_err(|_| NativeError::Io)?;
        Self::prepare_managed(&home, |_| {})
    }

    #[cfg(test)]
    pub(crate) fn managed_for_test(home: &Path) -> Result<Self, NativeError> {
        Self::prepare_managed(home, |_| {})
    }

    fn prepare_managed(
        home: &Path,
        mut after_open: impl FnMut(usize),
    ) -> Result<Self, NativeError> {
        if home == Path::new("/") {
            return Err(NativeError::InvalidPath);
        }
        let (names, name) = components(&home.join(SUFFIX))?;
        let start = names
            .len()
            .checked_sub(DIRECTORY_COUNT)
            .ok_or(NativeError::InvalidPath)?;
        let home_index = start.checked_sub(1).ok_or(NativeError::InvalidPath)?;
        if names[..start].windows(2).any(|pair| {
            let root = pair[1].as_bytes();
            pair[0].as_bytes().eq_ignore_ascii_case(b"Library")
                && (root.eq_ignore_ascii_case(b"CloudStorage")
                    || root.eq_ignore_ascii_case(b"Mobile Documents"))
        }) {
            return Err(NativeError::Unsupported);
        }
        let private_start = names.len() - 2;
        let mut result = Self {
            root: File::open("/").map_err(|_| NativeError::Io)?,
            directories: Vec::new(),
            name,
            managed_home: Some(home_index),
        };
        for (index, name) in names.into_iter().enumerate() {
            if index == start {
                result.verify()?;
            } else {
                result.verify_chain()?;
            }
            let open = || {
                native::open_at(
                    result.parent(),
                    &name,
                    libc::O_RDONLY | libc::O_DIRECTORY,
                    0,
                )
            };
            let file = match open() {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound && index >= start => {
                    if !native::local_writable_apfs(result.parent()).map_err(|_| NativeError::Io)? {
                        return Err(NativeError::Unsupported);
                    }
                    match native::mkdir_at(result.parent(), &name) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(open_error(error)),
                    }
                    open().map_err(open_error)?
                }
                Err(error) => return Err(open_error(error)),
            };
            let identity = Identity::of(&file.metadata().map_err(|_| NativeError::Io)?);
            result.directories.push(Directory {
                file,
                name,
                identity,
                private: index >= private_start,
            });
            after_open(index);
            result.verify_chain()?;
        }
        result.verify()?;
        Ok(result)
    }
}

pub(super) fn directory_permissions(file: &File, private: bool) -> Result<(), NativeError> {
    let meta = file.metadata().map_err(|_| NativeError::Io)?;
    if !meta.is_dir() {
        return Err(NativeError::Unsupported);
    }
    let uid = native::effective_uid();
    if private {
        if meta.uid() != uid || meta.mode() & 0o7777 != 0o700 {
            return Err(NativeError::Permissions);
        }
        private_acl(file)
    } else {
        if !ancestor_owner_mode(meta.uid(), meta.mode(), uid) {
            return Err(NativeError::Permissions);
        }
        match native::no_acl_grants(file) {
            Ok(true) => Ok(()),
            Ok(false) => Err(NativeError::Permissions),
            Err(_) => Err(NativeError::Unsupported),
        }
    }
}

fn ancestor_owner_mode(owner: u32, mode: u32, uid: u32) -> bool {
    (owner == 0 || owner == uid) && mode & 0o022 == 0
}

#[cfg(test)]
mod tests;
