//! Embed the ONLYOFFICE plugin (hosts/onlyoffice) into the binary so that
//! `lavatex setup onlyoffice` can install it without a separate download.

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

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    // Not canonicalized: on Windows that yields \\?\ paths.
    let plugin = manifest
        .join("..")
        .join("..")
        .join("hosts")
        .join("onlyoffice");
    assert!(
        plugin.is_dir(),
        "hosts/onlyoffice not found at {}",
        plugin.display()
    );
    println!("cargo:rerun-if-changed={}", plugin.display());

    let mut files = Vec::new();
    collect(&plugin, &plugin, &mut files);
    let mut code = String::from("/// ONLYOFFICE plugin files: (path inside the plugin folder, contents).\npub static PLUGIN_FILES: &[(&str, &[u8])] = &[\n");
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let rel = path
            .strip_prefix(&plugin)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        writeln!(
            code,
            "    ({rel:?}, include_bytes!({:?})),",
            path.to_string_lossy()
        )
        .unwrap();
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("plugin_files.rs");
    std::fs::write(out, code).unwrap();
}
