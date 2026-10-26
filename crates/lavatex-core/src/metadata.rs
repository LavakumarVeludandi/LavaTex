//! Equation metadata: the record that turns an ordinary image back into an
//! editable LaTeX equation.
//!
//! The same JSON record is stored in up to three places so that it survives as
//! many hosts and file formats as possible (see `docs/metadata.md`):
//!
//! 1. inside the image bytes (SVG `<metadata>`, PDF document info, PNG `iTXt`),
//! 2. in the picture's alt text / description, after the human-readable source,
//! 3. in host-native properties, which each host adapter handles itself.

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// Schema identifier written into every record.
pub const SCHEMA: &str = "lavatex-equation/1";
/// Prefix of the machine-readable line in alt text.
pub const ALT_TEXT_MARKER: &str = "lavatex:1:";
/// XML namespace used for the SVG metadata element.
pub const XML_NAMESPACE: &str = "https://lavatex.org/ns/1";
/// PDF document-info key and PNG text keyword.
pub const KEYWORD: &str = "LavaTeX";
/// Upper bound for an encoded record; anything larger is rejected when reading.
pub const MAX_RECORD_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EquationMeta {
    pub schema: String,
    pub source: String,
    #[serde(default)]
    pub preamble: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_font_size")]
    pub font_size: f64,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub engine: String,
    /// Rendered (display) size in PostScript points (1/72 in) at `font_size`.
    #[serde(default)]
    pub width_bp: f64,
    #[serde(default)]
    pub height_bp: f64,
    /// Distance from the baseline to the bottom edge, in points.
    #[serde(default)]
    pub depth_bp: f64,
    #[serde(default)]
    pub generator: String,
}

fn default_mode() -> String {
    "display".into()
}
fn default_font_size() -> f64 {
    20.0
}
fn default_color() -> String {
    "#000000".into()
}
fn default_format() -> String {
    "svg".into()
}

#[derive(Debug)]
pub enum MetaError {
    NotFound,
    TooLarge,
    Invalid(String),
    UnsupportedSchema(String),
}

impl std::fmt::Display for MetaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetaError::NotFound => write!(f, "no LavaTeX metadata found"),
            MetaError::TooLarge => write!(f, "metadata is larger than the allowed limit"),
            MetaError::Invalid(e) => write!(f, "metadata is not valid: {e}"),
            MetaError::UnsupportedSchema(s) => write!(f, "unsupported metadata schema: {s}"),
        }
    }
}

impl std::error::Error for MetaError {}

impl EquationMeta {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("metadata always serializes")
    }

    pub fn from_json(json: &str) -> Result<Self, MetaError> {
        if json.len() > MAX_RECORD_BYTES {
            return Err(MetaError::TooLarge);
        }
        let meta: EquationMeta =
            serde_json::from_str(json).map_err(|e| MetaError::Invalid(e.to_string()))?;
        if !meta.schema.starts_with("lavatex-equation/") {
            return Err(MetaError::UnsupportedSchema(meta.schema));
        }
        if meta.schema != SCHEMA {
            // Same family, newer revision: refuse rather than misinterpret.
            return Err(MetaError::UnsupportedSchema(meta.schema));
        }
        Ok(meta)
    }

    /// Alt text: the LaTeX source for people and screen readers, followed by a
    /// marker line carrying the full record for LavaTeX.
    pub fn to_alt_text(&self) -> String {
        format!(
            "{}\n\n{}{}",
            self.source.trim(),
            ALT_TEXT_MARKER,
            URL_SAFE_NO_PAD.encode(self.to_json())
        )
    }

    pub fn from_alt_text(text: &str) -> Result<Self, MetaError> {
        let start = text.rfind(ALT_TEXT_MARKER).ok_or(MetaError::NotFound)?;
        let encoded: String = text[start + ALT_TEXT_MARKER.len()..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '=')
            .filter(|c| *c != '=')
            .collect();
        if encoded.len() > MAX_RECORD_BYTES * 4 / 3 + 4 {
            return Err(MetaError::TooLarge);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded.as_bytes())
            .map_err(|e| MetaError::Invalid(e.to_string()))?;
        let json = String::from_utf8(bytes).map_err(|e| MetaError::Invalid(e.to_string()))?;
        Self::from_json(&json)
    }
}

/// Hex encoding used for the PDF document-info string (`<...>` avoids all
/// PDF/TeX escaping issues).
pub fn pdf_info_hex(meta: &EquationMeta) -> String {
    meta.to_json().bytes().map(|b| format!("{b:02X}")).collect()
}

