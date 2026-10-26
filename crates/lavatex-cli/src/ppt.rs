//! Commands used by the PowerPoint add-in (`LavaTeX.ppam`).
//!
//! The add-in is VBA. It runs the bundled `lavatex-helper.exe` and reads
//! plain files back, so VBA never parses JSON:
//!
//! - `editor`: shows the editor window (live preview). On *Insert* it writes
//!   the picture, `alt.txt` and `result.txt` into `--out` and exits with 0;
//!   on *Cancel* it exits with 1.
//! - `rerender`: renders an equation again from its alt text (Regenerate,
//!   or PNG instead of SVG for PowerPoint versions without SVG support).
//!
//! `result.txt` holds `key=value` lines (`file`, `format`, `widthBp`,
//! `heightBp`, `depthBp`, and when editing `previousWidthBp`, the natural
//! width of the equation being replaced, so the add-in can keep its scale).
//! Errors go to `error.txt`. All text is UTF-8.

use crate::apply_config;
use lavatex_core::config::{config_dir, Config};
use lavatex_core::metadata::EquationMeta;
use lavatex_core::render::{self, Format, Mode, RenderOutput, RenderRequest};
use lavatex_core::{toolchain, Engine};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Exit codes the add-in understands.
pub const EXIT_OK: u8 = 0;
pub const EXIT_CANCELLED: u8 = 1;
pub const EXIT_FAILED: u8 = 2;

const SETTINGS_FILE: &str = "powerpoint.json";

#[derive(clap::Args, Debug)]
pub struct EditorArgs {
    /// File with the alt text (or LAVATEX tag) of the equation to edit.
    #[arg(long)]
    pub alt_text_file: Option<PathBuf>,
    /// Folder to write the result into.
    #[arg(long)]
    pub out: PathBuf,
    /// Instead of a window, exchange the page's messages as JSON lines on
    /// stdin/stdout (for testing the editor page in a browser).
    #[arg(long, hide = true)]
    pub stdio: bool,
}

#[derive(clap::Args, Debug)]
pub struct RerenderArgs {
    /// File with the alt text (or LAVATEX tag) of the equation.
    #[arg(long)]
    pub alt_text_file: PathBuf,
    /// Folder to write the result into.
    #[arg(long)]
    pub out: PathBuf,
    /// Output format instead of the stored one (svg, pdf, png).
    #[arg(long)]
    pub format: Option<String>,
}

/// Defaults for new equations, remembered between uses.
pub fn load_settings() -> Value {
    let mut s = json!({
        "mode": "display",
        "fontSize": 20,
        "color": "#000000",
        "format": "svg",
        "pngDpi": 300,
        "preamble": render::DEFAULT_PREAMBLE,
        "toolchain": "",
        "engine": "",
    });
    if let Some(saved) = config_dir()
        .and_then(|d| std::fs::read_to_string(d.join(SETTINGS_FILE)).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
    {
        for (k, v) in saved {
            if s.get(&k).is_some() {
                s[&k] = v;
            }
        }
    }
    s
}

fn save_settings(request: &Value) {
    let Some(dir) = config_dir() else { return };
    let mut s = load_settings();
    for key in [
        "mode",
        "fontSize",
        "color",
        "format",
        "pngDpi",
        "preamble",
        "toolchain",
        "engine",
    ] {
        if let Some(v) = request.get(key) {
            s[key] = v.clone();
        }
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(SETTINGS_FILE), s.to_string());
}

fn read_meta(path: &Path) -> Result<EquationMeta, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    EquationMeta::from_alt_text(&text)
        .map_err(|e| format!("This picture is not a LavaTeX equation ({e})."))
}

/// The editor's starting values: the equation being edited, or the defaults.
pub fn initial_state(meta: Option<&EquationMeta>) -> Value {
    let settings = load_settings();
    match meta {
        Some(m) => {
            let mut note = String::new();
            if settings["preamble"].as_str() != Some(m.preamble.as_str()) {
                note = "This equation has its own preamble; review it if the file came from someone else.".into();
            }
            json!({
                "editing": true,
                "note": note,
                "values": {
                    "source": m.source,
                    "preamble": m.preamble,
                    "mode": m.mode,
                    "fontSize": m.font_size,
                    "color": m.color,
                    "format": if m.format == "png" { "png" } else { "svg" },
                    "pngDpi": settings["pngDpi"],
                    "toolchain": settings["toolchain"],
                    "engine": m.engine,
                }
            })
        }
        None => {
            let mut values = settings;
            values["source"] = json!("");
            json!({ "editing": false, "note": "", "values": values })
        }
    }
}

