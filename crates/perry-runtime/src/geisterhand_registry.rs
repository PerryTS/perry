//! Geisterhand: In-process input fuzzer callback registry and dispatch queue.
//!
//! Provides a global registry where UI widgets register their callbacks,
//! and a pending-action queue for cross-thread dispatch (HTTP server thread → main thread).

use std::sync::{Condvar, Mutex};

/// Widget type identifiers
pub const WIDGET_BUTTON: u8 = 0;
pub const WIDGET_TEXTFIELD: u8 = 1;
pub const WIDGET_SLIDER: u8 = 2;
pub const WIDGET_TOGGLE: u8 = 3;
pub const WIDGET_PICKER: u8 = 4;
pub const WIDGET_MENU: u8 = 5;
pub const WIDGET_SHORTCUT: u8 = 6;
pub const WIDGET_TABLE: u8 = 7;
pub const WIDGET_SCROLLVIEW: u8 = 8;

/// Callback kind identifiers
pub const CB_ON_CLICK: u8 = 0;
pub const CB_ON_CHANGE: u8 = 1;
pub const CB_ON_SUBMIT: u8 = 2;
pub const CB_ON_HOVER: u8 = 3;
pub const CB_ON_DOUBLE_CLICK: u8 = 4;
pub const CB_ON_FOCUS: u8 = 5;

/// A registered widget callback entry
pub struct RegisteredWidget {
    pub handle: i64,
    pub widget_type: u8,
    pub callback_kind: u8,
    pub closure_f64: f64,
    pub label: String,
    pub shortcut: String,
}

/// An action queued for main-thread execution
pub enum PendingAction {
    InvokeCallback {
        closure_f64: f64,
        args: Vec<f64>,
    },
    SetState {
        handle: i64,
        value: f64,
    },
    CaptureScreenshot,
    SetText {
        handle: i64,
        text: String,
    },
    ScrollTo {
        handle: i64,
        x: f64,
        y: f64,
    },
    ReadValue {
        handle: i64,
    },
    QueryWidgetTree,
    /// Issue #185 Phase D step 2: live-edit a single style prop on a
    /// widget. `prop_id` selects the setter (one of `STYLE_*` constants
    /// below); `args` carries the prop's payload — colors use all four
    /// (RGBA 0-1), scalar props use `args[0]`, bools use `args[0] != 0`.
    /// The pump dispatches to the platform UI lib's registered
    /// `perry_ui_geisterhand_apply_style` function on the main thread.
    ApplyStyle {
        handle: i64,
        prop_id: u32,
        args: [f64; 4],
    },
}

/// Stable prop-id namespace for `ApplyStyle`. Adds at the end only —
/// the inspector UI on the wire references these by string name and
/// the runtime maps the string → id, so the numeric values are
/// internal but must stay stable across HTTP server / platform UI
/// crate boundary.
pub const STYLE_BACKGROUND_COLOR: u32 = 1;
pub const STYLE_COLOR: u32 = 2;
pub const STYLE_BORDER_COLOR: u32 = 3;
pub const STYLE_BORDER_WIDTH: u32 = 4;
pub const STYLE_BORDER_RADIUS: u32 = 5;
pub const STYLE_OPACITY: u32 = 6;
pub const STYLE_PADDING_UNIFORM: u32 = 7;
pub const STYLE_HIDDEN: u32 = 8;
pub const STYLE_ENABLED: u32 = 9;

static REGISTRY: Mutex<Vec<RegisteredWidget>> = Mutex::new(Vec::new());
static PENDING_ACTIONS: Mutex<Vec<PendingAction>> = Mutex::new(Vec::new());

/// Screenshot result buffer: shared between main thread (writer) and HTTP server (reader).
/// The main thread captures the screenshot and writes PNG bytes here, then signals the condvar.
static SCREENSHOT_RESULT: Mutex<Option<Vec<u8>>> = Mutex::new(None);
static SCREENSHOT_CONDVAR: Condvar = Condvar::new();
/// Flag indicating a screenshot request is pending (prevents duplicate requests)
static SCREENSHOT_REQUESTED: Mutex<bool> = Mutex::new(false);

/// Value read result: same condvar pattern as screenshot.
static VALUE_RESULT: Mutex<Option<String>> = Mutex::new(None);
static VALUE_CONDVAR: Condvar = Condvar::new();
static VALUE_REQUESTED: Mutex<bool> = Mutex::new(false);

