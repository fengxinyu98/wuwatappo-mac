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

struct Ctx {
    app: AppHandle,
    tap: Ref,
    last_move: Instant,
}

pub fn start(app: AppHandle) {
    let mouse_mask = mask(&[
        LEFT_DOWN, LEFT_UP, RIGHT_DOWN, RIGHT_UP, MOUSE_MOVED, LEFT_DRAGGED, RIGHT_DRAGGED,
        OTHER_DOWN, OTHER_UP, OTHER_DRAGGED,
    ]);
    let key_mask = mask(&[KEY_DOWN, KEY_UP, FLAGS_CHANGED]);
    spawn_tap(app.clone(), "mouse", mouse_mask);
    spawn_tap(app, "keyboard", key_mask);
}

fn mask(types: &[u32]) -> u64 {
    types.iter().fold(0, |m, t| m | (1u64 << t))
}

fn spawn_tap(app: AppHandle, kind: &'static str, mask: u64) {
    let _ = std::thread::Builder::new()
        .name(format!("global-{kind}-tap"))
        .spawn(move || unsafe { run_tap(app, kind, mask) });
}

unsafe fn run_tap(app: AppHandle, kind: &str, mask: u64) {
    if kind == "keyboard" && !CGPreflightListenEventAccess() {
        CGRequestListenEventAccess();
    }
    let ctx = Box::into_raw(Box::new(Ctx {
        app: app.clone(),
        tap: std::ptr::null_mut(),
        last_move: Instant::now(),
    }));
    let mut warned = false;
    loop {
        let tap = CGEventTapCreate(SESSION_TAP, HEAD_INSERT, LISTEN_ONLY, mask, callback, ctx.cast());
        if !tap.is_null() {
            (*ctx).tap = tap;
            let source = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
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

unsafe extern "C" fn callback(_proxy: Ref, etype: u32, event: Ref, user: *mut c_void) -> Ref {
    let ctx = &mut *(user as *mut Ctx);
    if etype == TAP_DISABLED_BY_TIMEOUT || etype == TAP_DISABLED_BY_USER_INPUT {
        if !ctx.tap.is_null() {
            CGEventTapEnable(ctx.tap, true);
        }
        return event;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| handle(ctx, etype, event)));
    event
}

unsafe fn handle(ctx: &mut Ctx, etype: u32, event: Ref) {
    match etype {
        MOUSE_MOVED | LEFT_DRAGGED | RIGHT_DRAGGED | OTHER_DRAGGED => {
            let p = CGEventGetLocation(event);
            hit::update_capture(&ctx.app, Some((p.x, p.y)));
            let now = Instant::now();
            if now.duration_since(ctx.last_move) < MOVE_EMIT_INTERVAL {
                return;
            }
            ctx.last_move = now;
            let s = hit::scale(&ctx.app);
            let _ = ctx.app.emit_to("main", "mouse-move", json!({ "x": p.x * s, "y": p.y * s }));
        }
        LEFT_DOWN | LEFT_UP | RIGHT_DOWN | RIGHT_UP | OTHER_DOWN | OTHER_UP => {
            let p = CGEventGetLocation(event);
            hit::update_capture(&ctx.app, Some((p.x, p.y)));
            if hit::over_own_window(&ctx.app, p.x, p.y) {
                return;
            }
            let button = match etype {
                LEFT_DOWN | LEFT_UP => "left",
                RIGHT_DOWN | RIGHT_UP => "right",
                _ if CGEventGetIntegerValueField(event, FIELD_MOUSE_BUTTON_NUMBER) == 2 => "middle",
                _ => "unknown",
            };
            let action = match etype {
                LEFT_DOWN | RIGHT_DOWN | OTHER_DOWN => "press",
                _ => "release",
            };
            let s = hit::scale(&ctx.app);
            let _ = ctx.app.emit_to(
                "main",
                "mouse-button",
                json!({ "button": button, "action": action, "x": p.x * s, "y": p.y * s }),
            );
        }
        KEY_DOWN | KEY_UP => {
            if etype == KEY_DOWN && CGEventGetIntegerValueField(event, FIELD_KEY_AUTOREPEAT) != 0 {
                return;
            }
            let name = key_name(CGEventGetIntegerValueField(event, FIELD_KEY_CODE));
            let ev = if etype == KEY_DOWN { "key-press" } else { "key-release" };
            let _ = ctx.app.emit_to("main", ev, name);
        }
        FLAGS_CHANGED => {
            let code = CGEventGetIntegerValueField(event, FIELD_KEY_CODE);
            let Some(bit) = modifier_bit(code) else { return };
            let name = key_name(code);
            if code == 57 {
                // Caps Lock reports lock state, not key state: treat each change as a tap.
                let _ = ctx.app.emit_to("main", "key-press", name.clone());
                let _ = ctx.app.emit_to("main", "key-release", name);
                return;
            }
            let pressed = CGEventGetFlags(event) & bit != 0;
            let ev = if pressed { "key-press" } else { "key-release" };
            let _ = ctx.app.emit_to("main", ev, name);
        }
        _ => {}
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
