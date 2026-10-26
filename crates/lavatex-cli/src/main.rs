//! `lavatex` — command-line front end and local render helper.

use clap::{Parser, Subcommand};
use lavatex_cli::{apply_config, serve, setup};
use lavatex_core::config::Config;
use lavatex_core::render::{self, RenderRequest};
use lavatex_core::{metadata, toolchain, Engine, Format, Mode};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "lavatex",
    version,
    about = "Write once. Typeset everywhere. LaTeX equations for any presentation."
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List the TeX installations found on this machine (best first).
    Detect {
        #[arg(long)]
        json: bool,
    },
    /// Render an equation to SVG, PDF or PNG with embedded LavaTeX metadata.
    Render(Box<RenderArgs>),
    /// Print the LavaTeX metadata stored in an image or alt text.
    Inspect {
        /// Image file (SVG, PDF or PNG). Use `-` to read alt text from stdin.
        file: PathBuf,
    },
    /// Show or change shared defaults (toolchain, engine, format).
    Config {
        /// Set the preferred TeX installation (a bin folder, or "auto").
        #[arg(long)]
        toolchain: Option<String>,
        /// Set the preferred engine (latex, pdflatex, xelatex, lualatex, tectonic).
        #[arg(long)]
        engine: Option<String>,
        /// Set the preferred output format (svg, pdf, png).
        #[arg(long)]
        format: Option<String>,
    },
    /// Print the access token that browser-based plugins use to reach `lavatex serve`.
    Token,
    /// Run the local render helper used by the ONLYOFFICE plugin.
    Serve {
        #[arg(long, default_value_t = serve::DEFAULT_PORT)]
        port: u16,
        /// Use this token instead of the stored one.
        #[arg(long, env = "LAVATEX_TOKEN")]
        token: Option<String>,
        /// Exit after this many seconds without requests (0 = never).
        #[arg(long, default_value_t = 0)]
        idle_exit: u64,
    },
    /// One-time setup of a host application (currently: onlyoffice).
    Setup(setup::SetupArgs),
}

#[derive(clap::Args)]
struct RenderArgs {
    /// LaTeX source (math). Omit to use --input or --request.
    source: Option<String>,
    /// Read the LaTeX source from a file (`-` for stdin).
    #[arg(short, long)]
    input: Option<PathBuf>,
    /// Read a complete JSON render request from a file (`-` for stdin).
    #[arg(long, conflicts_with_all = ["source", "input"])]
    request: Option<PathBuf>,
    #[arg(short, long)]
    format: Option<String>,
    #[arg(short, long, default_value = "display")]
    mode: String,
    /// Font size in points.
    #[arg(short = 's', long, default_value_t = 20.0)]
    size: f64,
    #[arg(short, long, default_value = "#000000")]
    color: String,
    #[arg(long)]
    engine: Option<String>,
    /// TeX bin folder to use (default: preferred or best detected).
    #[arg(long)]
    toolchain: Option<String>,
    /// File with preamble lines (default: amsmath + amssymb).
    #[arg(long)]
    preamble_file: Option<PathBuf>,
    #[arg(long, default_value_t = 300)]
    dpi: u32,
    #[arg(long, default_value_t = 30)]
    timeout: u64,
    /// Output file (default: equation.<format>).
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Print a JSON result (for host integrations).
    #[arg(long)]
    json: bool,
}

fn read_path(path: &PathBuf) -> std::io::Result<String> {
    if path.as_os_str() == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        Ok(s)
    } else {
        std::fs::read_to_string(path)
    }
}

