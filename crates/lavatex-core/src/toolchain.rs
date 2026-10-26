//! Discovery of the TeX installations already present on the user's machine.
//!
//! LavaTeX never ships TeX itself. It looks for TeX Live, MiKTeX, MacTeX and
//! Tectonic in `PATH` and in their usual install locations, reports what each
//! one can do, and suggests a default when the user has not chosen one.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Latex,
    Pdflatex,
    Xelatex,
    Lualatex,
    Tectonic,
}

impl Engine {
    pub const ALL: [Engine; 5] = [
        Engine::Latex,
        Engine::Pdflatex,
        Engine::Xelatex,
        Engine::Lualatex,
        Engine::Tectonic,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Engine::Latex => "latex",
            Engine::Pdflatex => "pdflatex",
            Engine::Xelatex => "xelatex",
            Engine::Lualatex => "lualatex",
            Engine::Tectonic => "tectonic",
        }
    }

    pub fn parse(s: &str) -> Option<Engine> {
        Engine::ALL
            .into_iter()
            .find(|e| e.name() == s.to_ascii_lowercase())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Distribution {
    #[serde(rename = "TeX Live")]
    TexLive,
    MiKTeX,
    MacTeX,
    Tectonic,
    Unknown,
}

impl Distribution {
    fn rank(self) -> u8 {
        match self {
            Distribution::TexLive => 0,
            Distribution::MacTeX => 1,
            Distribution::MiKTeX => 2,
            Distribution::Unknown => 3,
            Distribution::Tectonic => 4,
        }
    }
}

/// External programs a toolchain may use, by role.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tools {
    pub latex: Option<PathBuf>,
    pub pdflatex: Option<PathBuf>,
    pub xelatex: Option<PathBuf>,
    pub lualatex: Option<PathBuf>,
    pub dvilualatex: Option<PathBuf>,
    pub tectonic: Option<PathBuf>,
    pub dvisvgm: Option<PathBuf>,
    pub dvipng: Option<PathBuf>,
    pub dvipdfmx: Option<PathBuf>,
    pub pdftocairo: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Toolchain {
    /// Stable identifier: the canonical binary directory.
    pub id: String,
    /// Human-readable label, e.g. "TeX Live 2024".
    pub label: String,
    pub distribution: Distribution,
    pub bin_dir: PathBuf,
    pub version: String,
    pub engines: Vec<Engine>,
    pub tools: Tools,
    /// Whether SVG output is possible with this toolchain.
    pub svg: bool,
    pub pdf: bool,
    pub png: bool,
}

impl Toolchain {
    pub fn engine_path(&self, engine: Engine) -> Option<&Path> {
        let t = &self.tools;
        match engine {
            Engine::Latex => t.latex.as_deref(),
            Engine::Pdflatex => t.pdflatex.as_deref(),
            Engine::Xelatex => t.xelatex.as_deref(),
            Engine::Lualatex => t.lualatex.as_deref(),
            Engine::Tectonic => t.tectonic.as_deref(),
        }
    }

    /// Engine to use when the user has not picked one.
    pub fn default_engine(&self) -> Option<Engine> {
        [
            Engine::Latex,
            Engine::Pdflatex,
            Engine::Lualatex,
            Engine::Xelatex,
            Engine::Tectonic,
        ]
        .into_iter()
        .find(|e| self.engines.contains(e))
    }

    fn score(&self) -> (u8, u8, u8) {
        // Prefer the classic latex + dvisvgm route (fast, exact SVG), then by
        // distribution, then whatever has the most capabilities.
        let classic = if self.tools.latex.is_some() && self.tools.dvisvgm.is_some() {
            0
        } else {
            1
        };
        let caps = 3 - (self.svg as u8 + self.pdf as u8 + self.png as u8);
        (classic, self.distribution.rank(), caps)
    }
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn find_in(dir: &Path, name: &str) -> Option<PathBuf> {
    let candidate = dir.join(exe(name));
    candidate.is_file().then_some(candidate)
}

fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default()
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    path_dirs().into_iter().find_map(|d| find_in(&d, name))
}

fn glob_children(parent: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(parent)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    // Newest TeX Live year first.
    out.sort();
    out.reverse();
    out
}

/// Well-known install locations, in addition to `PATH`.
fn well_known_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut texlive_roots: Vec<PathBuf> = vec![];
    if cfg!(windows) {
        texlive_roots.push(PathBuf::from(r"C:\texlive"));
        for var in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(base) = std::env::var_os(var) {
                let base = PathBuf::from(base);
                dirs.push(base.join(r"Programs\MiKTeX\miktex\bin\x64"));
                dirs.push(base.join(r"MiKTeX\miktex\bin\x64"));
                dirs.push(base.join(r"MiKTeX\miktex\bin"));
            }
        }
    } else {
        texlive_roots.push(PathBuf::from("/usr/local/texlive"));
        dirs.push(PathBuf::from("/Library/TeX/texbin"));
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/usr/bin"));
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            texlive_roots.push(home.join("texlive"));
            dirs.push(home.join("bin"));
            dirs.push(home.join(".local/bin"));
        }
    }
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        dirs.push(PathBuf::from(home).join(".cargo").join("bin"));
    }
    for root in texlive_roots {
        // <root>/<year>/bin/<platform>
        for year in glob_children(&root) {
            dirs.extend(glob_children(&year.join("bin")));
        }
    }
    dirs
}

