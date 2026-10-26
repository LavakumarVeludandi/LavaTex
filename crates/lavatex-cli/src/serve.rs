//! `lavatex serve`: a small HTTP helper on 127.0.0.1 so browser-based hosts
//! (ONLYOFFICE, the PowerPoint add-in) can use the user's local TeX.
//!
//! Hardening: loopback-only bind, Host header allow-list (blocks DNS
//! rebinding), bearer token on every endpoint except `/v1/health`, request
//! size limits, and the same sandboxed renderer as the CLI.
//!
//! When `lavatex setup powerpoint` has created a certificate, the helper also
//! listens on `https://localhost:<port + 1>` and serves the PowerPoint add-in
//! there (Office only loads add-ins over HTTPS). TLS connections are
//! decrypted and passed to the HTTP handler on `<port>`, so both share one
//! implementation.

use crate::{apply_config, new_token};
use base64::Engine as _;
use lavatex_core::config::{load_or_create_token, Config};
use lavatex_core::render::{self, RenderRequest};
use lavatex_core::{metadata, toolchain, Toolchain};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Request, Response, Server};

pub const DEFAULT_PORT: u16 = 7354;
/// Seconds without requests after which an on-demand helper exits.
pub const DEFAULT_IDLE_EXIT: u64 = 900;
const MAX_BODY: usize = 512 * 1024;
const WORKERS: usize = 4;
const TOOLCHAIN_CACHE: Duration = Duration::from_secs(60);
/// Most TLS connections handled at once.
const MAX_TLS_CONNECTIONS: usize = 32;
/// Replaced with the access token in the add-in's HTML pages.
const TOKEN_PLACEHOLDER: &str = "__LAVATEX_TOKEN__";
/// Replaced with `https://localhost:<port>` in the add-in's files.
const ORIGIN_PLACEHOLDER: &str = "__LAVATEX_ORIGIN__";
const VERSION_PLACEHOLDER: &str = "__LAVATEX_VERSION__";

/// The HTTPS port that goes with an HTTP port.
pub fn https_port(port: u16) -> u16 {
    port.wrapping_add(1)
}

struct State {
    port: u16,
    /// Set when the HTTPS listener (PowerPoint add-in) is running.
    https_port: Option<u16>,
    /// Seconds since `started` of the last request (for idle exit).
    last_activity: AtomicU64,
    started: Instant,
    token: String,
    toolchains: Mutex<Option<(Instant, Vec<Toolchain>)>>,
}

impl State {
    fn toolchains(&self, refresh: bool) -> Vec<Toolchain> {
        let mut cache = self.toolchains.lock().unwrap();
        if let Some((at, list)) = cache.as_ref() {
            if !refresh && at.elapsed() < TOOLCHAIN_CACHE {
                return list.clone();
            }
        }
        let list = toolchain::detect();
        *cache = Some((Instant::now(), list.clone()));
        list
    }
}

/// Sockets handed over by systemd socket activation (`LISTEN_FDS`): the
/// HTTP socket and, when configured, the HTTPS socket.
#[cfg(unix)]
fn inherited_listeners() -> Vec<TcpListener> {
    use std::os::fd::FromRawFd;
    let ours = std::env::var("LISTEN_PID")
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
        == Some(std::process::id());
    let fds: i32 = std::env::var("LISTEN_FDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    if !ours {
        return Vec::new();
    }
    (0..fds.clamp(0, 2))
        .filter_map(|i| {
            // SAFETY: systemd passes listening sockets from fd 3
            // (SD_LISTEN_FDS_START) on; nothing else in this process owns them.
            let listener = unsafe { TcpListener::from_raw_fd(3 + i) };
            listener.set_nonblocking(false).ok()?;
            Some(listener)
        })
        .collect()
}

#[cfg(not(unix))]
fn inherited_listeners() -> Vec<TcpListener> {
    Vec::new()
}

