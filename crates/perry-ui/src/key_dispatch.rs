//! Shared keyboard event dispatcher for all perry/ui platform backends.
//!
//! Each platform crate (macos, ios, gtk4, windows, android, …) is responsible
//! for two things only:
//!
//! 1. **A keycode LUT** — translate the platform's raw key identifier
//!    (`kVK_*`, `UIKey.keyCode`, `gdk::Key`, `VK_*`, `AKEYCODE_*`, `event.code`)
//!    into our canonical [`KeyCode`].
//! 2. **An event hook** — wire the platform's key-down / key-up event source
//!    (NSEvent monitor, UIView responder, GtkEventControllerKey, WndProc
//!    message, OnKeyListener) to call [`on_key_event`].
//!
//! Everything else — the held-key bitset, the focused-widget cache, the
//! per-widget callback maps, the modifier snapshot, the actual JS closure
//! invocation — lives here, identical across platforms.
//!
//! ## Hot path (per event)
//!
//! 1. Platform hook decodes the key and calls [`on_key_event`].
//! 2. Bitset update (atomic fetch_or/and, branchless).
//! 3. Modifier atomic store (relaxed).
//! 4. One `Cell::get` to read the active callback for the focused widget.
//! 5. One `js_closure_call*` to invoke JS.
//!
//! Zero heap allocations per event. The active callback cache is refreshed
//! only when focus changes or a handler is (re-)registered.

use crate::keys::KeyCode;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

