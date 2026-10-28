//! macOS backend for the Wuwa Tappo frontend extracted from the Windows build.
//!
//! The Vue frontend is used unchanged (except for one URL patch); this crate
//! re-implements the custom commands and native events that the Windows Rust
//! backend (`wuwatappo_lib`) provided.

mod applog;
mod gamepad;
mod hit;
#[cfg(target_os = "macos")]
mod input_macos;
mod menu;
mod store;
mod windows;
mod ws_proxy;

use std::sync::Mutex;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, CursorIcon, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize, RunEvent, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use hit::{Hit, HitState, Rect};
use menu::{MenuLabels, MenuState};

const TRAY_ID: &str = "main-tray";

#[derive(Default)]
struct Shared {
    update_detail: Mutex<Option<Value>>,
    maintenance: Mutex<Option<Value>>,
    menu: Mutex<MenuState>,
}

type CmdResult<T = ()> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------------------------------------------------------- main overlay input

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[derive(Default)]
struct HitRegionPayload {
    regions: Vec<Rect>,
    input_regions: Vec<Rect>,
    update_native_input: bool,
    window_passthrough: bool,
}

#[tauri::command]
fn set_main_hit_region(app: AppHandle, payload: HitRegionPayload) {
    {
        let hit = app.state::<Hit>();
        let mut s = hit.0.lock().unwrap();
        s.regions = payload.regions;
        if payload.update_native_input {
            s.input_regions = payload.input_regions;
        }
        s.window_passthrough = payload.window_passthrough;
    }
    hit::update_capture(&app, None);
}

#[tauri::command]
fn set_main_swallow_mouse(app: AppHandle, enabled: bool) {
    app.state::<Hit>().0.lock().unwrap().swallow = enabled;
    hit::update_capture(&app, None);
}

/// Windows makes the WebView itself input-transparent; on macOS `hit` handles this.
#[tauri::command]
fn set_main_force_cursor_ignore(_enabled: bool) {}

fn cursor_icon(kind: &str) -> CursorIcon {
    match kind {
        "pointer" | "hand" => CursorIcon::Hand,
        "grab" => CursorIcon::Grab,
        "grabbing" => CursorIcon::Grabbing,
        "move" => CursorIcon::Move,
        "text" => CursorIcon::Text,
        "not-allowed" => CursorIcon::NotAllowed,
        "ew-resize" | "col-resize" => CursorIcon::EwResize,
        "ns-resize" | "row-resize" => CursorIcon::NsResize,
        "nwse-resize" => CursorIcon::NwseResize,
        "nesw-resize" => CursorIcon::NeswResize,
        _ => CursorIcon::Default,
    }
}

#[tauri::command]
fn set_overlay_cursor(app: AppHandle, kind: String) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_cursor_icon(cursor_icon(&kind));
    }
}

#[tauri::command]
fn set_native_input_cursor(app: AppHandle, kind: String) {
    set_overlay_cursor(app, kind);
}

// ---------------------------------------------------------------- main overlay geometry

#[tauri::command]
async fn set_main_window_fullscreen_shell(app: AppHandle) -> CmdResult {
    let main = app.get_webview_window("main").ok_or("main window missing")?;
    windows::fit_to_monitor(&main);
    hit::refresh_geometry(&app);
    Ok(())
}

#[tauri::command]
async fn sync_main_window_to_current_monitor(app: AppHandle) -> CmdResult {
    set_main_window_fullscreen_shell(app).await
}

#[derive(Deserialize)]
struct Bounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[tauri::command]
async fn set_main_window_monitor_bounds(app: AppHandle, payload: Bounds) -> CmdResult {
    let main = app.get_webview_window("main").ok_or("main window missing")?;
    windows::apply_bounds(
        &main,
        PhysicalPosition::new(payload.x, payload.y),
        PhysicalSize::new(payload.width, payload.height),
    );
    hit::refresh_geometry(&app);
    Ok(())
}

#[tauri::command]
fn finish_main_window_monitor_switch(app: AppHandle) {
    hit::refresh_geometry(&app);
    hit::update_capture(&app, None);
}

#[tauri::command]
async fn sync_aux_windows_to_main_monitor(app: AppHandle) -> CmdResult {
    if let (Some(toast), Some(m)) = (app.get_webview_window("toast"), windows::main_monitor(&app)) {
        windows::apply_bounds(&toast, *m.position(), *m.size());
    }
    if let Some(panel) = app.get_webview_window("panel") {
        windows::center_on_main_monitor(&app, &panel);
    }
    Ok(())
}