fn first_line_of_version(program: &Path) -> String {
    crate::hide_console(&mut Command::new(program))
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn classify(version: &str, bin_dir: &Path) -> Distribution {
    let lower = version.to_ascii_lowercase();
    if lower.contains("miktex") {
        Distribution::MiKTeX
    } else if bin_dir.starts_with("/Library/TeX") {
        Distribution::MacTeX
    } else if lower.contains("tex live") {
        Distribution::TexLive
    } else {
        Distribution::Unknown
    }
}

fn label_for(distribution: Distribution, version: &str) -> String {
    // "pdfTeX 3.141592653-2.6-1.40.25 (TeX Live 2023/Debian)" -> "TeX Live 2023/Debian"
    let inner = version.rfind('(').and_then(|s| {
        version[s + 1..]
            .find(')')
            .map(|e| version[s + 1..s + 1 + e].to_string())
    });
    match (distribution, inner) {
        (Distribution::TexLive | Distribution::MacTeX, Some(i)) if i.contains("TeX Live") => {
            if distribution == Distribution::MacTeX {
                format!("MacTeX ({i})")
            } else {
                i
            }
        }
        (Distribution::MiKTeX, Some(i)) if i.contains("MiKTeX") => i,
        (Distribution::MiKTeX, _) => "MiKTeX".to_string(),
        (Distribution::Tectonic, _) => version.trim().to_string(),
        (_, _) => "TeX installation".to_string(),
    }
}

/// Inspect a single directory and build a toolchain if it contains a TeX engine.
pub fn probe_dir(dir: &Path) -> Option<Toolchain> {
    let canonical = dir.canonicalize().ok()?;
    let local = |n: &str| find_in(dir, n);
    let shared = |n: &str| local(n).or_else(|| find_on_path(n));

    let tectonic = local("tectonic");
    let latex = local("latex");
    let pdflatex = local("pdflatex");
    let is_tex = latex.is_some()
        || pdflatex.is_some()
        || local("xelatex").is_some()
        || local("lualatex").is_some();

    if !is_tex && tectonic.is_none() {
        return None;
    }

    let tools = if is_tex {
        Tools {
            latex,
            pdflatex,
            xelatex: local("xelatex"),
            lualatex: local("lualatex"),
            dvilualatex: local("dvilualatex"),
            tectonic: None,
            dvisvgm: local("dvisvgm"),
            dvipng: local("dvipng"),
            dvipdfmx: local("dvipdfmx"),
            pdftocairo: shared("pdftocairo"),
        }
    } else {
        Tools {
            tectonic,
            pdftocairo: shared("pdftocairo"),
            ..Tools::default()
        }
    };

    let (distribution, version) = if is_tex {
        let probe = tools
            .pdflatex
            .as_ref()
            .or(tools.latex.as_ref())
            .or(tools.xelatex.as_ref())
            .or(tools.lualatex.as_ref())?;
        let v = first_line_of_version(probe);
        (classify(&v, dir), v)
    } else {
        (
            Distribution::Tectonic,
            first_line_of_version(tools.tectonic.as_ref()?),
        )
    };

    let mut engines = Vec::new();
    if tools.latex.is_some() {
        engines.push(Engine::Latex);
    }
    if tools.pdflatex.is_some() {
        engines.push(Engine::Pdflatex);
    }
    if tools.xelatex.is_some() {
        engines.push(Engine::Xelatex);
    }
    if tools.lualatex.is_some() {
        engines.push(Engine::Lualatex);
    }
    if tools.tectonic.is_some() {
        engines.push(Engine::Tectonic);
    }

    let has_pdf_engine = tools.pdflatex.is_some()
        || tools.xelatex.is_some()
        || tools.lualatex.is_some()
        || tools.tectonic.is_some();
    let dvi_engine =
        tools.latex.is_some() || tools.dvilualatex.is_some() || tools.xelatex.is_some();
    let svg =
        (dvi_engine && tools.dvisvgm.is_some()) || (has_pdf_engine && tools.pdftocairo.is_some());
    let pdf = has_pdf_engine || (tools.latex.is_some() && tools.dvipdfmx.is_some());
    let png =
        (pdf && tools.pdftocairo.is_some()) || (tools.latex.is_some() && tools.dvipng.is_some());

    Some(Toolchain {
        id: canonical.to_string_lossy().into_owned(),
        label: label_for(distribution, &version),
        distribution,
        bin_dir: canonical,
        version,
        engines,
        tools,
        svg,
        pdf,
        png,
    })
}

/// Find every TeX installation on this machine, best suggestion first.
pub fn detect() -> Vec<Toolchain> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for dir in path_dirs().into_iter().chain(well_known_dirs()) {
        let Ok(canonical) = dir.canonicalize() else {
            continue;
        };
        if !seen.insert(canonical.clone()) {
            continue;
        }
        if let Some(tc) = probe_dir(&dir) {
            // Several PATH entries can point at the same installation through
            // symlinked binaries (e.g. /usr/bin vs /bin on merged-usr systems).
            let key = tc
                .tools
                .latex
                .as_ref()
                .or(tc.tools.pdflatex.as_ref())
                .or(tc.tools.tectonic.as_ref())
                .and_then(|p| p.canonicalize().ok());
            if let Some(key) = key {
                if !seen.insert(key) {
                    continue;
                }
            }
            found.push(tc);
        }
    }
    found.sort_by_key(|t| t.score());
    found
}

