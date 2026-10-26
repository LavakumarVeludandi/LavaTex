//! Shared code for the `lavatex` command and the `lavatex-helper` background
//! program: the local render helper, the one-time host setup, and the
//! commands used by the PowerPoint add-in.

#[cfg(windows)]
mod editor_window;
pub mod ppt;
pub mod serve;
pub mod setup;

/// Host files compiled into the binary (see build.rs).
pub mod embedded {
    include!(concat!(env!("OUT_DIR"), "/plugin_files.rs"));
}

use base64::Engine as _;
use lavatex_core::config::Config;
use lavatex_core::render::RenderRequest;
use lavatex_core::Engine;

/// Fill in unset choices from the shared config file.
pub fn apply_config(req: &mut RenderRequest, config: &Config) {
    if req.toolchain.as_deref().map_or(true, |t| t.is_empty()) && !config.toolchain.is_empty() {
        req.toolchain = Some(config.toolchain.clone());
    }
    if req.engine.is_none() {
        req.engine = Engine::parse(&config.engine);
    }
}

pub fn new_token() -> String {
    let mut bytes = [0u8; 24];
    getrandom::getrandom(&mut bytes).expect("OS random number generator");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}
