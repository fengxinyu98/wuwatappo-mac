//! Gamepad button presses count as taps (added in 1.0.2; Windows polls XInput).
//! Emits `gamepad-button-press` (no payload) to the main window on every button press.

use std::time::Duration;

use gilrs::{EventType, Gilrs};
use tauri::{AppHandle, Emitter};

use crate::applog;

pub fn start(app: AppHandle) {
    let _ = std::thread::Builder::new().name("gamepad".into()).spawn(move || {
        let mut gilrs = match Gilrs::new() {
            Ok(g) => g,
            Err(e) => {
                applog::write(&app, "warn", &format!("[INPUT] gamepad unavailable: {e}"));
                return;
            }
        };
        applog::write(&app, "info", "[INPUT] gamepad listener started");
        loop {
            while let Some(ev) = gilrs.next_event() {
                if let EventType::ButtonPressed(..) = ev.event {
                    let _ = app.emit_to("main", "gamepad-button-press", ());
                }
            }
            std::thread::sleep(Duration::from_millis(8));
        }
    });
}
