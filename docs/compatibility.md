# Compatibility matrix

Only combinations listed as **tested** have been run. Everything else is expected to work but is unverified.

## Tested (0.1.0)

| Area | Environment | What was verified |
|---|---|---|
| Core | Linux x86_64, TeX Live 2023 (Debian) | latex, pdflatex, xelatex, lualatex × SVG, PDF, PNG all render, and metadata reads back from every output. Error mapping and file sandbox work. |
| LibreOffice | LibreOffice 24.2.7, Linux, Python-UNO, headless | Insert SVG/PDF/PNG; edit keeps scale and position without duplicating; save as ODP and PPTX, reopen, edit again (`hosts/libreoffice/tests/test_headless.py`). |
| LibreOffice | LibreOffice 24.2.7, Linux, Xvfb, installed `.oxt` | `unopkg` installs and registers; menu, add-on and toolbar entries appear, and the Tabbed UI's Extension tab button opens the dialog; dialog opens on a selected equation with its source; typing and *Update* replace the equation. |
| ONLYOFFICE plugin | Chromium (Playwright) with a mock presentation API + real `lavatex serve` | Setup/token flow, live preview, insert as SVG with alt text, edit keeps scale/position/rotation, inline errors (`hosts/onlyoffice/tests/e2e.js`). |
| ONLYOFFICE plugin | **ONLYOFFICE Desktop Editors 10.0.0**, Linux x86_64 (official build), Xvfb | Installs with *Plugin Manager › Install plugin manually*; LavaTeX appears in the Plugins tab. Loads the local SDK (works offline). Token setup, live preview, insert of a multi-line equation, edit replaces in place (one picture, alt text updated). An equation made in LibreOffice and saved as PPTX opens in ONLYOFFICE and is editable with source, size and colour restored. |
| ONLYOFFICE setup | ONLYOFFICE Desktop Editors 10.0.0, Linux x86_64 | `lavatex setup onlyoffice` installs the plugin and pairing file; the plugin connects with no token entry; running setup again replaces the running helper; "Try again" recovers after the helper is started; `--uninstall` removes everything. |
| Helper | Linux | Token required (401), foreign Host header rejected (403), CORS/PNA preflight answered. Socket activation (`systemd-socket-activate`) and idle exit. |
| PowerPoint add-in | Chromium (Playwright) with a mock of Office.js that enforces load/sync rules and API versions, + real helper over HTTPS (`hosts/powerpoint/tests/e2e.js`) | `setup powerpoint` writes certificate and manifest; insert as SVG with tag and alt text, centred; edit keeps scale, position, rotation and stacking order; pane follows the selection without losing typing; equations from other apps (alt text only) are editable; PowerPoint API 1.5 (tags only) works; errors with line numbers; Regenerate and Help commands. |
| HTTPS | Linux, Chromium 153.0.8010.12 and OpenSSL | The name-constrained certificate is accepted by Chromium for `localhost` and `127.0.0.1` and verifies with OpenSSL; add-in pages refuse cross-site requests and carry no CORS headers. |

## Not yet tested

- ONLYOFFICE Document Server and Desktop Editors on Windows and macOS. Desktop versions before 10.0 can insert equations but cannot store their source (no `ApiDrawing.SetDescription`).
- **The PowerPoint add-in in real PowerPoint** (Windows, macOS, web), including `lavatex setup powerpoint` on Windows (certificate trust prompt, registry sideloading) and macOS (keychain, `wef` folder).
- `lavatex setup onlyoffice` on Windows (Run key) and macOS (LaunchAgent), and with a real systemd user session on Linux.
- Windows and macOS builds, MiKTeX, MacTeX and Tectonic on real machines. The detection paths are implemented, but CI for these platforms isn't wired up yet.
- LibreOffice versions older than 24.2. The extension declares 7.0 as its minimum.
