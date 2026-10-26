//! The rendering pipeline: LaTeX source in, SVG / PDF / PNG with embedded
//! metadata out.
//!
//! Every render runs in a fresh temporary directory, with shell escape
//! disabled, TeX file access restricted to that directory ("paranoid" mode),
//! no shell interpolation and a hard timeout.

use crate::metadata::{self, EquationMeta};
use crate::toolchain::{self, Distribution, Engine, Toolchain};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 20 * 1024 * 1024;
/// Equations are typeset at this size and then scaled to `font_size`, the same
/// approach IguanaTeX uses, so any font setup scales cleanly.
const BASE_FONT_PT: f64 = 10.0;
const TEX_PT_TO_BP: f64 = 72.0 / 72.27;

pub const DEFAULT_PREAMBLE: &str = "\\usepackage{amsmath}\n\\usepackage{amssymb}";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Svg,
    Pdf,
    Png,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Svg => "svg",
            Format::Pdf => "pdf",
            Format::Png => "png",
        }
    }
    pub fn mime(self) -> &'static str {
        match self {
            Format::Svg => "image/svg+xml",
            Format::Pdf => "application/pdf",
            Format::Png => "image/png",
        }
    }
    pub fn parse(s: &str) -> Option<Format> {
        match s.to_ascii_lowercase().as_str() {
            "svg" => Some(Format::Svg),
            "pdf" => Some(Format::Pdf),
            "png" => Some(Format::Png),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// `$\displaystyle ...$`
    Display,
    /// `$...$`
    Inline,
    /// Raw LaTeX in a horizontal box (text, `tikzpicture`, etc.).
    Text,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Display => "display",
            Mode::Inline => "inline",
            Mode::Text => "text",
        }
    }
    pub fn parse(s: &str) -> Option<Mode> {
        match s.to_ascii_lowercase().as_str() {
            "display" => Some(Mode::Display),
            "inline" => Some(Mode::Inline),
            "text" => Some(Mode::Text),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderRequest {
    pub source: String,
    /// `None` uses [`DEFAULT_PREAMBLE`].
    pub preamble: Option<String>,
    pub mode: Mode,
    /// Target size in points; the equation is scaled as if typeset at this size.
    pub font_size: f64,
    /// `#RRGGBB` or `#RGB`.
    pub color: String,
    pub format: Format,
    /// Raster resolution for PNG output.
    pub png_dpi: u32,
    pub engine: Option<Engine>,
    /// A toolchain id / bin directory; `None` or `"auto"` picks the best one.
    pub toolchain: Option<String>,
    pub timeout_secs: u64,
}

impl Default for RenderRequest {
    fn default() -> Self {
        RenderRequest {
            source: String::new(),
            preamble: None,
            mode: Mode::Display,
            font_size: 20.0,
            color: "#000000".into(),
            format: Format::Svg,
            png_dpi: 300,
            engine: None,
            toolchain: None,
            timeout_secs: 30,
        }
    }
}

impl RenderRequest {
    /// Rebuild a request from stored metadata (used when editing).
    pub fn from_meta(meta: &EquationMeta) -> Self {
        RenderRequest {
            source: meta.source.clone(),
            preamble: Some(meta.preamble.clone()),
            mode: Mode::parse(&meta.mode).unwrap_or(Mode::Display),
            font_size: meta.font_size,
            color: meta.color.clone(),
            format: Format::parse(&meta.format).unwrap_or(Format::Svg),
            engine: Engine::parse(&meta.engine),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderOutput {
    pub format: Format,
    pub mime: &'static str,
    #[serde(skip)]
    pub bytes: Vec<u8>,
    /// Display size in PostScript points (1/72 in) at the requested font size.
    pub width_bp: f64,
    pub height_bp: f64,
    pub depth_bp: f64,
    pub meta: EquationMeta,
    /// Ready-to-use alt text / description for the inserted picture.
    pub alt_text: String,
    pub engine: Engine,
    pub toolchain: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    InvalidInput,
    NoToolchain,
    Unsupported,
    Compile,
    Timeout,
    Tool,
    Output,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderError {
    pub kind: ErrorKind,
    /// One line suitable for a status bar.
    pub summary: String,
    /// Longer explanation / log excerpt for an expandable details view.
    pub details: String,
    /// Line in the user's source (1-based) when the error could be located.
    pub line: Option<usize>,
    /// Where `line` points: "source" or "preamble".
    pub location: Option<&'static str>,
}

impl RenderError {
    fn new(kind: ErrorKind, summary: impl Into<String>) -> Self {
        RenderError {
            kind,
            summary: summary.into(),
            details: String::new(),
            line: None,
            location: None,
        }
    }
    fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = details.into();
        self
    }
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.summary)?;
        if let Some(line) = self.line {
            write!(f, " ({} line {line})", self.location.unwrap_or("source"))?;
        }
        Ok(())
    }
}

impl std::error::Error for RenderError {}

/// Parse `#RRGGBB` / `#RGB` into 0..1 RGB components.
pub fn parse_color(color: &str) -> Option<(f64, f64, f64)> {
    let hex = color.trim().strip_prefix('#')?;
    let expanded: String = match hex.len() {
        3 => hex.chars().flat_map(|c| [c, c]).collect(),
        6 => hex.to_string(),
        _ => return None,
    };
    let v = u32::from_str_radix(&expanded, 16).ok()?;
    let c = |shift: u32| ((v >> shift) & 0xFF) as f64 / 255.0;
    Some((c(16), c(8), c(0)))
}

fn validate(req: &RenderRequest) -> Result<(), RenderError> {
    if req.source.trim().is_empty() {
        return Err(RenderError::new(
            ErrorKind::InvalidInput,
            "Enter some LaTeX to render.",
        ));
    }
    if req.source.len() > MAX_SOURCE_BYTES
        || req.preamble.as_deref().unwrap_or("").len() > MAX_SOURCE_BYTES
    {
        return Err(RenderError::new(
            ErrorKind::InvalidInput,
            "The equation or preamble is too long (limit 64 KiB).",
        ));
    }
    if !(1.0..=500.0).contains(&req.font_size) {
        return Err(RenderError::new(
            ErrorKind::InvalidInput,
            "Font size must be between 1 and 500 pt.",
        ));
    }
    if !(36..=2400).contains(&req.png_dpi) {
        return Err(RenderError::new(
            ErrorKind::InvalidInput,
            "PNG resolution must be between 36 and 2400 dpi.",
        ));
    }
    if parse_color(&req.color).is_none() {
        return Err(RenderError::new(
            ErrorKind::InvalidInput,
            "Color must look like #RRGGBB.",
        ));
    }
    Ok(())
}

/// How the compile step produces its intermediate output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intermediate {
    Dvi,
    Xdv,
    Pdf,
}

/// Concrete plan: which program compiles and what it produces.
#[derive(Debug, Clone)]
struct Plan {
    compiler: PathBuf,
    engine: Engine,
    intermediate: Intermediate,
    /// Scale inside TeX (PDF output only).
    tex_scale: bool,
}

fn plan(tc: &Toolchain, engine: Engine, format: Format) -> Result<Plan, RenderError> {
    let t = &tc.tools;
    let unsupported = |what: &str| {
        RenderError::new(
            ErrorKind::Unsupported,
            format!(
                "{} cannot produce {} with {}: {what}",
                tc.label,
                format.name().to_uppercase(),
                engine.name()
            ),
        )
    };
    let need = |p: &Option<PathBuf>, name: &str| {
        p.clone()
            .ok_or_else(|| unsupported(&format!("{name} is not installed")))
    };

    let p = |compiler, intermediate, tex_scale| {
        Ok(Plan {
            compiler,
            engine,
            intermediate,
            tex_scale,
        })
    };
    match (format, engine) {
        // Vector output via DVI/XDV + dvisvgm is the preferred route.
        (Format::Svg, Engine::Latex) => {
            need(&t.dvisvgm, "dvisvgm")?;
            p(need(&t.latex, "latex")?, Intermediate::Dvi, false)
        }
        (Format::Svg, Engine::Pdflatex) if t.latex.is_some() && t.dvisvgm.is_some() => {
            // pdflatex in DVI mode is exactly `latex`.
            p(t.latex.clone().unwrap(), Intermediate::Dvi, false)
        }
        (Format::Svg, Engine::Xelatex) if t.dvisvgm.is_some() => {
            p(need(&t.xelatex, "xelatex")?, Intermediate::Xdv, false)
        }
        (Format::Svg, Engine::Lualatex) if t.dvisvgm.is_some() && t.dvilualatex.is_some() => {
            p(t.dvilualatex.clone().unwrap(), Intermediate::Dvi, false)
        }
        (Format::Svg, _) => {
            need(&t.pdftocairo, "pdftocairo (Poppler)")?;
            let compiler = tc
                .engine_path(engine)
                .map(Path::to_path_buf)
                .ok_or_else(|| unsupported("engine not installed"))?;
            p(compiler, Intermediate::Pdf, false)
        }
        (Format::Pdf | Format::Png, Engine::Latex) => {
            let pdf = format == Format::Pdf;
            if let Some(pdflatex) = t.pdflatex.clone() {
                p(pdflatex, Intermediate::Pdf, pdf)
            } else if !pdf && t.dvipng.is_some() {
                p(need(&t.latex, "latex")?, Intermediate::Dvi, false)
            } else {
                need(&t.dvipdfmx, "dvipdfmx")?;
                p(need(&t.latex, "latex")?, Intermediate::Dvi, pdf)
            }
        }
        (Format::Pdf | Format::Png, _) => {
            let compiler = tc
                .engine_path(engine)
                .map(Path::to_path_buf)
                .ok_or_else(|| unsupported("engine not installed"))?;
            if format == Format::Png && t.pdftocairo.is_none() {
                if engine == Engine::Pdflatex && t.latex.is_some() && t.dvipng.is_some() {
                    return p(t.latex.clone().unwrap(), Intermediate::Dvi, false);
                }
                return Err(unsupported("PNG needs pdftocairo (Poppler) or dvipng"));
            }
            p(compiler, Intermediate::Pdf, format == Format::Pdf)
        }
    }
}

/// Build the TeX document. Returns the text and the 1-based line numbers where
/// the preamble and the user source start, for error mapping.
fn wrapper(req: &RenderRequest, plan: &Plan, meta_hex: Option<&str>) -> (String, usize, usize) {
    let (r, g, b) = parse_color(&req.color).unwrap_or((0.0, 0.0, 0.0));
    let preamble = req.preamble.as_deref().unwrap_or(DEFAULT_PREAMBLE);
    let factor = req.font_size / BASE_FONT_PT;
    let mut lines: Vec<String> = Vec::new();

    // Keep PDF objects uncompressed so the info dictionary stays readable.
    lines.push("\\ifdefined\\pdfobjcompresslevel\\pdfobjcompresslevel=0\\fi".into());
    lines.push("\\ifdefined\\pdfvariable\\pdfvariable objcompresslevel=0\\fi".into());
    if plan.intermediate == Intermediate::Dvi && plan.tex_scale {
        lines.push("\\PassOptionsToPackage{dvipdfmx}{graphicx}".into());
    }
    lines.push("\\documentclass[10pt]{article}".into());
    let preamble_line = lines.len() + 1;
    lines.extend(preamble.lines().map(str::to_string));
    lines.push("\\usepackage{color}".into());
    if plan.tex_scale {
        lines.push("\\usepackage{graphicx}".into());
    }
    lines.push("\\pagestyle{empty}".into());
    lines.push("\\begin{document}".into());
    lines.push("\\newsavebox\\lavatexbox".into());
    lines.push(format!(
        "\\begin{{lrbox}}{{\\lavatexbox}}\\color[rgb]{{{r:.4},{g:.4},{b:.4}}}%"
    ));

    let source = match req.mode {
        // Blank lines end a paragraph, which is never valid inside math.
        Mode::Display | Mode::Inline => req
            .source
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Mode::Text => req.source.clone(),
    };
    let (open, close) = match req.mode {
        Mode::Display => ("$\\displaystyle", "$%"),
        Mode::Inline => ("$", "$%"),
        Mode::Text => ("", "%"),
    };
    lines.push(open.into());
    let source_line = lines.len() + 1;
    lines.extend(source.lines().map(str::to_string));
    lines.push(close.into());
    lines.push("\\end{lrbox}".into());
    lines.push(
        "\\typeout{LAVATEX-DIMS:\\the\\wd\\lavatexbox;\\the\\ht\\lavatexbox;\\the\\dp\\lavatexbox}"
            .into(),
    );
    if plan.tex_scale && (factor - 1.0).abs() > 1e-9 {
        lines.push(format!(
            "\\sbox\\lavatexbox{{\\scalebox{{{factor:.6}}}{{\\usebox\\lavatexbox}}}}"
        ));
    }
    lines.push("\\hoffset=-1in \\voffset=-1in".into());
    lines.push(
        "\\dimen0=\\wd\\lavatexbox \\dimen2=\\dimexpr\\ht\\lavatexbox+\\dp\\lavatexbox\\relax"
            .into(),
    );
    lines.push(
        "\\ifdefined\\pdfpagewidth\\pdfpagewidth=\\dimen0 \\pdfpageheight=\\dimen2 \\fi".into(),
    );
    lines.push("\\ifdefined\\pagewidth\\pagewidth=\\dimen0 \\pageheight=\\dimen2 \\fi".into());

    let mut ship = String::from("\\shipout\\hbox{\\special{papersize=\\the\\dimen0,\\the\\dimen2}");
    if let Some(hex) = meta_hex {
        match plan.engine {
            Engine::Pdflatex | Engine::Latex if plan.intermediate == Intermediate::Pdf => {
                lines.push(format!("\\pdfinfo{{/LavaTeX <{hex}>}}"));
            }
            Engine::Lualatex => lines.push(format!("\\pdfextension info{{/LavaTeX <{hex}>}}")),
            // xdvipdfmx / dvipdfmx (XeLaTeX, Tectonic, latex + dvipdfmx)
            _ => ship.push_str(&format!("\\special{{pdf:docinfo<</LavaTeX <{hex}>>>}}")),
        }
    }
    ship.push_str("\\copy\\lavatexbox}");
    lines.push(ship);
    lines.push("\\end{document}".into());
    (lines.join("\n") + "\n", preamble_line, source_line)
}

struct Workspace {
    dir: tempfile::TempDir,
}

impl Workspace {
    fn new() -> Result<Self, RenderError> {
        let dir = tempfile::Builder::new()
            .prefix("lavatex-")
            .tempdir()
            .map_err(|e| {
                RenderError::new(
                    ErrorKind::Tool,
                    format!("Could not create a temporary folder: {e}"),
                )
            })?;
        Ok(Workspace { dir })
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

/// Run a program with an argument array (never through a shell), with output
/// redirected to files to avoid pipe deadlocks, and a timeout.
fn run(
    ws: &Workspace,
    program: &Path,
    args: &[&str],
    timeout: Duration,
    step: &str,
) -> Result<(bool, String), RenderError> {
    let out_path = ws.path(&format!("{step}.out"));
    let out = std::fs::File::create(&out_path).map_err(|e| {
        RenderError::new(
            ErrorKind::Tool,
            format!("Could not write to the temporary folder: {e}"),
        )
    })?;
    let err = out
        .try_clone()
        .map_err(|e| RenderError::new(ErrorKind::Tool, e.to_string()))?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(ws.dir.path())
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        // TeX Live / kpathsea: no \write18, and file access limited to the
        // working directory and the TeX tree.
        .env("shell_escape", "f")
        .env("openin_any", "p")
        .env("openout_any", "p")
        .env_remove("TEXMFOUTPUT");
    let mut child = cmd.spawn().map_err(|e| {
        RenderError::new(
            ErrorKind::Tool,
            format!("Could not start {}: {e}", program.display()),
        )
    })?;
    let status = match child
        .wait_timeout(timeout)
        .map_err(|e| RenderError::new(ErrorKind::Tool, e.to_string()))?
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RenderError::new(
                ErrorKind::Timeout,
                format!("{step} took longer than {} s and was stopped.", timeout.as_secs()),
            )
            .with_details("Try a simpler equation, or check whether your TeX installation is waiting to download a package."));
        }
    };
    let output = std::fs::read_to_string(&out_path).unwrap_or_default();
    Ok((status.success(), output))
}

fn tex_args(tc: &Toolchain, plan: &Plan) -> Vec<&'static str> {
    if plan.engine == Engine::Tectonic {
        return vec![
            "--untrusted",
            "--keep-logs",
            "--outfmt",
            "pdf",
            "--chatter",
            "minimal",
            "eq.tex",
        ];
    }
    let mut args = vec!["-interaction=nonstopmode", "-halt-on-error"];
    args.push(if tc.distribution == Distribution::MiKTeX {
        "-disable-write18"
    } else {
        "-no-shell-escape"
    });
    if plan.intermediate == Intermediate::Xdv {
        args.push("-no-pdf");
    } else if plan.engine == Engine::Xelatex {
        // PDF 1.4 output has no compressed object streams, so the document
        // info dictionary (and our metadata) stays readable.
        args.push("-output-driver=xdvipdfmx -V 4");
    }
    args.push("eq.tex");
    args
}

fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Turn a TeX log into a readable error.
fn parse_tex_error(
    log: &str,
    tc: &Toolchain,
    req: &RenderRequest,
    preamble_line: usize,
    source_line: usize,
) -> RenderError {
    let source_lines = req.source.lines().count();
    let lines: Vec<&str> = log.lines().collect();
    let Some(idx) = lines.iter().position(|l| l.starts_with("! ")) else {
        return RenderError::new(
            ErrorKind::Compile,
            "LaTeX stopped without producing output.",
        )
        .with_details(last_lines(log, 30));
    };
    let raw = lines[idx].trim_start_matches("! ").trim().to_string();
    let mut tex_line = None;
    let mut context = Vec::new();
    for l in lines.iter().skip(idx + 1).take(12) {
        context.push(*l);
        if let Some(rest) = l.strip_prefix("l.") {
            tex_line = rest
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<usize>().ok());
            break;
        }
    }
    let mut summary = raw.clone();
    let mut missing_file = None;
    if raw.starts_with("Undefined control sequence") {
        let cs = context
            .last()
            .and_then(|l| l.split_whitespace().last())
            .filter(|t| t.starts_with('\\'))
            .map(|t| format!(" {t}"))
            .unwrap_or_default();
        summary = format!("Unknown command{cs}. Check the spelling or add the package that defines it to the preamble.");
    } else if let Some(file) = raw
        .strip_prefix("LaTeX Error: File `")
        .and_then(|r| r.split('\'').next())
    {
        missing_file = Some(file.to_string());
        summary = if file.contains('/') || file.contains('\\') {
            format!("LaTeX could not open {file}. LavaTeX only lets LaTeX read files from your TeX installation.")
        } else if file.ends_with(".sty") || file.ends_with(".cls") {
            format!("{file} is not installed in {}. Install it with your TeX package manager or remove it from the preamble.", tc.label)
        } else {
            format!("LaTeX could not find {file}.")
        };
    } else if raw.starts_with("Missing $ inserted") {
        summary = "Math command outside math mode, or a blank line inside the equation (Missing $ inserted).".into();
    } else if raw.starts_with("Emergency stop") || raw.starts_with("==> Fatal error") {
        summary = "LaTeX could not finish. See the details for the first error.".into();
    }

    let mut err = RenderError::new(ErrorKind::Compile, summary).with_details(
        format!("{raw}\n{}", context.join("\n"))
            .trim_end()
            .to_string(),
    );
    if let Some(l) = tex_line {
        if l >= source_line && l < source_line + source_lines.max(1) {
            err.line = Some(l - source_line + 1);
            err.location = Some("source");
        } else if l >= preamble_line && l < source_line {
            err.line = Some(l - preamble_line + 1);
            err.location = Some("preamble");
        }
    }
    // \usepackage peeks at the following line before failing, so TeX's line
    // number is one too late; locate the file name directly instead.
    if let Some(file) = missing_file {
        let stem = file.rsplit(['/', '\\']).next().unwrap_or(&file);
        let stem = stem
            .strip_suffix(".sty")
            .or_else(|| stem.strip_suffix(".cls"))
            .or_else(|| stem.strip_suffix(".tex"))
            .unwrap_or(stem);
        let preamble = req.preamble.as_deref().unwrap_or(DEFAULT_PREAMBLE);
        if let Some(i) = preamble.lines().position(|l| l.contains(stem)) {
            err.line = Some(i + 1);
            err.location = Some("preamble");
        } else if let Some(i) = req.source.lines().position(|l| l.contains(stem)) {
            err.line = Some(i + 1);
            err.location = Some("source");
        }
    }
    err
}

