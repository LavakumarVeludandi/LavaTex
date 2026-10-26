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
| `lavatex serve` / `lavatex-helper` | Rust, `tiny_http`, `rustls` | Localhost helper for browser-based hosts: HTTP on 7354, and HTTPS on 7355 (serving the PowerPoint add-in) once a certificate exists. `lavatex-helper` is the same server without a console window, for autostart. |
| `lavatex setup onlyoffice` / `powerpoint` | Rust | Installs the embedded plugin or registers the add-in, creates the HTTPS certificate (PowerPoint), installs the programs and the helper's autostart |
| LibreOffice extension | Python UNO + bundled `lavatex` | Dialog, insert/replace `GraphicObjectShape`, metadata on shapes |
| ONLYOFFICE plugin | HTML/JS | Dialog with live preview, insert/replace through the Builder API, alt-text metadata |
| PowerPoint add-in | Office.js (served by the helper) | Ribbon tab, task pane with live preview, insert/replace through `setSelectedDataAsync` and the PowerPoint API, tag + alt-text metadata |
| `scripts/lavatex-core.js` | Plain JS (UMD) | Alt-text parsing, sizing, helper client; shared by the ONLYOFFICE plugin and the PowerPoint add-in |

### Loading the ONLYOFFICE plugin SDK

The plugin does not bundle ONLYOFFICE's SDK (`plugins.js`, AGPL). `scripts/sdk-loader.js` loads it from:
1. `../v1/` next to the plugin, which works for Document Server, built-in plugins, and Desktop's user plugin folder (Desktop keeps a copy there);
2. the Desktop app's own `editors/sdkjs-plugins/v1/`, found from the editor's address;
3. onlyoffice.github.io, only as a last resort.

Plugins that load the SDK from the internet fail to start offline or behind a blocking proxy. The SDK starts its handshake from `window.onload`, so the loader calls that handler if the page has already finished loading.

### Why a localhost helper for ONLYOFFICE

ONLYOFFICE plugins run in a sandboxed iframe and cannot start programs. Rendering with the user's own TeX therefore requires a small local process. `lavatex serve` is the same binary as the CLI. It is hardened as follows:

- binds to `127.0.0.1` only;
- accepts only `Host: 127.0.0.1|localhost|[::1]:<port>`, which blocks DNS rebinding;
- requires a bearer token (random, stored with `0600` permissions) on every endpoint except `/v1/health`;
- answers CORS and Chrome Private Network Access preflights;
- limits request size and uses the same sandboxed renderer as the CLI.

ONLYOFFICE does allow some plugins to start processes, but only plugins on a fixed internal list, so a third-party plugin can't use it.

#### Setup and autostart

Users should not have to start a server. `lavatex setup onlyoffice` does it once:

1. copies `lavatex` and `lavatex-helper` to a per-user folder;
2. installs the plugin (embedded in the binary at build time) into every ONLYOFFICE user plugin folder it finds (native, Flatpak, Snap on Linux; the per-user folders on Windows and macOS), plus `lavatex-helper.json` containing the helper URL and token (mode `0600`);
3. registers autostart: a systemd user socket where available, so the helper is started on the first request and exits after 15 minutes idle; otherwise an XDG autostart entry (Linux), a LaunchAgent (macOS) or an HKCU *Run* value (Windows);
4. stops any helper from an earlier setup (`POST /v1/shutdown`), starts the new one and checks it answers with the paired token.

The plugin reads `lavatex-helper.json` from its own folder, so it connects without asking the user anything. Manual URL and token entry remain under *Advanced* for Document Server.

We chose this over a browser-only renderer (MathJax): like IguanaTeX, LavaTeX renders with the user's own TeX, so any package and the same output as in LibreOffice are available.

### PowerPoint add-in

IguanaTeX is a VBA add-in (`.ppam`): it can run TeX directly, but macros are often blocked, and on macOS the sandbox makes running TeX hard. A VSTO/C# add-in would be Windows-only. LavaTeX uses an **Office.js web add-in** instead: it runs on Windows, macOS and the web, has a real ribbon tab (declared in the manifest's `VersionOverrides`), and reuses the ONLYOFFICE plugin's browser core and the helper.

**Hosting.** Office loads add-in pages only over HTTPS. Rather than depending on a website (which would need the internet and could drift from the installed helper), the helper serves the add-in itself at `https://localhost:7355/powerpoint/`. TLS connections are decrypted (rustls with *ring*) and handed to the same HTTP handler as port 7354, one request per connection.

