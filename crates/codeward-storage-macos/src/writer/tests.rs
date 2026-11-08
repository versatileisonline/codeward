use super::*;
use crate::test_support::{MountFault, MountProbe, TestDir, fixture, stages};
use std::{
    fs::{self, Permissions},
    os::unix::fs::PermissionsExt,
};

mod workflow;

fn lease(dir: &TestDir) -> ExperimentalWriterLease {
    ExperimentalStorage::from_path(VaultPath::resolve(&dir.vault()).unwrap())
        .acquire()
        .unwrap()
}

#[test]
fn barriers_require_a_selected_observation_and_do_not_infer_absence() {
    let dir = TestDir::new();
    let mut writer = lease(&dir);
    assert_eq!(writer.persist(), Err(StorageError::Barrier));
    writer.stage(&fixture()).unwrap();
    assert_eq!(
        writer.publish(PublishMode::Create),
        Err(PublishError::NotPublished)
    );
    assert_eq!(writer.persist(), Err(StorageError::Barrier));
    assert_eq!(writer.read_current(), Ok(None));
    assert_eq!(writer.persist(), Ok(()));
    assert!(!dir.vault().exists());
}

#[test]
fn failed_reads_invalidate_cached_bytes_until_fresh_selection() {
    let dir = TestDir::new();
    let bytes = fixture();
    dir.write(&bytes);
    let mut writer = lease(&dir);
    assert_eq!(writer.read_current(), Ok(Some(bytes.clone())));
    fs::set_permissions(dir.vault(), Permissions::from_mode(0o000)).unwrap();
    assert_eq!(writer.read_current(), Err(StorageError::Read));
    fs::set_permissions(dir.vault(), Permissions::from_mode(0o600)).unwrap();
    assert_eq!(writer.persist(), Err(StorageError::Barrier));
    assert_eq!(writer.read_current(), Ok(Some(bytes.clone())));
    assert_eq!(writer.persist(), Ok(()));
    assert_eq!(fs::read(dir.vault()).unwrap(), bytes);
}

#[test]
fn failed_new_stage_discards_old_authority_without_deleting_residue() {
    let dir = TestDir::new();
    let mut writer = lease(&dir);
    writer.read_current().unwrap();
    let bytes = fixture();
    writer.stage(&bytes).unwrap();
    let residue = stages(&dir.0);
    assert_eq!(residue.len(), 1);
    assert_eq!(
        writer.stage(&vec![0; MAX_FILE_LENGTH + 1]),
        Err(StorageError::CreateStage)
    );
    assert_eq!(
        writer.publish(PublishMode::Create),
        Err(PublishError::NotPublished)
    );
    assert_eq!(writer.persist(), Err(StorageError::Barrier));
    assert!(!dir.vault().exists());
    assert_eq!(stages(&dir.0), residue);
    assert_eq!(fs::read(&residue[0]).unwrap(), bytes);
}

#[test]
fn conflicting_publication_clears_observation_and_consumes_stage_but_retains_exclusion() {
    let dir = TestDir::new();
    dir.write(&fixture());
    let mut writer = lease(&dir);
    writer.read_current().unwrap();
    writer.stage(b"synthetic encrypted candidate").unwrap();
    fs::write(dir.vault(), b"synthetic external state").unwrap();
    assert_eq!(
        writer.publish(PublishMode::Replace),
        Err(PublishError::NotPublished)
    );
    assert_eq!(writer.persist(), Err(StorageError::Barrier));
    assert_eq!(
        writer.publish(PublishMode::Replace),
        Err(PublishError::NotPublished)
    );
    assert_eq!(
        VaultPath::resolve(&dir.vault()).unwrap().acquire().err(),
        Some(NativeError::WriterUnavailable)
    );
    assert_eq!(fs::read(dir.vault()).unwrap(), b"synthetic external state");
    assert_eq!(stages(&dir.0).len(), 1);
}

#[test]
fn failed_barrier_invalidates_selection_even_after_storage_policy_recovers() {
    let dir = TestDir::new();
    let probe = MountProbe::new();
    let mut writer = lease(&dir);
    assert_eq!(writer.read_current(), Ok(None));
    probe.set(Some(MountFault::Refused));
    assert_eq!(writer.persist(), Err(StorageError::Barrier));
    probe.set(None);
    assert_eq!(writer.persist(), Err(StorageError::Barrier));
    assert_eq!(writer.read_current(), Ok(None));
    assert_eq!(writer.persist(), Ok(()));
    assert!(!dir.vault().exists());
}
