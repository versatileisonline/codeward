use std::{
    ffi::{CStr, OsStr},
    fs::File,
    io,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};

use crate::{NativeError, metadata, native, open_error, open_path, same_inode, validate_directory};

const DIRECTORY: libc::c_int = libc::O_RDONLY | libc::O_DIRECTORY;

pub(super) struct Location {
    anchor_path: PathBuf,
    anchor: File,
    private_anchor: bool,
    pub(super) owner: u32,
    mount: u64,
    children: Vec<Child>,
}

struct Child {
    name: &'static CStr,
    directory: File,
    private: bool,
}

impl Location {
    pub(super) fn fixture(path: &Path, owner: u32) -> Result<Self, NativeError> {
        Self::open(path, owner, true)
    }

    pub(super) fn managed() -> Result<Self, NativeError> {
        // SAFETY: geteuid has no pointer arguments or preconditions.
        let owner = unsafe { libc::geteuid() };
        let xdg = std::env::var_os("XDG_DATA_HOME");
        let (path, use_default) = data_root(xdg.as_deref(), || native::account_home(owner))?;
        Self::prepare(&path, use_default, owner)
    }

    pub(super) fn prepare(path: &Path, use_default: bool, owner: u32) -> Result<Self, NativeError> {
        let mut location = Self::open(path, owner, false)?;
        if use_default {
            location.prepare_child(c".local", false)?;
            location.prepare_child(c"share", false)?;
        }
        location.prepare_child(c"codeward", true)?;
        location.prepare_child(c"vaults", true)?;
        location.verify()?;
        Ok(location)
    }

    fn open(path: &Path, owner: u32, private_anchor: bool) -> Result<Self, NativeError> {
        let anchor = open_path(path, owner, private_anchor)?;
        let mount = native::mount_id(&anchor)?;
        native::verify_mount(mount)?;
        if !private_anchor {
            validate_managed_directory(&anchor, owner, false)?;
        }
        Ok(Self {
            anchor_path: path.to_owned(),
            anchor,
            private_anchor,
            owner,
            mount,
            children: Vec::new(),
        })
    }

    fn prepare_child(&mut self, name: &'static CStr, private: bool) -> Result<(), NativeError> {
        self.verify()?;
        let parent = self.parent();
        let directory = match native::open_at(parent, name, DIRECTORY, true) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match native::create_private_directory(parent, name) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(open_error(error)),
                }
                native::open_at(parent, name, DIRECTORY, true).map_err(open_error)?
            }
            Err(error) => return Err(open_error(error)),
        };
        validate_managed_directory(&directory, self.owner, private)?;
        self.children.push(Child {
            name,
            directory,
            private,
        });
        Ok(())
    }

    pub(super) fn parent(&self) -> &File {
        self.children
            .last()
            .map_or(&self.anchor, |child| &child.directory)
    }

    pub(super) fn directories_from_leaf(&self) -> impl Iterator<Item = &File> {
        self.children
            .iter()
            .rev()
            .map(|child| &child.directory)
            .chain([&self.anchor])
    }

    pub(super) fn verify(&self) -> Result<(), NativeError> {
        let mut current = open_path(&self.anchor_path, self.owner, self.private_anchor)?;
        if native::mount_id(&current)? != self.mount
            || !same_inode(&metadata(&self.anchor)?, &metadata(&current)?)
        {
            return Err(NativeError::Conflict);
        }
        native::verify_mount(self.mount)?;
        if !self.private_anchor {
            validate_managed_directory(&current, self.owner, false)?;
        }
        for child in &self.children {
            current =
                native::open_at(&current, child.name, DIRECTORY, true).map_err(crate::changed)?;
            if !same_inode(&metadata(&child.directory)?, &metadata(&current)?) {
                return Err(NativeError::Conflict);
            }
            validate_managed_directory(&current, self.owner, child.private)?;
        }
        Ok(())
    }
}

fn validate_managed_directory(file: &File, owner: u32, private: bool) -> Result<(), NativeError> {
    validate_directory(file, owner, private)?;
    if !private {
        if metadata(file)?.uid() != owner {
            return Err(NativeError::Permissions);
        }
        native::refuse_acl(file, c"system.posix_acl_default")?;
    }
    Ok(())
}

pub(super) fn data_root(
    xdg: Option<&OsStr>,
    home: impl FnOnce() -> Result<PathBuf, NativeError>,
) -> Result<(PathBuf, bool), NativeError> {
    let (path, use_default) = match xdg.filter(|path| path.as_bytes().starts_with(b"/")) {
        Some(path) => (PathBuf::from(path), false),
        None => (home()?, true),
    };
    crate::validate_path(&path)?;
    Ok((path, use_default))
}