/// Widget tree result: same condvar pattern.
static TREE_RESULT: Mutex<Option<String>> = Mutex::new(None);
static TREE_CONDVAR: Condvar = Condvar::new();
static TREE_REQUESTED: Mutex<bool> = Mutex::new(false);

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

// Registered function pointers for UI operations. Platform UI crates call the register
// functions below during initialization. This avoids extern "C" declarations that would
// create hard linker dependencies on UI crate symbols.
use std::sync::atomic::{AtomicPtr, Ordering};

static UI_STATE_SET_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static UI_SCREENSHOT_CAPTURE_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static UI_TEXTFIELD_SET_STRING_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static UI_SCROLL_SET_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static UI_READ_VALUE_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static UI_QUERY_TREE_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static UI_APPLY_STYLE_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Register the platform UI crate's state_set function.
#[no_mangle]
pub extern "C" fn perry_geisterhand_register_state_set(f: extern "C" fn(i64, f64)) {
    UI_STATE_SET_FN.store(f as *mut (), Ordering::Release);
}

/// Register the platform UI crate's screenshot_capture function.
#[no_mangle]
pub extern "C" fn perry_geisterhand_register_screenshot_capture(
    f: extern "C" fn(*mut usize) -> *mut u8,
) {
    UI_SCREENSHOT_CAPTURE_FN.store(f as *mut (), Ordering::Release);
}

/// Register the platform UI crate's textfield_set_string function.
#[no_mangle]
pub extern "C" fn perry_geisterhand_register_textfield_set_string(f: extern "C" fn(i64, i64)) {
    UI_TEXTFIELD_SET_STRING_FN.store(f as *mut (), Ordering::Release);
}

/// Register the platform UI crate's scroll_set function.
#[no_mangle]
pub extern "C" fn perry_geisterhand_register_scroll_set(f: extern "C" fn(i64, f64, f64)) {
    UI_SCROLL_SET_FN.store(f as *mut (), Ordering::Release);
}

/// Register the platform UI crate's read_value function.
#[no_mangle]
pub extern "C" fn perry_geisterhand_register_read_value(
    f: extern "C" fn(i64, *mut usize) -> *mut u8,
) {
    UI_READ_VALUE_FN.store(f as *mut (), Ordering::Release);
}

/// Register the platform UI crate's query_tree function.
#[no_mangle]
pub extern "C" fn perry_geisterhand_register_query_tree(f: extern "C" fn(*mut usize) -> *mut u8) {
    UI_QUERY_TREE_FN.store(f as *mut (), Ordering::Release);
}

/// Register the platform UI crate's `apply_style` dispatcher (#185 Phase D
/// step 2). The dispatcher takes `(handle, prop_id, a0, a1, a2, a3)` and
/// is expected to switch on `prop_id` against the `STYLE_*` constants in
/// this module, calling the correct platform-specific setter (which must
/// already be main-thread-safe — the pump fires on the main thread).
#[no_mangle]
pub extern "C" fn perry_geisterhand_register_apply_style(
    f: extern "C" fn(i64, u32, f64, f64, f64, f64),
) {
    UI_APPLY_STYLE_FN.store(f as *mut (), Ordering::Release);
}

/// Register a widget callback in the global registry.
/// Called from platform UI crates when widgets are created or callbacks attached.
///
/// - `handle`: widget handle (1-based i64)
/// - `widget_type`: WIDGET_* constant
/// - `callback_kind`: CB_* constant
/// - `closure_f64`: NaN-boxed closure pointer
/// - `label_ptr`: pointer to a StringHeader (or null)
#[no_mangle]
pub extern "C" fn perry_geisterhand_register(
    handle: i64,
    widget_type: u8,
    callback_kind: u8,
    closure_f64: f64,
    label_ptr: *const u8,
) {
    let label = if label_ptr.is_null() {
        String::new()
    } else {
        // Read StringHeader: first 8 bytes are header (length at offset 0 as u32),
        // followed by UTF-8 data bytes
        unsafe {
            let len = *(label_ptr as *const u32) as usize;
            let data = label_ptr.add(std::mem::size_of::<[u64; 1]>()); // skip 8-byte GcHeader+length
            if len > 0 && len < 10000 {
                String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
            } else {
                String::new()
            }
        }
    };
    if let Ok(mut reg) = REGISTRY.lock() {
        reg.push(RegisteredWidget {
            handle,
            widget_type,
            callback_kind,
            closure_f64,
            label,
            shortcut: String::new(),
        });
    }
}

