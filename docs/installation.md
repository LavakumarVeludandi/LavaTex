# Installing LavaTeX

LavaTeX needs two things: a **TeX distribution** (LavaTeX uses yours, just like IguanaTeX does) and the **LavaTeX package for your office suite**.

- [1. Install TeX](#1-install-tex)
- [2. LibreOffice Impress and Draw](#2-libreoffice-impress-and-draw)
- [3. ONLYOFFICE Presentation Editor](#3-onlyoffice-presentation-editor)
- [Choosing a TeX installation, engine and format](#choosing-a-tex-installation-engine-and-format)
- [Uninstalling](#uninstalling)
- [Troubleshooting](#troubleshooting)

Downloads are on the [releases page](https://github.com/LavakumarVeludandi/LavaTex/releases):

| File | What it is |
|---|---|
| `lavatex-libreoffice-<version>-multi.oxt` | LibreOffice extension for every platform. **Use this one if unsure.** |
| `lavatex-libreoffice-<version>-<platform>.oxt` | Smaller LibreOffice extension for one platform. |
| `lavatex-<platform>.zip` | The `lavatex` program. Needed for ONLYOFFICE, optional otherwise. |
| `lavatex-onlyoffice-<version>.plugin` | The ONLYOFFICE plugin on its own. You don't normally need it: `lavatex setup onlyoffice` installs the plugin for you. |

Platforms are `windows-x86_64`, `macos-aarch64` (Apple silicon), `macos-x86_64` (Intel), `linux-x86_64` and `linux-aarch64`.

## 1. Install TeX

If you already compile LaTeX documents (in VS Code, TeXstudio, Overleaf's desktop tools, …) you have TeX and can skip this step.

| System | Command or download |
|---|---|
| Fedora | `sudo dnf install texlive-scheme-medium texlive-dvisvgm` |
| Ubuntu, Debian | `sudo apt install texlive-latex-recommended texlive-latex-extra dvisvgm` |
| Arch | `sudo pacman -S texlive-basic texlive-latex texlive-latexrecommended texlive-binextra` |
| Windows | [MiKTeX](https://miktex.org/download) or [TeX Live](https://tug.org/texlive/windows.html) |
| macOS | [MacTeX](https://tug.org/mactex/) (or `brew install --cask mactex-no-gui`) |
| Anywhere | [Tectonic](https://tectonic-typesetting.github.io/) (a single program; downloads packages on first use) |

SVG output, the default, needs `latex` and `dvisvgm`; both are part of every full TeX Live and MiKTeX install. PNG output uses `dvipng` or Poppler's `pdftocairo`.

**MiKTeX users:** MiKTeX may want to download a missing package the first time it is used. If a render times out, open the MiKTeX Console and set *Settings › Always install missing packages on-the-fly*.

## 2. LibreOffice Impress and Draw

Needs LibreOffice 7.0 or newer. On Linux the distribution's LibreOffice must include Python scripting (Fedora: `libreoffice-pyuno`, Debian/Ubuntu: `libreoffice-script-provider-python`).

1. Download `lavatex-libreoffice-<version>-multi.oxt`.
2. In LibreOffice choose *Tools › Extension Manager…*, click **Add…** and pick the file (or simply double-click the `.oxt`).
3. Restart LibreOffice.

The extension contains the `lavatex` program for every platform, so nothing else needs installing.

After the restart LavaTeX appears in four places:

| Where | Look for |
|---|---|
| *Insert* menu | **LaTeX Equation…** |
| Standard toolbar | the **Σ** button, next to *Insert Image* |
| *Tools › Add-Ons* | **LaTeX Equation (LavaTeX)…** |
| Tabbed interface (*View › User Interface › Tabbed*) | **Extension** tab › **Σ** |

![LaTeX Equation in LibreOffice's Insert menu](images/libreoffice-insert-menu.png)

![The LavaTeX button on the Extension tab of LibreOffice's Tabbed interface](images/libreoffice-ribbon.png)

Continue with the [usage guide](usage.md).

## 3. ONLYOFFICE Presentation Editor

Needs ONLYOFFICE Desktop Editors 8.3 or newer; **10.0 or newer** to edit equations after inserting them.

ONLYOFFICE plugins are web pages and are not allowed to start programs such as `latex`. So LavaTeX for ONLYOFFICE comes with a small **helper** program that does the rendering. One command sets everything up, once:

1. Download `lavatex-<platform>.zip` for your system and unzip it. It contains `lavatex` and `lavatex-helper`.
2. Open a terminal in the unzipped folder and run:

   ```console
   ./lavatex setup onlyoffice          # Windows: .\lavatex.exe setup onlyoffice
   ```

   ```text
   Setting up LavaTeX for ONLYOFFICE…

     ✓ TeX found: TeX Live 2026 (/usr/local/texlive/2026/bin/x86_64-linux)
     ✓ Plugin installed for ONLYOFFICE Desktop Editors
         /home/you/.local/share/onlyoffice/desktopeditors/data/sdkjs-plugins/{6C1B2C4E-8D53-4F0A-9B7E-4A1E5D2F7C31}
     ✓ LavaTeX installed in /home/you/.local/share/lavatex/bin
     ✓ Helper starts in the background when you log in (running now)
     ✓ Helper is running and paired with the plugin

   Done. Restart ONLYOFFICE, open a presentation, and choose Plugins › LavaTeX.
   To remove everything again: lavatex setup onlyoffice --uninstall
   ```

3. Restart ONLYOFFICE, open a **presentation**, and click **LavaTeX** on the **Plugins** tab.

![LavaTeX on ONLYOFFICE's Plugins tab](images/onlyoffice-plugins-tab.png)

That's all. You can delete the unzipped folder afterwards; the setup copied the programs to a permanent place.

### What the setup does

| Step | Linux | macOS | Windows |
|---|---|---|---|
| Copies `lavatex` and `lavatex-helper` to | `~/.local/share/lavatex/bin` | `~/Library/Application Support/LavaTeX/bin` | `%LOCALAPPDATA%\Programs\LavaTeX` |
| Installs the plugin into ONLYOFFICE's user plugin folder | native, Flatpak and Snap installs | `~/Library/Application Support/asc.onlyoffice.ONLYOFFICE/data/sdkjs-plugins` | `%LOCALAPPDATA%\ONLYOFFICE\DesktopEditors\data\sdkjs-plugins` |
| Starts the helper at login | systemd user socket (the helper only runs while it's being used), or an autostart entry on systems without systemd | a LaunchAgent | a *Run* entry in the registry (no console window) |

It also writes a pairing file next to the plugin, so the plugin finds the helper and its access token by itself. Nothing is installed system-wide and no administrator rights are needed. The helper only accepts connections from this computer and only from programs that know the token.

Options:

```console
lavatex setup onlyoffice --plugins-dir <folder>   # ONLYOFFICE's sdkjs-plugins folder, if it isn't found automatically
lavatex setup onlyoffice --no-autostart           # don't start the helper at login; run `lavatex serve` yourself
lavatex setup onlyoffice --port 7355              # use another local port
```

Run the setup again at any time, for example after updating LavaTeX or ONLYOFFICE. It replaces the previous installation.

### If the plugin says "One-time setup needed"

![The plugin's setup screen](images/onlyoffice-setup-needed.png)

The plugin couldn't reach the helper. Run `lavatex setup onlyoffice`, then click **Try again**. If the setup finished earlier, the helper might have been stopped: running the setup again restarts it.

### ONLYOFFICE Document Server (web)

The plugin works in Document Server too, but the helper has to run on each user's own computer, because that's where TeX is. Install the plugin on the server as usual, have each user run `lavatex serve` (or `lavatex setup onlyoffice --plugins-dir <any folder>` for autostart), and enter the helper's address and the output of `lavatex token` under **Advanced: connect to a helper manually** on the plugin's first screen. Document Server support is not tested yet.

## Choosing a TeX installation, engine and format

LavaTeX picks a TeX installation automatically. To see what it found:

```console
$ lavatex detect
TeX Live 2026 (suggested)
  folder:  /usr/local/texlive/2026/bin/x86_64-linux
  engines: latex, pdflatex, xelatex, lualatex
  formats: svg, pdf, png
```

Both dialogs also have a **TeX** list to choose an installation and an **Engine** list. To set defaults for every host from the command line:

```console
lavatex config --toolchain "C:\texlive\2026\bin\windows"   # a folder containing latex
lavatex config --engine xelatex                            # latex, pdflatex, xelatex, lualatex or tectonic
lavatex config --format svg                                # svg, pdf or png
lavatex config                                             # show the current settings
```

Settings are stored in:

| OS | Folder |
|---|---|
| Linux | `~/.config/lavatex` (or `$XDG_CONFIG_HOME/lavatex`) |
| macOS | `~/Library/Application Support/LavaTeX` |
| Windows | `%APPDATA%\LavaTeX` |

That folder holds `config.json` (TeX, engine, format), `helper-token` and `libreoffice.json` (LibreOffice dialog defaults). Set `LAVATEX_CONFIG_DIR` to use another folder.

## Uninstalling

- **LibreOffice:** *Tools › Extension Manager*, select LavaTeX, **Remove**.
- **ONLYOFFICE:** run `lavatex setup onlyoffice --uninstall`. It removes the plugin, the autostart entry and the installed programs.

Equations you already inserted stay in your presentations as ordinary pictures.

## Troubleshooting

| Problem | What to do |
|---|---|
| "No TeX installation was found" | Install TeX (step 1). If it's installed in an unusual place, run `lavatex config --toolchain <folder containing latex>`. |
| "*name*.sty is not installed" | Install the package: `tlmgr install name` (TeX Live), the MiKTeX Console, or your distribution's package manager (Fedora: `sudo dnf install 'tex(name.sty)'`). Or remove it from the preamble. |
| Rendering times out | MiKTeX or Tectonic is probably waiting to download a package. Run `lavatex render 'x'` in a terminal to see what it is doing. |
| LibreOffice: no LavaTeX menu entries | Restart LibreOffice completely (also its quick starter). On Linux, check that LibreOffice's Python support is installed (see step 2). |
| LibreOffice: no PDF in the *Output* list | Your LibreOffice build can't display PDF pictures (Fedora's can't). Use SVG, which is just as sharp. |
| ONLYOFFICE: LavaTeX isn't on the Plugins tab | Make sure a **presentation** is open. Run `lavatex setup onlyoffice` again and restart ONLYOFFICE. If ONLYOFFICE is installed in an unusual way, pass `--plugins-dir`. |
| ONLYOFFICE: "One-time setup needed" or "helper not available" | Run `lavatex setup onlyoffice`, then click **Try again**. |
| ONLYOFFICE: equations can't be edited | Editing needs ONLYOFFICE 10.0 or newer. |
| Equation is no longer editable after converting the file | Some converters drop alt text. The picture may still carry its source: `lavatex inspect picture.svg`. |
