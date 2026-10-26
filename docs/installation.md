# Installation and troubleshooting

## 1. TeX

LavaTeX uses your existing TeX installation. Supported:

| Distribution | Notes |
|---|---|
| TeX Live (Linux, Windows, macOS) | Recommended. Needs `latex` and `dvisvgm` for SVG; both are in the default scheme. |
| MacTeX | Detected in `/Library/TeX/texbin`. |
| MiKTeX (Windows, macOS, Linux) | Missing packages may trigger MiKTeX's installer prompt. If renders time out, open the MiKTeX Console and install the package, or set "install missing packages" to *Always*. |
| Tectonic | SVG and PNG need Poppler's `pdftocairo` on PATH. The first use downloads packages, so do one render while online. |

`lavatex detect` lists what was found. If your TeX is installed somewhere unusual, choose it explicitly:

```console
lavatex config --toolchain "C:\texlive\2024\bin\windows"
lavatex config --engine xelatex     # optional: preferred engine
lavatex config --format svg         # optional: preferred output
```

## 2. LibreOffice (Impress and Draw)

1. *Tools › Extension Manager › Add...* and choose `lavatex-libreoffice-<version>-<platform>.oxt`. Use the `multi` package if you're unsure of your platform.
2. Restart LibreOffice.
3. To insert, use *Insert › LaTeX Equation...*, the toolbar button next to *Insert Image*, or *Tools › Add-Ons › LaTeX Equation (LavaTeX)...*.
4. To edit, select an equation and run the same command. The dialog opens with its source and settings.

The `.oxt` contains the `lavatex` binary for its platform(s). To use another build, put `lavatex` on PATH and set `"corePath"` in `libreoffice.json` in the LavaTeX config folder.

## 3. ONLYOFFICE (Presentation Editor)

1. Install the plugin: *Plugins › Plugin Manager › Install plugin manually* (Desktop Editors 8.3+), and choose `lavatex-onlyoffice-<version>.plugin`. On Document Server, copy the unzipped folder into `sdkjs-plugins`.
2. Start the helper and keep it running:
   ```console
   lavatex serve        # listens on http://127.0.0.1:7354
   ```
   To run it at login: on Windows, put a shortcut to `lavatex.exe serve` in `shell:startup`. On macOS, use a LaunchAgent. On Linux, use a systemd user service.
3. Run `lavatex token` and paste the token into the plugin's first screen. It's stored in the plugin's local storage.

Editing an existing equation needs ONLYOFFICE 10.0 or newer, which can read and write picture descriptions.

## Where settings live

| OS | Folder |
|---|---|
| Linux | `$XDG_CONFIG_HOME/lavatex` (usually `~/.config/lavatex`) |
| macOS | `~/Library/Application Support/LavaTeX` |
| Windows | `%APPDATA%\LavaTeX` |

Files in that folder: `config.json` (shared TeX/engine/format choice), `helper-token`, and `libreoffice.json` (LibreOffice dialog defaults). Set `LAVATEX_CONFIG_DIR` to override the location.

## Troubleshooting

| Symptom | Fix |
|---|---|
| "No TeX installation was found" | Install TeX, or run `lavatex config --toolchain <folder containing latex>`. |
| "*x*.sty is not installed" | Install the package with `tlmgr install x` or the MiKTeX Console, or remove it from the preamble. |
| Rendering times out | Check whether MiKTeX or Tectonic is waiting to download a package. Try `lavatex render 'x'` in a terminal to see what happens. |
| ONLYOFFICE: "could not reach its helper" | Check that `lavatex serve` is running, and that the address matches if you changed `--port`. |
| ONLYOFFICE: "access token was not accepted" | Run `lavatex token` again and paste the result. |
| Equation is not editable after conversion | Some converters drop alt text. The image itself may still carry the source: run `lavatex inspect picture.svg`. |
