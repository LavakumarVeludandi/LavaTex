"""Insert, find and update LavaTeX pictures in Impress / Draw documents.

Metadata is written to three places (see docs/metadata.md): the image bytes
(done by the core), the shape's Description (alt text) and the shape's
UserDefinedAttributes. Reading tries them in the reverse order.
"""

import os

import uno
from com.sun.star.awt import Point, Size

from . import meta as lmeta

ATTR_NAMESPACE = "https://lavatex.org/ns/1"
ATTR_NAME = "lavatex:equation"
SHAPE_NAME = "LavaTeX equation"
SHAPE_TITLE = "LaTeX equation"
HMM_PER_BP = 2540.0 / 72.0  # 1/100 mm per PostScript point

_EXPORT_MIME = {"svg": "image/svg+xml", "pdf": "application/pdf", "png": "image/png"}


def _prop(name, value):
    p = uno.createUnoStruct("com.sun.star.beans.PropertyValue")
    p.Name = name
    p.Value = value
    return p


def _graphic_provider(ctx):
    return ctx.ServiceManager.createInstanceWithContext("com.sun.star.graphic.GraphicProvider", ctx)


def selected_shape(doc):
    """The single selected shape, or None."""
    try:
        sel = doc.getCurrentController().getSelection()
    except Exception:
        return None
    if sel is None:
        return None
    if hasattr(sel, "getCount"):
        return sel.getByIndex(0) if sel.getCount() == 1 else None
    if hasattr(sel, "supportsService") and sel.supportsService("com.sun.star.drawing.Shape"):
        return sel
    return None


_GRAPHIC_TYPES = ("com.sun.star.drawing.GraphicObjectShape", "com.sun.star.presentation.GraphicObjectShape")


def is_graphic(shape):
    # Presentation placeholders also claim the GraphicObjectShape service, so
    # check the concrete shape type and that a graphic is actually present.
    try:
        return shape is not None and shape.ShapeType in _GRAPHIC_TYPES and shape.Graphic is not None
    except Exception:
        return False


def _read_attribute(shape):
    try:
        attrs = shape.UserDefinedAttributes
    except Exception:
        return None
    if attrs is not None and attrs.hasByName(ATTR_NAME):
        return lmeta.from_json(attrs.getByName(ATTR_NAME).Value)
    return None


def _export_graphic(ctx, shape, core):
    """Write the shape's original image bytes to a temp file; return its path."""
    graphic = shape.Graphic
    if graphic is None:
        return None
    mime = ""
    try:
        mime = graphic.MimeType or ""
    except Exception:
        pass
    ext = {"image/svg+xml": "svg", "application/pdf": "pdf", "image/png": "png"}.get(mime)
    if ext is None:
        return None
    path = core.scratch_path(ext)
    try:
        _graphic_provider(ctx).storeGraphic(
            graphic, (_prop("URL", uno.systemPathToFileUrl(path)), _prop("MimeType", mime))
        )
    except Exception:
        return None
    return path if os.path.isfile(path) else None


def read_metadata(ctx, shape, core=None):
    """Return (record, where) for a LavaTeX picture, or (None, None)."""
    if not is_graphic(shape):
        return None, None
    record = _read_attribute(shape)
    if record:
        return record, "attributes"
    try:
        record = lmeta.from_alt_text(shape.Description)
    except Exception:
        record = None
    if record:
        return record, "alt text"
    if core is not None and core.available():
        path = _export_graphic(ctx, shape, core)
        if path:
            record = core.inspect_file(path)
            if lmeta.validate(record):
                return record, "image"
    return None, None


def _write_metadata(shape, record, alt_text):
    shape.Name = SHAPE_NAME
    shape.Title = SHAPE_TITLE
    shape.Description = alt_text
    try:
        import json

        attrs = shape.UserDefinedAttributes
        data = uno.createUnoStruct("com.sun.star.xml.AttributeData")
        data.Namespace = ATTR_NAMESPACE
        data.Type = "CDATA"
        data.Value = json.dumps(record, ensure_ascii=False, separators=(",", ":"))
        if attrs.hasByName(ATTR_NAME):
            attrs.replaceByName(ATTR_NAME, data)
        else:
            attrs.insertByName(ATTR_NAME, data)
        shape.UserDefinedAttributes = attrs
    except Exception:
        # Not every shape/version supports this; alt text and image still carry the record.
        pass


