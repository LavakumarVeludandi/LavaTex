//! `lavatex-helper`: LavaTeX without a console window (on Windows).
//!
//! - `lavatex-helper [--idle-exit SECONDS]`: the local render helper; this is
//!   what autostart entries launch.
//! - `lavatex-helper editor|rerender ...`: the commands the PowerPoint
//!   add-in runs (see `lavatex_cli::ppt`).
#![cfg_attr(windows, windows_subsystem = "windows")]

use clap::Parser;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "lavatex-helper")]
enum PptCommand {
    Editor(lavatex_cli::ppt::EditorArgs),
    Rerender(lavatex_cli::ppt::RerenderArgs),
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if matches!(args.get(1).map(String::as_str), Some("editor" | "rerender")) {
        return match PptCommand::try_parse_from(&args) {
            Ok(PptCommand::Editor(a)) => lavatex_cli::ppt::editor(&a),
            Ok(PptCommand::Rerender(a)) => lavatex_cli::ppt::rerender(&a),
            Err(e) => {
                // No console on Windows: leave the message where the add-in looks.
                if let Some(out) = args
                    .iter()
                    .position(|a| a == "--out")
                    .and_then(|i| args.get(i + 1))
                {
                    let _ = std::fs::create_dir_all(out);
                    let _ =
                        std::fs::write(std::path::Path::new(out).join("error.txt"), e.to_string());
                }
                ExitCode::from(lavatex_cli::ppt::EXIT_FAILED)
            }
        };
    }
    let idle_exit = args
        .iter()
        .position(|a| a == "--idle-exit")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());
    lavatex_cli::serve::run(lavatex_cli::serve::DEFAULT_PORT, None, idle_exit)
}
