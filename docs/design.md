# LavaTeX design (v0.2 of the SDD, as built)

This document updates *LavaTeX Software Design Document v0.1* with the decisions made while building the first release. Where they differ, this document wins.

## Principles

1. **An equation is just a picture.** LavaTeX never creates a special object type (no OLE). Any app and any format can display the equation; LavaTeX adds editing on top.
2. **Bring your own TeX.** Users already have TeX Live, MiKTeX, MacTeX or Tectonic. LavaTeX finds them, suggests one, and lets the user choose. No TeX is bundled and nothing is compiled in the cloud.
3. **One core, thin hosts.** All TeX work lives in one Rust binary (`lavatex`). Host integrations only provide UI and insert pictures.
4. **The source travels with the picture**, stored redundantly (see [metadata.md](metadata.md)).

## Architecture

| Component | Tech | Responsibility |
|---|---|---|
| `lavatex-core` | Rust library | TeX discovery, render pipelines, sandboxing, error mapping, metadata embedding and extraction |
| `lavatex` CLI | Rust binary | `detect`, `render` (JSON in/out for hosts), `inspect`, `config`, `token`, `serve` |
| `lavatex serve` | Rust, `tiny_http` | Localhost HTTP helper for browser-based hosts |
| LibreOffice extension | Python UNO + bundled `lavatex` | Dialog, insert/replace `GraphicObjectShape`, metadata on shapes |
| ONLYOFFICE plugin | HTML/JS | Dialog with live preview, insert/replace through the Builder API, alt-text metadata |
| `scripts/lavatex-core.js` | Plain JS (UMD) | Alt-text parsing, sizing, helper client; reusable by a future Office.js add-in |

### Why a localhost helper for ONLYOFFICE

ONLYOFFICE plugins run in a sandboxed iframe and cannot start programs. Rendering with the user's own TeX therefore requires a small local process. `lavatex serve` is the same binary as the CLI. It is hardened as follows:

- binds to `127.0.0.1` only;
- accepts only `Host: 127.0.0.1|localhost|[::1]:<port>`, which blocks DNS rebinding;
- requires a bearer token (random, stored with `0600` permissions) on every endpoint except `/v1/health`;
- answers CORS and Chrome Private Network Access preflights;
- limits request size and uses the same sandboxed renderer as the CLI.

A browser-only renderer (MathJax) is a possible later option for zero-install use, but it can't run arbitrary packages, so it isn't the default.

## Rendering pipeline

The equation is typeset at 10 pt inside an `lrbox` and measured, so the box's width, height and depth are exact and the baseline is known. The page is then sized to the box, and the result is scaled to the requested font size. This is the IguanaTeX approach, and it works for any font setup.

| Output | Preferred route | Alternatives |
|---|---|---|
| SVG | `latex` → DVI → `dvisvgm --no-fonts` (glyphs as paths, no font dependencies) | `xelatex -no-pdf` → XDV → dvisvgm; `dvilualatex` → dvisvgm; PDF engine → `pdftocairo -svg` (Tectonic) |
| PDF | `pdflatex` (or the chosen PDF engine), page cropped to the box, scaled in TeX | `latex` → `dvipdfmx`; XeLaTeX writes PDF 1.4 so the metadata stays readable |
| PNG | PDF → `pdftocairo -png -transp` at dpi × scale | `latex` → `dvipng -bg Transparent` |

Safety measures on every render:
- a fresh temporary directory;
- arguments passed as an array, never through a shell;
- `-no-shell-escape` (`-disable-write18` on MiKTeX);
- kpathsea `openin_any=p` / `openout_any=p`, so LaTeX can only read files in its working directory and the TeX tree (tested: `\input{/etc/passwd}` is refused);
- output to files rather than pipes, a hard timeout, and output size and format checks;
- the temporary directory is removed afterwards.

Error messages are mapped to plain language and to a line in the user's source or preamble. Examples: an unknown command, a missing package, a blank line inside math.

## Editing behaviour

- **Scale, not box size, is preserved.** On edit, the new picture is sized `newRenderedSize × (currentWidth / previousRenderedWidth)`, so a longer equation grows instead of being squashed.
- The top-left position and rotation are kept, and the replacement is a single undo step in LibreOffice.
- Metadata from a document is treated as untrusted. Nothing is compiled when a file opens. The dialog notes when an equation brings its own preamble.

## Decisions on the SDD's open questions

| SDD open decision | Decision |
|---|---|
| Full LaTeX in ONLYOFFICE | Local helper (`lavatex serve`) using the user's TeX. MathJax may be added later as an optional zero-install mode. |
| Primary TeX engine | The user's choice. Auto order: latex+dvisvgm (TeX Live → MacTeX → MiKTeX) → others → Tectonic. |
| SVG conversion | dvisvgm `--no-fonts` primarily; pdftocairo for PDF-only engines. |
| Metadata persistence | Three layers: host properties, alt text, image bytes. Verified on ODP and PPTX. |
| UI sharing | Shared web core for HTML hosts (ONLYOFFICE now, Office.js later); native UNO dialog for LibreOffice. |
| License | Apache-2.0. No IguanaTeX or TexMaths code is used. |

## Known limitations (0.1.0)

- In LibreOffice, the standard-toolbar button shows a text label instead of the icon.
- ONLYOFFICE older than 10.0 inserts equations but can't store their source (the plugin warns). It can't place PDF pictures, so PDF falls back to SVG there.
- Tectonic's PDF output compresses the info dictionary, so the source lives in alt text only for Tectonic PDFs.
- Inline equations inside text boxes, with baseline alignment, are out of scope. `depthBp` is recorded for a future version.
- The ONLYOFFICE helper must be started by the user (`lavatex serve`). An autostart installer is planned.

## Next steps

1. CI release builds for Windows, macOS (x86_64 and arm64) and Linux, producing a multi-platform `.oxt`.
2. Validate in real ONLYOFFICE Desktop Editors and Document Server builds and publish the compatibility matrix.
3. Add an autostart option for `lavatex serve` and a system-tray indicator.
4. Build a PowerPoint Office.js add-in reusing `lavatex-core.js` and the helper. Read IguanaTeX tags for round trips.
5. LibreOffice Writer and ONLYOFFICE Document Editor support.
