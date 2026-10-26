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
