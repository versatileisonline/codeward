use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
};

use codeward_core::{
    credentials::{Credential, CredentialInput, CredentialSnapshot},
    envelope::{HEADER_LENGTH, TAG_LENGTH},
    transaction::{CallerId, Committer, OperationStatus, Phase, Request, RequestId},
    vault::unlock_with_master,
};

use super::*;
use crate::{native, test_support::MASTER_TESTS};

const MIB: u64 = 1024 * 1024;
const IMAGE_BYTES: u64 = 160 * MIB;
const OWNER: CallerId = CallerId(7);
const PASSWORD: &str = "synthetic password";

fn new_file(path: &Path) -> File {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
}

fn fill_to_enospc(file: &mut File, volume_size: u64, chunk: &[u8]) {
    // The total writes are capped even if the actual filesystem never reports full.
    for _ in 0..volume_size.div_ceil(chunk.len() as u64) {
        if let Err(error) = file.write_all(chunk) {
            assert_eq!(error.raw_os_error(), Some(libc::ENOSPC));
            return;
        }
    }
    panic!("disposable APFS volume did not report ENOSPC within its capacity");
}

fn large_snapshot() -> CredentialSnapshot {
    let notes = "x".repeat(65_536);
    let credentials = (0..128)
        .map(|id| {
            Credential::new(CredentialInput {
                id: &format!("{id:032x}"),
                title: "Synthetic capacity record",
                username: "",
                password: "",
                url: "",
                notes: &notes,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap()
        })
        .collect();
    CredentialSnapshot::new(credentials).unwrap()
}

#[test]
#[ignore = "requires the isolated APFS image from tests/run_capacity_test.py"]
fn real_apfs_disk_full_preserves_base_and_requires_explicit_retry() {
    let _master = MASTER_TESTS.lock().unwrap();
    let volume = std::env::var_os("CODEWARD_CAPACITY_VOLUME").expect("use the APFS image runner");
    let volume = Path::new(&volume);
    let root = volume.parent().expect("disposable mount parent");
    assert_eq!(volume, root.join("Library"));
    assert_eq!(volume, volume.canonicalize().unwrap());
    assert_eq!(
        root.parent(),
        Some(std::env::temp_dir().canonicalize().unwrap().as_path())
    );
    assert!(
        root.file_name()
            .is_some_and(|name| name.as_encoded_bytes().starts_with(b"codeward-capacity-"))
    );
    let mounted = File::open(volume).unwrap();
    let device = mounted.metadata().unwrap().dev();
    assert_ne!(device, fs::metadata(root).unwrap().dev());
    let stats = native::filesystem_stats(&mounted).unwrap();
    let volume_size = stats
        .f_blocks
        .checked_mul(u64::from(stats.f_bsize))
        .unwrap();
    assert!((128 * MIB..=IMAGE_BYTES).contains(&volume_size));
    let admitted_before = native::local_writable_apfs(&mounted).unwrap();
    let mount_probe = MountProbe::new();
    mount_probe.allow_capacity_image(device);
    assert!(native::local_writable_apfs(&mounted).unwrap());

    // A mount above the account home is allowed. A mount in its app-data tree
    // is refused before creation can proceed on that second device.
    assert_eq!(
        VaultPath::managed_for_test(root).err(),
        Some(NativeError::Unsupported)
    );
    assert!(!volume.join("Application Support").exists());
    let home = volume.join("home");
    DirBuilder::new().mode(0o700).create(&home).unwrap();
    let storage = || ExperimentalStorage::from_path(VaultPath::managed_for_test(&home).unwrap());
    let initial = storage();
    let vault = home.join("Library/Application Support/Codeward/Vaults/vault.cw");
    let original = fixture();
    new_file(&vault).write_all(&original).unwrap();
    let (mut session, mut worker) = Committer::open_master(initial, OWNER, PASSWORD).unwrap();

    let snapshot = large_snapshot();
    let expected = snapshot.encode().unwrap();
    let candidate_len = expected.len() + HEADER_LENGTH + TAG_LENGTH;
    let reserve = volume.join("codeward-reserve");
    let filler = volume.join("codeward-filler");
    let chunk = vec![0xa5; MIB as usize];
    let mut reserve_file = new_file(&reserve);
    for _ in 0..3 {
        reserve_file.write_all(&chunk).unwrap();
    }
    reserve_file.sync_all().unwrap();
    drop(reserve_file);
    fill_to_enospc(&mut new_file(&filler), volume_size, &chunk);
    fs::remove_file(&reserve).unwrap();
    let stats = native::filesystem_stats(&mounted).unwrap();
    let available = stats
        .f_bavail
        .checked_mul(u64::from(stats.f_bsize))
        .unwrap();
    assert!(available > 0 && available < candidate_len as u64);

    let request = Request {
        id: RequestId([1; 16]),
        expected_revision: 1,
    };
    let operation = session
        .prepare_contents(&mut worker, OWNER, request, &snapshot)
        .unwrap();
    let failed = OperationStatus::Failed(StorageError::Write);
    assert_eq!(worker.advance(), Some(failed));
    assert_eq!(worker.committed_revision(), Some(1));
    assert_eq!(fs::read(&vault).unwrap(), original);
    let [stage] = stages(vault.parent().unwrap()).try_into().ok().unwrap();
    let partial = fs::read(&stage).unwrap();
    assert!(partial.len() < candidate_len);
    assert_eq!(fs::metadata(&stage).unwrap().mode() & 0o777, 0o600);

    // Release only the test's filler. An identical request reports its failed
    // status, while a new explicit request can commit with a fresh candidate.
    fs::remove_file(filler).unwrap();
    let duplicate = session
        .prepare_contents(&mut worker, OWNER, request, &snapshot)
        .unwrap();
    assert_eq!(worker.status(OWNER, &operation), Ok(failed));
    assert_eq!(worker.status(OWNER, &duplicate), Ok(failed));
    assert!(worker.advance().is_none());
    let retry = Request {
        id: RequestId([2; 16]),
        ..request
    };
    session
        .prepare_contents(&mut worker, OWNER, retry, &snapshot)
        .unwrap();
    for phase in [Phase::Staged, Phase::Admitted, Phase::Published] {
        assert_eq!(worker.advance(), Some(OperationStatus::Pending(phase)));
        assert_eq!(worker.committed_revision(), Some(1));
    }
    assert_eq!(
        worker.advance(),
        Some(OperationStatus::Committed { revision: 2 })
    );
    assert_eq!(
        stages(vault.parent().unwrap()),
        std::slice::from_ref(&stage)
    );
    assert_eq!(fs::read(stage).unwrap(), partial);
    drop((session, worker));

    let bytes = fs::read(vault).unwrap();
    let reopened = unlock_with_master(&bytes, PASSWORD).unwrap();
    assert_eq!(reopened.revision(), 2);
    assert!(*reopened.snapshot().encode().unwrap() == *expected);
    let paper =
        include_str!("../../../../codeward-core/tests/fixtures/authenticated-recovery.txt").trim();
    let (_, worker) = Committer::open_recovery(storage(), OWNER, paper).unwrap();
    assert_eq!(worker.committed_revision(), Some(2));
    drop((worker, mount_probe));
    assert_eq!(
        native::local_writable_apfs(&mounted).unwrap(),
        admitted_before
    );
    println!(
        "Observed kernel ENOSPC on {volume_size}-byte APFS; {available} bytes available for a {candidate_len}-byte candidate. Preserved the base and {length} staged bytes and committed an explicit retry.",
        length = partial.len()
    );
}
