#!/usr/bin/env python3
"""Build the distributable host packages into dist/.

    python3 tools/package.py libreoffice [--bin linux-x86_64=target/release/lavatex ...]
    python3 tools/package.py onlyoffice
    python3 tools/package.py all

Without --bin, the LibreOffice package bundles target/release/lavatex (or the
debug build) for the current platform. Pass several --bin options to build a
multi-platform .oxt (CI does this).
"""

import argparse
import json
import os
import platform
import sys
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DIST = os.path.join(ROOT, "dist")
sys.path.insert(0, os.path.join(ROOT, "hosts", "libreoffice", "pythonpath"))


def version():
    with open(os.path.join(ROOT, "Cargo.toml"), encoding="utf-8") as fh:
        for line in fh:
            if line.strip().startswith("version"):
                return line.split("=", 1)[1].strip().strip('"')
    return "0.0.0"


def add_tree(zf, src_dir, prefix="", skip=()):
    for base, dirs, files in os.walk(src_dir):
        dirs[:] = sorted(d for d in dirs if d not in ("__pycache__", "tests", "node_modules") and d not in skip)
        for name in sorted(files):
            if name.endswith(".pyc") or name in skip:
                continue
            path = os.path.join(base, name)
            rel = os.path.relpath(path, src_dir).replace(os.sep, "/")
            zf.write(path, prefix + rel)


def default_binary():
    exe = "lavatex.exe" if sys.platform.startswith("win") else "lavatex"
    for profile in ("release", "debug"):
        path = os.path.join(ROOT, "target", profile, exe)
        if os.path.isfile(path):
            return path
    return None


def build_libreoffice(bins):
    from lavatex_lo.bridge import platform_tag

    src = os.path.join(ROOT, "hosts", "libreoffice")
    if not bins:
        binary = default_binary()
        if binary is None:
            sys.exit("No lavatex binary found; run `cargo build --release` first or pass --bin.")
        bins = ["%s=%s" % (platform_tag(), binary)]
    os.makedirs(DIST, exist_ok=True)
    tags = [b.split("=", 1)[0] for b in bins]
    suffix = tags[0] if len(tags) == 1 else "multi"
    out = os.path.join(DIST, "lavatex-libreoffice-%s-%s.oxt" % (version(), suffix))
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as zf:
        add_tree(zf, src, skip=("LICENSE.txt",))
        zf.write(os.path.join(ROOT, "LICENSE"), "LICENSE.txt")
        for spec in bins:
            tag, path = spec.split("=", 1)
            name = "lavatex.exe" if tag.startswith("windows") else "lavatex"
            info = zipfile.ZipInfo("bin/%s/%s" % (tag, name))
            info.external_attr = 0o755 << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            with open(path, "rb") as fh:
                zf.writestr(info, fh.read())
    print(out)
    return out


def build_onlyoffice():
    src = os.path.join(ROOT, "hosts", "onlyoffice")
    with open(os.path.join(src, "config.json"), encoding="utf-8") as fh:
        json.load(fh)  # fail early on invalid JSON
    os.makedirs(DIST, exist_ok=True)
    out = os.path.join(DIST, "lavatex-onlyoffice-%s.plugin" % version())
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as zf:
        add_tree(zf, src, skip=("package.json",))
        zf.write(os.path.join(ROOT, "LICENSE"), "LICENSE")
    print(out)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("target", choices=["libreoffice", "onlyoffice", "all"])
    ap.add_argument("--bin", action="append", default=[], help="platform-tag=path/to/lavatex")
    args = ap.parse_args()
    if args.target in ("libreoffice", "all"):
        build_libreoffice(args.bin)
    if args.target in ("onlyoffice", "all"):
        build_onlyoffice()


if __name__ == "__main__":
    main()
