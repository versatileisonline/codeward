"""Regenerate synthetic credential fixtures using only Python's standard library."""
import json
from pathlib import Path

ROOT = Path(__file__).parent
records = [
    {
        "id": "00000000000000000000000000000001",
        "title": "Synthetic service",
        "username": "  résumé  ",
        "password": ' A\0"\\🔒e\u0301 ',
        "url": " https://example.invalid/ ",
        "notes": "line one\r\nline two\t[]{}",
        "created_at": 0,
        "updated_at": 253402300799999,
    },
    {
        "id": "ffffffffffffffffffffffffffffffff",
        "title": "Second synthetic service",
        "username": "",
        "password": "",
        "url": "",
        "notes": "",
        "created_at": 1,
        "updated_at": 1,
    },
]
snapshot = {"schema": 1, "credentials": records}
for name, ascii_only in [("credentials-v1.json", False), ("credentials-escaped.json", True)]:
    (ROOT / name).write_text(json.dumps(snapshot, ensure_ascii=ascii_only, separators=(",", ":")), encoding="utf-8")
