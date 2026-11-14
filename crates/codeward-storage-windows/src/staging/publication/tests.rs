use super::*;
use crate::tests::Fixture;
use std::{
    cell::Cell,
    fs::{self, OpenOptions},
    io::Write,
};

mod authenticated;

type Close<'a> = dyn FnMut(File) -> io::Result<()> + 'a;
type Rename<'a> = dyn FnMut(&File, &File, bool) -> io::Result<()> + 'a;
#[derive(Default)]
struct Hooks<'a> {
    close: Option<Box<Close<'a>>>,
    rename: Option<Box<Rename<'a>>>,
}
impl PublicationIo for Hooks<'_> {
    fn close(&mut self, file: File) -> io::Result<()> {
        match &mut self.close {
            Some(close) => close(file),
            None => native::close_checked(file),
        }
    }
    fn rename(&mut self, file: &File, parent: &File, replace: bool) -> io::Result<()> {
        match &mut self.rename {
            Some(rename) => rename(file, parent, replace),
            None => native::publish(file, parent, replace),
        }
    }
}

fn stages(parent: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".codeward.stage-")
        })
        .collect();
    paths.sort();
    paths
}

#[test]
fn ntfs_publication_refuses_conflicts_without_native_rename() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    let calls = Cell::new(0);
    let mut hooks = Hooks {
        rename: Some(Box::new(|source, parent, replace| {
            calls.set(calls.get() + 1);
            native::publish(source, parent, replace)
        })),
        ..Default::default()
    };
    let absent = lease.prepare_stage(b"candidate").unwrap();
    assert_eq!(
        absent.publish_with(Some(b"base"), &mut hooks),
        Err(PublicationError::NotPublished(NativeError::Conflict))
    );
    fs::write(fixture.0.join(VAULT), b"current").unwrap();
    for expected in [None, Some(&b"stale"[..])] {
        let stage = lease.prepare_stage(b"candidate").unwrap();
        assert_eq!(
            stage.publish_with(expected, &mut hooks),
            Err(PublicationError::NotPublished(NativeError::Conflict))
        );
    }
    assert_eq!(calls.get(), 0);
    assert_eq!(stages(&fixture.0).len(), 3);
    assert_eq!(lease.read_current().unwrap().unwrap(), b"current");
    let oversized = vec![0; MAX_FILE_LENGTH + 1];
    assert_eq!(
        lease
            .prepare_stage(b"candidate")
            .unwrap()
            .publish_replace(&oversized),
        Err(PublicationError::NotPublished(NativeError::TooLarge))
    );
}

#[test]
fn ntfs_no_replace_preserves_a_racing_creator() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    let stage = lease.prepare_stage(b"candidate").unwrap();
    let mut hooks = Hooks {
        rename: Some(Box::new(|source, parent, replace| {
            assert!(!replace);
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(fixture.0.join(VAULT))?
                .write_all(b"racing creator")?;
            native::publish(source, parent, replace)
        })),
        ..Default::default()
    };
    assert_eq!(
        stage.publish_with(None, &mut hooks),
        Err(PublicationError::OutcomeUnknown)
    );
    assert_eq!(lease.read_current().unwrap().unwrap(), b"racing creator");
    assert_eq!(fs::read(&stages(&fixture.0)[0]).unwrap(), b"candidate");
}

#[test]
fn ntfs_publication_detects_source_mutation_and_substitution_during_reopen() {
    for substitute in [false, true] {
        let fixture = Fixture::new();
        let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
        fs::write(fixture.0.join(VAULT), b"base").unwrap();
        let stage = lease.prepare_stage(b"candidate").unwrap();
        let mut closes = 0;
        let mut hooks = Hooks {
            close: Some(Box::new(|file| {
                let path = native::final_path(&file).unwrap();
                native::close_checked(file)?;
                closes += 1;
                if closes != 1 {
                    return Ok(());
                }
                if substitute {
                    fs::rename(&path, fixture.0.join("preserved-original-stage"))?;
                    let name = path.file_name().unwrap().to_str().unwrap();
                    let mut replacement = crate::managed::Admission::new()
                        .unwrap()
                        .create_stage(&lease.inner.directories.last().unwrap().1, name)?;
                    replacement.write_all(b"candidate")?;
                    replacement.sync_all()?;
                    native::close_checked(replacement)?;
                } else {
                    fs::write(path, b"different")?;
                }
                Ok(())
            })),
            ..Default::default()
        };
        assert_eq!(
            stage.publish_with(Some(b"base"), &mut hooks),
            Err(PublicationError::NotPublished(NativeError::Conflict))
        );
        assert_eq!(lease.read_current().unwrap().unwrap(), b"base");
        if substitute {
            assert_eq!(
                fs::read(fixture.0.join("preserved-original-stage")).unwrap(),
                b"candidate"
            );
        }
    }
}

