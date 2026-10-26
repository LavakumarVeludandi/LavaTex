# Compatibility matrix

Only combinations listed as **tested** have been run. Everything else is expected to work but is unverified.

## Tested (0.1.0)

| Area | Environment | What was verified |
|---|---|---|
| Core | Linux x86_64, TeX Live 2023 (Debian) | latex, pdflatex, xelatex, lualatex × SVG, PDF, PNG all render, and metadata reads back from every output. Error mapping and file sandbox work. |
| LibreOffice | LibreOffice 24.2.7, Linux, Python-UNO, headless | Insert SVG/PDF/PNG; edit keeps scale and position without duplicating; save as ODP and PPTX, reopen, edit again (`hosts/libreoffice/tests/test_headless.py`). |
| LibreOffice | LibreOffice 24.2.7, Linux, Xvfb, installed `.oxt` | `unopkg` installs and registers; menu, add-on and toolbar entries appear; dialog opens on a selected equation with its source; typing and *Update* replace the equation. |
| ONLYOFFICE plugin | Chromium (Playwright) with a mock presentation API + real `lavatex serve` | Setup/token flow, live preview, insert as SVG with alt text, edit keeps scale/position/rotation, inline errors (`hosts/onlyoffice/tests/e2e.js`). |
| Helper | Linux | Token required (401), foreign Host header rejected (403), CORS/PNA preflight answered. |

## Not yet tested

- Real ONLYOFFICE Desktop Editors and Document Server. The plugin uses `Api.GetSelection` (8.3+), `Api.CreateImage` with SVG data URLs (8.2+), and `ApiDrawing.Get/SetDescription` (10.0+), all checked against the ONLYOFFICE `sdkjs` source.
- Windows and macOS builds, MiKTeX, MacTeX and Tectonic on real machines. The detection paths are implemented, but CI for these platforms isn't wired up yet.
- LibreOffice versions older than 24.2. The extension declares 7.0 as its minimum.