fn parse_dims(log: &str) -> Option<(f64, f64, f64)> {
    // The marker can be wrapped across log lines at 79 columns.
    let flat: String = log.lines().collect::<Vec<_>>().join("");
    let start = flat.find("LAVATEX-DIMS:")? + "LAVATEX-DIMS:".len();
    let mut parts = flat[start..].split(';');
    let mut next = || -> Option<f64> {
        let p = parts.next()?;
        let num: String = p
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
            .collect();
        num.parse().ok()
    };
    Some((next()?, next()?, next()?))
}

/// Set explicit `pt` width/height on the root `<svg>` element.
fn set_svg_size(svg: &str, width_bp: f64, height_bp: f64) -> String {
    let Some(start) = svg.find("<svg") else {
        return svg.to_string();
    };
    let Some(end_rel) = svg[start..].find('>') else {
        return svg.to_string();
    };
    let end = start + end_rel;
    let mut tag = svg[start..end].to_string();
    for (attr, value) in [("width", width_bp), ("height", height_bp)] {
        let needle = format!(" {attr}=");
        if let Some(pos) = tag.find(&needle) {
            let q_pos = pos + needle.len();
            let quote = tag.as_bytes()[q_pos] as char;
            if let Some(close_rel) = tag[q_pos + 1..].find(quote) {
                tag.replace_range(
                    q_pos..q_pos + 2 + close_rel,
                    &format!("{quote}{value:.4}pt{quote}"),
                );
                continue;
            }
        }
        tag.push_str(&format!(" {attr}='{value:.4}pt'"));
    }
    format!("{}{}{}", &svg[..start], tag, &svg[end..])
}

fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || &png[12..16] != b"IHDR" {
        return None;
    }
    Some((
        u32::from_be_bytes(png[16..20].try_into().ok()?),
        u32::from_be_bytes(png[20..24].try_into().ok()?),
    ))
}

fn read_output(path: &Path, what: &str) -> Result<Vec<u8>, RenderError> {
    let bytes = std::fs::read(path).map_err(|_| {
        RenderError::new(
            ErrorKind::Output,
            format!("{what} did not produce an output file."),
        )
    })?;
    if bytes.is_empty() || bytes.len() > MAX_OUTPUT_BYTES {
        return Err(RenderError::new(
            ErrorKind::Output,
            format!("{what} produced an empty or oversized file."),
        ));
    }
    Ok(bytes)
}

/// Render with an already-resolved toolchain.
pub fn render_with(req: &RenderRequest, tc: &Toolchain) -> Result<RenderOutput, RenderError> {
    validate(req)?;
    let engine = match req.engine {
        Some(e) => e,
        None => tc.default_engine().ok_or_else(|| {
            RenderError::new(
                ErrorKind::NoToolchain,
                format!("{} has no usable LaTeX engine.", tc.label),
            )
        })?,
    };
    if !tc.engines.contains(&engine) {
        return Err(RenderError::new(
            ErrorKind::Unsupported,
            format!("{} is not available in {}.", engine.name(), tc.label),
        ));
    }
    let plan = plan(tc, engine, req.format)?;
    let timeout = Duration::from_secs(req.timeout_secs.clamp(1, 120));
    let ws = Workspace::new()?;
    let factor = req.font_size / BASE_FONT_PT;
    let generator = format!("LavaTeX {}", crate::VERSION);

    let mut meta = EquationMeta {
        schema: metadata::SCHEMA.into(),
        source: req.source.clone(),
        preamble: req
            .preamble
            .clone()
            .unwrap_or_else(|| DEFAULT_PREAMBLE.to_string()),
        mode: req.mode.name().into(),
        font_size: req.font_size,
        color: req.color.clone(),
        format: req.format.name().into(),
        engine: engine.name().into(),
        width_bp: 0.0,
        height_bp: 0.0,
        depth_bp: 0.0,
        generator,
    };

    // The size is only known after compiling; PDF info is written during the
    // compile, so for PDF we compile once to measure and once to embed.
    let compile = |meta_hex: Option<&str>| -> Result<(f64, f64, f64), RenderError> {
        let (tex, preamble_line, source_line) = wrapper(req, &plan, meta_hex);
        std::fs::write(ws.path("eq.tex"), tex)
            .map_err(|e| RenderError::new(ErrorKind::Tool, e.to_string()))?;
        let args = tex_args(tc, &plan);
        let (ok, console) = run(&ws, &plan.compiler, &args, timeout, engine.name())?;
        let log = std::fs::read_to_string(ws.path("eq.log")).unwrap_or(console.clone());
        let produced = match plan.intermediate {
            Intermediate::Dvi => ws.path("eq.dvi"),
            Intermediate::Xdv => ws.path("eq.xdv"),
            Intermediate::Pdf => ws.path("eq.pdf"),
        };
        if !ok || !produced.is_file() {
            let mut err = parse_tex_error(&log, tc, req, preamble_line, source_line);
            if err.details.is_empty() {
                err.details = last_lines(&console, 30);
            }
            return Err(err);
        }
        let (w, h, d) = parse_dims(&log).ok_or_else(|| {
            RenderError::new(
                ErrorKind::Output,
                "Could not read the equation size from the LaTeX log.",
            )
            .with_details(last_lines(&log, 20))
        })?;
        if w <= 0.0 || h + d <= 0.0 {
            return Err(RenderError::new(
                ErrorKind::Output,
                "The equation rendered as empty.",
            ));
        }
        Ok((w, h, d))
    };

    let (w, h, d) = compile(None)?;
    meta.width_bp = w * TEX_PT_TO_BP * factor;
    meta.height_bp = (h + d) * TEX_PT_TO_BP * factor;
    meta.depth_bp = d * TEX_PT_TO_BP * factor;
    let mut warnings = Vec::new();

    let bytes = match req.format {
        Format::Svg => {
            let svg_path = ws.path("eq.svg");
            match plan.intermediate {
                Intermediate::Dvi | Intermediate::Xdv => {
                    let input = if plan.intermediate == Intermediate::Dvi {
                        "eq.dvi"
                    } else {
                        "eq.xdv"
                    };
                    let dvisvgm = tc.tools.dvisvgm.as_deref().expect("planned");
                    let (ok, out) = run(
                        &ws,
                        dvisvgm,
                        &[
                            "--no-fonts",
                            "--bbox=papersize",
                            "--precision=4",
                            "-o",
                            "eq.svg",
                            input,
                        ],
                        timeout,
                        "dvisvgm",
                    )?;
                    if !ok {
                        return Err(RenderError::new(
                            ErrorKind::Tool,
                            "dvisvgm could not convert the equation to SVG.",
                        )
                        .with_details(last_lines(&out, 20)));
                    }
                }
                Intermediate::Pdf => {
                    let cairo = tc.tools.pdftocairo.as_deref().expect("planned");
                    let (ok, out) = run(
                        &ws,
                        cairo,
                        &["-svg", "eq.pdf", "eq.svg"],
                        timeout,
                        "pdftocairo",
                    )?;
                    if !ok {
                        return Err(RenderError::new(
                            ErrorKind::Tool,
                            "pdftocairo could not convert the equation to SVG.",
                        )
                        .with_details(last_lines(&out, 20)));
                    }
                }
            }
            let svg =
                String::from_utf8(read_output(&svg_path, "The SVG converter")?).map_err(|_| {
                    RenderError::new(ErrorKind::Output, "The SVG output is not valid UTF-8.")
                })?;
            if !svg.contains("<svg") {
                return Err(RenderError::new(
                    ErrorKind::Output,
                    "The SVG output is not a valid SVG file.",
                ));
            }
            let svg = set_svg_size(&svg, meta.width_bp, meta.height_bp);
            metadata::embed_svg(&svg, &meta).into_bytes()
        }
        Format::Pdf => {
            compile(Some(&metadata::pdf_info_hex(&meta)))?;
            if plan.intermediate == Intermediate::Dvi {
                let dvipdfmx = tc.tools.dvipdfmx.as_deref().expect("planned");
                let (ok, out) = run(
                    &ws,
                    dvipdfmx,
                    &["-o", "eq.pdf", "eq.dvi"],
                    timeout,
                    "dvipdfmx",
                )?;
                if !ok {
                    return Err(RenderError::new(
                        ErrorKind::Tool,
                        "dvipdfmx could not convert the equation to PDF.",
                    )
                    .with_details(last_lines(&out, 20)));
                }
            }
            let pdf = read_output(&ws.path("eq.pdf"), "LaTeX")?;
            if !pdf.starts_with(b"%PDF") {
                return Err(RenderError::new(
                    ErrorKind::Output,
                    "The PDF output is not a valid PDF file.",
                ));
            }
            if metadata::extract_pdf(&pdf).is_err() {
                warnings.push("The PDF was created, but its embedded source could not be verified; the alt text still carries it.".into());
            }
            pdf
        }
        Format::Png => {
            let effective_dpi = (req.png_dpi as f64 * factor).round().max(1.0) as u32;
            let dpi = effective_dpi.to_string();
            match plan.intermediate {
                Intermediate::Pdf => {
                    let cairo = tc.tools.pdftocairo.as_deref().expect("planned");
                    let (ok, out) = run(
                        &ws,
                        cairo,
                        &["-png", "-singlefile", "-transp", "-r", &dpi, "eq.pdf", "eq"],
                        timeout,
                        "pdftocairo",
                    )?;
                    if !ok {
                        return Err(RenderError::new(
                            ErrorKind::Tool,
                            "pdftocairo could not convert the equation to PNG.",
                        )
                        .with_details(last_lines(&out, 20)));
                    }
                }
                _ => {
                    let dvipng = tc.tools.dvipng.as_deref().expect("planned");
                    let (ok, out) = run(
                        &ws,
                        dvipng,
                        &[
                            "-D",
                            &dpi,
                            "-bg",
                            "Transparent",
                            "-T",
                            "tight",
                            "-z",
                            "9",
                            "-o",
                            "eq.png",
                            "eq.dvi",
                        ],
                        timeout,
                        "dvipng",
                    )?;
                    if !ok {
                        return Err(RenderError::new(
                            ErrorKind::Tool,
                            "dvipng could not convert the equation to PNG.",
                        )
                        .with_details(last_lines(&out, 20)));
                    }
                }
            }
            let png = read_output(&ws.path("eq.png"), "The PNG converter")?;
            let (pw, ph) = png_size(&png).ok_or_else(|| {
                RenderError::new(ErrorKind::Output, "The PNG output is not a valid PNG file.")
            })?;
            // Use the real pixel size so hosts never distort the picture.
            meta.width_bp = pw as f64 / effective_dpi as f64 * 72.0 * factor;
            meta.height_bp = ph as f64 / effective_dpi as f64 * 72.0 * factor;
            metadata::embed_png(&png, &meta).ok_or_else(|| {
                RenderError::new(ErrorKind::Output, "Could not embed metadata in the PNG.")
            })?
        }
    };

    Ok(RenderOutput {
        format: req.format,
        mime: req.format.mime(),
        bytes,
        width_bp: meta.width_bp,
        height_bp: meta.height_bp,
        depth_bp: meta.depth_bp,
        alt_text: meta.to_alt_text(),
        meta,
        engine,
        toolchain: tc.label.clone(),
        warnings,
    })
}

