//! Window creation and geometry. All windows load the same `index.html`; the Vue hash
//! router picks the page (`#/panel`, `#/login`, ...), and the window label drives
//! per-window behaviour in the frontend.

use std::time::Duration;

use serde_json::Value;
use tauri::{
    AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::hit;

/// Panel sizes from the frontend (nav rail 286/118 + content 821, height 756).
pub const PANEL_CONTENT_W: f64 = 821.0;
pub const PANEL_NAV_EXPANDED_W: f64 = 286.0;
pub const PANEL_NAV_COLLAPSED_W: f64 = 118.0;
pub const PANEL_H: f64 = 756.0;

/// On Windows the WebView of the main overlay never receives real mouse input; the
/// frontend synthesizes pointer events from the native `mouse-*` events instead. On
/// macOS the WebView would get real events too whenever we stop click-through, so drop
/// trusted mouse events to avoid double handling.
const MAIN_INIT_SCRIPT: &str = r#"
(() => {
  const types = ['pointerdown','pointerup','pointermove','pointerover','pointerout',
    'pointerenter','pointerleave','pointercancel','mousedown','mouseup','mousemove',
    'mouseover','mouseout','mouseenter','mouseleave','click','dblclick','auxclick',
    'contextmenu','wheel','dragstart','selectstart'];
  const cancel = new Set(['contextmenu','wheel','dragstart','selectstart']);
  for (const t of types) {
    window.addEventListener(t, (e) => {
      if (!e.isTrusted) return;
      e.stopImmediatePropagation();
      if (cancel.has(t)) e.preventDefault();
    }, { capture: true, passive: false });
  }
  // A native WebKit image/text drag would start an AppKit drag session the frontend
  // never expects (Windows never delivers real mouse input to this WebView).
  const style = document.createElement('style');
  style.textContent = '*{-webkit-user-drag:none!important;-webkit-user-select:none!important}';
  (document.head || document.documentElement).appendChild(style);
})();
"#;

/// WebKit ignores `document.cookie` on the `tauri://` custom scheme, but the frontend
/// keeps its login token in cookies (`kuro_infoToken`). Back `document.cookie` with
/// localStorage, which all windows share because they have the same origin.
const COOKIE_SHIM: &str = r#"
(() => {
  const KEY = '__tappo_cookies__';
  const load = () => {
    try { return JSON.parse(localStorage.getItem(KEY) || '{}') || {}; } catch { return {}; }
  };
  const live = (jar) => {
    const now = Date.now();
    for (const k of Object.keys(jar)) if (jar[k].e !== null && jar[k].e <= now) delete jar[k];
    return jar;
  };
  Object.defineProperty(document, 'cookie', {
    configurable: true,
    get() {
      const jar = live(load());
      return Object.keys(jar).map((k) => `${k}=${jar[k].v}`).join('; ');
    },
    set(str) {
      const parts = String(str).split(';');
      const first = parts.shift() || '';
      const eq = first.indexOf('=');
      if (eq <= 0) return;
      const name = first.slice(0, eq).trim();
      const value = first.slice(eq + 1).trim();
      let exp = null;
      for (const p of parts) {
        const i = p.indexOf('=');
        const attr = (i < 0 ? p : p.slice(0, i)).trim().toLowerCase();
        const val = i < 0 ? '' : p.slice(i + 1).trim();
        if (attr === 'max-age') exp = Date.now() + Number(val) * 1000;
        else if (attr === 'expires' && exp === null) {
          const t = Date.parse(val);
          if (Number.isFinite(t)) exp = t;
        }
      }
      const jar = live(load());
      if (exp !== null && exp <= Date.now()) delete jar[name];
      else jar[name] = { v: value, e: exp };
      try { localStorage.setItem(KEY, JSON.stringify(jar)); } catch {}
    },
  });
})();
"#;

/// Routes WebSockets to Kuro hosts through `ws_proxy` (the room server only accepts
/// the Windows Origin). Other hosts keep the native WebSocket.
const WS_SHIM: &str = r#"
(() => {
  const NativeWS = window.WebSocket;
  const PROXY_HOST = /(^|\.)(kurogames-global|kurogames|kurogame)\.com$/i;
  const STATES = { CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 };

  class ProxyWebSocket extends EventTarget {
    constructor(url, protocols) {
      super();
      const u = new URL(url, location.href);
      const tauri = window.__TAURI__;
      if (!PROXY_HOST.test(u.hostname) || !tauri?.core?.Channel) {
        return protocols === undefined ? new NativeWS(url) : new NativeWS(url, protocols);
      }
      this.url = u.href;
      this.readyState = 0;
      this.protocol = '';
      this.extensions = '';
      this.bufferedAmount = 0;
      this.binaryType = 'blob';
      this.onopen = this.onmessage = this.onerror = this.onclose = null;
      this._id = null;
      this._pendingClose = null;
      const ch = new tauri.core.Channel();
      ch.onmessage = (ev) => this._onEvent(ev);
      const list = protocols === undefined ? [] : [].concat(protocols);
      tauri.core.invoke('ws_open', { url: this.url, protocols: list, onEvent: ch }).then((id) => {
        this._id = id;
        if (this._pendingClose) this._sendClose(...this._pendingClose);
      }, (err) => {
        this._fire(new Event('error'));
        this._finish(1006, String(err), false);
      });
    }
    _fire(ev) {
      const h = this['on' + ev.type];
      if (typeof h === 'function') { try { h.call(this, ev); } catch (e) { setTimeout(() => { throw e; }); } }
      this.dispatchEvent(ev);
    }
    _finish(code, reason, wasClean) {
      if (this.readyState === 3) return;
      this.readyState = 3;
      this._fire(new CloseEvent('close', { code, reason, wasClean }));
    }
    _onEvent(ev) {
      switch (ev.type) {
        case 'open':
          if (this.readyState !== 0) return;
          this.readyState = 1;
          this.protocol = ev.protocol || '';
          this._fire(new Event('open'));
          break;
        case 'text':
          if (this.readyState === 1) this._fire(new MessageEvent('message', { data: ev.data }));
          break;
        case 'binary': {
          if (this.readyState !== 1) return;
          const bytes = new Uint8Array(ev.data);
          const data = this.binaryType === 'arraybuffer' ? bytes.buffer : new Blob([bytes]);
          this._fire(new MessageEvent('message', { data }));
          break;
        }
        case 'error':
          this._fire(new Event('error'));
          break;
        case 'close':
          this._finish(ev.code, ev.reason || '', !!ev.wasClean);
          break;
      }
    }
    send(data) {
      if (this.readyState === 0) throw new DOMException('WebSocket is still connecting', 'InvalidStateError');
      if (this.readyState !== 1) return;
      const invoke = window.__TAURI__.core.invoke;
      if (typeof data === 'string') {
        invoke('ws_send', { id: this._id, text: data });
      } else if (data instanceof Blob) {
        data.arrayBuffer().then((b) => invoke('ws_send', { id: this._id, binary: Array.from(new Uint8Array(b)) }));
      } else {
        const view = ArrayBuffer.isView(data)
          ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength)
          : new Uint8Array(data);
        invoke('ws_send', { id: this._id, binary: Array.from(view) });
      }
    }
    close(code, reason) {
      if (this.readyState >= 2) return;
      this.readyState = 2;
      if (this._id === null) this._pendingClose = [code, reason];
      else this._sendClose(code, reason);
    }
    _sendClose(code, reason) {
      window.__TAURI__.core.invoke('ws_close', { id: this._id, code: code ?? null, reason: reason ?? null });
    }
  }
  Object.assign(ProxyWebSocket, STATES);
  Object.assign(ProxyWebSocket.prototype, STATES);
  window.WebSocket = ProxyWebSocket;
})();
"#;

