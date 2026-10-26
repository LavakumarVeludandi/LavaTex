//! `lavatex-helper`: the local render helper as a background program. It is
//! what autostart entries launch; on Windows it has no console window.
//!
//! Usage: lavatex-helper [--idle-exit SECONDS]
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let idle_exit = args
        .iter()
        .position(|a| a == "--idle-exit")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());
    lavatex_cli::serve::run(lavatex_cli::serve::DEFAULT_PORT, None, idle_exit)
}