/// Insert the record into an SVG document as a `<metadata>` child of the root.
pub fn embed_svg(svg: &str, meta: &EquationMeta) -> String {
    let element = format!(
        "<metadata><lavatex:equation xmlns:lavatex='{XML_NAMESPACE}' encoding='base64'>{}</lavatex:equation></metadata>",
        STANDARD.encode(meta.to_json())
    );
    match find_svg_root_end(svg) {
        Some(pos) => {
            let mut out = String::with_capacity(svg.len() + element.len() + 1);
            out.push_str(&svg[..pos]);
            out.push('\n');
            out.push_str(&element);
            out.push_str(&svg[pos..]);
            out
        }
        None => svg.to_string(),
    }
}

/// Byte offset just after the `>` that closes the root `<svg ...>` start tag.
fn find_svg_root_end(svg: &str) -> Option<usize> {
    let mut search = 0;
    while let Some(rel) = svg[search..].find("<svg") {
        let start = search + rel;
        let after = svg.as_bytes().get(start + 4).copied();
        if matches!(after, Some(b' ' | b'\n' | b'\t' | b'\r' | b'>')) {
            let mut quote: Option<u8> = None;
            for (i, b) in svg.as_bytes()[start..].iter().enumerate() {
                match (quote, *b) {
                    (None, b'"' | b'\'') => quote = Some(*b),
                    (Some(q), c) if c == q => quote = None,
                    (None, b'>') => return Some(start + i + 1),
                    _ => {}
                }
            }
            return None;
        }
        search = start + 4;
    }
    None
}

pub fn extract_svg(svg: &str) -> Result<EquationMeta, MetaError> {
    let open = svg.find("<lavatex:equation").ok_or(MetaError::NotFound)?;
    let body_start = svg[open..].find('>').ok_or(MetaError::NotFound)? + open + 1;
    let body_end = svg[body_start..]
        .find("</lavatex:equation>")
        .ok_or(MetaError::NotFound)?
        + body_start;
    let encoded: String = svg[body_start..body_end]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if encoded.len() > MAX_RECORD_BYTES * 4 / 3 + 4 {
        return Err(MetaError::TooLarge);
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|e| MetaError::Invalid(e.to_string()))?;
    let json = String::from_utf8(bytes).map_err(|e| MetaError::Invalid(e.to_string()))?;
    EquationMeta::from_json(&json)
}

/// Find `/LavaTeX <hex>` or `/LavaTeX (literal)` in an uncompressed PDF
/// document-info dictionary. (pdfTeX keeps our hex string; dvipdfmx rewrites
/// it as an escaped literal string.)
pub fn extract_pdf(pdf: &[u8]) -> Result<EquationMeta, MetaError> {
    let key = b"/LavaTeX";
    let pos = find_bytes(pdf, key).ok_or(MetaError::NotFound)?;
    let rest = &pdf[pos + key.len()..];
    let start = rest
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .ok_or(MetaError::NotFound)?;
    let bytes = match rest[start] {
        b'<' => {
            let gt = rest[start..]
                .iter()
                .position(|b| *b == b'>')
                .ok_or(MetaError::NotFound)?;
            let hex: Vec<u8> = rest[start + 1..start + gt]
                .iter()
                .copied()
                .filter(|b| !b.is_ascii_whitespace())
                .collect();
            if hex.len() / 2 > MAX_RECORD_BYTES {
                return Err(MetaError::TooLarge);
            }
            decode_hex(&hex).ok_or_else(|| MetaError::Invalid("bad hex string".into()))?
        }
        b'(' => decode_pdf_literal(&rest[start + 1..])?,
        _ => return Err(MetaError::NotFound),
    };
    let json = if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]))
            .collect();
        String::from_utf16(&units).map_err(|e| MetaError::Invalid(e.to_string()))?
    } else {
        String::from_utf8(bytes).map_err(|e| MetaError::Invalid(e.to_string()))?
    };
    EquationMeta::from_json(&json)
}

