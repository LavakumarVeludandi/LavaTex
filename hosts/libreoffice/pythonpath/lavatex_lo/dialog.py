"""The LavaTeX equation dialog, built with the UNO dialog toolkit."""

import json

import uno
import unohelper
from com.sun.star.awt import Selection, XActionListener

from .bridge import BridgeError

MODES = [("display", "Display"), ("inline", "Inline"), ("text", "Text (raw LaTeX)")]
FORMATS = [("svg", "SVG (vector, recommended)"), ("pdf", "PDF (vector)"), ("png", "PNG (bitmap)")]


def available_formats(pdf_supported):
    return [f for f in FORMATS if f[0] != "pdf" or pdf_supported]
ENGINES = ["", "latex", "pdflatex", "xelatex", "lualatex", "tectonic"]

W = 400  # dialog width in app-font units
M = 6  # margin


class _Action(unohelper.Base, XActionListener):
    def __init__(self, fn):
        self.fn = fn

    def actionPerformed(self, event):
        self.fn()

    def disposing(self, event):
        pass


class EquationDialog:
    def __init__(self, ctx, parent_window, core, settings, toolchains, record=None, pdf_supported=True):
        self.ctx = ctx
        self.core = core
        self.settings = settings
        self.toolchains = toolchains
        self.record = record
        self.formats = available_formats(pdf_supported)
        self.result = None
        self.error = None
        self._cache_key = None
        self._cache_result = None

        smgr = ctx.ServiceManager
        self.toolkit = smgr.createInstanceWithContext("com.sun.star.awt.Toolkit", ctx)
        self.model = smgr.createInstanceWithContext("com.sun.star.awt.UnoControlDialogModel", ctx)
        self.model.Width = W
        self.model.Height = 318
        self.model.Title = "LavaTeX - Edit LaTeX equation" if record else "LavaTeX - Insert LaTeX equation"
        self._build()
        self.dialog = smgr.createInstanceWithContext("com.sun.star.awt.UnoControlDialog", ctx)
        self.dialog.setModel(self.model)
        self.dialog.createPeer(self.toolkit, parent_window)
        self._populate()

    # --- layout -----------------------------------------------------------

    def _add(self, kind, name, x, y, w, h, **props):
        model = self.model.createInstance("com.sun.star.awt.UnoControl%sModel" % kind)
        model.PositionX, model.PositionY, model.Width, model.Height = x, y, w, h
        for key, value in props.items():
            setattr(model, key, value)
        self.model.insertByName(name, model)
        return model

    def _build(self):
        inner = W - 2 * M
        self._add("FixedText", "lblSource", M, 6, inner, 9, Label="~LaTeX source")
        self._add("Edit", "source", M, 16, inner, 72, MultiLine=True, VScroll=True, FontName="Monospace",
                  HelpText="Type the formula. End a line with \\\\ to start a new line; use & to line lines up (e.g. at =).")
        self._add("FixedText", "lblPreamble", M, 92, inner, 9, Label="~Preamble (packages and macros)")
        self._add("Edit", "preamble", M, 102, inner, 30, MultiLine=True, VScroll=True, FontName="Monospace")

        y = 138
        self._add("FixedText", "lblMode", M, y + 2, 26, 9, Label="~Mode")
        self._add("ListBox", "mode", M + 26, y, 70, 12, Dropdown=True, StringItemList=tuple(l for _, l in MODES))
        self._add("FixedText", "lblSize", 108, y + 2, 26, 9, Label="~Size (pt)")
        self._add("NumericField", "size", 136, y, 36, 12, Spin=True, ValueMin=1, ValueMax=500, DecimalAccuracy=1, StrictFormat=True)
        self._add("FixedText", "lblColor", 180, y + 2, 24, 9, Label="~Color")
        self._add("Edit", "color", 204, y, 40, 12, HelpText="#RRGGBB")
        self._add("FixedText", "lblFormat", 252, y + 2, 28, 9, Label="~Output")
        self._add("ListBox", "format", 280, y, 114, 12, Dropdown=True, StringItemList=tuple(l for _, l in self.formats))

        y = 156
        self._add("FixedText", "lblTex", M, y + 2, 26, 9, Label="~TeX")
        self._add("ListBox", "toolchain", M + 26, y, 196, 12, Dropdown=True)
        self._add("Button", "browse", 230, y, 40, 12, Label="Folder...", HelpText="Use the TeX installation in a folder you choose")
        self._add("FixedText", "lblEngine", 278, y + 2, 28, 9, Label="~Engine")
        self._add("ListBox", "engine", 306, y, 50, 12, Dropdown=True,
                  StringItemList=tuple("Automatic" if e == "" else e for e in ENGINES))
        self._add("FixedText", "lblDpi", 360, y + 2, 14, 9, Label="dpi")
        self._add("NumericField", "dpi", 374, y, 20, 12, ValueMin=36, ValueMax=2400, DecimalAccuracy=0, StrictFormat=True,
                  HelpText="PNG resolution")

        self._add("ImageControl", "preview", M, 174, inner, 96, ScaleImage=True, ScaleMode=1, Border=1,
                  BackgroundColor=0xFFFFFF)
        self._add("FixedText", "status", M, 274, inner - 50, 20, MultiLine=True, Label="")
        self._add("Button", "details", W - M - 46, 274, 46, 13, Label="Details...", Enabled=False)

        y = 300
        self._add("Button", "previewBtn", M, y, 56, 14, Label="~Preview")
        self._add("Button", "cancel", W - M - 120, y, 56, 14, Label="Cancel", PushButtonType=2)
        self._add("Button", "insert", W - M - 60, y, 60, 14, Label="~Update" if self.record else "~Insert",
                  DefaultButton=True)

    def _ctl(self, name):
        return self.dialog.getControl(name)

    def _populate(self):
        s = self.settings
        r = self.record or {}
        self._ctl("source").setText(r.get("source", ""))
        self._ctl("preamble").setText(r.get("preamble", s["preamble"]))
        self._select("mode", [m for m, _ in MODES], r.get("mode", s["mode"]))
        self._ctl("size").setValue(float(r.get("fontSize", s["fontSize"])))
        self._ctl("color").setText(r.get("color", s["color"]))
        wanted_format = r.get("format", s["format"])
        self._select("format", [f for f, _ in self.formats], wanted_format)
        self._select("engine", ENGINES, r.get("engine", s["engine"]) if r.get("engine") in ENGINES else s["engine"])
        self._ctl("dpi").setValue(float(s["pngDpi"]))
        self._fill_toolchains(s["toolchain"])

        for name, fn in (("previewBtn", self.on_preview), ("insert", self.on_insert),
                         ("details", self.on_details), ("browse", self.on_browse)):
            self._ctl(name).addActionListener(_Action(fn))

        # Notices stay visible above every later status message.
        notices = []
        if not self.toolchains:
            notices.append("No TeX installation was found. Install TeX Live, MiKTeX, MacTeX or Tectonic, "
                           "or choose the folder that contains latex with Folder...")
        if self.record and self.record.get("preamble", "") != s["preamble"]:
            notices.append("This equation carries its own preamble from the document; review it if the file "
                           "came from someone else.")
        if wanted_format == "pdf" and not any(f == "pdf" for f, _ in self.formats):
            notices.append("This LibreOffice cannot display PDF pictures, so SVG (also vector) is used.")
        self._notice = " ".join(notices)
        self._status("")
        if self.record:
            self.on_preview()

    def _select(self, name, values, value):
        try:
            idx = values.index(value)
        except ValueError:
            idx = 0
        self._ctl(name).selectItemPos(idx, True)

    def _fill_toolchains(self, chosen):
        self.toolchain_ids = [""]
        suggested = self.toolchains[0]["label"] if self.toolchains else "none found"
        labels = ["Automatic (%s)" % suggested]
        for tc in self.toolchains:
            self.toolchain_ids.append(tc["id"])
            labels.append("%s - %s" % (tc["label"], tc["binDir"]))
        if chosen and chosen not in self.toolchain_ids:
            self.toolchain_ids.append(chosen)
            labels.append(chosen)
        ctl = self._ctl("toolchain")
        ctl.removeItems(0, ctl.getItemCount())
        ctl.addItems(tuple(labels), 0)
        self._select("toolchain", self.toolchain_ids, chosen)

    # --- behaviour --------------------------------------------------------

    def _status(self, text, details=""):
        notice = getattr(self, "_notice", "")
        self.model.getByName("status").Label = (notice + "\n" + text).strip() if notice else text
        self._details = details
        self.model.getByName("details").Enabled = bool(details)

    def options(self):
        return {
            "mode": MODES[max(0, self._ctl("mode").getSelectedItemPos())][0],
            "fontSize": float(self._ctl("size").getValue()),
            "color": self._ctl("color").getText().strip() or "#000000",
            "format": self.formats[max(0, self._ctl("format").getSelectedItemPos())][0],
            "pngDpi": int(self._ctl("dpi").getValue()),
            "preamble": self._ctl("preamble").getText(),
            "toolchain": self.toolchain_ids[max(0, self._ctl("toolchain").getSelectedItemPos())],
            "engine": ENGINES[max(0, self._ctl("engine").getSelectedItemPos())],
        }

    def _request(self):
        o = self.options()
        req = {
            "source": self._ctl("source").getText(),
            "preamble": o["preamble"],
            "mode": o["mode"],
            "fontSize": o["fontSize"],
            "color": o["color"],
            "format": o["format"],
            "pngDpi": o["pngDpi"],
        }
        if o["toolchain"]:
            req["toolchain"] = o["toolchain"]
        if o["engine"]:
            req["engine"] = o["engine"]
        return req

    def _render(self):
        req = self._request()
        key = json.dumps(req, sort_keys=True)
        if key == self._cache_key:
            return self._cache_result
        if not req["source"].strip():
            self._status("Enter some LaTeX to render.")
            return None
        self._status("Rendering...")
        try:
            result = self.core.render(req)
        except BridgeError as exc:
            self._status(exc.summary, exc.details)
            return None
        if not result.get("ok"):
            err = result.get("error", {})
            where = ""
            if err.get("line"):
                where = " (%s line %d)" % (err.get("location", "source"), err["line"])
                if err.get("location") == "source":
                    self._focus_line(err["line"])
            self._status(err.get("summary", "Rendering failed.") + where, err.get("details", ""))
            return None
        self._cache_key, self._cache_result = key, result
        warn = " ".join(result.get("warnings", []))
        self._status("%.0f x %.0f pt, %s via %s. %s" % (result["widthBp"], result["heightBp"], result["engine"],
                                                        result["toolchain"], warn))
        self._show_preview(result)
        return result

    def _show_preview(self, result):
        # Small equations are shown at natural size (crisp); only larger ones
        # are scaled down to fit.
        box = self._ctl("preview").getPosSize()
        px_w = result["widthBp"] * 96.0 / 72.0
        px_h = result["heightBp"] * 96.0 / 72.0
        preview = self.model.getByName("preview")
        preview.ScaleImage = px_w > box.Width - 4 or px_h > box.Height - 4
        preview.ImageURL = uno.systemPathToFileUrl(result["path"])

    def _focus_line(self, line):
        text = self._ctl("source").getText()
        lines = text.split("\n")
        if 1 <= line <= len(lines):
            start = sum(len(l) + 1 for l in lines[: line - 1])
            self._ctl("source").setSelection(Selection(start, start + len(lines[line - 1])))

    def on_preview(self):
        self._render()

    def on_insert(self):
        result = self._render()
        if result:
            self.result = result
            self.dialog.endExecute()

    def on_details(self):
        box = self.toolkit.createMessageBox(
            self.dialog.getPeer(), uno.Enum("com.sun.star.awt.MessageBoxType", "INFOBOX"), 1,
            "LavaTeX details", self._details or "")
        box.execute()

    def on_browse(self):
        picker = self.ctx.ServiceManager.createInstanceWithContext("com.sun.star.ui.dialogs.FolderPicker", self.ctx)
        picker.setTitle("Folder that contains latex / pdflatex / tectonic")
        if picker.execute() == 1:
            folder = uno.fileUrlToSystemPath(picker.getDirectory())
            self._fill_toolchains(folder)

    def run(self):
        """Show the dialog; return the successful render result or None."""
        self.dialog.execute()
        options = self.options()
        self.dialog.dispose()
        return self.result, options
