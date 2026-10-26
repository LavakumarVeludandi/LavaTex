# Building the PowerPoint add-in (maintainers)

`LavaTeX.ppam` is saved by hand in PowerPoint, the same way IguanaTeX's add-in is made. It only needs to be rebuilt when its VBA code (`hosts/powerpoint/vba/LavaTeX.bas`) or its ribbon (`hosts/powerpoint/ribbon/`) changes. Changes to the rendering, the editor window or anything else in the Rust code need no new `.ppam`: CI builds `lavatex-helper.exe` for every release and packs it next to the committed `.ppam`.

| File | What it is |
|---|---|
| `hosts/powerpoint/vba/LavaTeX.bas` | The add-in's VBA module: ribbon callbacks, running `lavatex-helper.exe`, inserting and replacing pictures. |
| `hosts/powerpoint/ribbon/customUI14.xml` | The LavaTeX ribbon tab. |
| `hosts/powerpoint/ribbon/images/*.png` | Ribbon icons (made by `tools/powerpoint-icons.js`). |
| `hosts/powerpoint/LavaTeX.ppam` | The add-in built from the above. **Committed**, packaged by CI. |
| `hosts/editor/` | The editor window's page, built into `lavatex-helper.exe`. |

## What you need (Windows)

- PowerPoint 2016 or newer.
- [Office RibbonX Editor](https://github.com/fernandreu/office-ribbonx-editor/releases), to add the ribbon tab.
- To test with your own build: Git, [Rust](https://rustup.rs) and the Visual Studio Build Tools with *Desktop development with C++*.

## First build

1. **Engine for testing.** In PowerShell:
   ```powershell
   git clone https://github.com/LavakumarVeludandi/LavaTex.git
   cd LavaTex
   cargo build --release -p lavatex-cli
   mkdir C:\LavaTeX
   copy .\target\release\lavatex-helper.exe C:\LavaTeX\
   ```
2. **Source presentation.** Open PowerPoint with a blank presentation and press **Alt+F11**.
   - *File › Import File…* and pick `hosts\powerpoint\vba\LavaTeX.bas`. A module **LavaTeX** appears under *Modules*.
   - *Debug › Compile VBAProject*. Nothing is shown when it succeeds; the menu item turns grey. A compile error shows a message box and highlights the line.
   - Close the VBA editor and save as **PowerPoint Macro-Enabled Presentation (\*.pptm)**, for example `C:\LavaTeX\LavaTeX-source.pptm`. Keep this file; later updates start from it.
   - Close PowerPoint.
3. **Ribbon.** Open `LavaTeX-source.pptm` in Office RibbonX Editor.
   - Right-click the file and choose **Insert Office 2010+ Custom UI Part**.
   - Paste the content of `hosts\powerpoint\ribbon\customUI14.xml`.
   - **Insert Icons**: select all PNGs in `hosts\powerpoint\ribbon\images\`. Keep their names; the ribbon refers to them by name.
   - **Validate**, then save and close.
4. **Add-in.** Open `LavaTeX-source.pptm` in PowerPoint (click **Enable Content** if asked) and save it as **PowerPoint Add-in (\*.ppam)** named `LavaTeX.ppam` in `C:\LavaTeX` (PowerPoint suggests its own add-in folder by default). If the LavaTeX tab is missing in the next step, add the ribbon to the `.ppam` with RibbonX Editor as in step 3.
5. **Test.** *File › Options › Add-ins › Manage: PowerPoint Add-ins › Go… › Add New…*, pick `C:\LavaTeX\LavaTeX.ppam`, **Enable Macros**. Try New Equation, Edit Equation and Regenerate.
6. **Commit** the add-in:
   ```powershell
   copy C:\LavaTeX\LavaTeX.ppam hosts\powerpoint\LavaTeX.ppam
   git add hosts/powerpoint/LavaTeX.ppam
   git commit -m "PowerPoint: update LavaTeX.ppam"
   ```

## Updating the VBA code

1. In PowerPoint: remove the add-in (*Add-ins › Go…*, select LavaTeX, **Remove**) so the file is not in use.
2. Open `LavaTeX-source.pptm`, press **Alt+F11**, right-click the **LavaTeX** module, **Remove LavaTeX** (answer **No** to exporting), then *File › Import File…* the new `LavaTeX.bas`. Compile, save.
3. Repeat steps 4 to 6 above. The ribbon stays in the source file; only redo step 3 when `customUI14.xml` or the icons change.

The version shown by **About** is the `LAVATEX_VERSION` constant at the top of `LavaTeX.bas`; keep it in step with the version in `Cargo.toml`.

## Releases

Pushing a `v*` tag makes CI build everything and attach `lavatex-powerpoint-<version>-windows-x86_64.zip` to the GitHub release. The zip holds the committed `LavaTeX.ppam`, the `lavatex-helper.exe` built for that release, a `README.txt` with the install steps, and the licence. A tag build fails if `hosts/powerpoint/LavaTeX.ppam` is missing; other builds only warn.

## How the add-in works

- The ribbon buttons call `LavaTeX_New`, `LavaTeX_Edit`, `LavaTeX_Regenerate`, `LavaTeX_Help` and `LavaTeX_About` in `LavaTeX.bas`.
- The add-in looks for `lavatex-helper.exe` next to `LavaTeX.ppam`, then in `%LOCALAPPDATA%\Programs\LavaTeX`.
- **New/Edit** run `lavatex-helper.exe editor --out <temp folder>` (for editing also `--alt-text-file` with the equation's stored LaTeX). The editor window (WebView2) renders previews with the same code as the other apps. On **Insert** it writes `equation.svg`, `alt.txt` and `result.txt` (`key=value` lines: file, format, sizes) and exits with 0; **Cancel** exits with 1; errors go to `error.txt`.
- **Regenerate** runs `lavatex-helper.exe rerender --alt-text-file … --out …` for each selected equation, without a window.
- The add-in inserts the picture, sets its alt text and the shape tag `LAVATEX` to the contents of `alt.txt`, and when replacing keeps position, rotation, stacking order and the user's scale (`previousWidthBp` in `result.txt`).
- If PowerPoint cannot insert SVG, the add-in asks for a PNG with `rerender --format png`.
