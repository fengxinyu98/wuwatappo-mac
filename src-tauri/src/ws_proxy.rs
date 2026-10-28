//! WebSocket relay for the room feature.
//!
//! The room server rejects the WebSocket handshake unless the Origin is the Windows
//! app's (`http://tauri.localhost`); on macOS pages run on `tauri://localhost`. The
//! page-side shim (`WS_SHIM` in windows.rs) replaces `window.WebSocket` for Kuro hosts
//! and routes the connection through these commands, which connect from Rust with the
//! Windows Origin header.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

const ORIGIN: &str = "http://tauri.localhost";
const ALLOWED_SUFFIXES: [&str; 3] = ["kurogames-global.com", "kurogames.com", "kurogame.com"];

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WsEvent {
    Open { protocol: String },
    Text { data: String },
    Binary { data: Vec<u8> },
    #[serde(rename_all = "camelCase")]
    Close { code: u16, reason: String, was_clean: bool },
    Error { message: String },
}

#[derive(Default)]
pub struct WsState {
    next_id: AtomicU32,
    conns: Mutex<HashMap<u32, UnboundedSender<Message>>>,
}

fn host_allowed(host: &str) -> bool {
    ALLOWED_SUFFIXES.iter().any(|s| host == *s || host.ends_with(&format!(".{s}")))
}

#[tauri::command]
pub async fn ws_open(
    app: AppHandle,
    state: State<'_, WsState>,
    url: String,
    protocols: Vec<String>,
    on_event: Channel<WsEvent>,
) -> Result<u32, String> {
    let mut req = url.as_str().into_client_request().map_err(|e| e.to_string())?;
    let host = req.uri().host().unwrap_or_default().to_string();
    if !host_allowed(&host) {
        return Err(format!("host not allowed: {host}"));
    }
    req.headers_mut().insert("Origin", HeaderValue::from_static(ORIGIN));
    if !protocols.is_empty() {
        let v = HeaderValue::from_str(&protocols.join(", ")).map_err(|e| e.to_string())?;
        req.headers_mut().insert("Sec-WebSocket-Protocol", v);
    }

    let id = state.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    let (tx, mut rx) = unbounded_channel::<Message>();
    state.conns.lock().unwrap().insert(id, tx);

    tauri::async_runtime::spawn(async move {
        let state = app.state::<WsState>();
        let ws = match connect_async(req).await {
            Ok((ws, resp)) => {
                let protocol = resp
                    .headers()
                    .get("Sec-WebSocket-Protocol")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                let _ = on_event.send(WsEvent::Open { protocol });
                ws
            }
            Err(e) => {
                let _ = on_event.send(WsEvent::Error { message: e.to_string() });
                let _ = on_event.send(WsEvent::Close { code: 1006, reason: String::new(), was_clean: false });
                state.conns.lock().unwrap().remove(&id);
                return;
            }
        };
        let (mut sink, mut stream) = ws.split();
        let mut closing = false;
        let close = loop {
            tokio::select! {
                out = rx.recv(), if !closing => match out {
                    Some(msg) => {
                        closing = matches!(msg, Message::Close(_));
                        if let Err(e) = sink.send(msg).await {
                            break WsEvent::Close { code: 1006, reason: e.to_string(), was_clean: false };
                        }
                    }
                    None => {
                        let _ = sink.send(Message::Close(None)).await;
                        closing = true;
                    }
                },
                incoming = stream.next() => match incoming {
                    Some(Ok(Message::Text(t))) => { let _ = on_event.send(WsEvent::Text { data: t.to_string() }); }
                    Some(Ok(Message::Binary(b))) => { let _ = on_event.send(WsEvent::Binary { data: b.to_vec() }); }
                    Some(Ok(Message::Close(frame))) => {
                        let (code, reason) = frame
                            .map(|f| (u16::from(f.code), f.reason.to_string()))
                            .unwrap_or((1005, String::new()));
                        break WsEvent::Close { code, reason, was_clean: true };
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        let _ = on_event.send(WsEvent::Error { message: e.to_string() });
                        break WsEvent::Close { code: 1006, reason: String::new(), was_clean: false };
                    }
                    None => break WsEvent::Close { code: 1006, reason: String::new(), was_clean: closing },
                },
            }
        };
        state.conns.lock().unwrap().remove(&id);
        let _ = on_event.send(close);
    });
    Ok(id)
}

#[tauri::command]
pub fn ws_send(state: State<'_, WsState>, id: u32, text: Option<String>, binary: Option<Vec<u8>>) -> Result<(), String> {
    let msg = match (text, binary) {
        (Some(t), _) => Message::Text(t.into()),
        (None, Some(b)) => Message::Binary(b.into()),
        (None, None) => return Ok(()),
    };
    let conns = state.conns.lock().unwrap();
    let tx = conns.get(&id).ok_or("socket closed")?;
    tx.send(msg).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ws_close(state: State<'_, WsState>, id: u32, code: Option<u16>, reason: Option<String>) {
    let frame = CloseFrame {
        code: CloseCode::from(code.unwrap_or(1000)),
        reason: reason.unwrap_or_default().into(),
    };
    if let Some(tx) = state.conns.lock().unwrap().get(&id) {
        let _ = tx.send(Message::Close(Some(frame)));
    }
}
