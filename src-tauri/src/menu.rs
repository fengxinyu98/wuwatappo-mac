//! Tray menu and the pet's right-click menu. Item clicks are forwarded to the main
//! window as the same `tray-*` / `dock-open-panel` events the Windows build emits.

use serde::Deserialize;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Wry};

pub const SCALES: [u32; 7] = [50, 75, 100, 125, 150, 175, 200];
pub const OPACITIES: [u32; 8] = [100, 90, 80, 70, 60, 50, 40, 30];

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MenuLabels {
    pub open_bag: String,
    pub open_atlas: String,
    pub open_exchange: String,
    pub open_room: String,
    pub hide_pet: String,
    pub show_pet: String,
    pub tray_open_settings: String,
    pub dock_settings: String,
    pub window_size: String,
    pub window_passthrough: String,
    pub window_opacity: String,
    pub game_mode: String,
    pub restart: String,
    pub quit: String,
    pub tray_tooltip: String,
}

impl MenuLabels {
    /// Fills empty labels with Chinese defaults (used before the frontend syncs).
    fn or_defaults(&self) -> MenuLabels {
        fn pick(v: &str, d: &str) -> String {
            if v.trim().is_empty() { d.to_string() } else { v.to_string() }
        }
        MenuLabels {
            open_bag: pick(&self.open_bag, "背包"),
            open_atlas: pick(&self.open_atlas, "图鉴"),
            open_exchange: pick(&self.open_exchange, "合成"),
            open_room: pick(&self.open_room, "房间"),
            hide_pet: pick(&self.hide_pet, "隐藏桌宠"),
            show_pet: pick(&self.show_pet, "显示桌宠"),
            tray_open_settings: pick(&self.tray_open_settings, "设置"),
            dock_settings: pick(&self.dock_settings, "设置"),
            window_size: pick(&self.window_size, "桌宠大小"),
            window_passthrough: pick(&self.window_passthrough, "鼠标穿透"),
            window_opacity: pick(&self.window_opacity, "透明度"),
            game_mode: pick(&self.game_mode, "游戏模式"),
            restart: pick(&self.restart, "重启"),
            quit: pick(&self.quit, "退出"),
            tray_tooltip: pick(&self.tray_tooltip, "Wuwa Tappo"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MenuState {
    pub labels: MenuLabels,
    pub pet_dock_visible: bool,
    pub pet_scale: u32,
    pub window_opacity: u32,
    pub window_passthrough: bool,
    pub game_mode: bool,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            labels: MenuLabels::default(),
            pet_dock_visible: true,
            pet_scale: 100,
            window_opacity: 100,
            window_passthrough: false,
            game_mode: false,
        }
    }
}

pub fn tooltip(st: &MenuState) -> String {
    st.labels.or_defaults().tray_tooltip
}

/// `for_dock` selects the pet context-menu variant (uses the dock settings label).
pub fn build(app: &AppHandle, st: &MenuState, for_dock: bool) -> tauri::Result<Menu<Wry>> {
    let l = st.labels.or_defaults();

    let scale_items = SCALES
        .iter()
        .map(|v| {
            CheckMenuItem::with_id(app, format!("pet_scale_{v}"), format!("{v}%"), true, *v == st.pet_scale, None::<&str>)
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let opacity_items = OPACITIES
        .iter()
        .map(|v| {
            CheckMenuItem::with_id(app, format!("opacity_{v}"), format!("{v}%"), true, *v == st.window_opacity, None::<&str>)
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let scale_refs: Vec<&dyn IsMenuItem<Wry>> = scale_items.iter().map(|i| i as &dyn IsMenuItem<Wry>).collect();
    let opacity_refs: Vec<&dyn IsMenuItem<Wry>> = opacity_items.iter().map(|i| i as &dyn IsMenuItem<Wry>).collect();

    let open_bag = MenuItem::with_id(app, "open_bag", &l.open_bag, true, None::<&str>)?;
    let open_atlas = MenuItem::with_id(app, "open_atlas", &l.open_atlas, true, None::<&str>)?;
    let open_exchange = MenuItem::with_id(app, "open_exchange", &l.open_exchange, true, None::<&str>)?;
    let open_room = MenuItem::with_id(app, "open_room", &l.open_room, true, None::<&str>)?;
    let settings_label = if for_dock { &l.dock_settings } else { &l.tray_open_settings };
    let open_settings = MenuItem::with_id(app, "open_settings", settings_label, true, None::<&str>)?;
    let toggle_label = if st.pet_dock_visible { &l.hide_pet } else { &l.show_pet };
    let toggle_pet = MenuItem::with_id(app, "toggle_pet", toggle_label, true, None::<&str>)?;
    let size = Submenu::with_id_and_items(app, "window_size", &l.window_size, true, &scale_refs)?;
    let opacity = Submenu::with_id_and_items(app, "window_opacity", &l.window_opacity, true, &opacity_refs)?;
    let passthrough = CheckMenuItem::with_id(
        app,
        "window_passthrough",
        &l.window_passthrough,
        true,
        st.window_passthrough,
        None::<&str>,
    )?;
    let game_mode = CheckMenuItem::with_id(app, "game_mode", &l.game_mode, true, st.game_mode, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", &l.restart, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", &l.quit, true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;

    Menu::with_items(
        app,
        &[
            &open_bag, &open_atlas, &open_exchange, &open_room, &open_settings, &sep1,
            &toggle_pet, &size, &opacity, &passthrough, &game_mode, &sep2, &restart, &sep3, &quit,
        ],
    )
}

pub fn handle_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    let emit = |name: &str, payload: serde_json::Value| {
        let _ = app.emit_to("main", name, payload);
    };
    match id {
        "open_bag" => emit("dock-open-panel", "bag".into()),
        "open_atlas" => emit("dock-open-panel", "atlas".into()),
        "open_exchange" => emit("dock-open-panel", "exchange".into()),
        "open_room" => emit("dock-open-panel", "room".into()),
        "open_settings" => emit("dock-open-panel", "setting".into()),
        "toggle_pet" => emit("tray-toggle-pet-visibility", serde_json::Value::Null),
        "window_passthrough" => emit("tray-toggle-window-passthrough", serde_json::Value::Null),
        "game_mode" => emit("tray-toggle-game-mode", serde_json::Value::Null),
        "restart" => emit("tray-restart", serde_json::Value::Null),
        "quit" => emit("tray-quit", serde_json::Value::Null),
        _ => {
            if let Some(v) = id.strip_prefix("pet_scale_").and_then(|v| v.parse::<u32>().ok()) {
                emit("tray-set-pet-scale", v.into());
            } else if let Some(v) = id.strip_prefix("opacity_").and_then(|v| v.parse::<u32>().ok()) {
                emit("tray-set-window-opacity", v.into());
            }
        }
    }
}
