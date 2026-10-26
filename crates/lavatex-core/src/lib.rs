//! LavaTeX core: find the user's TeX installation, render LaTeX equations to
//! SVG / PDF / PNG, and embed or read back the metadata that makes an inserted
//! picture editable again.

pub mod config;
pub mod metadata;
pub mod normalize;
pub mod render;
pub mod toolchain;

pub use metadata::EquationMeta;
pub use render::{render, render_with, Format, Mode, RenderError, RenderOutput, RenderRequest};
pub use toolchain::{detect, Engine, Toolchain};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Start TeX tools without a console window. On Windows, a console program
/// started from a program without a console (the PowerPoint editor window,
/// `lavatex-helper`) otherwise opens a new terminal window every time.
pub(crate) fn hide_console(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
