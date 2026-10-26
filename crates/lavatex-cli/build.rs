//! Embed the ONLYOFFICE plugin (hosts/onlyoffice) and the editor window page
//! (hosts/editor, used by the PowerPoint add-in) into the binary, so they
//! need no separate download.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn collect(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        let rel = path.strip_prefix(root).unwrap();
        let first = rel
            .components()
            .next()
            .unwrap()
            .as_os_str()
            .to_string_lossy()
            .into_owned();
        // Development-only files are not part of the plugin.
        if matches!(first.as_str(), "tests" | "node_modules" | "package.json") {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            out.push(path);
        }
    }
}

fn host_files(dir: &Path) -> Vec<(String, PathBuf)> {
    assert!(dir.is_dir(), "{} not found", dir.display());
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files = Vec::new();
    collect(dir, dir, &mut files);
    files
        .into_iter()
        .map(|path| {
            let rel = path
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (rel, path)
        })
        .collect()
}

fn write_table(code: &mut String, name: &str, doc: &str, files: &[(String, PathBuf)]) {
    writeln!(code, "/// {doc}\npub static {name}: &[(&str, &[u8])] = &[").unwrap();
    for (rel, path) in files {
        println!("cargo:rerun-if-changed={}", path.display());
        writeln!(
            code,
            "    ({rel:?}, include_bytes!({:?})),",
            path.to_string_lossy()
        )
        .unwrap();
    }
    code.push_str("];\n");
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    // Not canonicalized: on Windows that yields \\?\ paths.
    let hosts = manifest.join("..").join("..").join("hosts");
    let onlyoffice = host_files(&hosts.join("onlyoffice"));
    let mut editor = host_files(&hosts.join("editor"));
    // The editor shares the browser core with the ONLYOFFICE plugin.
    let core = onlyoffice
        .iter()
        .find(|(rel, _)| rel == "scripts/lavatex-core.js")
        .expect("hosts/onlyoffice/scripts/lavatex-core.js")
        .1
        .clone();
    editor.push(("lavatex-core.js".to_string(), core));

    let mut code = String::new();
    write_table(
        &mut code,
        "PLUGIN_FILES",
        "ONLYOFFICE plugin files: (path inside the plugin folder, contents).",
        &onlyoffice,
    );
    write_table(
        &mut code,
        "EDITOR_FILES",
        "Editor window page files: (path, contents).",
        &editor,
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("plugin_files.rs");
    std::fs::write(out, code).unwrap();
}