/// Register a widget callback with an associated keyboard shortcut string.
/// Used by menu items that have shortcuts (e.g., "s" for Cmd+S).
#[no_mangle]
pub extern "C" fn perry_geisterhand_register_with_shortcut(
    handle: i64,
    widget_type: u8,
    callback_kind: u8,
    closure_f64: f64,
    label_ptr: *const u8,
    shortcut_ptr: *const u8,
    shortcut_len: usize,
) {
    let label = if label_ptr.is_null() {
        String::new()
    } else {
        unsafe {
            let len = *(label_ptr as *const u32) as usize;
            let data = label_ptr.add(std::mem::size_of::<[u64; 1]>());
            if len > 0 && len < 10000 {
                String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
            } else {
                String::new()
            }
        }
    };
    let shortcut = if shortcut_ptr.is_null() || shortcut_len == 0 {
        String::new()
    } else {
        unsafe {
            String::from_utf8_lossy(std::slice::from_raw_parts(shortcut_ptr, shortcut_len))
                .into_owned()
        }
    };
    if let Ok(mut reg) = REGISTRY.lock() {
        reg.push(RegisteredWidget {
            handle,
            widget_type,
            callback_kind,
            closure_f64,
            label,
            shortcut,
        });
    }
}

/// Find a registered callback by shortcut string. Case-insensitive match.
/// Returns the closure_f64 or 0.0 if not found.
#[no_mangle]
pub extern "C" fn perry_geisterhand_find_by_shortcut(
    shortcut_ptr: *const u8,
    shortcut_len: usize,
) -> f64 {
    if shortcut_ptr.is_null() || shortcut_len == 0 {
        return 0.0;
    }
    let query =
        unsafe { String::from_utf8_lossy(std::slice::from_raw_parts(shortcut_ptr, shortcut_len)) }
            .to_lowercase();
    match REGISTRY.lock() {
        Ok(reg) => {
            for w in reg.iter() {
                if !w.shortcut.is_empty() && w.shortcut.to_lowercase() == query {
                    return w.closure_f64;
                }
            }
            0.0
        }
        Err(_) => 0.0,
    }
}

/// Queue a scroll action for main-thread dispatch.
#[no_mangle]
pub extern "C" fn perry_geisterhand_queue_scroll(handle: i64, x: f64, y: f64) {
    if let Ok(mut q) = PENDING_ACTIONS.lock() {
        q.push(PendingAction::ScrollTo { handle, x, y });
    }
}

/// Queue a callback invocation for main-thread dispatch.
#[no_mangle]
pub extern "C" fn perry_geisterhand_queue_action(closure_f64: f64) {
    if let Ok(mut q) = PENDING_ACTIONS.lock() {
        q.push(PendingAction::InvokeCallback {
            closure_f64,
            args: Vec::new(),
        });
    }
}

/// Queue a callback invocation with one argument for main-thread dispatch.
#[no_mangle]
pub extern "C" fn perry_geisterhand_queue_action1(closure_f64: f64, arg: f64) {
    if let Ok(mut q) = PENDING_ACTIONS.lock() {
        q.push(PendingAction::InvokeCallback {
            closure_f64,
            args: vec![arg],
        });
    }
}

/// Queue a state-set action for main-thread dispatch.
#[no_mangle]
pub extern "C" fn perry_geisterhand_queue_state_set(handle: i64, value: f64) {
    if let Ok(mut q) = PENDING_ACTIONS.lock() {
        q.push(PendingAction::SetState { handle, value });
    }
}

/// Queue a single-prop style edit for main-thread dispatch (#185 Phase
/// D step 2). The four `f64` args are interpreted per-prop:
///
/// - color props (`backgroundColor` / `color` / `borderColor`):
///   `(r, g, b, a)` in `[0.0, 1.0]`.
/// - scalar props (`borderWidth` / `borderRadius` / `opacity` /
///   `padding`): value lives in `a0`; `a1` / `a2` / `a3` are unused (0.0).
/// - bool props (`hidden` / `enabled`): `a0 != 0.0` for true; `a1` /
///   `a2` / `a3` unused.
///
/// The HTTP thread (server.rs `POST /style/:h`) is responsible for
/// parsing the inbound JSON into the right `prop_id` + arg layout
/// before queuing.
#[no_mangle]
pub extern "C" fn perry_geisterhand_queue_apply_style(
    handle: i64,
    prop_id: u32,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
) {
    if let Ok(mut q) = PENDING_ACTIONS.lock() {
        q.push(PendingAction::ApplyStyle {
            handle,
            prop_id,
            args: [a0, a1, a2, a3],
        });
    }
}

