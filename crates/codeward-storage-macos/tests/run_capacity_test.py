"""Run the opt-in macOS capacity case on a disposable, fixed-size APFS image."""

import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile


def hdiutil(*args):
    return subprocess.check_output(["/usr/bin/hdiutil", *args], timeout=60)


def attached_disk(image):
    # Match the exact image we created. Never detach a device supplied by a caller.
    for entry in plistlib.loads(hdiutil("info", "-plist"))["images"]:
        if entry.get("image-path") == str(image):
            for entity in entry["system-entities"]:
                disk = entity.get("dev-entry", "")
                if re.fullmatch(r"/dev/disk\d+", disk):
                    return disk
            raise RuntimeError("disposable image is attached without an identifiable disk")
    return None


def main():
    if sys.platform != "darwin":
        raise SystemExit("This test requires macOS and hdiutil.")
    root = Path(tempfile.mkdtemp(prefix="codeward-capacity-")).resolve()
    image = root / "capacity.dmg"
    mount = root / "Library"
    attach_attempted = False
    try:
        if shutil.disk_usage(root).free < 320 * 1024 * 1024:
            raise RuntimeError("the host needs at least 320 MiB free for the 160 MiB image")
        mount.mkdir(mode=0o700)
        hdiutil("create", "-size", "160m", "-fs", "APFS", "-type", "UDIF",
                "-volname", "CodewardCapacityTest", str(image))
        attach_attempted = True
        result = plistlib.loads(hdiutil("attach", "-mountpoint", str(mount), "-nobrowse",
                                        "-noautoopen", "-owners", "on", "-plist", str(image)))
        if not any(entry.get("mount-point") == str(mount) for entry in result["system-entities"]):
            raise RuntimeError("hdiutil did not mount the image at the disposable path")
        if not mount.is_mount() or mount.stat().st_dev == root.stat().st_dev:
            raise RuntimeError("disposable mount is not a separate filesystem")
        subprocess.run(
            ["cargo", "test", "-p", "codeward-storage-macos", "--lib", "--all-features",
             "--locked", "--offline",
             "writer::tests::capacity::real_apfs_disk_full_preserves_base_and_requires_explicit_retry",
             "--", "--ignored", "--exact", "--nocapture"],
            cwd=Path(__file__).resolve().parents[3],
            env={**os.environ, "CODEWARD_CAPACITY_VOLUME": str(mount)},
            check=True,
            timeout=240,
        )
    finally:
        try:
            disk = attached_disk(image) if attach_attempted else None
            if disk:
                hdiutil("detach", disk)
            if mount.is_mount():
                raise RuntimeError("disposable image is still mounted")
        except Exception:
            print(f"Preserved disposable capacity-test directory: {root}", file=sys.stderr)
            raise
        shutil.rmtree(root)


if __name__ == "__main__":
    main()
