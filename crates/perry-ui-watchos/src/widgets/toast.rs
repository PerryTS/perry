//! watchOS toast presenter for `showToast(msg)`.
//!
//! Bridges the cross-platform `perry_arkts_show_toast` handler registry
//! (perry-runtime/src/ui_text_registry.rs) to a SwiftUI overlay. Because
//! watchOS uses the data-driven tree model rather than direct view
//! manipulation, toasts are exposed as a global "active toast" slot plus
//! a monotonic sequence counter the SwiftUI host polls alongside
//! `perry_watchos_tree_version`.
//!
//! ## Wiring
//!
//! `app::register_cross_platform_text_handlers` calls
//! `js_register_show_toast_handler` at app startup, passing
//! `show_toast_handler` here as the registered fn pointer. When user TS
//! code calls `showToast("Saved!")`, the runtime decodes the NaN-boxed
//! string and forwards to this handler; we push it onto a FIFO and the
//! Swift host pops messages in turn through `perry_watchos_toast_*`.
//!
//! The handler may fire on any thread, so the queue is `Mutex`-guarded
//! and the sequence counter is `AtomicU64`. The Swift side polls every
//! frame via the existing `PerryBridge` timer.

use std::collections::VecDeque;
use std::ffi::CString;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const DEFAULT_DURATION_MS: u32 = 2500;

struct ToastEntry {
    text: CString,
    duration_ms: u32,
}

static TOAST_QUEUE: Mutex<VecDeque<ToastEntry>> = Mutex::new(VecDeque::new());
/// Bumped on every enqueue + every dismiss so SwiftUI re-renders.
static TOAST_SEQ: AtomicU64 = AtomicU64::new(0);

/// Cross-platform handler entry point. Registered with
/// `js_register_show_toast_handler` at app startup.
pub extern "C" fn show_toast_handler(msg_ptr: *const u8, msg_len: usize) {
    if msg_ptr.is_null() {
        return;
    }
    let msg = unsafe {
        let bytes = std::slice::from_raw_parts(msg_ptr, msg_len);
        String::from_utf8_lossy(bytes).into_owned()
    };
    enqueue(msg, DEFAULT_DURATION_MS);
}

fn enqueue(msg: String, duration_ms: u32) {
    let Ok(text) = CString::new(msg) else { return };
    let entry = ToastEntry { text, duration_ms };
    if let Ok(mut q) = TOAST_QUEUE.lock() {
        q.push_back(entry);
    }
    TOAST_SEQ.fetch_add(1, Ordering::SeqCst);
}

/// Returns the active toast text as a stable pointer, or null if none.
/// The pointer is valid until the next `perry_watchos_toast_dismiss`
/// or process exit; the Swift caller must copy if it needs to outlive
/// either.
#[no_mangle]
pub extern "C" fn perry_watchos_toast_active_text() -> *const std::ffi::c_char {
    if let Ok(q) = TOAST_QUEUE.lock() {
        if let Some(front) = q.front() {
            return front.text.as_ptr();
        }
    }
    std::ptr::null()
}

/// Duration in ms of the front toast (0 if none).
#[no_mangle]
pub extern "C" fn perry_watchos_toast_active_duration_ms() -> u32 {
    if let Ok(q) = TOAST_QUEUE.lock() {
        if let Some(front) = q.front() {
            return front.duration_ms;
        }
    }
    0
}

/// Sequence counter that bumps on every state change. Swift uses this
/// (alongside the active text pointer) to drive re-renders.
#[no_mangle]
pub extern "C" fn perry_watchos_toast_seq() -> u64 {
    TOAST_SEQ.load(Ordering::SeqCst)
}

/// Pop the front toast (called by SwiftUI when the display interval
/// elapses).
#[no_mangle]
pub extern "C" fn perry_watchos_toast_dismiss() {
    if let Ok(mut q) = TOAST_QUEUE.lock() {
        q.pop_front();
    }
    TOAST_SEQ.fetch_add(1, Ordering::SeqCst);
}
