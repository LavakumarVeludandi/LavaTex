//! `lavatex serve`: a small HTTP helper on 127.0.0.1 so browser-based hosts
//! (ONLYOFFICE today, Office.js later) can use the user's local TeX.
//!
//! Hardening: loopback-only bind, Host header allow-list (blocks DNS
//! rebinding), bearer token on every endpoint except `/v1/health`, request
//! size limits, and the same sandboxed renderer as the CLI.

use crate::{apply_config, new_token};
use base64::Engine as _;
use lavatex_core::config::{load_or_create_token, Config};
use lavatex_core::render::{self, RenderRequest};
use lavatex_core::{metadata, toolchain, Toolchain};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::Read;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Request, Response, Server};

pub const DEFAULT_PORT: u16 = 7354;
const MAX_BODY: usize = 512 * 1024;
const WORKERS: usize = 4;
const TOOLCHAIN_CACHE: Duration = Duration::from_secs(60);

struct State {
    port: u16,
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

pub fn run(port: u16, token: Option<String>) -> ExitCode {
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
    let server = match Server::http(("127.0.0.1", port)) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("error: cannot listen on 127.0.0.1:{port}: {e}");
            return ExitCode::from(1);
        }
    };
    let state = Arc::new(State {
        port,
        token,
        toolchains: Mutex::new(None),
    });
    eprintln!(
        "LavaTeX helper {} listening on http://127.0.0.1:{port}",
        lavatex_core::VERSION
    );
    eprintln!("Plugin access token: {}", state.token);

    let handles: Vec<_> = (0..WORKERS)
        .map(|_| {
            let server = Arc::clone(&server);
            let state = Arc::clone(&state);
            std::thread::spawn(move || {
                for request in server.incoming_requests() {
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

fn host_allowed(req: &Request, port: u16) -> bool {
    let Some(host) = header_value(req, "Host") else {
        return false;
    };
    let host = host.trim().to_ascii_lowercase();
    [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ]
    .contains(&host)
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
    if !host_allowed(&req, state.port) {
        return respond(
            req,
            403,
            json!({"ok": false, "error": {"summary": "forbidden host"}}),
        );
    }
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