/// Only used by the non-fullscreen layout; the fullscreen shell keeps the overlay as is.
#[tauri::command]
fn set_main_window_content_bounds(_payload: Value) {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MainPositionPayload {
    always_on_top: bool,
}

#[tauri::command]
fn apply_main_window_position(app: AppHandle, payload: MainPositionPayload) -> CmdResult {
    let main = app.get_webview_window("main").ok_or("main window missing")?;
    main.set_always_on_top(payload.always_on_top).map_err(err)
}

#[tauri::command]
fn get_main_desktop_safe_area(app: AppHandle) -> Value {
    json!({
        "insets": windows::safe_insets(&app),
        "autoHideEdge": null,
        "foregroundFullscreen": false,
    })
}

#[tauri::command]
fn get_main_taskbar_insets(app: AppHandle) -> Value {
    windows::safe_insets(&app)
}

/// "Show taskbar icon" maps to showing the Dock icon.
#[tauri::command]
fn set_main_skip_taskbar(app: AppHandle, skip: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if skip {
            tauri::ActivationPolicy::Accessory
        } else {
            tauri::ActivationPolicy::Regular
        };
        let _ = app.set_activation_policy(policy);
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_skip_taskbar(skip);
    }
}

// ---------------------------------------------------------------- menus

#[tauri::command]
fn sync_native_menu(
    app: AppHandle,
    labels: MenuLabels,
    pet_dock_visible: bool,
    pet_scale: u32,
    window_opacity: u32,
    window_passthrough: bool,
) -> CmdResult {
    let st = {
        let shared = app.state::<Shared>();
        let mut st = shared.menu.lock().unwrap();
        *st = MenuState { labels, pet_dock_visible, pet_scale, window_opacity, window_passthrough };
        st.clone()
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let m = menu::build(&app, &st, false).map_err(err)?;
        tray.set_menu(Some(m)).map_err(err)?;
        let _ = tray.set_tooltip(Some(menu::tooltip(&st)));
    }
    Ok(())
}

#[tauri::command]
async fn popup_main_dock_context_menu(app: AppHandle, x: f64, y: f64) -> CmdResult {
    let st = app.state::<Shared>().menu.lock().unwrap().clone();
    let main = app.get_webview_window("main").ok_or("main window missing")?;
    let m = menu::build(&app, &st, true).map_err(err)?;
    main.popup_menu_at(&m, LogicalPosition::new(x, y)).map_err(err)
}

// ---------------------------------------------------------------- panel & modal windows

#[derive(Deserialize, Default)]
#[serde(default)]
struct PanelPayload {
    tab: Option<String>,
}

#[tauri::command]
async fn open_panel_window(app: AppHandle, payload: Option<PanelPayload>) -> CmdResult {
    windows::open_panel(&app, payload.unwrap_or_default().tab)
}

/// Hide rather than destroy: the panel closes itself and then keeps running code
/// (e.g. Settings → Login closes the panel, then opens the login window from it).
#[tauri::command]
async fn close_panel_window(app: AppHandle) {
    windows::hide(&app, "panel");
}

#[tauri::command]
fn raise_panel_window_above_pets(app: AppHandle) {
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.set_always_on_top(true);
        let _ = panel.set_focus();
    }
}

#[derive(Deserialize)]
struct CollapsedPayload {
    collapsed: bool,
}

#[tauri::command]
fn set_panel_window_collapsed(app: AppHandle, payload: CollapsedPayload) -> CmdResult {
    let Some(panel) = app.get_webview_window("panel") else { return Ok(()) };
    let nav = if payload.collapsed { windows::PANEL_NAV_COLLAPSED_W } else { windows::PANEL_NAV_EXPANDED_W };
    let scale = panel.scale_factor().map_err(err)?;
    let height = panel.inner_size().map_err(err)?.to_logical::<f64>(scale).height;
    panel.set_size(LogicalSize::new(nav + windows::PANEL_CONTENT_W, height)).map_err(err)
}