extern "C" {
    fn js_closure_call2(closure: *const u8, a: f64, b: f64) -> f64;
    fn js_closure_call3(closure: *const u8, a: f64, b: f64, c: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

// --- Global state -----------------------------------------------------------

thread_local! {
    /// Maps from widget handle → callback. Cold path only.
    static KEY_DOWN_CB: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static KEY_UP_CB:   RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());

    /// Widget handle currently receiving keyboard events. `0` = app-level.
    static FOCUSED_WIDGET: Cell<i64> = const { Cell::new(0) };

    /// App-level fallback handlers. `NAN` = unset.
    static APP_KEY_DOWN_CB: Cell<f64> = const { Cell::new(f64::NAN) };
    static APP_KEY_UP_CB:   Cell<f64> = const { Cell::new(f64::NAN) };

    /// HOT-PATH cache: resolved callback for the current focused widget,
    /// falling back to the app-level handler. Refreshed only on focus
    /// change / handler (re-)registration.
    static ACTIVE_DOWN_CB: Cell<f64> = const { Cell::new(f64::NAN) };
    static ACTIVE_UP_CB:   Cell<f64> = const { Cell::new(f64::NAN) };
}

/// 128-bit held-key bitset, keyed by `KeyCode.0`. Covers the full canonical
/// table (max id ~100) with headroom. Atomic + `Relaxed` so `is_key_down`
/// works lock-free even mid-event.
static HELD: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

const HELD_BITS: usize = 64 * 2;

/// Current modifier bitfield in Perry's encoding (1=Cmd, 2=Shift, 4=Alt, 8=Ctrl).
static CURRENT_MODS: AtomicU32 = AtomicU32::new(0);

// --- Cold-path public API ---------------------------------------------------

/// Register a key-down handler for a widget. `handle == 0` is reserved — use
/// [`app_set_on_key_down`] for app-level capture.
pub fn set_on_key_down(handle: i64, callback: f64) {
    if handle == 0 {
        APP_KEY_DOWN_CB.with(|c| c.set(callback));
    } else {
        KEY_DOWN_CB.with(|m| {
            m.borrow_mut().insert(handle, callback);
        });
    }
    refresh_active_callbacks();
}

pub fn set_on_key_up(handle: i64, callback: f64) {
    if handle == 0 {
        APP_KEY_UP_CB.with(|c| c.set(callback));
    } else {
        KEY_UP_CB.with(|m| {
            m.borrow_mut().insert(handle, callback);
        });
    }
    refresh_active_callbacks();
}

pub fn app_set_on_key_down(callback: f64) {
    set_on_key_down(0, callback);
}
pub fn app_set_on_key_up(callback: f64) {
    set_on_key_up(0, callback);
}

pub fn focus_widget(handle: i64) {
    FOCUSED_WIDGET.with(|c| c.set(handle));
    refresh_active_callbacks();
}

pub fn blur_widget(handle: i64) {
    FOCUSED_WIDGET.with(|c| {
        if c.get() == handle {
            c.set(0);
        }
    });
    refresh_active_callbacks();
}

/// Branchless O(1) held-key check.
pub fn is_key_down(code: u16) -> bool {
    let id = code as usize;
    if id == 0 || id >= HELD_BITS {
        return false;
    }
    let word = HELD[id >> 6].load(Ordering::Relaxed);
    (word >> (id & 63)) & 1 != 0
}

/// Snapshot of the current modifier bitfield.
pub fn current_modifiers() -> u32 {
    CURRENT_MODS.load(Ordering::Relaxed)
}

// --- Hot-path entry (called by platform backends) ---------------------------

/// Called by each platform's event hook for every physical key transition.
///
/// - `code`: canonical key id (already translated from the platform's raw code)
/// - `mods`: Perry modifier bitfield (1=Cmd, 2=Shift, 4=Alt, 8=Ctrl)
/// - `is_down`: `true` for press, `false` for release
/// - `is_repeat`: OS auto-repeat flag (ignored for key-up)
///
/// `KeyCode::UNKNOWN` is a no-op — platforms should still call us with it so
/// modifier state stays accurate, but most won't bother.
pub fn on_key_event(code: KeyCode, mods: u32, is_down: bool, is_repeat: bool) {
    update_modifiers(mods);
    if code == KeyCode::UNKNOWN {
        return;
    }
    set_held(code, is_down);

    let callback = if is_down {
        ACTIVE_DOWN_CB.with(|c| c.get())
    } else {
        ACTIVE_UP_CB.with(|c| c.get())
    };
    if callback.is_nan() {
        return;
    }

    dispatch(callback, code, mods, is_down, is_repeat);
}

/// Called by platforms that distinguish "modifier changed without key event"
/// (macOS `flagsChanged:`, GTK4 `modifiers` on EventControllerKey, etc.).
/// Keeps [`current_modifiers`] accurate when only the Shift/Ctrl/Alt/Cmd
/// state transitions without any physical key being pressed.
pub fn update_modifiers(mods: u32) {
    CURRENT_MODS.store(mods, Ordering::Relaxed);
}

// --- Internal ---------------------------------------------------------------

#[inline]
fn set_held(code: KeyCode, down: bool) {
    let id = code.0 as usize;
    if id == 0 || id >= HELD_BITS {
        return;
    }
    let bit = 1u64 << (id & 63);
    let slot = &HELD[id >> 6];
    if down {
        slot.fetch_or(bit, Ordering::Relaxed);
    } else {
        slot.fetch_and(!bit, Ordering::Relaxed);
    }
}

fn refresh_active_callbacks() {
    let focused = FOCUSED_WIDGET.with(|c| c.get());

    let pick = |map: &RefCell<HashMap<i64, f64>>, app: f64| -> f64 {
        if focused != 0 {
            if let Some(cb) = map.borrow().get(&focused).copied() {
                return cb;
            }
        }
        app
    };

    let app_down = APP_KEY_DOWN_CB.with(|c| c.get());
    let app_up = APP_KEY_UP_CB.with(|c| c.get());
    let down = KEY_DOWN_CB.with(|m| pick(m, app_down));
    let up = KEY_UP_CB.with(|m| pick(m, app_up));

    ACTIVE_DOWN_CB.with(|c| c.set(down));
    ACTIVE_UP_CB.with(|c| c.set(up));
}

#[inline]
fn dispatch(callback: f64, code: KeyCode, mods: u32, is_down: bool, is_repeat: bool) {
    let closure_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
    if closure_ptr.is_null() {
        return;
    }
    let key_f = code.0 as f64;
    let mods_f = mods as f64;
    unsafe {
        if is_down {
            let repeat_f = if is_repeat { 1.0 } else { 0.0 };
            js_closure_call3(closure_ptr, key_f, mods_f, repeat_f);
        } else {
            js_closure_call2(closure_ptr, key_f, mods_f);
        }
    }
}
