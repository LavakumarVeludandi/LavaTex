//! User-level settings shared by every host integration (the preferred TeX
//! installation and engine, and the local helper's access token).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// Toolchain id / bin directory; empty means "auto".
    pub toolchain: String,
    /// Default engine name; empty means the toolchain's default.
    pub engine: String,
    /// Default output format: "svg", "pdf" or "png".
    pub format: String,
}

/// `$XDG_CONFIG_HOME/lavatex`, `~/Library/Application Support/LavaTeX` or
/// `%APPDATA%\LavaTeX`.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("LAVATEX_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("LavaTeX"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Application Support/LavaTeX"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join("lavatex"))
    }
}

fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.json"))
}

impl Config {
    pub fn load() -> Config {
        config_file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<PathBuf> {
        let path = config_file().ok_or_else(|| std::io::Error::other("no config directory"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            &path,
            serde_json::to_string_pretty(self).expect("serializes"),
        )?;
        Ok(path)
    }
}

/// Read the helper token, creating one with `generate` on first use. The file
/// is created with owner-only permissions where the OS supports it.
pub fn load_or_create_token(generate: impl FnOnce() -> String) -> std::io::Result<String> {
    let dir = config_dir().ok_or_else(|| std::io::Error::other("no config directory"))?;
    let path = dir.join("helper-token");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let t = existing.trim().to_string();
        if t.len() >= 16 {
            return Ok(t);
        }
    }
    std::fs::create_dir_all(&dir)?;
    let token = generate();
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    options.open(&path)?.write_all(token.as_bytes())?;
    Ok(token)
}