/// Decode a PDF literal string body (after the opening parenthesis).
fn decode_pdf_literal(input: &[u8]) -> Result<Vec<u8>, MetaError> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < input.len() {
        if out.len() > MAX_RECORD_BYTES {
            return Err(MetaError::TooLarge);
        }
        let b = input[i];
        match b {
            b'\\' => {
                i += 1;
                let Some(&e) = input.get(i) else { break };
                match e {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'0'..=b'7' => {
                        let mut v = 0u32;
                        let mut n = 0;
                        while n < 3 && matches!(input.get(i), Some(b'0'..=b'7')) {
                            v = v * 8 + (input[i] - b'0') as u32;
                            i += 1;
                            n += 1;
                        }
                        out.push(v as u8);
                        continue;
                    }
                    b'\r' => {
                        if input.get(i + 1) == Some(&b'\n') {
                            i += 1;
                        }
                    }
                    b'\n' => {}
                    other => out.push(other),
                }
            }
            b'(' => {
                depth += 1;
                out.push(b);
            }
            b')' => {
                if depth == 0 {
                    return Ok(out);
                }
                depth -= 1;
                out.push(b);
            }
            _ => out.push(b),
        }
        i += 1;
    }
    Err(MetaError::Invalid("unterminated PDF string".into()))
}

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Add an uncompressed `iTXt` chunk (keyword `LavaTeX`) just before `IEND`.
pub fn embed_png(png: &[u8], meta: &EquationMeta) -> Option<Vec<u8>> {
    if !png.starts_with(PNG_SIGNATURE) {
        return None;
    }
    let iend = find_chunk(png, b"IEND")?;
    let mut data = Vec::new();
    data.extend_from_slice(KEYWORD.as_bytes());
    data.extend_from_slice(&[0, 0, 0]); // separator, compression flag, method
    data.push(0); // empty language tag
    data.push(0); // empty translated keyword
    data.extend_from_slice(meta.to_json().as_bytes());

    let mut chunk = Vec::with_capacity(data.len() + 12);
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut crc_input = b"iTXt".to_vec();
    crc_input.extend_from_slice(&data);
    chunk.extend_from_slice(&crc_input);
    chunk.extend_from_slice(&crc32(&crc_input).to_be_bytes());

    let mut out = Vec::with_capacity(png.len() + chunk.len());
    out.extend_from_slice(&png[..iend]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[iend..]);
    Some(out)
}

pub fn extract_png(png: &[u8]) -> Result<EquationMeta, MetaError> {
    if !png.starts_with(PNG_SIGNATURE) {
        return Err(MetaError::NotFound);
    }
    let mut pos = PNG_SIGNATURE.len();
    while pos + 8 <= png.len() {
        let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
        let kind = &png[pos + 4..pos + 8];
        let data_end = pos + 8 + len;
        if data_end + 4 > png.len() {
            break;
        }
        let data = &png[pos + 8..data_end];
        if (kind == b"iTXt" || kind == b"tEXt") && data.starts_with(KEYWORD.as_bytes()) {
            let after = &data[KEYWORD.len()..];
            if after.first() == Some(&0) {
                let text = if kind == b"iTXt" {
                    // keyword\0 flag method lang\0 translated\0 text
                    if after.get(1) != Some(&0) {
                        return Err(MetaError::Invalid("compressed iTXt not supported".into()));
                    }
                    let rest = &after[3..];
                    let lang_end = rest
                        .iter()
                        .position(|b| *b == 0)
                        .ok_or(MetaError::NotFound)?;
                    let rest = &rest[lang_end + 1..];
                    let tr_end = rest
                        .iter()
                        .position(|b| *b == 0)
                        .ok_or(MetaError::NotFound)?;
                    &rest[tr_end + 1..]
                } else {
                    &after[1..]
                };
                if text.len() > MAX_RECORD_BYTES {
                    return Err(MetaError::TooLarge);
                }
                let json =
                    std::str::from_utf8(text).map_err(|e| MetaError::Invalid(e.to_string()))?;
                return EquationMeta::from_json(json);
            }
        }
        pos = data_end + 4;
    }
    Err(MetaError::NotFound)
}

/// Read metadata from image bytes of any supported format.
pub fn extract_any(bytes: &[u8]) -> Result<EquationMeta, MetaError> {
    if bytes.starts_with(PNG_SIGNATURE) {
        return extract_png(bytes);
    }
    if bytes.starts_with(b"%PDF") {
        return extract_pdf(bytes);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) if text.contains("<svg") => extract_svg(text),
        Ok(text) => EquationMeta::from_alt_text(text),
        Err(_) => Err(MetaError::NotFound),
    }
}