/// Queue a text-set action for main-thread dispatch (sets Win32 Edit control text + fires onChange).
#[no_mangle]
pub extern "C" fn perry_geisterhand_queue_set_text(
    handle: i64,
    text_ptr: *const u8,
    text_len: usize,
) {
    let text = if !text_ptr.is_null() && text_len > 0 {
        unsafe {
            String::from_utf8_lossy(std::slice::from_raw_parts(text_ptr, text_len)).into_owned()
        }
    } else {
        String::new()
    };
    if let Ok(mut q) = PENDING_ACTIONS.lock() {
        q.push(PendingAction::SetText { handle, text });
    }
}

/// Drain and execute all pending actions on the main thread.
/// Called from the platform pump timer (every 8ms).
#[no_mangle]
pub extern "C" fn perry_geisterhand_pump() {
    let actions: Vec<PendingAction> = match PENDING_ACTIONS.lock() {
        Ok(mut q) => q.drain(..).collect(),
        Err(_) => return,
    };
    for action in actions {
        match action {
            PendingAction::InvokeCallback { closure_f64, args } => {
                let ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
                unsafe {
                    match args.len() {
                        0 => {
                            js_closure_call0(ptr);
                        }
                        _ => {
                            js_closure_call1(ptr, args[0]);
                        }
                    }
                }
            }
            PendingAction::SetState { handle, value } => {
                let f = UI_STATE_SET_FN.load(Ordering::Acquire);
                if !f.is_null() {
                    unsafe {
                        let func: extern "C" fn(i64, f64) = std::mem::transmute(f);
                        func(handle, value);
                    }
                }
            }
            PendingAction::SetText { handle, text } => {
                let f = UI_TEXTFIELD_SET_STRING_FN.load(Ordering::Acquire);
                if !f.is_null() {
                    extern "C" {
                        fn js_string_from_bytes(ptr: *const u8, len: usize) -> *mut u8;
                        fn js_nanbox_string(ptr: i64) -> f64;
                    }
                    // Create a Perry StringHeader from the text bytes
                    let bytes = text.as_bytes();
                    let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len()) };
                    unsafe {
                        let func: extern "C" fn(i64, i64) = std::mem::transmute(f);
                        func(handle, str_ptr as i64);
                    }
                    // Fire onChange callback if registered
                    if let Ok(reg) = REGISTRY.lock() {
                        for w in reg.iter() {
                            if w.handle == handle && w.callback_kind == CB_ON_CHANGE {
                                let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };
                                let ptr =
                                    unsafe { js_nanbox_get_pointer(w.closure_f64) } as *const u8;
                                unsafe {
                                    js_closure_call1(ptr, nanboxed);
                                }
                                break;
                            }
                        }
                    }
                }
            }
            PendingAction::ApplyStyle {
                handle,
                prop_id,
                args,
            } => {
                let f = UI_APPLY_STYLE_FN.load(Ordering::Acquire);
                if !f.is_null() {
                    unsafe {
                        let func: extern "C" fn(i64, u32, f64, f64, f64, f64) =
                            std::mem::transmute(f);
                        func(handle, prop_id, args[0], args[1], args[2], args[3]);
                    }
                }
            }
            PendingAction::ScrollTo { handle, x, y } => {
                let f = UI_SCROLL_SET_FN.load(Ordering::Acquire);
                if !f.is_null() {
                    unsafe {
                        let func: extern "C" fn(i64, f64, f64) = std::mem::transmute(f);
                        func(handle, x, y);
                    }
                }
            }
            PendingAction::ReadValue { handle } => {
                let f = UI_READ_VALUE_FN.load(Ordering::Acquire);
                let result = if !f.is_null() {
                    unsafe {
                        let func: extern "C" fn(i64, *mut usize) -> *mut u8 =
                            std::mem::transmute(f);
                        let mut len: usize = 0;
                        let ptr = func(handle, &mut len);
                        if !ptr.is_null() && len > 0 {
                            let s = String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len))
                                .into_owned();
                            libc::free(ptr as *mut libc::c_void);
                            s
                        } else {
                            String::new()
                        }
                    }
                } else {
                    String::new()
                };
                if let Ok(mut r) = VALUE_RESULT.lock() {
                    *r = Some(result);
                }
                VALUE_CONDVAR.notify_all();
            }
            PendingAction::QueryWidgetTree => {
                let f = UI_QUERY_TREE_FN.load(Ordering::Acquire);
                let json = if !f.is_null() {
                    let mut len: usize = 0;
                    unsafe {
                        let func: extern "C" fn(*mut usize) -> *mut u8 = std::mem::transmute(f);
                        let ptr = func(&mut len);
                        if !ptr.is_null() && len > 0 {
                            let s = String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len))
                                .into_owned();
                            libc::free(ptr as *mut libc::c_void);
                            s
                        } else {
                            "[]".to_string()
                        }
                    }
                } else {
                    "[]".to_string()
                };
                if let Ok(mut r) = TREE_RESULT.lock() {
                    *r = Some(json);
                }
                TREE_CONDVAR.notify_all();
            }
            PendingAction::CaptureScreenshot => {
                let f = UI_SCREENSHOT_CAPTURE_FN.load(Ordering::Acquire);
                let (ptr, len) = if !f.is_null() {
                    let mut len: usize = 0;
                    let func: extern "C" fn(*mut usize) -> *mut u8 =
                        unsafe { std::mem::transmute(f) };
                    let ptr = func(&mut len);
                    (ptr, len)
                } else {
                    (std::ptr::null_mut(), 0)
                };
                let png_data = if !ptr.is_null() && len > 0 {
                    let data = unsafe { std::slice::from_raw_parts(ptr, len).to_vec() };
                    unsafe {
                        libc::free(ptr as *mut libc::c_void);
                    }
                    data
                } else {
                    Vec::new()
                };
                // Store result and signal the waiting HTTP thread
                if let Ok(mut result) = SCREENSHOT_RESULT.lock() {
                    *result = Some(png_data);
                }
                SCREENSHOT_CONDVAR.notify_all();
            }
        }
    }
}

