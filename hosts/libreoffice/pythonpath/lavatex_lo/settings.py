"""Per-user dialog settings for the LibreOffice extension.

The TeX installation, engine and output format are also written to the shared
LavaTeX config (via ``lavatex config``) so every host uses the same choice.
"""

import json
import os
import sys

DEFAULT_PREAMBLE = "\\usepackage{amsmath}\n\\usepackage{amssymb}"

DEFAULTS = {
    "mode": "display",
    "fontSize": 20.0,
    "color": "#000000",
    "format": "svg",
    "pngDpi": 300,
    "preamble": DEFAULT_PREAMBLE,
    "toolchain": "",
    "engine": "",
    "corePath": "",
}


def config_dir():
    override = os.environ.get("LAVATEX_CONFIG_DIR")
    if override:
        return override
    if sys.platform.startswith("win"):
        return os.path.join(os.environ.get("APPDATA", os.path.expanduser("~")), "LavaTeX")
    if sys.platform == "darwin":
        return os.path.expanduser("~/Library/Application Support/LavaTeX")
    base = os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config")
    return os.path.join(base, "lavatex")


def _path():
    return os.path.join(config_dir(), "libreoffice.json")


def load():
    settings = dict(DEFAULTS)
    try:
        with open(_path(), encoding="utf-8") as fh:
            stored = json.load(fh)
        settings.update({k: v for k, v in stored.items() if k in DEFAULTS})
    except (OSError, ValueError):
        pass
    return settings


def save(settings):
    try:
        os.makedirs(config_dir(), exist_ok=True)
        with open(_path(), "w", encoding="utf-8") as fh:
            json.dump({k: settings.get(k, v) for k, v in DEFAULTS.items()}, fh, indent=2)
    except OSError:
        pass
