use super::*;
use crate::{
    COMPANION, PublicationError,
    tests::{Fixture, flush_probe::RestrictedContext},
};
use std::{fs, path::PathBuf};

fn fixture() -> (RestrictedContext, Fixture, ReadLease) {
    let context = RestrictedContext::enter();
    let fixture = Fixture::new();
    managed::protect_fixture(&fixture.0);
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    (context, fixture, lease)
}

#[derive(Default)]
struct Trace {
    paths: Vec<PathBuf>,
    flushes: usize,
    closes: usize,
    fail_flush: Option<usize>,
    fail_close: Option<usize>,
}
impl NamespaceIo for Trace {
    fn flush(&mut self, file: &File) -> io::Result<()> {
        self.paths.push(native::final_path(file).unwrap());
        self.flushes += 1;
        if self.fail_flush == Some(self.flushes) {
            return Err(io::ErrorKind::Other.into());
        }
        native::flush(file)
    }
    fn close(&mut self, file: File) -> io::Result<()> {
        self.closes += 1;
        native::close_checked(file)?;
        if self.fail_close == Some(self.closes) {
            return Err(io::ErrorKind::Other.into());
        }
        Ok(())
    }
}

#[test]
fn ntfs_namespace_matches_complete_presence_absence_and_stale_state() {
    let (_context, _fixture, lease) = fixture();
    lease.synchronize_namespace(None).unwrap();
    assert_eq!(
        lease.synchronize_namespace(Some(b"missing")),
        Err(NativeError::Conflict)
    );
    lease
        .prepare_stage(b"candidate")
        .unwrap()
        .publish_create()
        .unwrap();
    lease.synchronize_namespace(Some(b"candidate")).unwrap();
    for expected in [None, Some(&b"stale"[..]), Some(&b"candidata"[..])] {
        assert_eq!(
            lease.synchronize_namespace(expected),
            Err(NativeError::Conflict)
        );
    }
    lease
        .prepare_stage(b"next")
        .unwrap()
        .publish_replace(b"candidate")
        .unwrap();
    lease.synchronize_namespace(Some(b"next")).unwrap();
    assert_eq!(lease.read_current().unwrap().as_deref(), Some(&b"next"[..]));
}

#[test]
fn ntfs_namespace_caps_expected_and_actual_before_barriers() {
    let (_context, fixture, lease) = fixture();
    let mut trace = Trace::default();
    assert_eq!(
        lease.synchronize_with(Some(&vec![0; MAX_FILE_LENGTH + 1]), &mut trace),
        Err(NativeError::TooLarge)
    );
    lease
        .prepare_stage(b"candidate")
        .unwrap()
        .publish_create()
        .unwrap();
    let file = OpenOptions::new()
        .write(true)
        .open(fixture.0.join(VAULT))
        .unwrap();
    file.set_len(MAX_FILE_LENGTH as u64 + 1).unwrap();
    drop(file);
    assert_eq!(
        lease.synchronize_with(Some(b"candidate"), &mut trace),
        Err(NativeError::TooLarge)
    );
    assert_eq!(trace.flushes, 0);
}

#[test]
fn ntfs_namespace_orders_file_then_leaf_and_stops_at_fixture_anchor() {
    let (_context, _fixture, lease) = fixture();
    lease
        .prepare_stage(b"candidate")
        .unwrap()
        .publish_create()
        .unwrap();
    let mut trace = Trace::default();
    lease
        .synchronize_with(Some(b"candidate"), &mut trace)
        .unwrap();
    assert_eq!(
        trace.paths,
        [lease.inner.parent.join(VAULT), lease.inner.parent.clone()]
    );
    assert_eq!(trace.closes, 2);
}

#[test]
fn ntfs_namespace_covers_fresh_managed_names_through_existing_anchor() {
    let _context = RestrictedContext::enter();
    let fixture = Fixture::new();
    managed::protect_fixture(&fixture.0);
    let lease = ReadLease::acquire_managed_at(&fixture.0).unwrap();
    lease
        .prepare_stage(b"candidate")
        .unwrap()
        .publish_create()
        .unwrap();
    let anchor = lease.inner.managed.as_ref().unwrap().namespace_anchor();
    let mut trace = Trace::default();
    lease
        .synchronize_with(Some(b"candidate"), &mut trace)
        .unwrap();
    let expected: Vec<_> = std::iter::once(lease.inner.parent.join(VAULT))
        .chain(
            lease.inner.directories[anchor..]
                .iter()
                .rev()
                .map(|(path, _)| path.clone()),
        )
        .collect();
    assert_eq!(trace.paths, expected);
    assert_eq!(trace.paths.len(), 4); // file, Vaults, Codeward, existing AppData proxy
    drop(lease);
    let reopened = ReadLease::acquire_managed_at(&fixture.0).unwrap();
    reopened.synchronize_namespace(Some(b"candidate")).unwrap();
}

