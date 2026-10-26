# Contributing

Thanks for helping. A few ground rules:

- **TeX work goes in the Rust core.** Host integrations should only collect input, call `lavatex`, and place pictures.
- **Changing the metadata format** means updating `docs/metadata.md`, regenerating the fixture with `LAVATEX_UPDATE_FIXTURES=1 cargo test`, and keeping the JavaScript and Python readers passing.
- **Never weaken the sandbox** (shell escape, `openin_any`/`openout_any`, timeouts) without discussing it in an issue first.
- **Do not copy code from IguanaTeX, TexMaths or the ONLYOFFICE plugin SDK** into this repository. Their licences differ from ours.

## Running the tests

```console
cargo fmt --check && cargo clippy --all-targets && cargo test
node --test hosts/onlyoffice/tests/core.test.js
node hosts/onlyoffice/tests/e2e.js target/debug/lavatex        # needs Playwright + TeX
python3 hosts/libreoffice/tests/test_headless.py              # needs LibreOffice + python3-uno + TeX
```
