"""SYNTHETIC full-envelope reference fixtures, never production cryptography.

The test-only XChaCha20-Poly1305 model below uses Python standard-library integer
arithmetic; published HChaCha/AEAD vectors are checked before any fixture is emitted.
Argon2 uses the existing argon2-cffi C implementation. Runtime Rust tests need only
checked-in hex. No real credentials or production randomness are used here.
Reference: draft-irtf-cfrg-xchacha-03 section 2.2.1 and appendix A.1.
"""
from pathlib import Path
import hashlib
import hmac
import struct
from argon2.low_level import Type, hash_secret_raw

ROOT = Path(__file__).parent
MASK = (1 << 32) - 1


def rounds(words):
    state = list(words)
    def quarter(a, b, c, d):
        for left, right, rotate in [(a, d, 16), (c, b, 12), (a, d, 8), (c, b, 7)]:
            addend = b if left == a else d
            state[left] = (state[left] + state[addend]) & MASK
            value = state[right] ^ state[left]
            state[right] = ((value << rotate) | (value >> (32 - rotate))) & MASK
    for _ in range(10):
        for indices in [(0,4,8,12),(1,5,9,13),(2,6,10,14),(3,7,11,15),
                        (0,5,10,15),(1,6,11,12),(2,7,8,13),(3,4,9,14)]:
            quarter(*indices)
    return state


def hchacha(key, nonce):
    state = rounds(struct.unpack('<4I', b'expand 32-byte k') + struct.unpack('<8I', key) + struct.unpack('<4I', nonce))
    return struct.pack('<8I', *(state[:4] + state[12:]))


def block(key, nonce, counter):
    initial = struct.unpack('<4I', b'expand 32-byte k') + struct.unpack('<8I', key) + (counter,) + struct.unpack('<3I', nonce)
    return struct.pack('<16I', *((a + b) & MASK for a, b in zip(initial, rounds(initial))))


def seal(key, nonce, aad, plaintext):
    subkey = hchacha(key, nonce[:16])
    short_nonce = bytes(4) + nonce[16:]
    stream = b''.join(block(subkey, short_nonce, counter) for counter in range(1, (len(plaintext) + 63) // 64 + 1))
    ciphertext = bytes(a ^ b for a, b in zip(plaintext, stream))
    poly_key = block(subkey, short_nonce, 0)[:32]
    r = int.from_bytes(poly_key[:16], 'little') & 0x0ffffffc0ffffffc0ffffffc0fffffff
    s = int.from_bytes(poly_key[16:], 'little')
    message = aad + bytes((-len(aad)) % 16) + ciphertext + bytes((-len(ciphertext)) % 16) + struct.pack('<QQ', len(aad), len(ciphertext))
    accumulator = 0
    for start in range(0, len(message), 16):
        chunk = message[start:start + 16]
        accumulator = ((accumulator + int.from_bytes(chunk, 'little') + (1 << (8 * len(chunk)))) * r) % ((1 << 130) - 5)
    tag = ((accumulator + s) % (1 << 128)).to_bytes(16, 'little')
    return ciphertext + tag


def verify_vectors():
    assert hchacha(bytes(range(32)), bytes.fromhex('000000090000004a0000000031415927')).hex() == '82413b4227b27bfed30e42508a877d73a0f9e4d58a74a853c12ec41326d3ecdc'
    plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it."
    expected = bytes.fromhex('bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b4522f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff921f9664c97637da9768812f615c68b13b52ec0875924c1c7987947deafd8780acf49')
    assert seal(bytes(range(0x80,0xA0)), bytes(range(0x40,0x58)), bytes.fromhex('50515253c0c1c2c3c4c5c6c7'), plaintext) == expected


def generate():
    verify_vectors()
    vault_id, data_key, recovery_secret = bytes(range(16)), bytes(range(0x30,0x50)), bytes(range(32))
    master_salt, master_nonce = bytes(range(0x10,0x20)), bytes(range(0x20,0x38))
    recovery_salt, recovery_nonce = bytes(range(0x70,0x90)), bytes(range(0x90,0xA8))
    master_key = hash_secret_raw(b'synthetic password', master_salt, 3, 65536, 4, 32, Type.ID, 19)
    info = b'Codeward/recovery-wrap/v1\0' + struct.pack('<HH', 1, 1) + vault_id
    prk = hmac.digest(recovery_salt, recovery_secret, 'sha256')
    recovery_key = hmac.digest(prk, info + b'\x01', 'sha256')
    common = b'CODEWARD' + struct.pack('<HH', 1, 1) + vault_id
    master_metadata = bytes([1,1,0x13,0]) + struct.pack('<III',65536,3,4) + master_salt + master_nonce
    recovery_metadata = bytes([2,2,0,0]) + recovery_salt + recovery_nonce
    master_slot = seal(master_key, master_nonce, b'Codeward/slot/v1\0' + common + master_metadata, data_key)
    recovery_slot = seal(recovery_key, recovery_nonce, b'Codeward/slot/v1\0' + common + recovery_metadata, data_key)
    for index, (name, revision, plaintext) in enumerate([
            ('authenticated-v1.hex', 1, (ROOT/'credentials-v1.json').read_bytes()),
            ('authenticated-invalid-schema.hex', 1, b'{"schema":1,"schema":1,"credentials":[]}'),
            ('authenticated-max-revision.hex', (1 << 64) - 1, (ROOT/'credentials-v1.json').read_bytes())]):
        payload_nonce = bytes((b + index) % 256 for b in range(0xE0,0xF8))
        header = common + struct.pack('<QII', revision, 280, len(plaintext) + 16) + master_metadata + master_slot + recovery_metadata + recovery_slot + payload_nonce
        assert len(header) == 280
        envelope = header + seal(data_key, payload_nonce, b'Codeward/payload/v1\0' + header, plaintext)
        text = envelope.hex()
        (ROOT/name).write_text('\n'.join(text[i:i+64] for i in range(0,len(text),64)) + '\n')
    checksum = hashlib.sha256(b'Codeward/recovery-check/v1\0' + recovery_secret).digest()[:5]
    import base64
    code = base64.b32encode(recovery_secret).decode().rstrip('=') + base64.b32encode(checksum).decode()
    (ROOT/'authenticated-recovery.txt').write_text('-'.join(code[i:i+4] for i in range(0,60,4)) + '\n')


if __name__ == '__main__':
    generate()