/// Run the helper. With `idle_exit`, exit after that many seconds without a
/// request (used when the OS starts the helper on demand).
pub fn run(port: u16, token: Option<String>, idle_exit: Option<u64>) -> ExitCode {
    let token = match token {
        Some(t) if t.len() >= 16 => t,
        Some(_) => {
            eprintln!("error: the token must be at least 16 characters");
            return ExitCode::from(1);
        }
        None => match load_or_create_token(new_token) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("error: cannot create helper token: {e}");
                return ExitCode::from(1);
            }
        },
    };
    let mut inherited = inherited_listeners().into_iter();
    let (server, port) = match inherited.next() {
        Some(listener) => {
            let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
            match Server::from_listener(listener, None) {
                Ok(s) => (Arc::new(s), port),
                Err(e) => {
                    eprintln!("error: cannot use the socket from the service manager: {e}");
                    return ExitCode::from(1);
                }
            }
        }
        None => match Server::http(("127.0.0.1", port)) {
            Ok(s) => (Arc::new(s), port),
            Err(e) => {
                eprintln!("error: cannot listen on 127.0.0.1:{port}: {e}");
                return ExitCode::from(1);
            }
        },
    };
    let https = start_https(port, inherited.next());
    let state = Arc::new(State {
        port,
        https_port: https,
        last_activity: AtomicU64::new(0),
        started: Instant::now(),
        token,
        toolchains: Mutex::new(None),
    });
    eprintln!(
        "LavaTeX helper {} listening on http://127.0.0.1:{port}",
        lavatex_core::VERSION
    );
    if let Some(p) = https {
        eprintln!("PowerPoint add-in at https://localhost:{p}/powerpoint/taskpane.html");
    }

    if let Some(limit) = idle_exit.filter(|s| *s > 0) {
        let state = Arc::clone(&state);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5));
            let idle =
                state.started.elapsed().as_secs() - state.last_activity.load(Ordering::Relaxed);
            if idle >= limit {
                std::process::exit(0);
            }
        });
    }

    let handles: Vec<_> = (0..WORKERS)
        .map(|_| {
            let server = Arc::clone(&server);
            let state = Arc::clone(&state);
            std::thread::spawn(move || {
                for request in server.incoming_requests() {
                    state
                        .last_activity
                        .store(state.started.elapsed().as_secs(), Ordering::Relaxed);
                    handle(&state, request);
                }
            })
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
    ExitCode::SUCCESS
}

/// Start the HTTPS listener if a certificate exists. Failure only disables
/// HTTPS (and so the PowerPoint add-in); ONLYOFFICE keeps working.
fn start_https(port: u16, inherited: Option<TcpListener>) -> Option<u16> {
    let dir = crate::tls::tls_dir()?;
    if !crate::tls::is_configured(&dir) {
        return None;
    }
    let config = match crate::tls::server_config(&dir) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("warning: HTTPS disabled: {e}");
            return None;
        }
    };
    let tls_port = https_port(port);
    let listener = match inherited
        .map(Ok)
        .unwrap_or_else(|| TcpListener::bind(("127.0.0.1", tls_port)))
    {
        Ok(l) => l,
        Err(e) => {
            eprintln!("warning: HTTPS disabled: cannot listen on 127.0.0.1:{tls_port}: {e}");
            return None;
        }
    };
    let tls_port = listener.local_addr().map(|a| a.port()).unwrap_or(tls_port);
    let active = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if active.fetch_add(1, Ordering::SeqCst) >= MAX_TLS_CONNECTIONS {
                active.fetch_sub(1, Ordering::SeqCst);
                continue; // dropped: the browser retries
            }
            let config = Arc::clone(&config);
            let active = Arc::clone(&active);
            std::thread::spawn(move || {
                let _ = tls_connection(stream, config, port);
                active.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Some(tls_port)
}

/// Read one HTTP request from a TLS connection, pass it to the plain HTTP
/// handler on `backend_port`, and send its response back. One request per
/// connection (the forwarded request asks for `Connection: close`).
fn tls_connection(
    tcp: TcpStream,
    config: Arc<rustls::ServerConfig>,
    backend_port: u16,
) -> std::io::Result<()> {
    let timeout = Some(Duration::from_secs(30));
    tcp.set_read_timeout(timeout)?;
    tcp.set_write_timeout(timeout)?;
    let conn = rustls::ServerConnection::new(config).map_err(std::io::Error::other)?;
    let mut tls = rustls::StreamOwned::new(conn, tcp);

    // Request head.
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() > 32 * 1024 {
            return Err(std::io::Error::other("request head too large"));
        }
        let n = tls.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default().to_string();
    let mut content_length = 0usize;
    let mut forwarded = format!("{request_line}\r\n");
    for line in lines.filter(|l| !l.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name_lc = name.trim().to_ascii_lowercase();
        match name_lc.as_str() {
            "content-length" => {
                content_length = value.trim().parse().unwrap_or(usize::MAX);
            }
            "transfer-encoding" => {
                // Browsers send fetch() bodies with a length; refuse the rest.
                write!(
                    tls,
                    "HTTP/1.1 411 Length Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )?;
                tls.conn.send_close_notify();
                return tls.flush();
            }
            "connection" | "keep-alive" | "proxy-connection" | "upgrade" => continue,
            _ => {}
        }
        forwarded.push_str(line);
        forwarded.push_str("\r\n");
    }
    forwarded.push_str("Connection: close\r\n\r\n");
    if content_length > MAX_BODY {
        write!(
            tls,
            "HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )?;
        tls.conn.send_close_notify();
        return tls.flush();
    }
    let mut body = buf[head_end..].to_vec();
    while body.len() < content_length {
        let n = tls.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);

    let mut backend = TcpStream::connect(("127.0.0.1", backend_port))?;
    backend.set_read_timeout(Some(Duration::from_secs(150)))?;
    backend.write_all(forwarded.as_bytes())?;
    backend.write_all(&body)?;
    let mut response = Vec::new();
    backend.read_to_end(&mut response)?;
    tls.write_all(&response)?;
    tls.conn.send_close_notify();
    tls.flush()
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "xml" => "application/xml; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Office wants four-part versions (`0.2.0.0`).
fn manifest_version() -> String {
    let mut parts: Vec<&str> = lavatex_core::VERSION
        .split(['.', '-', '+'])
        .take(3)
        .collect();
    while parts.len() < 3 {
        parts.push("0");
    }
    format!("{}.0", parts.join("."))
}

fn fill_placeholders(text: &str, https_port: u16) -> String {
    text.replace(
        ORIGIN_PLACEHOLDER,
        &format!("https://localhost:{https_port}"),
    )
    .replace(VERSION_PLACEHOLDER, &manifest_version())
}

/// The add-in manifest for a helper whose HTTPS port is `https_port`.
pub fn addin_manifest(https_port: u16) -> String {
    let (_, bytes) = crate::embedded::ADDIN_FILES
        .iter()
        .find(|(name, _)| *name == "manifest.xml")
        .expect("manifest.xml is embedded");
    fill_placeholders(&String::from_utf8_lossy(bytes), https_port)
}

/// Serve a file of the PowerPoint add-in (`/powerpoint/<rel>`). HTML pages
/// get the access token written into them; they are never sent with CORS
/// headers, so other web sites cannot read them.
fn serve_addin_file(state: &State, req: Request, rel: &str, https_port: u16) {
    let Some((_, bytes)) = crate::embedded::ADDIN_FILES
        .iter()
        .find(|(name, _)| *name == rel)
    else {
        let _ = req.respond(Response::from_string("not found").with_status_code(404));
        return;
    };
    // A cross-site page has no business loading these.
    let cross_site = header_value(&req, "Sec-Fetch-Site").is_some_and(|v| v == "cross-site");
    let origin = format!("https://localhost:{https_port}");
    let foreign_origin = header_value(&req, "Origin").is_some_and(|o| o != origin);
    if cross_site || foreign_origin {
        let _ = req.respond(Response::from_string("forbidden").with_status_code(403));
        return;
    }
    let ctype = content_type(rel);
    let body: Vec<u8> = if ctype.starts_with("text/") || ctype.starts_with("application/xml") {
        let mut text = fill_placeholders(&String::from_utf8_lossy(bytes), https_port);
        if rel.ends_with(".html") {
            text = text.replace(TOKEN_PLACEHOLDER, &state.token);
        }
        text.into_bytes()
    } else {
        bytes.to_vec()
    };
    let response = Response::from_data(body)
        .with_header(header("Content-Type", ctype))
        .with_header(header("Cache-Control", "no-cache"))
        .with_header(header("X-Content-Type-Options", "nosniff"))
        .with_header(header("Referrer-Policy", "no-referrer"));
    let _ = req.respond(response);
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

fn header_value<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn respond(req: Request, status: u16, body: Value) {
    let origin = header_value(&req, "Origin").unwrap_or("*").to_string();
    let response = Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(header("Content-Type", "application/json; charset=utf-8"))
        .with_header(header("Cache-Control", "no-store"))
        .with_header(header("Access-Control-Allow-Origin", &origin))
        .with_header(header("Vary", "Origin"))
        .with_header(header(
            "Access-Control-Allow-Headers",
            "Authorization, Content-Type",
        ))
        .with_header(header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"))
        .with_header(header("Access-Control-Allow-Private-Network", "true"))
        .with_header(header("Access-Control-Max-Age", "600"));
    let _ = req.respond(response);
}

/// The port named in an allowed Host header, if the header is allowed.
fn allowed_host_port(req: &Request, ports: &[u16]) -> Option<u16> {
    let host = header_value(req, "Host")?.trim().to_ascii_lowercase();
    ports.iter().copied().find(|port| {
        [
            format!("127.0.0.1:{port}"),
            format!("localhost:{port}"),
            format!("[::1]:{port}"),
        ]
        .contains(&host)
    })
}

fn authorized(req: &Request, token: &str) -> bool {
    header_value(req, "Authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| constant_time_eq(t.trim().as_bytes(), token.as_bytes()))
}

fn read_json<T: for<'de> Deserialize<'de>>(req: &mut Request) -> Result<T, String> {
    if req.body_length().is_some_and(|n| n > MAX_BODY) {
        return Err("request body too large".into());
    }
    let mut body = Vec::new();
    req.as_reader()
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() > MAX_BODY {
        return Err("request body too large".into());
    }
    serde_json::from_slice(&body).map_err(|e| format!("invalid JSON: {e}"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectRequest {
    alt_text: Option<String>,
    data_base64: Option<String>,
}

fn handle(state: &State, mut req: Request) {
    let ports: Vec<u16> = std::iter::once(state.port)
        .chain(state.https_port)
        .collect();
    let Some(host_port) = allowed_host_port(&req, &ports) else {
        return respond(
            req,
            403,
            json!({"ok": false, "error": {"summary": "forbidden host"}}),
        );
    };
    let path = req.url().split('?').next().unwrap_or("").to_string();
    let query = req
        .url()
        .split_once('?')
        .map(|(_, q)| q.to_string())
        .unwrap_or_default();
    let method = req.method().clone();

    if method == Method::Options {
        return respond(req, 204, Value::Null);
    }
    // The PowerPoint add-in, only over HTTPS.
    if let Some(rel) = path.strip_prefix("/powerpoint/") {
        if method == Method::Get && state.https_port == Some(host_port) {
            let rel = rel.to_string();
            return serve_addin_file(state, req, &rel, host_port);
        }
    }
    if method == Method::Get && path == "/v1/health" {
        return respond(
            req,
            200,
            json!({"ok": true, "name": "lavatex", "version": lavatex_core::VERSION}),
        );
    }
    if !authorized(&req, &state.token) {
        return respond(
            req,
            401,
            json!({"ok": false, "error": {"kind": "unauthorized", "summary": "Missing or wrong access token. Run `lavatex token` and paste it into the plugin settings."}}),
        );
    }

    match (method, path.as_str()) {
        (Method::Post, "/v1/shutdown") => {
            // Used by `lavatex setup` to replace an older helper.
            respond(req, 200, json!({"ok": true}));
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(200));
                std::process::exit(0);
            });
        }
        (Method::Get, "/v1/toolchains") => {
            let list = state.toolchains(query.contains("refresh=1"));
            let config = Config::load();
            let suggested = list.first().map(|t| t.id.clone());
            respond(
                req,
                200,
                json!({"ok": true, "toolchains": list, "suggested": suggested, "config": config}),
            )
        }
        (Method::Post, "/v1/render") => {
            let mut render_req: RenderRequest = match read_json(&mut req) {
                Ok(r) => r,
                Err(e) => {
                    return respond(
                        req,
                        400,
                        json!({"ok": false, "error": {"kind": "invalidInput", "summary": e}}),
                    )
                }
            };
            apply_config(&mut render_req, &Config::load());
            let result = match render_req
                .toolchain
                .as_deref()
                .filter(|t| !t.is_empty() && *t != "auto")
            {
                Some(_) => render::render(&render_req),
                None => match state.toolchains(false).into_iter().next() {
                    Some(tc) => render::render_with(&render_req, &tc),
                    None => render::render(&render_req),
                },
            };
            match result {
                Ok(out) => {
                    let mut v = serde_json::to_value(&out).expect("serializes");
                    v["ok"] = true.into();
                    v["dataBase64"] = base64::engine::general_purpose::STANDARD
                        .encode(&out.bytes)
                        .into();
                    respond(req, 200, v)
                }
                Err(e) => respond(req, 200, json!({"ok": false, "error": e})),
            }
        }
        (Method::Post, "/v1/inspect") => {
            let body: InspectRequest = match read_json(&mut req) {
                Ok(b) => b,
                Err(e) => return respond(req, 400, json!({"ok": false, "error": {"summary": e}})),
            };
            let result = if let Some(alt) = body.alt_text {
                metadata::EquationMeta::from_alt_text(&alt)
            } else if let Some(data) = body.data_base64 {
                match base64::engine::general_purpose::STANDARD.decode(data) {
                    Ok(bytes) => metadata::extract_any(&bytes),
                    Err(e) => Err(metadata::MetaError::Invalid(e.to_string())),
                }
            } else {
                Err(metadata::MetaError::NotFound)
            };
            match result {
                Ok(meta) => respond(req, 200, json!({"ok": true, "meta": meta})),
                Err(e) => respond(
                    req,
                    200,
                    json!({"ok": false, "error": {"summary": e.to_string()}}),
                ),
            }
        }
        _ => respond(
            req,
            404,
            json!({"ok": false, "error": {"summary": "not found"}}),
        ),
    }
}
