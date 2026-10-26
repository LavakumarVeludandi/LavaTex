"""End-to-end test against a real (headless) LibreOffice.

Inserts equations in SVG, PDF and PNG, saves as ODP and PPTX, reopens, and
checks that each metadata layer survives and that editing keeps the user's
scale and position.

    python3 hosts/libreoffice/tests/test_headless.py [--core target/debug/lavatex]
"""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "pythonpath"))

import uno  # noqa: E402
from com.sun.star.awt import Size  # noqa: E402

from lavatex_lo import bridge, shapes  # noqa: E402
from lavatex_lo import meta as lmeta  # noqa: E402

FILTERS = {"odp": "impress8", "pptx": "Impress MS PowerPoint 2007 XML"}


def connect(port, profile):
    proc = subprocess.Popen(
        [
            "soffice",
            "--headless",
            "--invisible",
            "--norestore",
            "--nologo",
            "-env:UserInstallation=" + uno.systemPathToFileUrl(profile),
            "--accept=socket,host=127.0.0.1,port=%d;urp;" % port,
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    local = uno.getComponentContext()
    resolver = local.ServiceManager.createInstanceWithContext("com.sun.star.bridge.UnoUrlResolver", local)
    for _ in range(120):
        try:
            ctx = resolver.resolve("uno:socket,host=127.0.0.1,port=%d;urp;StarOffice.ComponentContext" % port)
            return proc, ctx
        except Exception:
            time.sleep(0.5)
    proc.kill()
    raise RuntimeError("LibreOffice did not start")


def prop(name, value):
    return shapes._prop(name, value)


def find_equations(doc):
    page = doc.getDrawPages().getByIndex(0)
    return [page.getByIndex(i) for i in range(page.getCount()) if shapes.is_graphic(page.getByIndex(i))]


def settled_equations(doc, timeout=10.0):
    """Headless LibreOffice can expose a transient, empty graphic shape for a
    moment after loading while images are imported on a worker thread. Wait
    until the page's graphic shapes are fully populated."""
    deadline = time.time() + timeout
    while True:
        eqs = find_equations(doc)
        if all(s.Description is not None for s in eqs) or time.time() > deadline:
            return eqs
        time.sleep(0.2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--core", default=os.path.join(HERE, "..", "..", "..", "target", "debug", "lavatex"))
    ap.add_argument("--port", type=int, default=2099)
    args = ap.parse_args()

    core = bridge.Core(os.path.abspath(args.core))
    assert core.available(), "lavatex binary not found: %s" % args.core
    work = tempfile.mkdtemp(prefix="lavatex-lo-test-")
    proc, ctx = connect(args.port, os.path.join(work, "profile"))
    failures = []

    def check(cond, msg):
        print(("PASS " if cond else "FAIL ") + msg)
        if not cond:
            failures.append(msg)

    try:
        desktop = ctx.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
        for fmt in ("svg", "pdf", "png"):
            doc = desktop.loadComponentFromURL("private:factory/simpress", "_blank", 0, (prop("Hidden", True),))
            source = r"\sum_{k=1}^{n} k = \frac{n(n+1)}{2} \quad \text{(%s)}" % fmt
            result = core.render({"source": source, "format": fmt, "fontSize": 28, "color": "#003399"})
            check(result.get("ok"), "%s: render" % fmt)
            if not result.get("ok"):
                print(result)
                doc.close(True)
                continue
            shape = shapes.place(ctx, doc, result)
            record, where = shapes.read_metadata(ctx, shape, core)
            check(record and record["source"] == source and where == "attributes", "%s: metadata after insert (%s)" % (fmt, where))
            check(abs(shape.Size.Width - result["widthBp"] * shapes.HMM_PER_BP) < 5, "%s: inserted at rendered size" % fmt)

            # Simulate the user doubling the size and moving it, then edit.
            shape.Size = Size(shape.Size.Width * 2, shape.Size.Height * 2)
            pos = shape.Position
            new_source = source.replace("n(n+1)", "n(n+1) + 0")
            result2 = core.render({"source": new_source, "format": fmt, "fontSize": 28, "color": "#003399"})
            edited = shapes.place(ctx, doc, result2, existing=shape)
            expected_w = result2["widthBp"] * shapes.HMM_PER_BP * 2
            check(abs(edited.Size.Width - expected_w) < 10, "%s: edit keeps the user's 2x scale" % fmt)
            check(edited.Position.X == pos.X and edited.Position.Y == pos.Y, "%s: edit keeps position" % fmt)
            check(len(find_equations(doc)) == 1, "%s: edit replaces instead of duplicating" % fmt)

            for ext, flt in FILTERS.items():
                path = os.path.join(work, "deck-%s.%s" % (fmt, ext))
                doc.storeToURL(uno.systemPathToFileUrl(path), (prop("FilterName", flt),))
                reopened = desktop.loadComponentFromURL(uno.systemPathToFileUrl(path), "_blank", 0, (prop("Hidden", True),))
                eqs = settled_equations(reopened)
                check(len(eqs) == 1, "%s/%s: picture present after reopen" % (fmt, ext))
                if eqs:
                    s = eqs[0]
                    layers = []
                    if shapes._read_attribute(s):
                        layers.append("attributes")
                    if lmeta.from_alt_text(s.Description):
                        layers.append("alt text")
                    exported = shapes._export_graphic(ctx, s, core)
                    if exported and core.inspect_file(exported):
                        layers.append("image")
                    try:
                        mime = s.Graphic.MimeType
                    except Exception:
                        mime = "?"
                    record, where = shapes.read_metadata(ctx, s, core)
                    check(record is not None and record["source"] == new_source,
                          "%s/%s: editable after reopen via %s (layers: %s; stored as %s)" % (fmt, ext, where, ", ".join(layers) or "none", mime))
                reopened.close(True)
            doc.close(True)
    finally:
        try:
            desktop.terminate()
        except Exception:
            pass
        time.sleep(1)
        proc.kill()
        shutil.rmtree(work, ignore_errors=True)

    print("\n%d failure(s)" % len(failures))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
