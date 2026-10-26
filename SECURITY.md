# Security

## Threat model

LavaTeX compiles LaTeX, which is a programming language. The main risks are:

1. **Equations from other people's documents.** The source and preamble stored in a picture come from whoever made the file.
2. **Other software on the same machine**, and **web pages in the browser**, talking to the helper used by ONLYOFFICE and PowerPoint.

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
  - random bearer token stored with owner-only permissions, compared in constant time; `lavatex setup onlyoffice` also writes it, owner-only, into the plugin's own folder so the plugin can pair;
  - request bodies capped at 512 KiB;
  - no endpoints that read files or run anything other than the renderer; `/v1/shutdown` (token required) only stops the helper.
- **PowerPoint add-in over HTTPS:**
  - the certificate authority created by `lavatex setup powerpoint` is name-constrained to `localhost`, `127.0.0.1` and `::1`, and its private key is deleted right after it signs the helper's certificate, so it cannot be used to impersonate other sites;
  - the helper's private key is stored with owner-only permissions;
  - the add-in pages contain the access token, so they are served only over HTTPS, without CORS headers, and refused to cross-site requests and foreign origins.


Custom preambles and packages extend what LaTeX can do. Packages from CTAN are as trusted as your TeX installation.

## Reporting a vulnerability

Please open a private security advisory on the GitHub repository rather than a public issue.