struct AuxSpec {
    url: &'static str,
    width: f64,
    height: f64,
    resizable: bool,
}

fn aux_spec(label: &str) -> Option<AuxSpec> {
    let (url, width, height, resizable) = match label {
        // .login-dialog-card is 500x378; the KrSDK view resizes via resize_login_window_to_content.
        "login" => ("index.html#/login", 500.0, 378.0, false),
        "update" | "maintenance" => ("index.html#/update", 520.0, 440.0, false),
        "about" => ("index.html#/about", 520.0, 440.0, false),
        "oss-notice" => ("index.html#/oss-notice", 760.0, 560.0, true),
        _ => return None,
    };
    Some(AuxSpec { url, width, height, resizable })
}

pub fn create_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let main = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Wuwa Tappo")
        .initialization_script(COOKIE_SHIM)
        .initialization_script(WS_SHIM)
        .inner_size(800.0, 600.0)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .focused(false)
        .accept_first_mouse(true)
        .initialization_script(MAIN_INIT_SCRIPT)
        .build()?;
    fit_to_monitor(&main);
    let _ = main.set_ignore_cursor_events(true);
    hit::refresh_geometry(app);
    Ok(main)
}

fn build_aux(app: &AppHandle, label: &str, spec: &AuxSpec, visible: bool) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App(spec.url.into()))
        .title("Wuwa Tappo")
        .initialization_script(COOKIE_SHIM)
        .initialization_script(WS_SHIM)
        .inner_size(spec.width, spec.height)
        .resizable(spec.resizable)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .center()
        .visible(visible)
        .focused(visible)
        .build()
}

/// Pre-creates the modal windows hidden so their event listeners are ready, like the
/// Windows build does.
pub fn prewarm(app: &AppHandle) {
    for label in ["login", "update", "about", "oss-notice"] {
        if let Some(spec) = aux_spec(label) {
            let _ = build_aux(app, label, &spec, false);
        }
    }
    let _ = ensure_toast(app);
}