#[tauri::command]
async fn open_login_window(app: AppHandle, intent: Option<String>, compact: Option<bool>) -> CmdResult {
    let _ = compact;
    let intent = intent.unwrap_or_else(|| "login".into());
    windows::show_aux(&app, "login", Some(("login-modal-open", json!({ "intent": intent }))))
}

#[tauri::command]
async fn close_login_window(app: AppHandle) {
    windows::hide(&app, "login");
}

#[derive(Deserialize)]
struct SizePayload {
    width: f64,
    height: f64,
}

#[tauri::command]
fn resize_login_window_to_content(app: AppHandle, payload: SizePayload) -> CmdResult {
    let login = app.get_webview_window("login").ok_or("login window missing")?;
    login.set_size(LogicalSize::new(payload.width, payload.height)).map_err(err)?;
    login.center().map_err(err)
}

#[tauri::command]
async fn open_update_window(app: AppHandle, detail: Value) -> CmdResult {
    *app.state::<Shared>().update_detail.lock().unwrap() = Some(detail);
    windows::show_aux(&app, "update", Some(("update-modal-open", Value::Null)))
}

#[tauri::command]
async fn close_update_window(app: AppHandle) {
    windows::hide(&app, "update");
}

#[tauri::command]
fn get_app_update_detail(app: AppHandle) -> Option<Value> {
    app.state::<Shared>().update_detail.lock().unwrap().clone()
}

#[tauri::command]
async fn open_about_window(app: AppHandle) -> CmdResult {
    windows::show_aux(&app, "about", Some(("about-open", Value::Null)))
}

#[tauri::command]
async fn close_about_window(app: AppHandle) {
    windows::hide(&app, "about");
}

#[tauri::command]
async fn open_oss_notice_window(app: AppHandle) -> CmdResult {
    windows::show_aux(&app, "oss-notice", Some(("oss-notice-open", Value::Null)))
}

#[tauri::command]
async fn close_oss_notice_window(app: AppHandle) {
    windows::hide(&app, "oss-notice");
}

#[tauri::command]
async fn show_toast_overlay(app: AppHandle) -> CmdResult {
    let toast = windows::ensure_toast(&app).map_err(err)?;
    toast.show().map_err(err)
}

// ---------------------------------------------------------------- maintenance & updates

#[tauri::command]
async fn open_maintenance_notice(app: AppHandle, payload: Option<Value>) -> CmdResult {
    let payload = payload.unwrap_or(Value::Null);
    *app.state::<Shared>().maintenance.lock().unwrap() = Some(payload.clone());
    windows::show_aux(&app, "maintenance", Some(("maintenance-notice-open", payload)))
}

#[tauri::command]
fn has_maintenance_notice(app: AppHandle) -> bool {
    app.state::<Shared>().maintenance.lock().unwrap().is_some()
}

#[tauri::command]
fn get_maintenance_notice_payload(app: AppHandle) -> Value {
    app.state::<Shared>().maintenance.lock().unwrap().clone().unwrap_or(Value::Null)
}

#[tauri::command]
fn hide_main_after_maintenance_notice_ready(app: AppHandle) {
    windows::hide(&app, "main");
}

#[tauri::command]
fn exit_maintenance_app(app: AppHandle) {
    app.exit(0);
}

/// The official updater only ships Windows packages, so never report an update.
#[tauri::command]
fn check_app_update(_endpoint: Option<String>) -> Option<Value> {
    None
}

#[tauri::command]
fn is_force_update_locked() -> bool {
    false
}

// ---------------------------------------------------------------- persistence & misc

#[tauri::command]
fn load_key_press_count(app: AppHandle, environment: String) -> CmdResult<f64> {
    store::load_key_press(&app, &environment)
}

#[tauri::command]
fn save_key_press_count(app: AppHandle, count: f64, environment: String) -> CmdResult {
    store::save_key_press(&app, &environment, count)
}

#[tauri::command]
fn load_cumulative_click_count(app: AppHandle, environment: String) -> CmdResult<store::Cumulative> {
    store::load_cumulative(&app, &environment)
}

#[tauri::command]
fn save_cumulative_click_count(app: AppHandle, count: f64, cuid: String, environment: String) -> CmdResult {
    store::save_cumulative(&app, &environment, count, &cuid)
}

#[tauri::command]
fn write_app_log(app: AppHandle, level: String, message: String) {
    applog::write(&app, &level, &message);
}

