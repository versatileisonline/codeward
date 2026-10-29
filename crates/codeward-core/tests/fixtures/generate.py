"""Regenerate synthetic structural/AAD fixtures using only Python's standard library.

This is an independent encoding of docs/vault-format-v1.md, not an encrypted vault.
There are no real keys, passwords or credentials. Do not use fixture byte sequences
as production randomness. Normal Rust tests consume the checked-in hex files and
do not require Python.
"""

from pathlib import Path
import struct


def generate():
    header = bytearray()
    header += b"CODEWARD"
    header += struct.pack("<HH", 1, 1)
    header += bytes(range(16))
    header += struct.pack("<QII", 0x0102030405060708, 280, 32)
    header += bytes([1, 1, 0x13, 0])
    header += struct.pack("<III", 65536, 3, 4)
    header += bytes(range(0x10, 0x20))
    header += bytes(range(0x20, 0x38))
    header += bytes(range(0x40, 0x70))
    header += bytes([2, 2, 0, 0])
    header += bytes(range(0x70, 0x90))
    header += bytes(range(0x90, 0xA8))
    header += bytes(range(0xB0, 0xE0))
    header += bytes(range(0xE0, 0xF8))
    assert len(header) == 280

    common = b"CODEWARD" + struct.pack("<HH", 1, 1) + bytes(range(16))
    master_metadata = (
        bytes([1, 1, 0x13, 0])
        + struct.pack("<III", 65536, 3, 4)
        + bytes(range(0x10, 0x20))
        + bytes(range(0x20, 0x38))
    )
    recovery_metadata = (
        bytes([2, 2, 0, 0])
        + bytes(range(0x70, 0x90))
        + bytes(range(0x90, 0xA8))
    )
    fixtures = {
        "structural-v1.hex": header + bytes([0xA5] * 32),
        "master-aad.hex": b"Codeward/slot/v1\0" + common + master_metadata,
        "recovery-aad.hex": b"Codeward/slot/v1\0" + common + recovery_metadata,
        "payload-aad.hex": b"Codeward/payload/v1\0" + header,
        "recovery-info.hex": (
            b"Codeward/recovery-wrap/v1\0"
            + struct.pack("<HH", 1, 1)
            + bytes(range(16))
        ),
    }
    root = Path(__file__).parent
    for name, contents in fixtures.items():
        text = contents.hex()
        (root / name).write_text(
            "\n".join(text[i:i + 64] for i in range(0, len(text), 64)) + "\n"
        )


if __name__ == "__main__":
    generate()
