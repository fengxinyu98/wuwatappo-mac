//! Click-through control for the full-screen transparent main overlay.
//!
//! The original Windows build keeps the WebView input-transparent and decides in native
//! code which screen areas the overlay "swallows". The frontend reports those areas via
//! `set_main_hit_region`; here we toggle `set_ignore_cursor_events` on the main window
//! whenever the global cursor enters or leaves them.

use serde::Deserialize;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
    /// Room teammate this region belongs to; native mouse events over it carry the id.
    pub member_id: Option<serde_json::Value>,
}

impl Rect {
    fn contains(&self, x: f64, y: f64) -> bool {
        self.width > 0.0
            && self.height > 0.0
            && x >= self.left
            && y >= self.top
            && x < self.left + self.width
            && y < self.top + self.height
    }
}

pub struct HitState {
    pub regions: Vec<Rect>,
    pub input_regions: Vec<Rect>,
    pub window_passthrough: bool,
    pub swallow: bool,
    /// Main window inner origin in global logical points.
    pub origin: (f64, f64),
    pub scale: f64,
    /// Last global cursor position in logical points.
    pub last_cursor: Option<(f64, f64)>,
    pub capturing: bool,
}

impl Default for HitState {
    fn default() -> Self {
        Self {
            regions: Vec::new(),
            input_regions: Vec::new(),
            window_passthrough: false,
            swallow: false,
            origin: (0.0, 0.0),
            scale: 1.0,
            last_cursor: None,
            capturing: false,
        }
    }
}

impl HitState {
    fn wants_capture(&self, gx: f64, gy: f64) -> bool {
        if self.swallow {
            return true;
        }
        let (x, y) = (gx - self.origin.0, gy - self.origin.1);
        if self.input_regions.iter().any(|r| r.contains(x, y)) {
            return true;
        }
        !self.window_passthrough && self.regions.iter().any(|r| r.contains(x, y))
    }

    fn member_at(&self, gx: f64, gy: f64) -> Option<serde_json::Value> {
        let (x, y) = (gx - self.origin.0, gy - self.origin.1);
        self.input_regions
            .iter()
            .chain(self.regions.iter())
            .find(|r| r.member_id.is_some() && r.contains(x, y))
            .and_then(|r| r.member_id.clone())
    }
}

/// Teammate whose region is under global logical point (gx, gy), if any.
pub fn member_at(app: &AppHandle, gx: f64, gy: f64) -> Option<serde_json::Value> {
    app.state::<Hit>().0.lock().unwrap().member_at(gx, gy)
}

pub struct Hit(pub Mutex<HitState>);

pub fn scale(app: &AppHandle) -> f64 {
    app.state::<Hit>().0.lock().unwrap().scale
}

/// Re-evaluates capture at `cursor` (global logical points) or at the last known cursor
/// position, and flips the main window's click-through only when the answer changes.
pub fn update_capture(app: &AppHandle, cursor: Option<(f64, f64)>) {
    let change = {
        let hit = app.state::<Hit>();
        let mut s = hit.0.lock().unwrap();
        if cursor.is_some() {
            s.last_cursor = cursor;
        }
        let Some((x, y)) = s.last_cursor else { return };
        let want = s.wants_capture(x, y);
        if want == s.capturing {
            None
        } else {
            s.capturing = want;
            Some(want)
        }
    };
    if let Some(want) = change {
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.set_ignore_cursor_events(!want);
        }
    }
}

/// True if global logical point (gx, gy) is over one of our own visible windows other
/// than the overlays. Those windows report their own clicks (e.g. the panel sends a
/// `pet-clap` action), so the global listener must not count them again — same as
/// the Windows build.
pub fn over_own_window(app: &AppHandle, gx: f64, gy: f64) -> bool {
    app.webview_windows().iter().any(|(label, w)| {
        if label == "main" || label == "toast" || !w.is_visible().unwrap_or(false) {
            return false;
        }
        let (Ok(scale), Ok(pos), Ok(size)) = (w.scale_factor(), w.outer_position(), w.outer_size()) else {
            return false;
        };
        let (x, y) = (pos.x as f64 / scale, pos.y as f64 / scale);
        let (wd, ht) = (size.width as f64 / scale, size.height as f64 / scale);
        gx >= x && gy >= y && gx < x + wd && gy < y + ht
    })
}

/// Caches the main window origin/scale so the input thread never has to query the window.
pub fn refresh_geometry(app: &AppHandle) {
    let Some(main) = app.get_webview_window("main") else { return };
    let (Ok(scale), Ok(pos)) = (main.scale_factor(), main.inner_position()) else { return };
    let hit = app.state::<Hit>();
    let mut s = hit.0.lock().unwrap();
    s.scale = scale;
    s.origin = (pos.x as f64 / scale, pos.y as f64 / scale);
}
