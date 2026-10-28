//! Local persistence for the tap counters (`kuro-mc-keypress-store-v1` /
//! `kuro-mc-cumulative-click-v1` on Windows), keyed by API environment.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Cumulative {
    pub count: f64,
    pub cuid: String,
    pub environment: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Counters {
    key_press: HashMap<String, f64>,
    cumulative: HashMap<String, Cumulative>,
}

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("counters.json"))
}

fn load(app: &AppHandle) -> Result<Counters, String> {
    let p = path(app)?;
    match fs::read_to_string(&p) {
        Ok(s) => Ok(serde_json::from_str(&s).unwrap_or_default()),
        Err(_) => Ok(Counters::default()),
    }
}

fn save(app: &AppHandle, c: &Counters) -> Result<(), String> {
    let p = path(app)?;
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

pub fn load_key_press(app: &AppHandle, env: &str) -> Result<f64, String> {
    let _g = LOCK.lock().unwrap();
    Ok(load(app)?.key_press.get(env).copied().unwrap_or(0.0))
}

pub fn save_key_press(app: &AppHandle, env: &str, count: f64) -> Result<(), String> {
    let _g = LOCK.lock().unwrap();
    let mut c = load(app)?;
    c.key_press.insert(env.to_string(), count);
    save(app, &c)
}

pub fn load_cumulative(app: &AppHandle, env: &str) -> Result<Cumulative, String> {
    let _g = LOCK.lock().unwrap();
    Ok(load(app)?.cumulative.get(env).cloned().unwrap_or_default())
}

pub fn save_cumulative(app: &AppHandle, env: &str, count: f64, cuid: &str) -> Result<(), String> {
    let _g = LOCK.lock().unwrap();
    let mut c = load(app)?;
    c.cumulative.insert(
        env.to_string(),
        Cumulative { count, cuid: cuid.to_string(), environment: env.to_string() },
    );
    save(app, &c)
}
