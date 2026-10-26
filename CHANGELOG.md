# Changelog

## [Unreleased]

### PowerPoint add-in (new)

LavaTeX now works in **PowerPoint for Microsoft 365** (Windows and macOS) and PowerPoint for the web, like IguanaTeX but with the same equations as LibreOffice and ONLYOFFICE.

- A **LavaTeX tab** on the ribbon: *New Equation*, *Edit Equation*, *Regenerate* (render selected equations again from their LaTeX), *Settings* and *Help*.
- A side pane with live preview that follows your selection: click an equation on the slide to edit it.
- Editing keeps the equation's scale, position, rotation and place in the stacking order.
- The LaTeX source is stored in the picture's tags and alt text, so equations are editable in PowerPoint, LibreOffice and ONLYOFFICE alike.
- Set up with one command, `lavatex setup powerpoint`: it registers the add-in, creates and trusts a certificate for `https://localhost` (PowerPoint only loads add-ins over HTTPS), and starts the helper at login. `--uninstall` removes it all.
- Tested against a simulated PowerPoint (Office.js) in Chromium; please report how it works in real PowerPoint.

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
