//! Continuous pointer events for perry/ui on Windows (issue #1868).
//!
//! Wires `onMouseDown`, `onMouseUp`, `onMouseMove` (and the
//! `(isHovering)` flavor of `onHover`) by subclassing each registered
//! widget's HWND with `SetWindowSubclass`. The subclass proc intercepts
//! `WM_MOUSEMOVE`, `WM_*BUTTONDOWN`, `WM_*BUTTONUP`, `WM_XBUTTONDOWN/UP`
//! and `WM_MOUSELEAVE`, then forwards the rest to `DefSubclassProc` so
//! the underlying control keeps behaving normally.
//!
//! Coordinates from `LPARAM` are client-area pixels; we divide by the
//! app DPI scale so callbacks receive widget-local *points* (top-left
//! origin), matching the macOS / GTK4 backends.
//!
//! `TrackMouseEvent` is registered on the first `WM_MOUSEMOVE` per
//! widget so `WM_MOUSELEAVE` actually fires — that's also what drives
//! the `onHover(false)` callback on exit.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_pointer_event_new(x: f64, y: f64, button: u32, pointer_type: u32) -> f64;
}

const POINTER_TYPE_MOUSE: u32 = 0;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const SUBCLASS_ID: usize = 0xAFA_1868;

thread_local! {
    static MOUSE_DOWN_CB: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static MOUSE_UP_CB: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static MOUSE_MOVE_CB: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static HOVER_CB: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    /// HWND addresses that already have our subclass proc installed.
    static SUBCLASSED: RefCell<HashSet<isize>> = RefCell::new(HashSet::new());
    /// HWNDs that called TrackMouseEvent during their last WM_MOUSEMOVE
    /// — we re-arm after every WM_MOUSELEAVE because Win32's tracking
    /// is one-shot.
    static TRACKING: RefCell<HashSet<isize>> = RefCell::new(HashSet::new());
    static HOVER_STATE: RefCell<HashMap<i64, bool>> = RefCell::new(HashMap::new());
}

#[cfg(target_os = "windows")]
fn install_subclass(handle: i64) {
    let Some(hwnd) = crate::widgets::get_hwnd(handle) else {
        return;
    };
    let key = hwnd.0 as isize;
    let already = SUBCLASSED.with(|s| s.borrow().contains(&key));
    if already {
        return;
    }
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(pointer_subclass_proc), SUBCLASS_ID, 0);
    }
    SUBCLASSED.with(|s| {
        s.borrow_mut().insert(key);
    });
}

#[cfg(not(target_os = "windows"))]
fn install_subclass(_handle: i64) {}

#[cfg(target_os = "windows")]
fn arm_mouse_leave(hwnd: HWND) {
    let key = hwnd.0 as isize;
    let already = TRACKING.with(|s| s.borrow().contains(&key));
    if already {
        return;
    }
    let mut tme = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    unsafe {
        let _ = TrackMouseEvent(&mut tme);
    }
    TRACKING.with(|s| {
        s.borrow_mut().insert(key);
    });
}

/// Decode `LPARAM` into widget-local *points* (top-left origin),
/// dividing by the app DPI scale so callbacks see the same coordinate
/// space as the macOS / GTK4 backends. `LPARAM` is `(y_hi, x_lo)` of
/// signed 16-bit client-area pixels.
#[cfg(target_os = "windows")]
fn decode_xy(lparam: LPARAM) -> (f64, f64) {
    let raw = lparam.0 as u32;
    let x_px = (raw & 0xFFFF) as i16 as f64;
    let y_px = ((raw >> 16) & 0xFFFF) as i16 as f64;
    let scale = crate::app::get_dpi_scale().max(1.0);
    (x_px / scale, y_px / scale)
}

