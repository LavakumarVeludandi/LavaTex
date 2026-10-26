# LavaTeX

**Write once. Typeset everywhere.** LaTeX equations for LibreOffice Impress/Draw and the ONLYOFFICE Presentation Editor, inspired by IguanaTeX for PowerPoint.

LavaTeX turns LaTeX into an ordinary **SVG, PDF or PNG picture** using the TeX installation you already have (TeX Live, MiKTeX, MacTeX or Tectonic). The LaTeX source travels *inside* the picture and its alt text, so:

- **without LavaTeX**, the equation is just a crisp picture: it opens in any presentation format and any app (PPTX, ODP, PDF export, Keynote, Google Slides, ...);
- **with LavaTeX**, select the picture and edit its LaTeX again. Your scaling, position and rotation are kept.

```
                 ┌──────────────────────── lavatex (Rust) ─────────────────────────┐
LaTeX source ──► │ find your TeX  →  sandboxed compile  →  SVG / PDF / PNG          │ ──► picture + source
                 │ (TeX Live, MiKTeX, MacTeX, Tectonic)    + embedded metadata      │
                 └──────────────────────────────────────────────────────────────────┘
                      ▲ CLI (stdin/stdout JSON)              ▲ lavatex serve (localhost HTTP)
          LibreOffice extension (Python UNO)        ONLYOFFICE plugin (HTML/JS)
```

## Status

Version 0.1.0, the first working build.

| Host | Package | State |
|---|---|---|
| LibreOffice Impress & Draw 7.0+ | `lavatex-libreoffice-<version>-<platform>.oxt` | Insert, edit, and preview work. Tested end to end on LibreOffice 24.2 (Linux). |
| ONLYOFFICE Presentation Editor 8.3+ (editing needs 10.0+) | `lavatex-onlyoffice-<version>.plugin` | Insert, edit, and live preview work. Tested in Chromium against a mock of the editor API (see [compatibility](docs/compatibility.md)). |
| PowerPoint | — | Planned: Office.js add-in reusing the ONLYOFFICE web UI and helper. |

## Quick start

1. **Have a TeX distribution.** If you can compile LaTeX in VS Code, you already do. Otherwise install [TeX Live](https://tug.org/texlive/), [MiKTeX](https://miktex.org/) or [MacTeX](https://tug.org/mactex/).
2. **Check that LavaTeX finds it:**
   ```console
   $ lavatex detect
   TeX Live 2023/Debian (suggested)
     folder:  /usr/bin
     engines: latex, pdflatex, xelatex, lualatex
     formats: svg, pdf, png
   ```
   Several installations? Pick one with `lavatex config --toolchain <bin folder>`; every host uses this choice.
3. **Install a host package:**
   - **LibreOffice:** *Tools › Extension Manager › Add* and pick the `.oxt`. It includes the `lavatex` binary. Then use *Insert › LaTeX Equation...* or the toolbar button. Select an existing equation and use the same command to edit it.
   - **ONLYOFFICE:** *Plugins › Plugin Manager › Install plugin manually* and pick the `.plugin`. Run `lavatex serve` (keep it running), then `lavatex token`, and paste the token into the plugin the first time.

See [docs/installation.md](docs/installation.md) for details and troubleshooting.

## Writing equations

Type just the math, e.g. `\frac{a}{b}`; LavaTeX adds the document around it. Habits from IguanaTeX and LaTeX work too:

- your own delimiters: `$…$` (inline), `\[…\]` or `$$…$$` (display);
- display environments: `equation`, `align`, `gather`, `multline` and their starred forms. On a slide these become `aligned`/`gathered`, with no equation numbers;
- a complete IguanaTeX-style document (`\documentclass … \begin{document} … \end{document}`). Its preamble is used, and the whole document is kept as the editable source.

## Command line

```console
$ lavatex render '\frac{-b \pm \sqrt{b^2-4ac}}{2a}' -f svg -s 24 -c '#1a4f9c' -o quadratic.svg
quadratic.svg (168.9 x 54.4 pt, latex via TeX Live 2023/Debian)
$ lavatex inspect quadratic.svg      # prints the embedded source and settings
$ lavatex render --request req.json --json -o out.pdf   # machine interface used by host plugins
```

## Repository layout

```
crates/lavatex-core     Rust library: TeX discovery, sandboxed rendering, metadata embed/extract
crates/lavatex-cli      `lavatex` binary: detect, render, inspect, config, token, serve
hosts/libreoffice       LibreOffice extension (Python UNO): dialog, insert/edit, packaging metadata
hosts/onlyoffice        ONLYOFFICE plugin (HTML/JS) + shared browser core (scripts/lavatex-core.js)
fixtures/metadata       Cross-language metadata fixture (Rust writes it; JS and Python must parse it)
docs/                   Design, metadata format, installation, compatibility
tools/package.py        Builds dist/*.oxt and dist/*.plugin
```

## Development

```console
cargo test                                         # core unit tests (incl. shared fixture)
node --test hosts/onlyoffice/tests/core.test.js    # browser core
node hosts/onlyoffice/tests/e2e.js target/debug/lavatex   # plugin page + real helper (needs Playwright)
python3 hosts/libreoffice/tests/test_headless.py   # real LibreOffice: insert, edit, save ODP/PPTX, reopen
python3 tools/package.py all                       # build dist/ packages
```

## License

Apache License 2.0, see [LICENSE](LICENSE). LavaTeX contains no IguanaTeX code. The ONLYOFFICE plugin loads ONLYOFFICE's plugin SDK from ONLYOFFICE's site at runtime and does not bundle it.