/// Build a render request from the editor's values.
pub fn request_from_values(v: &Value) -> RenderRequest {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let mut req = RenderRequest {
        source: s("source"),
        preamble: v
            .get("preamble")
            .and_then(Value::as_str)
            .map(str::to_string),
        mode: Mode::parse(&s("mode")).unwrap_or(Mode::Display),
        font_size: v.get("fontSize").and_then(Value::as_f64).unwrap_or(20.0),
        color: Some(s("color"))
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "#000000".into()),
        format: Format::parse(&s("format")).unwrap_or(Format::Svg),
        png_dpi: v.get("pngDpi").and_then(Value::as_u64).unwrap_or(300) as u32,
        engine: Engine::parse(&s("engine")),
        toolchain: Some(s("toolchain")).filter(|t| !t.is_empty()),
        ..Default::default()
    };
    apply_config(&mut req, &Config::load());
    req
}

fn render_request(req: &RenderRequest) -> Result<RenderOutput, render::RenderError> {
    match req.toolchain.as_deref().filter(|t| *t != "auto") {
        Some(_) => render::render(req),
        None => match toolchain::detect().into_iter().next() {
            Some(tc) => render::render_with(req, &tc),
            None => render::render(req),
        },
    }
}

fn error_json(e: &render::RenderError) -> Value {
    json!({"ok": false, "error": e})
}

fn output_json(out: &RenderOutput) -> Value {
    use base64::Engine as _;
    let mut v = serde_json::to_value(out).expect("serializes");
    v["ok"] = true.into();
    v["dataBase64"] = base64::engine::general_purpose::STANDARD
        .encode(&out.bytes)
        .into();
    v
}

/// Write the picture and its description for the add-in to pick up.
pub fn write_result(
    out_dir: &Path,
    out: &RenderOutput,
    previous: Option<&EquationMeta>,
) -> Result<(), String> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    let file = format!("equation.{}", out.format.name());
    let io = |e: std::io::Error| format!("cannot write into {}: {e}", out_dir.display());
    std::fs::write(out_dir.join(&file), &out.bytes).map_err(io)?;
    std::fs::write(out_dir.join("alt.txt"), &out.alt_text).map_err(io)?;
    let mut result = format!(
        "file={file}\nformat={}\nwidthBp={}\nheightBp={}\ndepthBp={}\n",
        out.format.name(),
        out.width_bp,
        out.height_bp,
        out.depth_bp
    );
    if let Some(p) = previous.filter(|p| p.width_bp > 0.0) {
        result.push_str(&format!("previousWidthBp={}\n", p.width_bp));
    }
    // Written last: its presence means the other files are complete.
    std::fs::write(out_dir.join("result.txt"), result).map_err(io)
}

fn write_error(out_dir: &Path, message: &str) {
    let _ = std::fs::create_dir_all(out_dir);
    let _ = std::fs::write(out_dir.join("error.txt"), message);
}

/// What the editor window should do after a message from its page.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// Send this JSON back to the page.
    Reply(Value),
    /// Close the window and exit with this code.
    Exit(u8),
}

/// Handle one message from the editor page: `{cmd, ...}`.
pub fn handle_message(msg: &Value, out_dir: &Path, previous: Option<&EquationMeta>) -> Action {
    match msg.get("cmd").and_then(Value::as_str) {
        Some("toolchains") => {
            let list = toolchain::detect();
            let suggested = list.first().map(|t| t.id.clone());
            Action::Reply(json!({"ok": true, "toolchains": list, "suggested": suggested}))
        }
        Some("render") => {
            let req = request_from_values(msg.get("values").unwrap_or(&Value::Null));
            Action::Reply(match render_request(&req) {
                Ok(out) => output_json(&out),
                Err(e) => error_json(&e),
            })
        }
        Some("insert") => {
            let values = msg.get("values").cloned().unwrap_or(Value::Null);
            let req = request_from_values(&values);
            match render_request(&req) {
                Ok(out) => match write_result(out_dir, &out, previous) {
                    Ok(()) => {
                        save_settings(&values);
                        Action::Exit(EXIT_OK)
                    }
                    Err(e) => Action::Reply(json!({"ok": false, "error": {"summary": e}})),
                },
                Err(e) => Action::Reply(error_json(&e)),
            }
        }
        Some("cancel") => Action::Exit(EXIT_CANCELLED),
        _ => Action::Reply(json!({"ok": false, "error": {"summary": "unknown command"}})),
    }
}