/// Pick the toolchain to use: an explicit choice (a bin directory or a
/// detected id) or the best detected installation.
pub fn resolve(choice: Option<&str>) -> Result<Toolchain, String> {
    match choice
        .map(str::trim)
        .filter(|c| !c.is_empty() && *c != "auto")
    {
        Some(dir) => probe_dir(Path::new(dir))
            .ok_or_else(|| format!("no TeX engine found in the selected folder: {dir}")),
        None => detect().into_iter().next().ok_or_else(|| {
            "No TeX installation was found. Install TeX Live, MiKTeX, MacTeX or Tectonic, \
             or choose the folder that contains your latex executable."
                .to_string()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(
            label_for(
                Distribution::TexLive,
                "pdfTeX 3.141592653-2.6-1.40.25 (TeX Live 2023/Debian)"
            ),
            "TeX Live 2023/Debian"
        );
        assert_eq!(
            label_for(Distribution::MiKTeX, "MiKTeX-pdfTeX 4.18 (MiKTeX 24.4)"),
            "MiKTeX 24.4"
        );
        assert_eq!(
            classify("MiKTeX-pdfTeX 4.18 (MiKTeX 24.4)", Path::new("C:/x")),
            Distribution::MiKTeX
        );
    }

    #[test]
    fn engine_names_round_trip() {
        for e in Engine::ALL {
            assert_eq!(Engine::parse(e.name()), Some(e));
        }
    }
}