#[test]
fn ntfs_namespace_failures_preserve_candidate_residue_and_exclusion() {
    for (flush, close) in (1..=4)
        .map(|index| (Some(index), None))
        .chain((1..=4).map(|index| (None, Some(index))))
    {
        let _context = RestrictedContext::enter();
        let fixture = Fixture::new();
        managed::protect_fixture(&fixture.0);
        let lease = ReadLease::acquire_managed_at(&fixture.0).unwrap();
        lease
            .prepare_stage(b"candidate")
            .unwrap()
            .publish_create()
            .unwrap();
        drop(lease.prepare_stage(b"residue").unwrap());
        let mut before: Vec<_> = fs::read_dir(&lease.inner.parent)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        let mut trace = Trace {
            fail_flush: flush,
            fail_close: close,
            ..Trace::default()
        };
        assert_eq!(
            lease.synchronize_with(Some(b"candidate"), &mut trace),
            Err(NativeError::SyncNamespace)
        );
        let mut after: Vec<_> = fs::read_dir(&lease.inner.parent)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        before.sort();
        after.sort();
        assert_eq!(before, after);
        assert_eq!(
            lease.read_current().unwrap().as_deref(),
            Some(&b"candidate"[..])
        );
        assert_eq!(
            ReadLease::acquire_managed_at(&fixture.0).err(),
            Some(NativeError::WriterUnavailable)
        );
        lease.synchronize_namespace(Some(b"candidate")).unwrap();
    }
}

#[test]
fn ntfs_namespace_shares_nonblocking_publication_mutex() {
    let (_context, _fixture, lease) = fixture();
    let stage = lease.prepare_stage(b"candidate").unwrap();
    let guard = lease.inner.publication.lock().unwrap();
    assert_eq!(
        lease.synchronize_namespace(None),
        Err(NativeError::WriterUnavailable)
    );
    assert_eq!(
        stage.publish_create(),
        Err(PublicationError::NotPublished(
            NativeError::WriterUnavailable
        ))
    );
    drop(guard);
    lease.synchronize_namespace(None).unwrap();
}

#[test]
fn ntfs_namespace_refuses_directory_substitution_and_checks_close() {
    struct Substitute {
        other: PathBuf,
        closes: usize,
    }
    impl NamespaceIo for Substitute {
        fn open_directory(&mut self, _: &Path) -> io::Result<File> {
            SystemIo.open_directory(&self.other)
        }
        fn close(&mut self, file: File) -> io::Result<()> {
            self.closes += 1;
            native::close_checked(file)
        }
    }
    let (_context, _fixture, lease) = fixture();
    let other = Fixture::new();
    managed::protect_fixture(&other.0);
    let mut operations = Substitute {
        other: other.0.clone(),
        closes: 0,
    };
    assert_eq!(
        lease.synchronize_with(None, &mut operations),
        Err(NativeError::Conflict)
    );
    assert_eq!(operations.closes, 1);
    lease.synchronize_namespace(None).unwrap();
}

#[test]
fn ntfs_namespace_checks_absence_after_each_directory_barrier() {
    struct Appears {
        path: PathBuf,
    }
    impl NamespaceIo for Appears {
        fn flush(&mut self, file: &File) -> io::Result<()> {
            native::flush(file)?;
            fs::write(self.path.join(VAULT), b"external")
        }
    }
    let (_context, fixture, lease) = fixture();
    assert_eq!(
        lease.synchronize_with(
            None,
            &mut Appears {
                path: fixture.0.clone()
            }
        ),
        Err(NativeError::Conflict)
    );
    assert_eq!(fs::read(fixture.0.join(VAULT)).unwrap(), b"external");
    assert!(fixture.0.join(COMPANION).is_file());
}