**Certificate.** `lavatex setup powerpoint` creates a CA with X.509 name constraints that permit only `localhost`, `127.0.0.1` and `::1`, signs a server certificate with it and **discards the CA's private key**. Only the CA certificate is trusted (Windows: current user's Root store with `certutil -user`; macOS: login keychain; Linux: the NSS database used by Chrome). Even if the CA certificate leaked, nothing else can be signed with it, and it could not vouch for any other host.

**Pairing.** The helper writes the access token into the HTML pages it serves. They are sent without CORS headers, and requests marked `Sec-Fetch-Site: cross-site` or with a foreign `Origin` are refused, so other web pages cannot read the token. The pages then call the API on their own origin.

**Registration.** Windows: the manifest path under `HKCU\Software\Microsoft\Office\16.0\WEF\Developer` (how Office's own tools sideload add-ins). macOS: a copy in PowerPoint's `wef` folder. PowerPoint for the web: the user uploads the manifest.

**Editing.** PowerPoint has no "replace picture" call. The add-in deselects the old picture, inserts the new one at the old position with `setSelectedDataAsync` (SVG via `CoercionType.XmlSvg`, PNG via `Image`), finds it by comparing the slide's shape ids, copies rotation, moves it down with `setZOrder` until it sits just above the old one, and deletes the old one. Features are used according to the PowerPoint API version: 1.5 (selection) is required; tags need 1.3, z-order 1.8, alt text, rotation and slide size 1.10.

## Accepted input

`normalize.rs` unwraps what users type before it is typeset:
- a full IguanaTeX-style document: its preamble is used and `\documentclass` is dropped;
- `$…$`, `\(…\)`, `\[…\]` and `$$…$$` around a single formula;
- display environments, which are mapped to their inline-safe amsmath forms (`align*` becomes `aligned`).

Error line numbers are mapped back to the text as typed, and the metadata stores the source unchanged.

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
- If LibreOffice refuses an SVG or PDF picture (for example, a build without PDF import), the half-inserted shape is removed, a 600 dpi PNG is inserted instead, and the user is told why.
- Metadata from a document is treated as untrusted. Nothing is compiled when a file opens. The dialog notes when an equation brings its own preamble.

## Decisions on the SDD's open questions

| SDD open decision | Decision |
|---|---|
| Full LaTeX in ONLYOFFICE | Local helper using the user's TeX, installed and auto-started by `lavatex setup onlyoffice`. No MathJax. |
| Primary TeX engine | The user's choice. Auto order: latex+dvisvgm (TeX Live → MacTeX → MiKTeX) → others → Tectonic. |
| SVG conversion | dvisvgm `--no-fonts` primarily; pdftocairo for PDF-only engines. |
| Metadata persistence | Three layers: host properties, alt text, image bytes. Verified on ODP and PPTX. |
| UI sharing | Shared web core for HTML hosts (ONLYOFFICE plugin, PowerPoint add-in); native UNO dialog for LibreOffice. |
| License | Apache-2.0. No IguanaTeX or TexMaths code is used. |

## Known limitations (0.1.0)

- ONLYOFFICE older than 10.0 inserts equations but can't store their source (the plugin warns). It can't place PDF pictures, so PDF falls back to SVG there.
- Tectonic's PDF output compresses the info dictionary, so the source lives in alt text only for Tectonic PDFs.
- Inline equations inside text boxes, with baseline alignment, are out of scope. `depthBp` is recorded for a future version.
- The ONLYOFFICE setup has been tested on Linux (systemd socket activation and XDG autostart). The Windows and macOS autostart paths are implemented but untested.
- The PowerPoint add-in has been tested against a simulated Office.js, not yet in real PowerPoint. The add-in page needs the helper running; if it is not, PowerPoint shows its own "can't load this add-in" message.
- The PowerPoint add-in loads `office.js` from Microsoft's CDN, as Office requires, so opening the pane needs an internet connection the first time.

## Next steps

1. CI release builds for Windows, macOS (x86_64 and arm64) and Linux, producing a multi-platform `.oxt`.
2. Validate in ONLYOFFICE Document Server (Desktop Editors 10.0 is tested).
3. Test the setup on Windows and macOS; consider an installer that runs it.
4. Test the PowerPoint add-in in real PowerPoint on Windows and macOS. Then: import IguanaTeX equations (read IguanaTeX's shape tags) so existing IguanaTeX decks become editable with LavaTeX.
5. LibreOffice Writer and ONLYOFFICE Document Editor support.
