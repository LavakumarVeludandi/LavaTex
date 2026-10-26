# Changelog

## [Unreleased]

## [0.2.0] - 2026-10-26

Adds **PowerPoint** (Windows) and **ONLYOFFICE**, next to LibreOffice.

### Download

| File | For |
|---|---|
| `lavatex-powerpoint-0.2.0-windows-x86_64.zip` | **PowerPoint (Windows).** `LavaTeX.ppam` and `lavatex-helper.exe`; unzip and add the `.ppam` under *File › Options › Add-ins*. |
| `lavatex-libreoffice-0.2.0-multi.oxt` | **LibreOffice** on any platform. |
| `lavatex-<platform>.zip` | The `lavatex` program; for ONLYOFFICE run `lavatex setup onlyoffice`. |
| `lavatex-onlyoffice-0.2.0.plugin` | The ONLYOFFICE plugin on its own (Document Server). |

See the [installation guide](https://github.com/LavakumarVeludandi/LavaTex/blob/master/docs/installation.md).

### PowerPoint add-in (new, Windows)

- **`LavaTeX.ppam`**, a PowerPoint add-in like IguanaTeX: add it once under *File › Options › Add-ins*, and a **LavaTeX** tab appears on the ribbon with *New Equation*, *Edit Equation*, *Regenerate*, *Help* and *About*.
- The editor opens in its own window with **live preview**, the same editor and rendering as in ONLYOFFICE and LibreOffice.
- Editing keeps the equation's size, position, rotation and place in the stacking order.
- *Regenerate* renders the selected equations again from their LaTeX, without opening the editor.
- The LaTeX source is stored in the picture's alt text and a shape tag, so equations are editable in PowerPoint, LibreOffice and ONLYOFFICE alike.
- Download `lavatex-powerpoint-<version>-windows-x86_64.zip`: it contains `LavaTeX.ppam` and `lavatex-helper.exe`. See the [installation guide](docs/installation.md#4-powerpoint-windows).

### ONLYOFFICE plugin

Tested in ONLYOFFICE Desktop Editors 10.0.

- **One-time setup, then it just works:** `lavatex setup onlyoffice` installs the plugin into ONLYOFFICE, installs LavaTeX for your user, and makes its helper start by itself when you log in. The plugin finds the helper on its own, so there is no server to start and no token to copy. `lavatex setup onlyoffice --uninstall` removes everything.
- New `lavatex-helper` program: the helper without a console window. On Linux with systemd it only runs while ONLYOFFICE uses it (socket activation), and exits when idle.
- If the helper isn't running, the plugin explains the one-time setup and offers **Try again**, instead of asking for an address and token.
- The CLI download is now `lavatex-<platform>.zip` and contains `lavatex` and `lavatex-helper`.
- **Works offline:** the plugin now uses the copy of ONLYOFFICE's plugin SDK that ships with ONLYOFFICE. It used to download it from the internet, and failed to start when that site couldn't be reached.
- Fixed the equation editor showing underneath the "connect to your TeX installation" screen.
- The release also includes `lavatex-onlyoffice-<version>.plugin`, for installing the plugin by hand (for example on Document Server).
- Equations made in LibreOffice and saved as PPTX can be edited in ONLYOFFICE.

### Documentation

- New [installation](docs/installation.md) and [usage](docs/usage.md) guides with screenshots.

### LibreOffice

- The dialog preview no longer crops tall (multi-line) equations.

### Fixes

- ONLYOFFICE: clicking Insert right after typing could do nothing, because the pending preview replaced the render that Insert was waiting for.
- Windows: TeX tools no longer open terminal windows when LavaTeX runs them from a program without a console.

## [0.1.0] - 2026-10-26

First release, for **LibreOffice Impress and Draw** (7.0 or newer). Equations are rendered with the TeX installation you already have: TeX Live, MiKTeX, MacTeX or Tectonic.

### Download

| File | For |
|---|---|
| `lavatex-libreoffice-0.1.0-multi.oxt` | **Recommended.** Works on Windows, macOS (Intel and Apple silicon) and Linux (x86_64 and ARM). |
| `lavatex-libreoffice-0.1.0-<platform>.oxt` | Smaller package for a single platform. |
| `lavatex-cli-<platform>.zip` | The `lavatex` command-line tool on its own. |

Install the `.oxt` with *Tools › Extension Manager › Add…* and restart LibreOffice.

### Features

- Insert LaTeX equations as **SVG, PDF or PNG** pictures. They stay ordinary pictures in any app or file format, including PPTX and ODP.
- **Edit** an equation later: select it and run the command again. Your scaling, position and rotation are kept.
- The LaTeX source is stored in the picture, its alt text, and LibreOffice shape properties, so it survives saving as ODP and PPTX.
- Available from *Insert › LaTeX Equation…*, the standard toolbar, *Tools › Add-Ons*, and the **Extension** tab of the Tabbed (ribbon) interface.
- **Multi-line equations:** end a line with `\\` for a new line, and use `&` to line lines up. Several `\[…\]` or `align*` blocks in a row are stacked.
- Type just the formula, or IguanaTeX-style input: `$…$`, `\[…\]`, `align*` and similar environments, or a complete `\documentclass … \end{document}` document.
- Preview in the dialog, with readable errors that point at the line you typed.
- Choose the TeX installation and engine (latex, pdflatex, xelatex, lualatex, tectonic), font size, colour and output format.
- LaTeX runs sandboxed: no shell escape, no file access outside its working folder, and time limits.

### Known limitations

- PDF output is only offered when your LibreOffice build can display PDF pictures. Fedora's build cannot, so use SVG there.
- The ONLYOFFICE plugin is included in the source but is not part of this release yet.