#[test]
fn ntfs_namespace_refuses_broad_leaf_admission_and_open_failures() {
    let (_context, fixture, lease) = fixture();
    managed::create_broad_vault(&fixture.0.join(VAULT));
    assert_eq!(
        lease.synchronize_namespace(Some(b"")),
        Err(NativeError::Permissions)
    );
    struct Denied;
    impl NamespaceIo for Denied {
        fn open_file(&mut self, _: &File) -> io::Result<File> {
            Err(io::ErrorKind::PermissionDenied.into())
        }
    }
    assert_eq!(
        lease.synchronize_with(None, &mut Denied),
        Err(NativeError::Open)
    );
}

#[test]
fn ntfs_namespace_rechecks_file_and_directory_admission_after_flush() {
    use std::{os::windows::io::AsRawHandle, ptr::null_mut};
    use windows_sys::Win32::{
        Security::{
            Authorization::{SE_FILE_OBJECT, SetSecurityInfo},
            DACL_SECURITY_INFORMATION,
        },
        Storage::FileSystem::WRITE_DAC,
    };
    struct Broaden {
        at: usize,
        count: usize,
    }
    impl NamespaceIo for Broaden {
        fn flush(&mut self, file: &File) -> io::Result<()> {
            native::flush(file)?;
            self.count += 1;
            if self.count == self.at {
                let path = native::final_path(file).unwrap();
                let security = OpenOptions::new()
                    .access_mode(WRITE_DAC)
                    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                    .open(path)?;
                // SAFETY: this disposable fixture handle has WRITE_DAC. A null
                // DACL deliberately models a now-unrestricted object; admission
                // must refuse it. No owner/group/SACL update is requested.
                assert_eq!(
                    unsafe {
                        SetSecurityInfo(
                            security.as_raw_handle(),
                            SE_FILE_OBJECT,
                            DACL_SECURITY_INFORMATION,
                            null_mut(),
                            null_mut(),
                            null_mut(),
                            null_mut(),
                        )
                    },
                    0
                );
                native::close_checked(security)?;
            }
            Ok(())
        }
    }
    for at in [1, 2] {
        let (_context, fixture, lease) = fixture();
        lease
            .prepare_stage(b"candidate")
            .unwrap()
            .publish_create()
            .unwrap();
        assert_eq!(
            lease.synchronize_with(Some(b"candidate"), &mut Broaden { at, count: 0 }),
            Err(NativeError::Permissions)
        );
        assert_eq!(fs::read(fixture.0.join(VAULT)).unwrap(), b"candidate");
    }
}

#[test]
fn ntfs_namespace_refuses_file_handle_substitution_and_closes_on_directory_open_failure() {
    struct Substitute {
        other: File,
        closes: usize,
    }
    impl NamespaceIo for Substitute {
        fn open_file(&mut self, _: &File) -> io::Result<File> {
            SystemIo.open_file(&self.other)
        }
        fn close(&mut self, file: File) -> io::Result<()> {
            self.closes += 1;
            native::close_checked(file)
        }
    }
    let (_context, _fixture, lease) = fixture();
    lease
        .prepare_stage(b"candidate")
        .unwrap()
        .publish_create()
        .unwrap();
    let other = Fixture::new();
    managed::protect_fixture(&other.0);
    let other_lease = ReadLease::acquire_synthetic(&other.0).unwrap();
    other_lease
        .prepare_stage(b"candidate")
        .unwrap()
        .publish_create()
        .unwrap();
    let mut substitute = Substitute {
        other: crate::open_directory(&other.0).unwrap(),
        closes: 0,
    };
    assert_eq!(
        lease.synchronize_with(Some(b"candidate"), &mut substitute),
        Err(NativeError::Unsupported)
    );
    assert_eq!(substitute.closes, 1);
    struct DeniedDirectory {
        closes: usize,
    }
    impl NamespaceIo for DeniedDirectory {
        fn open_directory(&mut self, _: &Path) -> io::Result<File> {
            Err(io::ErrorKind::PermissionDenied.into())
        }
        fn close(&mut self, file: File) -> io::Result<()> {
            self.closes += 1;
            native::close_checked(file)
        }
    }
    let mut denied = DeniedDirectory { closes: 0 };
    assert_eq!(
        lease.synchronize_with(Some(b"candidate"), &mut denied),
        Err(NativeError::Open)
    );
    assert_eq!(denied.closes, 1);
    lease.synchronize_namespace(Some(b"candidate")).unwrap();
}
