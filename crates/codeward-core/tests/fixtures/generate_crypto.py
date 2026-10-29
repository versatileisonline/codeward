"""Regenerate SYNTHETIC key/checksum fixtures, never production secrets.

Argon2 uses the independently implemented C library via an existing argon2-cffi
installation. HKDF, Base32 and checksum use Python's standard library. Normal
Rust tests consume checked-in outputs and need no Python packages.
"""

from pathlib import Path
import base64
import hashlib
import hmac

from argon2.low_level import Type, hash_secret_raw


def generate():
    root = Path(__file__).parent
    passwords = ["synthetic password", " synthetic password ", "Synthetic password", "é", "e\u0301"]
    master = []
    for password in passwords:
        derived = hash_secret_raw(password.encode(), bytes(range(16)), 3, 65536, 4, 32, Type.ID, 19)
        master.append(derived.hex())
    (root / "master-keys.hex").write_text("\n".join(master) + "\n")

    secret, salt, vault_id = bytes(range(32)), bytes(range(32, 64)), bytes(range(16))
    info = b"Codeward/recovery-wrap/v1\0" + bytes([1, 0, 1, 0]) + vault_id
    prk = hmac.digest(salt, secret, "sha256")
    derived = hmac.digest(prk, info + b"\x01", "sha256")
    (root / "recovery-key.hex").write_text(derived.hex() + "\n")

    codes = []
    for secret in [bytes(32), bytes(range(32)), bytes([255] * 32)]:
        checksum = hashlib.sha256(b"Codeward/recovery-check/v1\0" + secret).digest()[:5]
        text = base64.b32encode(secret).decode().rstrip("=") + base64.b32encode(checksum).decode()
        codes.append("-".join(text[i:i+4] for i in range(0, 60, 4)))
    (root / "recovery-codes.txt").write_text("\n".join(codes) + "\n")


if __name__ == "__main__":
    generate()
