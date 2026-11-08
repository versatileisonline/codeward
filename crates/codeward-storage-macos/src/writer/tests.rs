use super::*;
use crate::test_support::{MountFault, MountProbe, TestDir, fixture, stages};
use std::{
    fs::{self, OpenOptions, Permissions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    path::Path,
};

mod capacity;
mod workflow;

fn lease(dir: &TestDir) -> ExperimentalWriterLease {
    ExperimentalStorage::from_path(VaultPath::resolve(&dir.vault()).unwrap())
        .acquire()
        .unwrap()
}

#[test]
fn managed_writer_uses_only_named_vault_and_preserves_all_leftovers() {
    use codeward_core::{
        transaction::{CallerId, Committer, TransactionError},
        vault::{VaultError, begin_recovery},
    };

    let dir = TestDir::new();
    let vault = dir.managed_vault();
    let parent = vault.parent().unwrap();
    let caller = CallerId(7);
    let paper =
        include_str!("../../../codeward-core/tests/fixtures/authenticated-recovery.txt").trim();
    let storage = || ExperimentalStorage::from_path(VaultPath::managed_for_test(&dir.0).unwrap());
    let write_private = |path: &Path, bytes: &[u8]| {
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    };

    let higher_revision = crate::test_support::maximum_revision_fixture();
    assert_eq!(
        begin_recovery(&higher_revision, paper).unwrap().revision(),
        u64::MAX
    );
    let mut writer = storage().acquire().unwrap();
    writer.stage(&higher_revision).unwrap();
    drop(writer);
    let [complete] = stages(parent).try_into().ok().unwrap();
    let partial = parent.join(".codeward.stage-00000000000000000000000000000000");
    let linked = parent.join(".codeward.stage-11111111111111111111111111111111");
    let unrelated = parent.join("unrelated");
    write_private(&partial, b"synthetic partial stage");
    fs::write(&unrelated, b"synthetic unrelated file").unwrap();
    symlink("unrelated", &linked).unwrap();

    assert_eq!(
        Committer::open_locked(storage(), caller).err(),
        Some(TransactionError::ReloadRequired)
    );
    let (_, enrollment) = Committer::begin_create(storage(), caller).unwrap();
    assert_eq!(enrollment.committed_revision(), None);
    drop(enrollment);
    assert!(!vault.exists());

    let malformed = b"malformed synthetic live vault";
    write_private(&vault, malformed);
    assert!(matches!(
        Committer::open_locked(storage(), caller),
        Err(TransactionError::Vault(VaultError::Format(_)))
    ));
    assert_eq!(fs::read(&vault).unwrap(), malformed);

    let original = fixture();
    let mut tampered = original.clone();
    *tampered.last_mut().unwrap() ^= 1;
    fs::write(&vault, &tampered).unwrap();
    assert!(Committer::open_recovery(storage(), caller, paper).is_err());
    assert_eq!(fs::read(&vault).unwrap(), tampered);
    fs::write(&vault, &original).unwrap();
    let (_, reopened) = Committer::open_recovery(storage(), caller, paper).unwrap();
    assert_eq!(reopened.committed_revision(), Some(1));
    assert_eq!(fs::read(&vault).unwrap(), original);
    assert_eq!(fs::read(complete).unwrap(), higher_revision);
    assert_eq!(fs::read(partial).unwrap(), b"synthetic partial stage");
    assert_eq!(fs::read_link(linked).unwrap(), Path::new("unrelated"));
    assert_eq!(fs::read(unrelated).unwrap(), b"synthetic unrelated file");
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
