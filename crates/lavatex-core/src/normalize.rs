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
    /// The body now uses amsmath environments (aligned, gathered, ...).
    pub needs_amsmath: bool,
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
        needs_amsmath: false,
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

    // 2. The body is a single math block with its own delimiters, or several
    //    display blocks in a row (stacked, one per line).
    if let Some((mode, inner, skipped_lines)) = strip_math(&out.body) {
        out.needs_amsmath |= inner.contains("\\begin{aligned")
            || inner.contains("\\begin{gathered}")
            || inner.contains("\\begin{alignedat}");
        out.mode = mode;
        out.body_line_offset += skipped_lines;
        out.body = inner;
    } else if let Some(stacked) = stack_display_blocks(&out.body) {
        out.mode = Mode::Display;
        out.body = stacked;
        out.needs_amsmath = true;
    }

    // 3. Line breaks: `\\` outside any environment starts a new line. In math
    //    that needs an amsmath environment (inline math ignores `\\`): lines
    //    are aligned at `&` if the user used `&`, centred otherwise. In text
    //    mode a one-column table does the same.
    let (breaks, ampersand) = top_level_breaks(&out.body);
    if breaks {
        out.body = match out.mode {
            Mode::Display | Mode::Inline => {
                out.needs_amsmath = true;
                let env = if ampersand { "aligned" } else { "gathered" };
                format!("\\begin{{{env}}}{}\\end{{{env}}}", out.body)
            }
            Mode::Text => format!(
                "\\begin{{tabular}}{{@{{}}l@{{}}}}{}\\end{{tabular}}",
                out.body
            ),
        };
    }
    out
}

/// Does `body` contain `\\` (and `&`) outside braces and environments?
fn top_level_breaks(body: &str) -> (bool, bool) {
    let b = body.as_bytes();
    let (mut depth, mut envs) = (0i32, 0i32);
    let (mut breaks, mut amp) = (false, false);
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'&' if depth == 0 && envs == 0 => amp = true,
            b'\\' => {
                let next = b.get(i + 1).copied();
                if next == Some(b'\\') {
                    if depth == 0 && envs == 0 {
                        breaks = true;
                    }
                    i += 2;
                    continue;
                }
                if next.is_some_and(|c| c.is_ascii_alphabetic()) {
                    let start = i + 1;
                    let mut j = start;
                    while j < b.len() && b[j].is_ascii_alphabetic() {
                        j += 1;
                    }
                    match &body[start..j] {
                        "begin" => envs += 1,
                        "end" => envs -= 1,
                        _ => {}
                    }
                    i = j;
                    continue;
                }
                i += 2; // escaped character such as \& or \{
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    (breaks, amp)
}

/// Several display blocks in a row (`\[a\] \[b\]`, two `align*`, ...) become
/// one `gathered` block with one row per original block. Delimiters are
/// replaced in place so line numbers stay the same.
fn stack_display_blocks(body: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = body;
    let mut count = 0;
    loop {
        let skip = leading_blank_and_comment_len(rest);
        out.push_str(&rest[..skip]);
        rest = &rest[skip..];
        if rest.trim().is_empty() {
            out.push_str(rest);
            break;
        }
        let sep = if count > 0 { "\\\\ " } else { "" };
        let mut matched = false;
        for (open, close) in [("\\[", "\\]"), ("$$", "$$")] {
            if let Some(after) = rest.strip_prefix(open) {
                let end = after.find(close)?;
                out.push_str(sep);
                out.push_str(&after[..end]);
                rest = &after[end + close.len()..];
                matched = true;
                break;
            }
        }
        if !matched {
            for (env, replacement) in ENV_MAP {
                let open = format!("\\begin{{{env}}}");
                let close = format!("\\end{{{env}}}");
                if let Some(after) = rest.strip_prefix(open.as_str()) {
                    let end = after.find(&close)?;
                    let inner = after[..end]
                        .replace("\\nonumber", "")
                        .replace("\\notag", "");
                    out.push_str(sep);
                    match replacement {
                        None => out.push_str(&inner),
                        Some(rep) => {
                            out.push_str(&format!("\\begin{{{rep}}}{inner}\\end{{{rep}}}"))
                        }
                    }
                    rest = &after[end + close.len()..];
                    matched = true;
                    break;
                }
            }
        }
        if !matched {
            return None; // something other than display blocks
        }
        count += 1;
    }
    (count >= 2).then(|| format!("\\begin{{gathered}}{out}\\end{{gathered}}"))
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
    fn double_backslash_makes_lines() {
        let n = normalize("a = b \\\\\nc = d", Mode::Display);
        assert_eq!(n.body, "\\begin{gathered}a = b \\\\\nc = d\\end{gathered}");
        assert!(n.needs_amsmath);
        let n = normalize("a &= b \\\\\n  &= c", Mode::Display);
        assert!(n.body.starts_with("\\begin{aligned}"));
        // Breaks inside an environment or a group are left alone.
        let n = normalize(
            "\\begin{pmatrix} 1 & 2 \\\\ 3 & 4 \\end{pmatrix}",
            Mode::Display,
        );
        assert!(!n.body.starts_with("\\begin{gathered}"));
        let n = normalize("\\frac{a \\\\ b}{c}", Mode::Display);
        assert!(!n.body.starts_with("\\begin"));
        // Text mode uses a one-column table.
        let n = normalize("first line \\\\ second", Mode::Text);
        assert!(n.body.starts_with("\\begin{tabular}"));
        // A newline alone does not break the line (LaTeX semantics).
        let n = normalize("a + b\n+ c", Mode::Display);
        assert_eq!(n.body, "a + b\n+ c");
    }

    #[test]
    fn stacks_several_display_blocks() {
        let n = normalize("\\[ a = b \\]\n\\[ c = d \\]", Mode::Display);
        assert_eq!(n.mode, Mode::Display);
        assert_eq!(
            n.body,
            "\\begin{gathered} a = b \n\\\\  c = d \\end{gathered}"
        );
        let doc = "\\documentclass{article}\n\\begin{document}\n\\begin{align*}\na &= 1\n\\end{align*}\n\\begin{equation}\nb = 2\n\\end{equation}\n\\end{document}";
        let n = normalize(doc, Mode::Display);
        assert!(
            n.body.contains("\\begin{aligned}\na &= 1\n\\end{aligned}"),
            "{}",
            n.body
        );
        assert!(n.body.contains("\\\\ \nb = 2"), "{}", n.body);
        // Mixed text and math is not stacked.
        assert!(stack_display_blocks("\\[a\\] some text \\[b\\]").is_none());
    }

    #[test]
    fn commented_begin_document_is_ignored() {
        let n = normalize("% \\begin{document}\nx", Mode::Display);
        assert_eq!(n.preamble, None);
    }
}