/// Resolve the toolchain from the request and render.
pub fn render(req: &RenderRequest) -> Result<RenderOutput, RenderError> {
    validate(req)?;
    let tc = toolchain::resolve(req.toolchain.as_deref())
        .map_err(|e| RenderError::new(ErrorKind::NoToolchain, e))?;
    render_with(req, &tc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(parse_color("#ff0000"), Some((1.0, 0.0, 0.0)));
        assert_eq!(parse_color("#fff"), Some((1.0, 1.0, 1.0)));
        assert_eq!(parse_color("red"), None);
    }

    #[test]
    fn dims_parse_even_when_wrapped() {
        let log = "foo\nLAVATEX-DIMS:82.69511pt;14.767\n79pt;9.11122pt\nbar";
        assert_eq!(parse_dims(log), Some((82.69511, 14.76779, 9.11122)));
    }

    #[test]
    fn svg_size_rewrite() {
        let svg = "<?xml?><svg version='1.1' width='82.38pt' height=\"23.7\" viewBox='0 0 1 1'><g/></svg>";
        let out = set_svg_size(svg, 10.0, 5.5);
        assert!(out.contains("width='10.0000pt'"));
        assert!(out.contains("height=\"5.5000pt\""));
        assert!(out.contains("viewBox='0 0 1 1'"));
    }

    #[test]
    fn error_mapping() {
        let log = "This is pdfTeX\n! Undefined control sequence.\n<argument> \\fracc\n                \nl.15 \\fracc\n          {a}{b}\n";
        let tc = Toolchain {
            id: "x".into(),
            label: "TeX Live".into(),
            distribution: Distribution::TexLive,
            bin_dir: PathBuf::from("/x"),
            version: String::new(),
            engines: vec![],
            tools: Default::default(),
            svg: true,
            pdf: true,
            png: true,
        };
        let req = RenderRequest {
            source: "x\n\\fracc{a}{b}\ny".into(),
            ..Default::default()
        };
        let err = parse_tex_error(log, &tc, &req, 4, 14);
        assert_eq!(err.kind, ErrorKind::Compile);
        assert!(err.summary.contains("\\fracc"), "{}", err.summary);
        assert_eq!(err.line, Some(2));
        assert_eq!(err.location, Some("source"));
    }

    #[test]
    fn wrapper_drops_blank_lines_in_math() {
        let req = RenderRequest {
            source: "a\n\n= b".into(),
            ..Default::default()
        };
        let plan = Plan {
            compiler: PathBuf::new(),
            engine: Engine::Latex,
            intermediate: Intermediate::Dvi,
            tex_scale: false,
        };
        let (tex, _, src_line) = wrapper(&req, &plan, None);
        let lines: Vec<&str> = tex.lines().collect();
        assert_eq!(lines[src_line - 1], "a");
        assert_eq!(lines[src_line], "= b");
        assert!(tex.contains("\\displaystyle"));
    }

    #[test]
    fn validation() {
        assert!(validate(&RenderRequest::default()).is_err());
        let ok = RenderRequest {
            source: "x".into(),
            ..Default::default()
        };
        assert!(validate(&ok).is_ok());
        let bad = RenderRequest {
            source: "x".into(),
            color: "blue".into(),
            ..Default::default()
        };
        assert!(validate(&bad).is_err());
    }
}
