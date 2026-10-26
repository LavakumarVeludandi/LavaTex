"""LavaTeX equation metadata (see docs/metadata.md).

Only the alt-text form is parsed here; image bytes are inspected by the core.
"""

import base64
import json

SCHEMA = "lavatex-equation/1"
ALT_TEXT_MARKER = "lavatex:1:"
MAX_RECORD_BYTES = 256 * 1024

_B64URL = set("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_")


def validate(record):
    if not isinstance(record, dict) or record.get("schema") != SCHEMA:
        return None
    if not isinstance(record.get("source"), str):
        return None
    return record


def from_json(text):
    if not text or len(text) > MAX_RECORD_BYTES:
        return None
    try:
        return validate(json.loads(text))
    except ValueError:
        return None


def from_alt_text(text):
    if not text:
        return None
    pos = text.rfind(ALT_TEXT_MARKER)
    if pos < 0:
        return None
    encoded = []
    for ch in text[pos + len(ALT_TEXT_MARKER):]:
        if ch not in _B64URL:
            break
        encoded.append(ch)
    encoded = "".join(encoded)
    if len(encoded) > MAX_RECORD_BYTES * 4 // 3 + 4:
        return None
    try:
        raw = base64.urlsafe_b64decode(encoded + "=" * (-len(encoded) % 4))
        return from_json(raw.decode("utf-8"))
    except (ValueError, UnicodeDecodeError):
        return None