/// Get a snapshot of the registry as JSON bytes.
/// Returns a heap-allocated string (caller must free with perry_geisterhand_free_string).
#[no_mangle]
pub extern "C" fn perry_geisterhand_get_registry_json(out_len: *mut usize) -> *mut u8 {
    let json = match REGISTRY.lock() {
        Ok(reg) => {
            let mut s = String::from("[");
            for (i, w) in reg.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                let escaped_label = w.label.replace('\\', "\\\\").replace('"', "\\\"");
                let escaped_shortcut = w.shortcut.replace('\\', "\\\\").replace('"', "\\\"");
                s.push_str(&format!(
                    r#"{{"handle":{},"widget_type":{},"callback_kind":{},"label":"{}","shortcut":"{}"}}"#,
                    w.handle, w.widget_type, w.callback_kind,
                    escaped_label, escaped_shortcut
                ));
            }
            s.push(']');
            s
        }
        Err(_) => "[]".to_string(),
    };
    let bytes = json.into_bytes();
    let len = bytes.len();
    let ptr = bytes.as_ptr();
    let boxed = bytes.into_boxed_slice();
    let raw = Box::into_raw(boxed);
    unsafe {
        *out_len = len;
    }
    raw as *mut u8
}

/// Free a string returned by perry_geisterhand_get_registry_json.
#[no_mangle]
pub extern "C" fn perry_geisterhand_free_string(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        unsafe {
            let _ = Box::from_raw(std::slice::from_raw_parts_mut(ptr, len));
        }
    }
}

/// Get the number of registered widgets.
#[no_mangle]
pub extern "C" fn perry_geisterhand_registry_count() -> usize {
    REGISTRY.lock().map(|r| r.len()).unwrap_or(0)
}

