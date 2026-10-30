//! User-configurable global shortcuts (added in 1.0.3; replaced the fixed F7/F8/F9).
//!
//! Config shape (shared with the frontend): `{ action: { modifiers, key } }` where
//! `key` is a Windows virtual-key code (0 = unset) and `modifiers` is a bit set:
//! Alt=1, Ctrl=2, Shift=4, Win/Cmd=8. The global key listener calls `on_key_down`;
//! a match emits `app-shortcut` with the action name to the main window.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Binding {
    pub modifiers: u32,
    pub key: u32,
}

pub type Config = BTreeMap<String, Binding>;

fn defaults() -> Config {
    [("gameMode", 118), ("switchScreen", 119), ("togglePet", 120)]
        .into_iter()
        .map(|(a, key)| (a.to_string(), Binding { modifiers: 0, key }))
        .collect()
}

#[derive(Default)]
pub struct Shortcuts {
    config: Mutex<Option<Config>>,
    /// The settings page is recording a new shortcut: don't fire actions meanwhile.
    capturing: AtomicBool,
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    let _ = fs::create_dir_all(&dir);
    Some(dir.join("shortcuts.json"))
}

fn current(app: &AppHandle) -> Config {
    let st = app.state::<Shortcuts>();
    let mut cfg = st.config.lock().unwrap();
    cfg.get_or_insert_with(|| {
        let mut c = defaults();
        if let Some(saved) = path(app)
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<Config>(&s).ok())
        {
            c.extend(saved);
        }
        c
    })
    .clone()
}

/// Called by the global key listener for every (non-repeat) key press.
pub fn on_key_down(app: &AppHandle, vk: u32, modifiers: u32) {
    if app.state::<Shortcuts>().capturing.load(Ordering::Relaxed) {
        return;
    }
    let pressed = Binding { modifiers, key: vk };
    for (action, b) in current(app) {
        if b.key != 0 && b == pressed {
            let _ = app.emit_to("main", "app-shortcut", action);
        }
    }
}

#[tauri::command]
pub fn get_app_shortcuts(app: AppHandle) -> Config {
    current(&app)
}

#[tauri::command]
pub fn set_app_shortcuts(app: AppHandle, state: State<'_, Shortcuts>, config: Config) -> Result<(), String> {
    let mut seen = Vec::new();
    for b in config.values().filter(|b| b.key != 0) {
        if seen.contains(b) {
            return Err("duplicate".into());
        }
        seen.push(*b);
    }
    let mut merged = current(&app);
    merged.extend(config);
    if let Some(p) = path(&app) {
        fs::write(p, serde_json::to_vec_pretty(&merged).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    *state.config.lock().unwrap() = Some(merged.clone());
    let _ = app.emit("app-shortcuts-changed", merged);
    Ok(())
}

#[tauri::command]
pub fn set_shortcut_capture(state: State<'_, Shortcuts>, active: bool) {
    state.capturing.store(active, Ordering::Relaxed);
}