#[cfg(target_os = "windows")]
fn js_button_from_x_wparam(wparam: WPARAM) -> u32 {
    // X-button HIWORD: XBUTTON1=1 → web "back", XBUTTON2=2 → "forward".
    match (wparam.0 >> 16) & 0xFFFF {
        1 => 3, // Back
        2 => 4, // Forward
        _ => 0,
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn pointer_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _refdata: usize,
) -> LRESULT {
    let handle = crate::widgets::find_handle_by_hwnd(hwnd);
    if handle > 0 {
        dispatch_message(handle, hwnd, msg, wparam, lparam);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

#[cfg(target_os = "windows")]
fn dispatch_message(handle: i64, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) {
    match msg {
        m if m == WM_MOUSEMOVE => {
            arm_mouse_leave(hwnd);
            let (x, y) = decode_xy(lparam);
            // Drive hover transitions: WM_MOUSEMOVE that didn't already
            // come from inside the widget marks an "enter".
            let was_in = HOVER_STATE.with(|s| s.borrow().get(&handle).copied().unwrap_or(false));
            if !was_in {
                HOVER_STATE.with(|s| {
                    s.borrow_mut().insert(handle, true);
                });
                fire_hover(handle, true);
            }
            if let Some(cb) = MOUSE_MOVE_CB.with(|c| c.borrow().get(&handle).copied()) {
                fire_pointer_event(cb, x, y, 0);
            }
        }
        m if m == WM_MOUSELEAVE => {
            TRACKING.with(|s| {
                s.borrow_mut().remove(&(hwnd.0 as isize));
            });
            HOVER_STATE.with(|s| {
                s.borrow_mut().insert(handle, false);
            });
            fire_hover(handle, false);
        }
        m if m == WM_LBUTTONDOWN => {
            let (x, y) = decode_xy(lparam);
            if let Some(cb) = MOUSE_DOWN_CB.with(|c| c.borrow().get(&handle).copied()) {
                fire_pointer_event(cb, x, y, 0);
            }
        }
        m if m == WM_LBUTTONUP => {
            let (x, y) = decode_xy(lparam);
            if let Some(cb) = MOUSE_UP_CB.with(|c| c.borrow().get(&handle).copied()) {
                fire_pointer_event(cb, x, y, 0);
            }
        }
        m if m == WM_RBUTTONDOWN => {
            let (x, y) = decode_xy(lparam);
            if let Some(cb) = MOUSE_DOWN_CB.with(|c| c.borrow().get(&handle).copied()) {
                fire_pointer_event(cb, x, y, 2);
            }
        }
        m if m == WM_RBUTTONUP => {
            let (x, y) = decode_xy(lparam);
            if let Some(cb) = MOUSE_UP_CB.with(|c| c.borrow().get(&handle).copied()) {
                fire_pointer_event(cb, x, y, 2);
            }
        }
        m if m == WM_MBUTTONDOWN => {
            let (x, y) = decode_xy(lparam);
            if let Some(cb) = MOUSE_DOWN_CB.with(|c| c.borrow().get(&handle).copied()) {
                fire_pointer_event(cb, x, y, 1);
            }
        }
        m if m == WM_MBUTTONUP => {
            let (x, y) = decode_xy(lparam);
            if let Some(cb) = MOUSE_UP_CB.with(|c| c.borrow().get(&handle).copied()) {
                fire_pointer_event(cb, x, y, 1);
            }
        }
        m if m == WM_XBUTTONDOWN => {
            let (x, y) = decode_xy(lparam);
            let btn = js_button_from_x_wparam(wparam);
            if let Some(cb) = MOUSE_DOWN_CB.with(|c| c.borrow().get(&handle).copied()) {
                fire_pointer_event(cb, x, y, btn);
            }
        }
        m if m == WM_XBUTTONUP => {
            let (x, y) = decode_xy(lparam);
            let btn = js_button_from_x_wparam(wparam);
            if let Some(cb) = MOUSE_UP_CB.with(|c| c.borrow().get(&handle).copied()) {
                fire_pointer_event(cb, x, y, btn);
            }
        }
        _ => {}
    }
}

#[cfg(target_os = "windows")]
fn fire_pointer_event(cb_f64: f64, x: f64, y: f64, button: u32) {
    unsafe {
        let closure_ptr = js_nanbox_get_pointer(cb_f64);
        if closure_ptr == 0 {
            return;
        }
        let pe = js_pointer_event_new(x, y, button, POINTER_TYPE_MOUSE);
        js_closure_call1(closure_ptr as *const u8, pe);
    }
}

#[cfg(target_os = "windows")]
fn fire_hover(handle: i64, is_hovering: bool) {
    let Some(cb_f64) = HOVER_CB.with(|c| c.borrow().get(&handle).copied()) else {
        return;
    };
    unsafe {
        let closure_ptr = js_nanbox_get_pointer(cb_f64);
        if closure_ptr == 0 {
            return;
        }
        let bits = if is_hovering { TAG_TRUE } else { TAG_FALSE };
        js_closure_call1(closure_ptr as *const u8, f64::from_bits(bits));
    }
}

// ---------------------------------------------------------------------------
// Public registration. Mirrors the macOS / GTK4 / iOS surface.
// ---------------------------------------------------------------------------

pub fn set_on_mouse_down(handle: i64, callback: f64) {
    MOUSE_DOWN_CB.with(|c| {
        c.borrow_mut().insert(handle, callback);
    });
    install_subclass(handle);
}

pub fn set_on_mouse_up(handle: i64, callback: f64) {
    MOUSE_UP_CB.with(|c| {
        c.borrow_mut().insert(handle, callback);
    });
    install_subclass(handle);
}

pub fn set_on_mouse_move(handle: i64, callback: f64) {
    MOUSE_MOVE_CB.with(|c| {
        c.borrow_mut().insert(handle, callback);
    });
    install_subclass(handle);
}

pub fn set_on_hover(handle: i64, callback: f64) {
    HOVER_CB.with(|c| {
        c.borrow_mut().insert(handle, callback);
    });
    install_subclass(handle);
}