#[tauri::command]
fn get_track_os_info() -> Value {
    let version = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    json!({ "os": "macOS", "osVersion": version })
}

#[tauri::command]
fn get_boot_diagnostics() -> Value {
    json!({ "platform": "macos", "shell": "wuwatappo-mac" })
}

#[tauri::command]
fn is_internal_testing_build() -> bool {
    false
}

#[tauri::command]
fn is_devtools_hotkeys_build() -> bool {
    false
}

/// The Windows build uses a scheduled task for elevated autostart; a LaunchAgent is enough here.
#[tauri::command]
fn is_elevated_autostart_enabled(app: AppHandle) -> CmdResult<bool> {
    app.autolaunch().is_enabled().map_err(err)
}

#[tauri::command]
fn set_elevated_autostart(app: AppHandle, enabled: bool) -> CmdResult {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(err)
}

#[tauri::command]
fn exit_app(app: AppHandle) {
    app.exit(0);
}

// ---------------------------------------------------------------- app

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let st = app.state::<Shared>().menu.lock().unwrap().clone();
    let m = menu::build(app, &st, false)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&m)
        .tooltip(menu::tooltip(&st))
        .show_menu_on_left_click(true);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            applog::write(app, "info", "[APP] second launch received");
        }))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .manage(Hit(Mutex::new(HitState::default())))
        .manage(Shared::default())
        .manage(ws_proxy::WsState::default())
        .on_menu_event(|app, event| menu::handle_event(app, event))
        .on_window_event(|window, event| {
            let app = window.app_handle();
            match (window.label(), event) {
                ("main", WindowEvent::Moved(_) | WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }) => {
                    hit::refresh_geometry(app);
                }
                ("panel", WindowEvent::CloseRequested { api, .. }) => {
                    api.prevent_close();
                    let _ = window.hide();
                    let _ = app.emit_to("main", "cat-panel-window-closed", Value::Null);
                }
                // Modal windows are reused: hide instead of destroying on Cmd+W.
                ("login" | "update" | "about" | "oss-notice", WindowEvent::CloseRequested { api, .. }) => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                _ => {}
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            windows::create_main(&handle)?;
            windows::prewarm(&handle);
            setup_tray(&handle)?;
            #[cfg(target_os = "macos")]
            input_macos::start(handle.clone());
            gamepad::start(handle.clone());
            applog::write(&handle, "info", "[APP] started (macOS shell)");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_main_hit_region,
            set_main_swallow_mouse,
            set_main_force_cursor_ignore,
            set_overlay_cursor,
            set_native_input_cursor,
            set_main_window_fullscreen_shell,
            sync_main_window_to_current_monitor,
            set_main_window_monitor_bounds,
            finish_main_window_monitor_switch,
            sync_aux_windows_to_main_monitor,
            set_main_window_content_bounds,
            apply_main_window_position,
            get_main_desktop_safe_area,
            get_main_taskbar_insets,
            set_main_skip_taskbar,
            sync_native_menu,
            popup_main_dock_context_menu,
            open_panel_window,
            close_panel_window,
            raise_panel_window_above_pets,
            set_panel_window_collapsed,
            open_login_window,
            close_login_window,
            resize_login_window_to_content,
            open_update_window,
            close_update_window,
            get_app_update_detail,
            open_about_window,
            close_about_window,
            open_oss_notice_window,
            close_oss_notice_window,
            show_toast_overlay,
            open_maintenance_notice,
            has_maintenance_notice,
            get_maintenance_notice_payload,
            hide_main_after_maintenance_notice_ready,
            exit_maintenance_app,
            check_app_update,
            is_force_update_locked,
            load_key_press_count,
            save_key_press_count,
            load_cumulative_click_count,
            save_cumulative_click_count,
            write_app_log,
            get_track_os_info,
            get_boot_diagnostics,
            is_internal_testing_build,
            is_devtools_hotkeys_build,
            is_elevated_autostart_enabled,
            set_elevated_autostart,
            exit_app,
            ws_proxy::ws_open,
            ws_proxy::ws_send,
            ws_proxy::ws_close,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Wuwa Tappo");

    app.run(|app, event| {
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen { .. } = event {
            // Clicking the Dock icon opens the panel, like clicking the taskbar icon on Windows.
            let _ = app.emit_to("main", "dock-open-panel", "bag");
        }
        let _ = (app, &event);
    });
}