fn build_request(args: &RenderArgs, config: &Config) -> Result<RenderRequest, String> {
    let mut req = if let Some(path) = &args.request {
        let text = read_path(path).map_err(|e| format!("cannot read request: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("invalid request JSON: {e}"))?
    } else {
        let source = match (&args.source, &args.input) {
            (Some(s), _) => s.clone(),
            (None, Some(p)) => read_path(p).map_err(|e| format!("cannot read input: {e}"))?,
            (None, None) => {
                return Err("give the LaTeX source as an argument, --input or --request".into())
            }
        };
        let preamble = match &args.preamble_file {
            Some(p) => {
                Some(std::fs::read_to_string(p).map_err(|e| format!("cannot read preamble: {e}"))?)
            }
            None => None,
        };
        let format = match &args.format {
            Some(f) => Format::parse(f).ok_or("format must be svg, pdf or png")?,
            None => Format::parse(&config.format).unwrap_or(Format::Svg),
        };
        RenderRequest {
            source,
            preamble,
            mode: Mode::parse(&args.mode).ok_or("mode must be display, inline or text")?,
            font_size: args.size,
            color: args.color.clone(),
            format,
            png_dpi: args.dpi,
            engine: match &args.engine {
                Some(e) => Some(Engine::parse(e).ok_or("unknown engine")?),
                None => None,
            },
            toolchain: args.toolchain.clone(),
            timeout_secs: args.timeout,
        }
    };
    apply_config(&mut req, config);
    Ok(req)
}

fn cmd_render(args: RenderArgs) -> ExitCode {
    let config = Config::load();
    let req = match build_request(&args, &config) {
        Ok(r) => r,
        Err(e) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::json!({"ok": false, "error": {"kind": "invalidInput", "summary": e, "details": ""}})
                );
            } else {
                eprintln!("error: {e}");
            }
            return ExitCode::from(2);
        }
    };
    match render::render(&req) {
        Ok(out) => {
            let path = args
                .output
                .clone()
                .unwrap_or_else(|| PathBuf::from(format!("equation.{}", out.format.name())));
            if let Err(e) = std::fs::write(&path, &out.bytes) {
                eprintln!("error: cannot write {}: {e}", path.display());
                return ExitCode::from(1);
            }
            if args.json {
                let mut v = serde_json::to_value(&out).expect("serializes");
                v["ok"] = true.into();
                v["path"] = path.to_string_lossy().into_owned().into();
                println!("{v}");
            } else {
                for w in &out.warnings {
                    eprintln!("warning: {w}");
                }
                println!(
                    "{} ({:.1} x {:.1} pt, {} via {})",
                    path.display(),
                    out.width_bp,
                    out.height_bp,
                    out.engine.name(),
                    out.toolchain
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            if args.json {
                println!("{}", serde_json::json!({"ok": false, "error": e}));
            } else {
                eprintln!("error: {e}");
                if !e.details.is_empty() {
                    eprintln!("\n{}", e.details);
                }
            }
            ExitCode::from(2)
        }
    }
}

fn cmd_detect(json: bool) -> ExitCode {
    let found = toolchain::detect();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&found).expect("serializes")
        );
        return ExitCode::SUCCESS;
    }
    if found.is_empty() {
        println!("No TeX installation found.");
        println!("Install TeX Live (https://tug.org/texlive/), MiKTeX (https://miktex.org/), MacTeX or Tectonic,");
        println!("or point LavaTeX at your bin folder: lavatex config --toolchain <folder>");
        return ExitCode::from(1);
    }
    let preferred = Config::load().toolchain;
    for (i, tc) in found.iter().enumerate() {
        let mark = if !preferred.is_empty() && tc.id == preferred {
            " (preferred)"
        } else if i == 0 && preferred.is_empty() {
            " (suggested)"
        } else {
            ""
        };
        let engines: Vec<&str> = tc.engines.iter().map(|e| e.name()).collect();
        let formats: Vec<&str> = [("svg", tc.svg), ("pdf", tc.pdf), ("png", tc.png)]
            .iter()
            .filter(|(_, ok)| *ok)
            .map(|(n, _)| *n)
            .collect();
        println!(
            "{}{}\n  folder:  {}\n  engines: {}\n  formats: {}",
            tc.label,
            mark,
            tc.bin_dir.display(),
            engines.join(", "),
            formats.join(", ")
        );
    }
    ExitCode::SUCCESS
}

fn cmd_inspect(file: PathBuf) -> ExitCode {
    let result = if file.as_os_str() == "-" {
        read_path(&file)
            .map_err(|e| e.to_string())
            .and_then(|t| metadata::EquationMeta::from_alt_text(&t).map_err(|e| e.to_string()))
    } else {
        std::fs::read(&file)
            .map_err(|e| e.to_string())
            .and_then(|b| metadata::extract_any(&b).map_err(|e| e.to_string()))
    };
    match result {
        Ok(meta) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&meta).expect("serializes")
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}: {e}", file.display());
            ExitCode::from(1)
        }
    }
}

fn cmd_config(
    toolchain_arg: Option<String>,
    engine: Option<String>,
    format: Option<String>,
) -> ExitCode {
    let mut config = Config::load();
    let changing = toolchain_arg.is_some() || engine.is_some() || format.is_some();
    if let Some(t) = toolchain_arg {
        if t == "auto" || t.is_empty() {
            config.toolchain.clear();
        } else {
            match toolchain::probe_dir(std::path::Path::new(&t)) {
                Some(tc) => config.toolchain = tc.id,
                None => {
                    eprintln!("error: no TeX engine found in {t}");
                    return ExitCode::from(1);
                }
            }
        }
    }
    if let Some(e) = engine {
        if e != "auto" && Engine::parse(&e).is_none() {
            eprintln!("error: unknown engine {e}");
            return ExitCode::from(1);
        }
        config.engine = if e == "auto" { String::new() } else { e };
    }
    if let Some(f) = format {
        if Format::parse(&f).is_none() {
            eprintln!("error: format must be svg, pdf or png");
            return ExitCode::from(1);
        }
        config.format = f;
    }
    if changing {
        if let Err(e) = config.save() {
            eprintln!("error: cannot save config: {e}");
            return ExitCode::from(1);
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&config).expect("serializes")
    );
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Detect { json } => cmd_detect(json),
        Cmd::Render(args) => cmd_render(*args),
        Cmd::Inspect { file } => cmd_inspect(file),
        Cmd::Config {
            toolchain,
            engine,
            format,
        } => cmd_config(toolchain, engine, format),
        Cmd::Token => match lavatex_core::config::load_or_create_token(lavatex_cli::new_token) {
            Ok(t) => {
                println!("{t}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(1)
            }
        },
        Cmd::Serve {
            port,
            token,
            idle_exit,
        } => serve::run(port, token, Some(idle_exit)),
        Cmd::Setup(args) => setup::run(args),
    }
}