pub fn rerender(args: &RerenderArgs) -> ExitCode {
    let meta = match read_meta(&args.alt_text_file) {
        Ok(m) => m,
        Err(e) => {
            write_error(&args.out, &e);
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let mut req = RenderRequest::from_meta(&meta);
    if let Some(f) = args.format.as_deref().and_then(Format::parse) {
        req.format = f;
    }
    let settings = load_settings();
    if let Some(tc) = settings["toolchain"].as_str().filter(|t| !t.is_empty()) {
        req.toolchain = Some(tc.to_string());
    }
    if let Some(dpi) = settings["pngDpi"].as_u64() {
        req.png_dpi = dpi as u32;
    }
    apply_config(&mut req, &Config::load());
    match render_request(&req)
        .map_err(|e| e.to_string())
        .and_then(|out| write_result(&args.out, &out, Some(&meta)))
    {
        Ok(()) => ExitCode::from(EXIT_OK),
        Err(e) => {
            write_error(&args.out, &e);
            ExitCode::from(EXIT_FAILED)
        }
    }
}

pub fn editor(args: &EditorArgs) -> ExitCode {
    let meta = match args.alt_text_file.as_deref().map(read_meta).transpose() {
        Ok(m) => m,
        Err(e) => {
            write_error(&args.out, &e);
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let init = initial_state(meta.as_ref());
    if args.stdio {
        return editor_stdio(init, &args.out, meta.as_ref());
    }
    #[cfg(windows)]
    {
        crate::editor_window::run(init, args.out.clone(), meta)
    }
    #[cfg(not(windows))]
    {
        let _ = init;
        let msg = "The LavaTeX editor window is only available on Windows.";
        write_error(&args.out, msg);
        eprintln!("{msg}");
        ExitCode::from(EXIT_FAILED)
    }
}

/// `editor --stdio`: print `{"init": ...}`, then answer each message line
/// with `{"reply": ...}` or `{"exit": code}`.
fn editor_stdio(init: Value, out: &Path, previous: Option<&EquationMeta>) -> ExitCode {
    use std::io::BufRead;
    println!("{}", json!({ "init": init }));
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match handle_message(&msg, out, previous) {
            Action::Reply(v) => println!("{}", json!({ "reply": v })),
            Action::Exit(code) => {
                println!("{}", json!({ "exit": code }));
                return ExitCode::from(code);
            }
        }
    }
    ExitCode::from(EXIT_CANCELLED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_become_a_request() {
        let req = request_from_values(&json!({
            "source": "x^2", "preamble": "\\usepackage{amsmath}", "mode": "inline",
            "fontSize": 28, "color": "#ff0000", "format": "png", "pngDpi": 600, "engine": "xelatex"
        }));
        assert_eq!(req.source, "x^2");
        assert_eq!(req.mode, Mode::Inline);
        assert_eq!(req.font_size, 28.0);
        assert_eq!(req.format, Format::Png);
        assert_eq!(req.png_dpi, 600);
        assert_eq!(req.engine, Some(Engine::Xelatex));
    }

    #[test]
    fn editing_state_comes_from_the_record() {
        let meta = EquationMeta::from_json(
            r##"{"schema":"lavatex-equation/1","source":"a+b","preamble":"\\usepackage{foo}","mode":"display","fontSize":24,"color":"#00ff00","format":"png","engine":"","widthBp":10,"heightBp":5}"##,
        )
        .unwrap();
        let init = initial_state(Some(&meta));
        assert_eq!(init["editing"], true);
        assert_eq!(init["values"]["source"], "a+b");
        assert_eq!(init["values"]["format"], "png");
        assert!(init["note"].as_str().unwrap().contains("own preamble"));
        let fresh = initial_state(None);
        assert_eq!(fresh["editing"], false);
        assert_eq!(fresh["values"]["source"], "");
    }

    #[test]
    fn cancel_and_unknown_messages() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            handle_message(&json!({"cmd": "cancel"}), dir.path(), None),
            Action::Exit(EXIT_CANCELLED)
        );
        assert!(matches!(
            handle_message(&json!({"cmd": "nope"}), dir.path(), None),
            Action::Reply(_)
        ));
    }
}
