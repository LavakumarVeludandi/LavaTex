# Security

## Threat model

LavaTeX compiles LaTeX, which is a programming language. The main risks are:

1. **Equations from other people's documents.** The source and preamble stored in a picture come from whoever made the file.
2. **Other software on the same machine** talking to the ONLYOFFICE helper.

## Defences

- **No compilation without a user action.** Opening a document never compiles anything. Rendering happens only when the user previews or inserts.
- **No shell escape.** Every engine runs with `-no-shell-escape` (TeX Live) or `-disable-write18` (MiKTeX), and with `shell_escape=f` in the environment.
- **File access limited.** `openin_any=p` and `openout_any=p` stop LaTeX reading or writing files outside its fresh temporary working directory and the TeX tree, and refuse absolute paths, `..` and dotfiles.
- **No shell interpolation.** Programs are started with argument arrays. User text is only ever written into the `.tex` file.
- **Limits.** Source and preamble are capped at 64 KiB, output at 20 MiB, and each step has a timeout (default 30 s, maximum 120 s). Metadata records over 256 KiB are rejected when read.
- **Untrusted preambles are flagged.** When an equation brings a preamble that differs from the user's own, the dialog says so before rendering.
- **Helper (`lavatex serve`):**
  - loopback only;
  - Host-header allow-list against DNS rebinding;
  - random bearer token stored with owner-only permissions, compared in constant time;
  - request bodies capped at 512 KiB;
  - no endpoints that read files or run anything other than the renderer.

Custom preambles and packages extend what LaTeX can do. Packages from CTAN are as trusted as your TeX installation.

## Reporting a vulnerability

Please open a private security advisory on the GitHub repository rather than a public issue.
