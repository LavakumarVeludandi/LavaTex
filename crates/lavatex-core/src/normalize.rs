//! Accept LaTeX the way people actually type it.
//!
//! IguanaTeX users are used to editing a complete document
//! (`\documentclass … \begin{document} $…$ \end{document}`), and many people
//! type their own math delimiters (`$…$`, `\[…\]`, `\begin{align*}…`). LavaTeX
//! wraps the source itself, so those would end up nested inside our own
//! delimiters. This module turns any of these forms into a plain body plus a
//! mode and preamble, and remembers line offsets so errors still point at the
//! right line of what the user typed.

use crate::render::Mode;

#[derive(Debug, Clone, PartialEq)]
pub struct Normalized {
    /// Preamble to use; `None` keeps the request's preamble.
    pub preamble: Option<String>,
    pub mode: Mode,
    pub body: String,
    /// Lines of the user's source that precede `body` (for error mapping).
    pub body_line_offset: usize,
    /// When the preamble came from a pasted document, the source line where
    /// it starts (0-based offset), for error mapping.
    pub preamble_source_offset: Option<usize>,
}

/// Math environments that only work in vertical mode, mapped to the
/// inline-safe amsmath equivalent.
const ENV_MAP: &[(&str, Option<&str>)] = &[
    ("equation*", None),
    ("equation", None),
    ("displaymath", None),
    ("math", None),
    ("align*", Some("aligned")),
    ("align", Some("aligned")),
    ("flalign*", Some("aligned")),
    ("flalign", Some("aligned")),
    ("alignat*", Some("alignedat")),
    ("alignat", Some("alignedat")),
    ("gather*", Some("gathered")),
    ("gather", Some("gathered")),
    ("multline*", Some("gathered")),
    ("multline", Some("gathered")),
    ("eqnarray*", Some("aligned")),
    ("eqnarray", Some("aligned")),
];

pub fn normalize(source: &str, mode: Mode) -> Normalized {
    let mut out = Normalized {
        preamble: None,
        mode,
        body: source.to_string(),
        body_line_offset: 0,
        preamble_source_offset: None,
    };

    // 1. A complete document, IguanaTeX style.
    if let Some(begin) = find_command(source, "\\begin{document}") {
        let head = &source[..begin];
        let after = begin + "\\begin{document}".len();
        let end = find_command(&source[after..], "\\end{document}")
            .map(|e| after + e)
            .unwrap_or(source.len());
        // LavaTeX supplies the document class; blank its line so preamble
        // line numbers still match the user's source.
        let preamble_lines: Vec<&str> = head
            .lines()
            .map(|l| {
                if l.trim_start().starts_with("\\documentclass") {
                    ""
                } else {
                    l
                }
            })
            .collect();
        out.preamble = Some(preamble_lines.join("\n"));
        out.preamble_source_offset = Some(0);
        out.body_line_offset = source[..after].matches('\n').count();
        out.body = source[after..end].to_string();
        out.mode = Mode::Text;
    }

    // 2. The body is a single math block with its own delimiters.
    if let Some((mode, inner, skipped_lines)) = strip_math(&out.body) {
        out.mode = mode;
        out.body_line_offset += skipped_lines;
        out.body = inner;
    }
    out
}

/// Position of `cmd` outside comments.
fn find_command(text: &str, cmd: &str) -> Option<usize> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let code = strip_comment(line);
        if let Some(p) = code.find(cmd) {
            return Some(offset + p);
        }
        offset += line.len();
    }
    None
}

fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'%' => return &line[..i],
            _ => i += 1,
        }
    }
    line
}

/// If `body` (ignoring surrounding whitespace and comment lines) is exactly one
/// delimited math block, return the mode, the inner math, and how many source
/// lines precede the inner math.
fn strip_math(body: &str) -> Option<(Mode, String, usize)> {
    let lead_len = leading_blank_and_comment_len(body);
    let trimmed = body[lead_len..].trim_end();
    let base_lines = body[..lead_len].matches('\n').count();

    let simple: &[(&str, &str, Mode)] = &[
        ("$$", "$$", Mode::Display),
        ("\\[", "\\]", Mode::Display),
        ("\\(", "\\)", Mode::Inline),
        ("$", "$", Mode::Inline),
    ];
    for (open, close, mode) in simple {
        if let Some(rest) = trimmed.strip_prefix(open) {
            if let Some(inner) = rest.strip_suffix(close) {
                // Exactly one block: the closer must not appear inside.
                if !inner.contains(close) && !inner.trim().is_empty() {
                    let lines = base_lines + trimmed[..open.len()].matches('\n').count();
                    return Some((*mode, inner.to_string(), lines));
                }
            }
        }
    }

    for (env, replacement) in ENV_MAP {
        let open = format!("\\begin{{{env}}}");
        let close = format!("\\end{{{env}}}");
        if let Some(rest) = trimmed.strip_prefix(open.as_str()) {
            if let Some(inner) = rest.strip_suffix(close.as_str()) {
                if inner.contains(&open) {
                    continue;
                }
                // Numbered environments: the number makes no sense on a slide.
                let inner = inner.replace("\\nonumber", "").replace("\\notag", "");
                let body = match replacement {
                    None => inner,
                    // alignat needs its column-count argument to stay attached.
                    Some(rep) if *rep == "alignedat" => {
                        let (arg, rest) = split_group(&inner);
                        format!("\\begin{{alignedat}}{arg}{rest}\\end{{alignedat}}")
                    }
                    Some(rep) => format!("\\begin{{{rep}}}{inner}\\end{{{rep}}}"),
                };
                return Some((Mode::Display, body, base_lines));
            }
        }
    }
    None
}

