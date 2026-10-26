"""Entry point for the LavaTeX command: insert a new equation, or edit the
selected one."""

import uno

from . import settings as lsettings
from . import shapes
from .bridge import BridgeError, Core
from .dialog import EquationDialog

SUPPORTED = ("com.sun.star.presentation.PresentationDocument", "com.sun.star.drawing.DrawingDocument")


def _message(ctx, doc, text, kind="ERRORBOX"):
    toolkit = ctx.ServiceManager.createInstanceWithContext("com.sun.star.awt.Toolkit", ctx)
    parent = doc.getCurrentController().getFrame().getContainerWindow() if doc else None
    box = toolkit.createMessageBox(parent, uno.Enum("com.sun.star.awt.MessageBoxType", kind), 1, "LavaTeX", text)
    box.execute()


def run(ctx):
    desktop = ctx.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
    doc = desktop.getCurrentComponent()
    if doc is None or not any(doc.supportsService(s) for s in SUPPORTED):
        _message(ctx, doc, "LavaTeX works in Impress and Draw documents.")
        return

    settings = lsettings.load()
    core = Core(settings.get("corePath", ""))
    if not core.available():
        _message(ctx, doc, "The LavaTeX core (lavatex) was not found for this computer. "
                           "Install a LavaTeX package built for your system, or put lavatex on PATH.")
        return

    shape = shapes.selected_shape(doc)
    record = None
    if shapes.is_graphic(shape):
        record, _ = shapes.read_metadata(ctx, shape, core)
    existing = shape if record else None

    try:
        toolchains = core.detect()
    except BridgeError:
        toolchains = []

    window = doc.getCurrentController().getFrame().getContainerWindow()
    result, options = EquationDialog(ctx, window, core, settings, toolchains, record).run()

    # Remember choices for next time, and share TeX/engine/format with other hosts.
    changed_shared = any(options[k] != settings.get(k) for k in ("toolchain", "engine", "format"))
    settings.update(options)
    lsettings.save(settings)
    if changed_shared:
        try:
            core.save_defaults(options["toolchain"], options["engine"], options["format"])
        except BridgeError:
            pass

    if result:
        shapes.place(ctx, doc, result, existing=existing)
