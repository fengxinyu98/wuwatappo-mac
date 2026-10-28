//! Global keyboard/mouse listener for macOS (replacement for rdev + SetWindowsHookEx).
//!
//! Uses listen-only CGEventTaps on dedicated threads. Mouse events need no permission;
//! keyboard events need "Input Monitoring", so the keyboard tap retries until granted.
//! Events are emitted to the main window with the same names/payloads the frontend uses
//! on Windows: `mouse-move` {x, y}, `mouse-button` {button, action, x, y},
//! `key-press` / `key-release` (KeyboardEvent.key-like string). Coordinates are physical
//! pixels, matching `innerPosition()` / `scaleFactor()` math in the frontend.

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::{applog, hit};

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

type Ref = *mut c_void;
type TapCallback = unsafe extern "C" fn(Ref, u32, Ref, *mut c_void) -> Ref;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: TapCallback,
        user_info: *mut c_void,
    ) -> Ref;
    fn CGEventTapEnable(tap: Ref, enable: bool);
    fn CGEventTapIsEnabled(tap: Ref) -> bool;
    fn CGEventGetLocation(event: Ref) -> CGPoint;
    fn CGEventGetIntegerValueField(event: Ref, field: u32) -> i64;
    fn CGEventGetFlags(event: Ref) -> u64;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFMachPortCreateRunLoopSource(alloc: Ref, port: Ref, order: isize) -> Ref;
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFRunLoopAddSource(rl: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRun();
    static kCFRunLoopCommonModes: Ref;
}

const SESSION_TAP: u32 = 1;
const HEAD_INSERT: u32 = 0;
const LISTEN_ONLY: u32 = 1;

const LEFT_DOWN: u32 = 1;
const LEFT_UP: u32 = 2;
const RIGHT_DOWN: u32 = 3;
const RIGHT_UP: u32 = 4;
const MOUSE_MOVED: u32 = 5;
const LEFT_DRAGGED: u32 = 6;
const RIGHT_DRAGGED: u32 = 7;
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const OTHER_DOWN: u32 = 25;
const OTHER_UP: u32 = 26;
const OTHER_DRAGGED: u32 = 27;
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

const FIELD_MOUSE_BUTTON_NUMBER: u32 = 3;
const FIELD_KEY_AUTOREPEAT: u32 = 8;
const FIELD_KEY_CODE: u32 = 9;

const MOVE_EMIT_INTERVAL: Duration = Duration::from_millis(8);
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(2);

