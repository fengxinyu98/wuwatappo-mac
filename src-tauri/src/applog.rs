//! Plain-text app log in the platform log dir (~/Library/Logs/<identifier>/).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};

static LOCK: Mutex<()> = Mutex::new(());

pub fn write(app: &AppHandle, level: &str, message: &str) {
    let Ok(dir) = app.path().app_log_dir() else { return };
    let _guard = LOCK.lock().unwrap();
    let _ = fs::create_dir_all(&dir);
    let Ok(mut f) = OpenOptions::new().create(true).append(true).open(dir.join("wuwatappo.log")) else {
        return;
    };
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let _ = writeln!(f, "{ts} [{}] {message}", level.to_uppercase());
}