fn find_chunk(png: &[u8], name: &[u8; 4]) -> Option<usize> {
    let mut pos = PNG_SIGNATURE.len();
    while pos + 8 <= png.len() {
        let len = u32::from_be_bytes(png[pos..pos + 4].try_into().ok()?) as usize;
        if &png[pos + 4..pos + 8] == name {
            return Some(pos);
        }
        pos += 12 + len;
    }
    None
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn decode_hex(hex: &[u8]) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    hex.chunks(2)
        .map(|pair| {
            let s = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(s, 16).ok()
        })
        .collect()
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> EquationMeta {
        EquationMeta {
            schema: SCHEMA.into(),
            source: "\\frac{a}{b} + ü <>&\"'".into(),
            preamble: "\\usepackage{amsmath}".into(),
            mode: "display".into(),
            font_size: 24.0,
            color: "#112233".into(),
            format: "svg".into(),
            engine: "latex".into(),
            width_bp: 40.5,
            height_bp: 20.25,
            depth_bp: 5.0,
            generator: "lavatex test".into(),
        }
    }

    #[test]
    fn alt_text_round_trip() {
        let meta = sample();
        let alt = meta.to_alt_text();
        assert!(alt.starts_with("\\frac{a}{b}"));
        assert_eq!(EquationMeta::from_alt_text(&alt).unwrap(), meta);
        // Hosts sometimes append whitespace or trailing text.
        let padded = format!("{alt}\n  ");
        assert_eq!(EquationMeta::from_alt_text(&padded).unwrap(), meta);
    }

    #[test]
    fn svg_round_trip() {
        let svg = "<?xml version='1.0'?>\n<!-- svg comment -->\n<svg version='1.1' a='x>y' xmlns='http://www.w3.org/2000/svg'><path d='M0 0'/></svg>";
        let out = embed_svg(svg, &sample());
        assert!(
            out.contains("a='x>y'\n<metadata>")
                || out.contains("xmlns='http://www.w3.org/2000/svg'>\n<metadata>")
        );
        assert_eq!(extract_svg(&out).unwrap(), sample());
        assert_eq!(extract_any(out.as_bytes()).unwrap(), sample());
    }

    #[test]
    fn png_round_trip() {
        // 1x1 transparent PNG
        let png = STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==")
            .unwrap();
        let out = embed_png(&png, &sample()).unwrap();
        assert!(out.ends_with(&png[png.len() - 12..]));
        assert_eq!(extract_png(&out).unwrap(), sample());
        assert!(matches!(extract_png(&png), Err(MetaError::NotFound)));
    }

    #[test]
    fn pdf_extract() {
        let pdf = format!(
            "%PDF-1.5\n1 0 obj << /Producer (x) /LavaTeX <{}> >> endobj",
            pdf_info_hex(&sample())
        );
        assert_eq!(extract_pdf(pdf.as_bytes()).unwrap(), sample());
        // dvipdfmx style: escaped literal string
        let json = sample()
            .to_json()
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        let pdf = format!("%PDF-1.4\n2 0 obj <</Creator(x)/LavaTeX({json})/Producer(y)>> endobj");
        assert_eq!(extract_pdf(pdf.as_bytes()).unwrap(), sample());
    }

    #[test]
    fn rejects_unknown_schema() {
        let mut meta = sample();
        meta.schema = "lavatex-equation/2".into();
        assert!(matches!(
            EquationMeta::from_alt_text(&meta.to_alt_text()),
            Err(MetaError::UnsupportedSchema(_))
        ));
    }

    #[test]
    fn crc_matches_known_value() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;

    /// The shared fixture lets the JavaScript and Python readers check that
    /// they parse exactly what the Rust core writes.
    #[test]
    fn shared_alt_text_fixture_matches() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/metadata/alt-text.json"
        );
        let meta = EquationMeta {
            schema: SCHEMA.into(),
            source: "\\int_0^1 x^2\\,dx = \\tfrac{1}{3} \\quad \\text{Größe ✓}".into(),
            preamble: "\\usepackage{amsmath}\n\\usepackage{amssymb}".into(),
            mode: "display".into(),
            font_size: 24.0,
            color: "#1a4f9c".into(),
            format: "svg".into(),
            engine: "latex".into(),
            width_bp: 120.5,
            height_bp: 30.25,
            depth_bp: 8.0,
            generator: "LavaTeX fixture".into(),
        };
        let expected = serde_json::json!({ "altText": meta.to_alt_text(), "meta": meta });
        let text = serde_json::to_string_pretty(&expected).unwrap() + "\n";
        if std::env::var_os("LAVATEX_UPDATE_FIXTURES").is_some() {
            std::fs::write(path, &text).unwrap();
        }
        let stored = std::fs::read_to_string(path)
            .expect("fixture exists; run with LAVATEX_UPDATE_FIXTURES=1");
        assert_eq!(
            stored, text,
            "fixture is stale; run with LAVATEX_UPDATE_FIXTURES=1"
        );
        assert_eq!(
            EquationMeta::from_alt_text(&meta.to_alt_text()).unwrap(),
            meta
        );
    }
}