def _size_hmm(result, scale=1.0):
    w = max(1, int(round(result["widthBp"] * HMM_PER_BP * scale)))
    h = max(1, int(round(result["heightBp"] * HMM_PER_BP * scale)))
    return w, h


class PlacementError(Exception):
    """LibreOffice refused the rendered picture."""

    def __init__(self, summary, details=""):
        super().__init__(summary)
        self.summary = summary
        self.details = details


def _uno_message(exc):
    return getattr(exc, "Message", "") or str(exc) or exc.__class__.__name__


def load_graphic(ctx, path):
    """Load a picture from its bytes (more robust than a file URL), falling
    back to the URL, and make sure LibreOffice actually understood it."""
    provider = _graphic_provider(ctx)
    errors = []
    try:
        with open(path, "rb") as fh:
            data = fh.read()
        stream = ctx.ServiceManager.createInstanceWithContext("com.sun.star.io.SequenceInputStream", ctx)
        stream.initialize((uno.ByteSequence(data),))
        graphic = provider.queryGraphic((_prop("InputStream", stream),))
        if graphic is not None:
            return graphic
        errors.append("loading from bytes returned nothing")
    except Exception as exc:
        errors.append("loading from bytes: " + _uno_message(exc))
    try:
        graphic = provider.queryGraphic((_prop("URL", uno.systemPathToFileUrl(path)),))
        if graphic is not None:
            return graphic
        errors.append("loading from file returned nothing")
    except Exception as exc:
        errors.append("loading from file: " + _uno_message(exc))
    raise PlacementError("LibreOffice could not read the rendered %s picture." % os.path.splitext(path)[1][1:].upper(),
                         "; ".join(errors))


def _assign_graphic(shape, graphic):
    try:
        shape.Graphic = graphic
    except Exception as exc:
        raise PlacementError("LibreOffice refused the rendered picture.", _uno_message(exc))


def place(ctx, doc, result, existing=None):
    """Insert a rendered equation, or replace the picture in ``existing``.

    When replacing, the user's scaling relative to the original rendered size
    is kept (not the old box size, which would distort a longer equation),
    along with the top-left position and rotation.

    Raises PlacementError, leaving the slide unchanged, if LibreOffice does
    not accept the picture.
    """
    graphic = load_graphic(ctx, result["path"])
    controller = doc.getCurrentController()
    undo = None
    try:
        undo = doc.getUndoManager()
        undo.enterUndoContext("Edit LaTeX equation" if existing is not None else "Insert LaTeX equation")
    except Exception:
        undo = None
    try:
        if existing is not None:
            old, _ = read_metadata(ctx, existing)
            scale = 1.0
            if old and old.get("widthBp"):
                scale = existing.Size.Width / (old["widthBp"] * HMM_PER_BP)
                if not (0.01 < scale < 100):
                    scale = 1.0
            shape = existing
            pos = shape.Position
            _assign_graphic(shape, graphic)
            w, h = _size_hmm(result, scale)
            shape.Size = Size(w, h)
            shape.Position = pos
        else:
            page = controller.getCurrentPage()
            shape = doc.createInstance("com.sun.star.drawing.GraphicObjectShape")
            page.add(shape)
            try:
                _assign_graphic(shape, graphic)
            except PlacementError:
                page.remove(shape)  # never leave an empty frame behind
                raise
            w, h = _size_hmm(result)
            shape.Size = Size(w, h)
            shape.Position = Point(max(0, (page.Width - w) // 2), max(0, (page.Height - h) // 2))
        _write_metadata(shape, result["meta"], result["altText"])
    finally:
        if undo is not None:
            undo.leaveUndoContext()
    try:
        controller.select(shape)
    except Exception:
        pass
    return shape