/// Event data copied out of the CGEvent inside the tap callback. The callback must
/// return quickly or macOS disables the tap, so all work (hit-testing, window queries
/// that wait on the main thread, emitting) happens on a separate worker thread.
enum Raw {
    Move { x: f64, y: f64 },
    Button { button: &'static str, press: bool, x: f64, y: f64 },
    Key { name: String, press: bool },
    /// Press immediately followed by release (Caps Lock only reports its lock state).
    KeyTap { name: String },
}

struct Ctx {
    tx: Sender<Raw>,
    tap: Arc<AtomicPtr<c_void>>,
}

pub fn start(app: AppHandle) {
    let (tx, rx) = channel::<Raw>();
    let worker_app = app.clone();
    let _ = std::thread::Builder::new()
        .name("global-input-worker".into())
        .spawn(move || worker(worker_app, rx));

    let mouse_mask = mask(&[
        LEFT_DOWN, LEFT_UP, RIGHT_DOWN, RIGHT_UP, MOUSE_MOVED, LEFT_DRAGGED, RIGHT_DRAGGED,
        OTHER_DOWN, OTHER_UP, OTHER_DRAGGED,
    ]);
    let key_mask = mask(&[KEY_DOWN, KEY_UP, FLAGS_CHANGED]);
    spawn_tap(app.clone(), tx.clone(), "mouse", mouse_mask);
    spawn_tap(app, tx, "keyboard", key_mask);
}

fn mask(types: &[u32]) -> u64 {
    types.iter().fold(0, |m, t| m | (1u64 << t))
}

fn spawn_tap(app: AppHandle, tx: Sender<Raw>, kind: &'static str, mask: u64) {
    let _ = std::thread::Builder::new()
        .name(format!("global-{kind}-tap"))
        .spawn(move || unsafe { run_tap(app, tx, kind, mask) });
}

unsafe fn run_tap(app: AppHandle, tx: Sender<Raw>, kind: &'static str, mask: u64) {
    if kind == "keyboard" && !CGPreflightListenEventAccess() {
        CGRequestListenEventAccess();
    }
    let tap_slot = Arc::new(AtomicPtr::new(std::ptr::null_mut()));
    let ctx = Box::into_raw(Box::new(Ctx { tx, tap: tap_slot.clone() }));
    let mut warned = false;
    loop {
        let tap = CGEventTapCreate(SESSION_TAP, HEAD_INSERT, LISTEN_ONLY, mask, callback, ctx.cast());
        if !tap.is_null() {
            tap_slot.store(tap, Ordering::Release);
            let source = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            spawn_watchdog(app.clone(), kind, tap_slot.clone());
            applog::write(&app, "info", &format!("[INPUT] {kind} tap started"));
            CFRunLoopRun();
            return;
        }
        if !warned {
            warned = true;
            applog::write(
                &app,
                "warn",
                &format!("[INPUT] {kind} tap unavailable, waiting for Input Monitoring permission"),
            );
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}

/// Re-enables the tap if macOS disabled it and the in-callback re-enable was missed.
fn spawn_watchdog(app: AppHandle, kind: &'static str, tap: Arc<AtomicPtr<c_void>>) {
    let _ = std::thread::Builder::new()
        .name(format!("global-{kind}-watchdog"))
        .spawn(move || loop {
            std::thread::sleep(WATCHDOG_INTERVAL);
            let t = tap.load(Ordering::Acquire);
            unsafe {
                if !t.is_null() && !CGEventTapIsEnabled(t) {
                    CGEventTapEnable(t, true);
                    applog::write(&app, "warn", &format!("[INPUT] {kind} tap was disabled, re-enabled"));
                }
            }
        });
}

unsafe extern "C" fn callback(_proxy: Ref, etype: u32, event: Ref, user: *mut c_void) -> Ref {
    let ctx = &*(user as *const Ctx);
    if etype == TAP_DISABLED_BY_TIMEOUT || etype == TAP_DISABLED_BY_USER_INPUT {
        let t = ctx.tap.load(Ordering::Acquire);
        if !t.is_null() {
            CGEventTapEnable(t, true);
        }
        return event;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(raw) = read_event(etype, event) {
            let _ = ctx.tx.send(raw);
        }
    }));
    event
}

unsafe fn read_event(etype: u32, event: Ref) -> Option<Raw> {
    match etype {
        MOUSE_MOVED | LEFT_DRAGGED | RIGHT_DRAGGED | OTHER_DRAGGED => {
            let p = CGEventGetLocation(event);
            Some(Raw::Move { x: p.x, y: p.y })
        }
        LEFT_DOWN | LEFT_UP | RIGHT_DOWN | RIGHT_UP | OTHER_DOWN | OTHER_UP => {
            let p = CGEventGetLocation(event);
            let button = match etype {
                LEFT_DOWN | LEFT_UP => "left",
                RIGHT_DOWN | RIGHT_UP => "right",
                _ if CGEventGetIntegerValueField(event, FIELD_MOUSE_BUTTON_NUMBER) == 2 => "middle",
                _ => "unknown",
            };
            let press = matches!(etype, LEFT_DOWN | RIGHT_DOWN | OTHER_DOWN);
            Some(Raw::Button { button, press, x: p.x, y: p.y })
        }
        KEY_DOWN | KEY_UP => {
            if etype == KEY_DOWN && CGEventGetIntegerValueField(event, FIELD_KEY_AUTOREPEAT) != 0 {
                return None;
            }
            let name = key_name(CGEventGetIntegerValueField(event, FIELD_KEY_CODE));
            Some(Raw::Key { name, press: etype == KEY_DOWN })
        }
        FLAGS_CHANGED => {
            let code = CGEventGetIntegerValueField(event, FIELD_KEY_CODE);
            let bit = modifier_bit(code)?;
            let name = key_name(code);
            if code == 57 {
                return Some(Raw::KeyTap { name });
            }
            Some(Raw::Key { name, press: CGEventGetFlags(event) & bit != 0 })
        }
        _ => None,
    }
}

fn worker(app: AppHandle, rx: Receiver<Raw>) {
    let mut last_move = Instant::now() - MOVE_EMIT_INTERVAL;
    let mut pending: Vec<Raw> = Vec::new();
    while let Ok(first) = rx.recv() {
        pending.push(first);
        pending.extend(rx.try_iter());
        // Coalesce runs of moves: only the last position of each run matters.
        let mut iter = std::mem::take(&mut pending).into_iter().peekable();
        while let Some(ev) = iter.next() {
            let next_is_move = matches!(iter.peek(), Some(Raw::Move { .. }));
            match ev {
                Raw::Move { x, y } => {
                    if next_is_move {
                        continue;
                    }
                    hit::update_capture(&app, Some((x, y)));
                    let now = Instant::now();
                    if now.duration_since(last_move) < MOVE_EMIT_INTERVAL {
                        continue;
                    }
                    last_move = now;
                    let s = hit::scale(&app);
                    let _ = app.emit_to("main", "mouse-move", json!({ "x": x * s, "y": y * s }));
                }
                Raw::Button { button, press, x, y } => {
                    hit::update_capture(&app, Some((x, y)));
                    if hit::over_own_window(&app, x, y) {
                        continue;
                    }
                    let s = hit::scale(&app);
                    let action = if press { "press" } else { "release" };
                    let _ = app.emit_to(
                        "main",
                        "mouse-button",
                        json!({ "button": button, "action": action, "x": x * s, "y": y * s }),
                    );
                }
                Raw::Key { name, press } => {
                    let ev = if press { "key-press" } else { "key-release" };
                    let _ = app.emit_to("main", ev, name);
                }
                Raw::KeyTap { name } => {
                    let _ = app.emit_to("main", "key-press", name.clone());
                    let _ = app.emit_to("main", "key-release", name);
                }
            }
        }
    }
}


fn modifier_bit(code: i64) -> Option<u64> {
    Some(match code {
        56 | 60 => 0x0002_0000,  // Shift
        59 | 62 => 0x0004_0000,  // Control
        58 | 61 => 0x0008_0000,  // Option
        54 | 55 => 0x0010_0000,  // Command
        57 => 0x0001_0000,       // Caps Lock
        63 => 0x0080_0000,       // Fn
        _ => return None,
    })
}

/// macOS virtual key code -> `KeyboardEvent.key`-style name (US layout).
fn key_name(code: i64) -> String {
    let s = match code {
        0 => "a", 1 => "s", 2 => "d", 3 => "f", 4 => "h", 5 => "g", 6 => "z", 7 => "x",
        8 => "c", 9 => "v", 11 => "b", 12 => "q", 13 => "w", 14 => "e", 15 => "r",
        16 => "y", 17 => "t", 18 => "1", 19 => "2", 20 => "3", 21 => "4", 22 => "6",
        23 => "5", 24 => "=", 25 => "9", 26 => "7", 27 => "-", 28 => "8", 29 => "0",
        30 => "]", 31 => "o", 32 => "u", 33 => "[", 34 => "i", 35 => "p", 36 => "Enter",
        37 => "l", 38 => "j", 39 => "'", 40 => "k", 41 => ";", 42 => "\\", 43 => ",",
        44 => "/", 45 => "n", 46 => "m", 47 => ".", 48 => "Tab", 49 => " ", 50 => "`",
        51 => "Backspace", 53 => "Escape", 54 | 55 => "Meta", 56 | 60 => "Shift",
        57 => "CapsLock", 58 | 61 => "Alt", 59 | 62 => "Control", 63 => "Fn",
        64 => "F17", 65 => ".", 67 => "*", 69 => "+", 71 => "Clear", 75 => "/",
        76 => "Enter", 78 => "-", 79 => "F18", 80 => "F19", 81 => "=", 82 => "0",
        83 => "1", 84 => "2", 85 => "3", 86 => "4", 87 => "5", 88 => "6", 89 => "7",
        90 => "F20", 91 => "8", 92 => "9", 96 => "F5", 97 => "F6", 98 => "F7", 99 => "F3",
        100 => "F8", 101 => "F9", 103 => "F11", 105 => "F13", 106 => "F16", 107 => "F14",
        109 => "F10", 111 => "F12", 113 => "F15", 114 => "Insert", 115 => "Home",
        116 => "PageUp", 117 => "Delete", 118 => "F4", 119 => "End", 120 => "F2",
        121 => "PageDown", 122 => "F1", 123 => "ArrowLeft", 124 => "ArrowRight",
        125 => "ArrowDown", 126 => "ArrowUp",
        _ => return format!("Key{code}"),
    };
    s.to_string()
}
