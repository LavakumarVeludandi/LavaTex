# Changelog

## [Unreleased]

### ONLYOFFICE plugin

Tested in ONLYOFFICE Desktop Editors 10.0.

- **Works offline:** the plugin now uses the copy of ONLYOFFICE's plugin SDK that ships with ONLYOFFICE. It used to download it from the internet, and failed to start when that site couldn't be reached.
- Fixed the equation editor showing underneath the "connect to your TeX installation" screen.
- The release now includes `lavatex-onlyoffice-<version>.plugin`. Install it with *Plugins › Plugin Manager › Install plugin manually*.
- Equations made in LibreOffice and saved as PPTX can be edited in ONLYOFFICE.

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
