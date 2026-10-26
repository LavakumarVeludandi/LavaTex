"""Calls the LavaTeX command-line core (`lavatex`), which does all TeX work.

The extension ships the binary under ``bin/<platform>/``; a ``lavatex`` on
PATH or a path chosen in the settings is used otherwise.
"""

import json
import os
import platform
import shutil
import stat
import subprocess
import sys
import tempfile
import uuid

EXTENSION_DIR = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


class BridgeError(Exception):
    """The core could not be started or returned something unexpected."""

    def __init__(self, summary, details=""):
        super().__init__(summary)
        self.summary = summary
        self.details = details


def platform_tag():
    machine = platform.machine().lower()
    arch = {"amd64": "x86_64", "x64": "x86_64", "arm64": "aarch64"}.get(machine, machine)
    if sys.platform.startswith("win"):
        system = "windows"
    elif sys.platform == "darwin":
        system = "macos"
    else:
        system = "linux"
    return "%s-%s" % (system, arch)


def _exe(name):
    return name + ".exe" if sys.platform.startswith("win") else name


def find_core(configured=""):
    """Return the path of the lavatex executable, or None."""
    candidates = []
    if configured:
        candidates.append(configured)
    candidates.append(os.path.join(EXTENSION_DIR, "bin", platform_tag(), _exe("lavatex")))
    on_path = shutil.which("lavatex")
    if on_path:
        candidates.append(on_path)
    home = os.path.expanduser("~")
    candidates.append(os.path.join(home, ".cargo", "bin", _exe("lavatex")))
    for path in candidates:
        if path and os.path.isfile(path):
            if not sys.platform.startswith("win") and not os.access(path, os.X_OK):
                # Extension packages do not always keep the executable bit.
                try:
                    os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
                except OSError:
                    continue
            return path
    return None


class Core:
    def __init__(self, configured_path=""):
        self.path = find_core(configured_path)
        self.workdir = tempfile.mkdtemp(prefix="lavatex-lo-")

    def available(self):
        return self.path is not None

    def _run(self, args, stdin_text=None, timeout=150):
        if not self.path:
            raise BridgeError(
                "The LavaTeX core (lavatex) was not found.",
                "This package does not include a lavatex binary for %s. Install lavatex and make sure "
                "it is on PATH, or set its location in the LavaTeX settings." % platform_tag(),
            )
        kwargs = {}
        if sys.platform.startswith("win"):
            kwargs["creationflags"] = 0x08000000  # CREATE_NO_WINDOW
        try:
            proc = subprocess.run(
                [self.path] + list(args),
                input=stdin_text,
                capture_output=True,
                text=True,
                encoding="utf-8",
                timeout=timeout,
                **kwargs
            )
        except subprocess.TimeoutExpired:
            raise BridgeError("The LavaTeX core did not respond in time.")
        except OSError as exc:
            raise BridgeError("Could not start the LavaTeX core.", str(exc))
        return proc

    def detect(self):
        proc = self._run(["detect", "--json"], timeout=60)
        try:
            return json.loads(proc.stdout or "[]")
        except ValueError:
            raise BridgeError("Could not list TeX installations.", proc.stderr)

    def render(self, request):
        """Render; returns the parsed JSON result (``ok`` true or false)."""
        ext = request.get("format", "svg")
        out = os.path.join(self.workdir, "eq-%s.%s" % (uuid.uuid4().hex[:12], ext))
        proc = self._run(["render", "--request", "-", "--json", "-o", out], json.dumps(request))
        try:
            return json.loads(proc.stdout)
        except ValueError:
            raise BridgeError("The LavaTeX core returned an unexpected answer.", (proc.stderr or proc.stdout)[-2000:])

    def inspect_file(self, path):
        proc = self._run(["inspect", path], timeout=30)
        if proc.returncode != 0:
            return None
        try:
            return json.loads(proc.stdout)
        except ValueError:
            return None

    def save_defaults(self, toolchain=None, engine=None, fmt=None):
        args = ["config"]
        if toolchain is not None:
            args += ["--toolchain", toolchain or "auto"]
        if engine is not None:
            args += ["--engine", engine or "auto"]
        if fmt is not None:
            args += ["--format", fmt]
        self._run(args, timeout=60)

    def scratch_path(self, ext):
        return os.path.join(self.workdir, "graphic-%s.%s" % (uuid.uuid4().hex[:12], ext))
