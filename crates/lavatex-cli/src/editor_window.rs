//! The editor window for the PowerPoint add-in (Windows): the editor page
//! (hosts/editor) in a WebView2 window. The page talks to Rust through
//! `window.ipc.postMessage`; replies come back through
//! `window.__lavatexReply(id, data)`. Rendering runs on worker threads so the
//! window stays responsive.

use crate::ppt::{handle_message, Action, EXIT_CANCELLED, EXIT_FAILED};
use lavatex_core::metadata::EquationMeta;
use serde_json::Value;
use std::borrow::Cow;
use std::path::PathBuf;
use std::process::ExitCode;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::http::{header::CONTENT_TYPE, Response};
use wry::{WebContext, WebViewBuilder};

enum UserEvent {
    Message(String),
    Reply(u64, String),
    Exit(u8),
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "png" => "image/png",
        _ => "application/octet-stream",
    }
}

fn serve(path: &str) -> Response<Cow<'static, [u8]>> {
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    match crate::embedded::EDITOR_FILES
        .iter()
        .find(|(n, _)| *n == rel)
    {
        Some((_, bytes)) => Response::builder()
            .header(CONTENT_TYPE, content_type(rel))
            .body(Cow::Borrowed(*bytes))
            .expect("valid response"),
        None => Response::builder()
            .status(404)
            .body(Cow::Borrowed(&b"not found"[..]))
            .expect("valid response"),
    }
}

fn fail(out: &std::path::Path, message: &str) -> ExitCode {
    let _ = std::fs::create_dir_all(out);
    let _ = std::fs::write(out.join("error.txt"), message);
    ExitCode::from(EXIT_FAILED)
}

pub fn run(init: Value, out: PathBuf, previous: Option<EquationMeta>) -> ExitCode {
    let previous = std::sync::Arc::new(previous);
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let editing = init["editing"].as_bool().unwrap_or(false);
    let window = match WindowBuilder::new()
        .with_title(if editing {
            "LavaTeX \u{2013} Edit equation"
        } else {
            "LavaTeX \u{2013} New equation"
        })
        .with_inner_size(LogicalSize::new(760.0, 680.0))
        .with_min_inner_size(LogicalSize::new(480.0, 420.0))
        .build(&event_loop)
    {
        Ok(w) => w,
        Err(e) => return fail(&out, &format!("Cannot open the LavaTeX window: {e}")),
    };

    // WebView2 keeps its profile here (cache, local storage).
    let data_dir =
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("LavaTeX").join("WebView2"));
    let mut context = WebContext::new(data_dir);
    let proxy = event_loop.create_proxy();
    let webview = match WebViewBuilder::with_web_context(&mut context)
        .with_initialization_script(&format!("window.__lavatexInit = {init};"))
        .with_custom_protocol("lavatex".into(), |_id, request| serve(request.uri().path()))
        .with_ipc_handler(move |request| {
            let _ = proxy.send_event(UserEvent::Message(request.into_body()));
        })
        // Custom protocols are served as http://<scheme>.localhost on Windows.
        .with_url("http://lavatex.localhost/index.html")
        .build(&window)
    {
        Ok(w) => w,
        Err(e) => {
            return fail(
                &out,
                &format!(
                    "Cannot show the LavaTeX editor: {e}\n\nIt needs the Microsoft Edge WebView2 Runtime, which comes with Windows 11 and Microsoft 365. Install it from https://go.microsoft.com/fwlink/p/?LinkId=2124703 and try again."
                ),
            )
        }
    };
    window.set_focus();

    let proxy = event_loop.create_proxy();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Message(text)) => {
                let Ok(msg) = serde_json::from_str::<Value>(&text) else {
                    return;
                };
                let id = msg.get("id").and_then(Value::as_u64).unwrap_or(0);
                let proxy = proxy.clone();
                let out = out.clone();
                let previous = std::sync::Arc::clone(&previous);
                std::thread::spawn(move || {
                    let event = match handle_message(&msg, &out, previous.as_ref().as_ref()) {
                        Action::Reply(v) => UserEvent::Reply(id, v.to_string()),
                        Action::Exit(code) => UserEvent::Exit(code),
                    };
                    let _ = proxy.send_event(event);
                });
            }
            Event::UserEvent(UserEvent::Reply(id, json)) => {
                let _ = webview.evaluate_script(&format!("window.__lavatexReply({id}, {json});"));
            }
            Event::UserEvent(UserEvent::Exit(code)) => std::process::exit(code.into()),
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => std::process::exit(EXIT_CANCELLED.into()),
            _ => {}
        }
    })
}