/// Shows (creating if needed) a modal window and emits `event` to it. A freshly created
/// window gets the event re-sent after it had time to register listeners.
pub fn show_aux(app: &AppHandle, label: &str, event: Option<(&'static str, Value)>) -> Result<(), String> {
    let spec = aux_spec(label).ok_or_else(|| format!("unknown window {label}"))?;
    let (win, created) = match app.get_webview_window(label) {
        Some(w) => (w, false),
        None => (build_aux(app, label, &spec, true).map_err(|e| e.to_string())?, true),
    };
    let _ = win.unminimize();
    win.show().map_err(|e| e.to_string())?;
    let _ = win.set_focus();
    if let Some((name, payload)) = event {
        let _ = win.emit_to(label, name, payload.clone());
        if created {
            let app = app.clone();
            let label = label.to_string();
            std::thread::spawn(move || {
                for delay in [700, 1500] {
                    std::thread::sleep(Duration::from_millis(delay));
                    let _ = app.emit_to(label.as_str(), name, payload.clone());
                }
            });
        }
    }
    Ok(())
}

pub fn hide(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.hide();
    }
}

pub fn open_panel(app: &AppHandle, tab: Option<String>) -> Result<(), String> {
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.unminimize();
        panel.show().map_err(|e| e.to_string())?;
        let _ = panel.set_focus();
        if let Some(tab) = tab {
            let _ = panel.emit_to("panel", "panel-open-tab", tab);
        }
        return Ok(());
    }
    let url = match &tab {
        Some(t) => format!("index.html#/panel?tab={t}"),
        None => "index.html#/panel".to_string(),
    };
    let panel = WebviewWindowBuilder::new(app, "panel", WebviewUrl::App(url.into()))
        .title("Wuwa Tappo")
        .initialization_script(COOKIE_SHIM)
        .initialization_script(WS_SHIM)
        .inner_size(PANEL_NAV_EXPANDED_W + PANEL_CONTENT_W, PANEL_H)
        .resizable(true)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .focused(true)
        .build()
        .map_err(|e| e.to_string())?;
    center_on_main_monitor(app, &panel);
    Ok(())
}

pub fn ensure_toast(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(t) = app.get_webview_window("toast") {
        return Ok(t);
    }
    let toast = WebviewWindowBuilder::new(app, "toast", WebviewUrl::App("index.html#/toast".into()))
        .title("Wuwa Tappo")
        .initialization_script(COOKIE_SHIM)
        .initialization_script(WS_SHIM)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .focused(false)
        .visible(false)
        .build()?;
    let _ = toast.set_ignore_cursor_events(true);
    if let Some(m) = main_monitor(app) {
        apply_bounds(&toast, *m.position(), *m.size());
    }
    Ok(toast)
}

pub fn main_monitor(app: &AppHandle) -> Option<Monitor> {
    let main = app.get_webview_window("main")?;
    main.current_monitor().ok().flatten().or_else(|| main.primary_monitor().ok().flatten())
}

pub fn apply_bounds(win: &WebviewWindow, pos: PhysicalPosition<i32>, size: PhysicalSize<u32>) {
    let _ = win.set_position(pos);
    let _ = win.set_size(size);
}

/// Stretches the main overlay over its whole current monitor ("fullscreen shell").
pub fn fit_to_monitor(main: &WebviewWindow) {
    let monitor = main
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| main.primary_monitor().ok().flatten());
    if let Some(m) = monitor {
        apply_bounds(main, *m.position(), *m.size());
    }
}

pub fn center_on_main_monitor(app: &AppHandle, win: &WebviewWindow) {
    let (Some(m), Ok(size)) = (main_monitor(app), win.outer_size()) else { return };
    let x = m.position().x + (m.size().width as i32 - size.width as i32) / 2;
    let y = m.position().y + (m.size().height as i32 - size.height as i32) / 2;
    let _ = win.set_position(PhysicalPosition::new(x, y));
}

/// Insets (logical px) of the area outside the monitor's work area: menu bar and Dock.
pub fn safe_insets(app: &AppHandle) -> serde_json::Value {
    let Some(m) = main_monitor(app) else {
        return serde_json::json!({ "left": 0, "top": 0, "right": 0, "bottom": 0 });
    };
    let s = m.scale_factor();
    let (mp, ms) = (m.position(), m.size());
    let wa = m.work_area();
    let left = (wa.position.x - mp.x).max(0) as f64 / s;
    let top = (wa.position.y - mp.y).max(0) as f64 / s;
    let right = ((mp.x + ms.width as i32) - (wa.position.x + wa.size.width as i32)).max(0) as f64 / s;
    let bottom = ((mp.y + ms.height as i32) - (wa.position.y + wa.size.height as i32)).max(0) as f64 / s;
    serde_json::json!({ "left": left, "top": top, "right": right, "bottom": bottom })
}