#[test]
fn ntfs_publication_rechecks_base_after_source_handle_transition() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    fs::write(fixture.0.join(VAULT), b"base").unwrap();
    let stage = lease.prepare_stage(b"candidate").unwrap();
    let mut hooks = Hooks {
        close: Some(Box::new(|file| {
            native::close_checked(file)?;
            fs::write(fixture.0.join(VAULT), b"changed base")?;
            Ok(())
        })),
        ..Default::default()
    };
    assert_eq!(
        stage.publish_with(Some(b"base"), &mut hooks),
        Err(PublicationError::NotPublished(NativeError::Conflict))
    );
    assert_eq!(lease.read_current().unwrap().unwrap(), b"changed base");
    assert_eq!(fs::read(&stages(&fixture.0)[0]).unwrap(), b"candidate");
}

#[test]
fn ntfs_publication_distinguishes_preclose_failure_from_uncertain_final_close() {
    for fail_at in [1, 2] {
        let fixture = Fixture::new();
        let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
        fs::write(fixture.0.join(VAULT), b"base").unwrap();
        let stage = lease.prepare_stage(b"candidate").unwrap();
        let mut count = 0;
        let mut hooks = Hooks {
            close: Some(Box::new(|file| {
                native::close_checked(file)?;
                count += 1;
                if count == fail_at {
                    return Err(io::ErrorKind::Other.into());
                }
                Ok(())
            })),
            ..Default::default()
        };
        let expected = if fail_at == 1 {
            PublicationError::NotPublished(NativeError::CloseStage)
        } else {
            PublicationError::OutcomeUnknown
        };
        assert_eq!(stage.publish_with(Some(b"base"), &mut hooks), Err(expected));
        assert_eq!(
            lease.read_current().unwrap().unwrap(),
            if fail_at == 1 {
                &b"base"[..]
            } else {
                &b"candidate"[..]
            }
        );
        assert_eq!(stages(&fixture.0).len(), if fail_at == 1 { 1 } else { 0 });
    }
}

#[test]
fn ntfs_publication_keeps_native_error_lost_reply_and_postcheck_unknown() {
    for mode in ["error", "lost-reply", "postcheck"] {
        let fixture = Fixture::new();
        let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
        fs::write(fixture.0.join(VAULT), b"base").unwrap();
        let stage = lease.prepare_stage(b"candidate").unwrap();
        let mut hooks = Hooks {
            rename: Some(Box::new(|source, parent, replace| {
                if mode == "error" {
                    return Err(io::ErrorKind::Other.into());
                }
                native::publish(source, parent, replace)?;
                if mode == "lost-reply" {
                    return Err(io::ErrorKind::Other.into());
                }
                let mut companion = &lease.inner.companion;
                companion.write_all(b"changed companion")?;
                Ok(())
            })),
            ..Default::default()
        };
        assert_eq!(
            stage.publish_with(Some(b"base"), &mut hooks),
            Err(PublicationError::OutcomeUnknown)
        );
        assert_eq!(
            fs::read(fixture.0.join(VAULT)).unwrap(),
            if mode == "error" {
                &b"base"[..]
            } else {
                &b"candidate"[..]
            }
        );
        assert_eq!(
            stages(&fixture.0).len(),
            if mode == "error" { 1 } else { 0 }
        );
    }
}

#[test]
fn ntfs_publication_refuses_same_lease_contention_and_retains_exclusion() {
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    let stage = lease.prepare_stage(b"candidate").unwrap();
    let guard = lease.inner.publication.lock().unwrap();
    assert_eq!(
        stage.publish_create(),
        Err(PublicationError::NotPublished(
            NativeError::WriterUnavailable
        ))
    );
    assert_eq!(lease.read_current().unwrap(), None);
    drop(guard);
    let stage = lease.prepare_stage(b"candidate").unwrap();
    drop(lease);
    stage.publish_create().unwrap();
    let reopened = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    assert_eq!(reopened.read_current().unwrap().unwrap(), b"candidate");
    assert_eq!(stages(&fixture.0).len(), 1);
}

#[test]
fn ntfs_publication_respects_a_destination_reader_and_preserves_whole_files() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let lease = ReadLease::acquire_synthetic(&fixture.0).unwrap();
    fs::write(fixture.0.join(VAULT), b"base").unwrap();
    let reader = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(fixture.0.join(VAULT))
        .unwrap();
    assert_eq!(
        lease
            .prepare_stage(b"candidate")
            .unwrap()
            .publish_replace(b"base"),
        Err(PublicationError::OutcomeUnknown)
    );
    assert_eq!(lease.read_current().unwrap().unwrap(), b"base");
    assert_eq!(fs::read(&stages(&fixture.0)[0]).unwrap(), b"candidate");
    drop(reader);
    lease
        .prepare_stage(b"candidate")
        .unwrap()
        .publish_replace(b"base")
        .unwrap();
    assert_eq!(lease.read_current().unwrap().unwrap(), b"candidate");
}