/// Request a screenshot capture. Called from the HTTP server thread.
/// Queues a CaptureScreenshot action and blocks until the main thread completes it.
/// Returns (ptr, len) of heap-allocated PNG data. Caller must free with perry_geisterhand_free_string.
#[no_mangle]
pub extern "C" fn perry_geisterhand_request_screenshot(out_len: *mut usize) -> *mut u8 {
    // Prevent duplicate requests
    if let Ok(mut requested) = SCREENSHOT_REQUESTED.lock() {
        if *requested {
            unsafe {
                *out_len = 0;
            }
            return std::ptr::null_mut();
        }
        *requested = true;
    }

    // Clear any previous result
    if let Ok(mut result) = SCREENSHOT_RESULT.lock() {
        *result = None;
    }

    // Queue the capture action for the main thread pump
    if let Ok(mut q) = PENDING_ACTIONS.lock() {
        q.push(PendingAction::CaptureScreenshot);
    }

    // Wait for the main thread to complete the capture (timeout 5s)
    let png_data = {
        let result = SCREENSHOT_RESULT.lock().unwrap();
        let (result, timeout) = SCREENSHOT_CONDVAR
            .wait_timeout_while(result, std::time::Duration::from_secs(5), |r| r.is_none())
            .unwrap();
        if timeout.timed_out() {
            None
        } else {
            result.clone()
        }
    };

    // Clear request flag
    if let Ok(mut requested) = SCREENSHOT_REQUESTED.lock() {
        *requested = false;
    }

    match png_data {
        Some(data) if !data.is_empty() => {
            let len = data.len();
            let boxed = data.into_boxed_slice();
            let raw = Box::into_raw(boxed);
            unsafe {
                *out_len = len;
            }
            raw as *mut u8
        }
        _ => {
            unsafe {
                *out_len = 0;
            }
            std::ptr::null_mut()
        }
    }
}

/// Shared request-on-main-thread helper for string results (value reads, tree queries).
/// Queues an action, waits on the condvar, returns a Box-allocated buffer for the caller
/// to free with `perry_geisterhand_free_string`.
fn request_string_from_main(
    requested: &Mutex<bool>,
    result: &Mutex<Option<String>>,
    condvar: &Condvar,
    action: PendingAction,
    out_len: *mut usize,
) -> *mut u8 {
    if let Ok(mut r) = requested.lock() {
        if *r {
            unsafe {
                *out_len = 0;
            }
            return std::ptr::null_mut();
        }
        *r = true;
    }
    if let Ok(mut r) = result.lock() {
        *r = None;
    }
    if let Ok(mut q) = PENDING_ACTIONS.lock() {
        q.push(action);
    }
    let string_result = {
        let guard = result.lock().unwrap();
        let (guard, timeout) = condvar
            .wait_timeout_while(guard, std::time::Duration::from_secs(5), |r| r.is_none())
            .unwrap();
        if timeout.timed_out() {
            None
        } else {
            guard.clone()
        }
    };
    if let Ok(mut r) = requested.lock() {
        *r = false;
    }
    match string_result {
        Some(s) if !s.is_empty() => {
            let bytes = s.into_bytes();
            let len = bytes.len();
            let raw = Box::into_raw(bytes.into_boxed_slice());
            unsafe {
                *out_len = len;
            }
            raw as *mut u8
        }
        _ => {
            unsafe {
                *out_len = 0;
            }
            std::ptr::null_mut()
        }
    }
}

/// Request a widget value read. Called from the HTTP server thread.
#[no_mangle]
pub extern "C" fn perry_geisterhand_request_value(handle: i64, out_len: *mut usize) -> *mut u8 {
    request_string_from_main(
        &VALUE_REQUESTED,
        &VALUE_RESULT,
        &VALUE_CONDVAR,
        PendingAction::ReadValue { handle },
        out_len,
    )
}

/// Request widget tree with visibility/frame data. Called from HTTP server thread.
#[no_mangle]
pub extern "C" fn perry_geisterhand_request_tree(out_len: *mut usize) -> *mut u8 {
    request_string_from_main(
        &TREE_REQUESTED,
        &TREE_RESULT,
        &TREE_CONDVAR,
        PendingAction::QueryWidgetTree,
        out_len,
    )
}

/// Get the closure_f64 for a given handle and callback kind. Returns 0.0 if not found.
#[no_mangle]
pub extern "C" fn perry_geisterhand_get_closure(handle: i64, callback_kind: u8) -> f64 {
    match REGISTRY.lock() {
        Ok(reg) => {
            for w in reg.iter() {
                if w.handle == handle && w.callback_kind == callback_kind {
                    return w.closure_f64;
                }
            }
            0.0
        }
        Err(_) => 0.0,
    }
}
