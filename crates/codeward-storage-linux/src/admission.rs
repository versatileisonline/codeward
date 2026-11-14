//! Bounded location policy decisions; filesystem observations remain native.

use std::path::Path;

pub(super) fn recognized_cloud_path(path: &Path) -> bool {
    path.components().any(|part| {
        matches!(
            part.as_os_str().to_str(),
            Some(
                "Dropbox"
                    | "OneDrive"
                    | "Nextcloud"
                    | "ownCloud"
                    | "Google Drive"
                    | "pCloudDrive"
                    | "MEGA"
                    | "Insync"
            )
        )
    })
}

pub(super) fn fixed_local_device(
    path: &Path,
    removable: &[u8],
    slaves_empty: bool,
    has_device: bool,
    nvme_transport: Option<&[u8]>,
) -> bool {
    let components: Vec<_> = path
        .components()
        .map(|part| part.as_os_str().as_encoded_bytes())
        .collect();
    let pci = components.iter().any(|part| part.starts_with(b"pci"));
    let ata = components.iter().any(|part| {
        part.strip_prefix(b"ata")
            .is_some_and(|number| !number.is_empty() && number.iter().all(u8::is_ascii_digit))
    });
    let direct_transport = nvme_transport == Some(b"pcie\n") || ata;
    path.starts_with("/sys/devices")
        && pci
        && direct_transport
        && !path.components().any(|part| {
            let component = part.as_os_str().as_encoded_bytes();
            component == b"virtual" || component.starts_with(b"usb")
        })
        && removable == b"0\n"
        && slaves_empty
        && has_device
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognized_cloud_roots_are_refused_as_ancestors() {
        for path in [
            "/home/user/Dropbox/data",
            "/home/user/OneDrive/data",
            "/home/user/Nextcloud/data",
            "/home/user/ownCloud/data",
            "/home/user/Google Drive/data",
            "/home/user/pCloudDrive/data",
            "/home/user/MEGA/data",
            "/home/user/Insync/data",
        ] {
            assert!(recognized_cloud_path(Path::new(path)), "{path}");
        }
        assert!(!recognized_cloud_path(Path::new("/home/user/.local/share")));
        assert!(!recognized_cloud_path(Path::new(
            "/home/user/Dropbox-not-a-root"
        )));
    }

    #[test]
    fn unknown_layered_usb_and_removable_devices_are_refused() {
        let disk = Path::new("/sys/devices/pci0000:00/0000:00:01.0/nvme/nvme0/nvme0n1");
        assert!(fixed_local_device(
            disk,
            b"0\n",
            true,
            true,
            Some(b"pcie\n")
        ));
        for transport in [None, Some(&b"tcp\n"[..]), Some(&b"rdma\n"[..])] {
            assert!(!fixed_local_device(disk, b"0\n", true, true, transport));
        }
        assert!(!fixed_local_device(
            Path::new("/sys/devices/platform/host4/session1/block/sda"),
            b"0\n",
            true,
            true,
            None
        ));
        assert!(fixed_local_device(
            Path::new("/sys/devices/pci0000:00/0000:00:01.0/ata1/host0/target0/block/sda"),
            b"0\n",
            true,
            true,
            None
        ));
        for (path, removable, empty, device) in [
            (disk, &b"1\n"[..], true, true),
            (disk, b"", true, true),
            (disk, b"0\n", false, true),
            (disk, b"0\n", true, false),
            (
                Path::new("/sys/devices/virtual/block/loop0"),
                b"0\n",
                true,
                true,
            ),
            (
                Path::new("/sys/devices/virtual/block/dm-0"),
                b"0\n",
                true,
                true,
            ),
            (
                Path::new("/sys/devices/pci0000:00/usb1/1-1/block/sda"),
                b"0\n",
                true,
                true,
            ),
            (Path::new("/tmp/untrusted/nvme0n1"), b"0\n", true, true),
        ] {
            assert!(
                !fixed_local_device(path, removable, empty, device, Some(b"pcie\n")),
                "{path:?}"
            );
        }
    }
}