fn leading_blank_and_comment_len(text: &str) -> usize {
    let mut len = 0;
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        if t.is_empty() || t.starts_with('%') {
            len += line.len();
        } else {
            let indent = line.len() - line.trim_start().len();
            return len + indent;
        }
    }
    len
}

/// Split a leading `{...}` group from `text`.
fn split_group(text: &str) -> (String, String) {
    let t = text.trim_start();
    if !t.starts_with('{') {
        return (String::new(), text.to_string());
    }
    let mut depth = 0;
    for (i, c) in t.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return (t[..=i].to_string(), t[i + 1..].to_string());
                }
            }
            _ => {}
        }
    }
    (String::new(), text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_math_is_untouched() {
        let n = normalize("\\frac{a}{b}", Mode::Display);
        assert_eq!(n.body, "\\frac{a}{b}");
        assert_eq!(n.mode, Mode::Display);
        assert_eq!(n.preamble, None);
    }

    #[test]
    fn strips_user_delimiters() {
        assert_eq!(normalize("$x^2$", Mode::Display).body, "x^2");
        assert_eq!(normalize("$x^2$", Mode::Display).mode, Mode::Inline);
        let n = normalize("\\[\n  x = 1\n\\]", Mode::Display);
        assert_eq!(n.mode, Mode::Display);
        assert_eq!(n.body.trim(), "x = 1");
        assert_eq!(normalize("$$ y $$", Mode::Inline).mode, Mode::Display);
        // Two separate formulas are text, not one math block.
        assert_eq!(normalize("$a$ and $b$", Mode::Display).body, "$a$ and $b$");
    }

    #[test]
    fn converts_display_environments() {
        let n = normalize(
            "\\begin{align*}\na &= b \\\\\nc &= d\n\\end{align*}",
            Mode::Display,
        );
        assert_eq!(n.mode, Mode::Display);
        assert!(n.body.starts_with("\\begin{aligned}"));
        assert!(n.body.ends_with("\\end{aligned}"));
        let n = normalize(
            "\\begin{equation}\nE=mc^2 \\label{x}\n\\end{equation}",
            Mode::Display,
        );
        assert_eq!(n.body.trim(), "E=mc^2 \\label{x}");
        let n = normalize("\\begin{alignat*}{2} a &= b \\end{alignat*}", Mode::Display);
        assert_eq!(n.body, "\\begin{alignedat}{2} a &= b \\end{alignedat}");
    }

    #[test]
    fn full_iguanatex_document() {
        let doc = "\\documentclass{article}\n\\usepackage{amsmath}\n\\pagestyle{empty}\n\\begin{document}\n\n$\\displaystyle\n\\int_0^1 x\\,dx\n$\n\n\\end{document}\n";
        let n = normalize(doc, Mode::Display);
        assert_eq!(n.mode, Mode::Inline);
        assert!(n.body.contains("\\int_0^1 x\\,dx"));
        let pre = n.preamble.unwrap();
        assert!(pre.contains("\\usepackage{amsmath}"));
        assert!(!pre.contains("documentclass"));
        // body starts after "\begin{document}" on line 4 plus the blank line
        assert_eq!(n.body_line_offset, 5);
    }

    #[test]
    fn document_with_text_body_uses_text_mode() {
        let doc = "\\documentclass{article}\n\\begin{document}\nHello $x$ world\n\\end{document}";
        let n = normalize(doc, Mode::Display);
        assert_eq!(n.mode, Mode::Text);
        assert_eq!(n.body.trim(), "Hello $x$ world");
    }

    #[test]
    fn commented_begin_document_is_ignored() {
        let n = normalize("% \\begin{document}\nx", Mode::Display);
        assert_eq!(n.preamble, None);
    }
}
