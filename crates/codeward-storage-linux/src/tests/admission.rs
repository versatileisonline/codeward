use super::*;

struct BindMount(PathBuf);

impl BindMount {
    fn attach(source: &Path, destination: &Path) -> Self {
        assert_eq!(
            env::var("CODEWARD_LINUX_PRIVILEGED_MOUNT_TEST").as_deref(),
            Ok("1")
        );
        assert!(
            Command::new("sudo")
                .args(["-n", "mount", "--bind"])
                .arg(source)
                .arg(destination)
                .status()
                .unwrap()
                .success()
        );
        Self(destination.to_owned())
    }
}

impl Drop for BindMount {
    fn drop(&mut self) {
        assert!(
            Command::new("sudo")
                .args(["-n", "umount"])
                .arg(&self.0)
                .status()
                .unwrap()
                .success()
        );
    }
}

#[test]
#[ignore = "requires the disposable ext4 mount and explicit passwordless sudo mount opt-in"]
fn privileged_managed_submount_refuses_preparation_reads_and_publication() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source");
    fs::DirBuilder::new().mode(0o700).create(&source).unwrap();
    let location = managed::Location::prepare(&fixture.0, true, test_owner()).unwrap();
    let parent = fixture.0.join(".local/share/codeward/vaults");
    let lease = ReadLease::acquire(location).unwrap();
    lease
        .prepare_stage(b"base")
        .unwrap()
        .publish_create()
        .unwrap();
    lease.synchronize_namespace(Some(b"base")).unwrap();
    let stage = lease.prepare_stage(b"candidate").unwrap();
    let residue = fs::read_dir(&parent)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .find(|name| name.as_encoded_bytes().starts_with(b".codeward.stage-"))
        .unwrap();
    let mounted = BindMount::attach(&source, parent.parent().unwrap());
    assert_eq!(
        managed::Location::prepare(&fixture.0, true, test_owner()).err(),
        Some(NativeError::Unsupported)
    );
    assert_eq!(lease.read_current(), Err(NativeError::Unsupported));
    assert_eq!(
        stage.publish_replace(b"base"),
        Err(PublicationError::NotPublished(NativeError::Unsupported))
    );
    assert_eq!(
        lease.synchronize_namespace(Some(b"base")),
        Err(NativeError::Unsupported)
    );
    assert!(fs::read_dir(&source).unwrap().next().is_none());
    drop(mounted);
    assert_eq!(lease.read_current().unwrap().unwrap(), b"base");
    assert_eq!(fs::read(parent.join(residue)).unwrap(), b"candidate");
}
