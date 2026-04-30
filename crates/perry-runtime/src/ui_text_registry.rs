//! Cross-platform `showToast` / `setText` runtime backbone (Phase 2 v3.3).
//!
//! Phase 2 v3 (v0.5.405) shipped HarmonyOS-only `perry_arkts_show_toast` and
//! `perry_arkts_set_text` symbols guarded behind `feature = "ohos-napi"`. On
//! every other backend (macOS / iOS / Linux GTK4 / Windows / Android / tvOS /
//! visionOS / watchOS) the codegen at `lower_call/native.rs` would emit calls
//! to those symbols regardless of target — and the link would fail because
//! the symbols only exist when `ohos-napi` is on.
//!
//! This module fills the gap: when `ohos-napi` is OFF, it provides
//! cross-platform stub definitions of `perry_arkts_show_toast` /
//! `perry_arkts_set_text` / `perry_arkts_register_text_id` that route to a
//! per-process **handler registry**. The platform-specific UI crate
//! (perry-ui-macos, perry-ui-gtk4, etc.) registers its own handlers at
//! startup via `js_register_show_toast_handler` / `js_register_set_text_handler`
//! / `js_register_text_id_handler`. Backends that haven't wired anything yet
//! get a hilog/eprintln "not yet implemented on <platform>" line so missing
//! coverage is discoverable.
//!
//! When `ohos-napi` is ON, `arkts_callbacks.rs` provides the canonical
//! drain-queue implementations of `perry_arkts_show_toast` /
//! `perry_arkts_set_text`, and this module's stubs are gated out via
//! `#[cfg(not(feature = "ohos-napi"))]` so there's no symbol collision.
//!
//! ## Symbol shape
//!
//! All three functions take **NaN-boxed JS value** arguments (raw `f64` bits
//! per Perry's tagging convention, `STRING_TAG=0x7FFF` for heap strings,
//! `SHORT_STRING_TAG=0x7FF9` for SSO). The handler-callback signature
//! receives plain Rust `&str` so the platform UI code doesn't need to know
//! about Perry's value representation.
//!
//! ## Registration model
//!
//! Each handler slot is an `AtomicPtr<()>` storing a function pointer. UI
//! crates register at `app_run` startup (or whenever they initialize),
//! before any user TS code calls `showToast`. Calls before registration
//! emit a one-time "no handler registered" warning and silently no-op.
//!
//! Mirrors the existing `js_register_stdlib_pump` pattern in `lib.rs`
//! (the v0.5.x cross-crate callback wiring that lets perry-ui-macos's
//! pump timer drive `js_stdlib_process_pending` without a hard link
//! dep on perry-stdlib).

#[cfg(not(feature = "ohos-napi"))]
use std::ptr::null_mut;
#[cfg(not(feature = "ohos-napi"))]
use std::sync::atomic::{AtomicPtr, Ordering};

use std::sync::Mutex;

/// Decode a NaN-boxed JS value to a Rust `String`. Matches the
/// `arkts_callbacks::decode_jsvalue_string` helper exactly so harmonyos
/// and non-harmonyos builds see identical string semantics. Falls back
/// to empty string on null header (defensive — should never happen with
/// codegen-emitted shape).
pub(crate) fn decode_jsvalue_string(handle: f64) -> String {
    let header = crate::value::js_jsvalue_to_string(handle);
    if header.is_null() {
        return String::new();
    }
    unsafe {
        let blen = (*header).byte_len as usize;
        let data_ptr =
            (header as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let bytes = std::slice::from_raw_parts(data_ptr, blen);
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Cross-platform handler signature. Receives a UTF-8 string view of the
/// already-decoded JS value. UI crates implement this on the main thread
/// (AppKit / UIKit / GTK4 / Win32 / etc).
pub type ShowToastHandler = extern "C" fn(msg_ptr: *const u8, msg_len: usize);

/// Cross-platform setText handler signature.
pub type SetTextHandler =
    extern "C" fn(id_ptr: *const u8, id_len: usize, val_ptr: *const u8, val_len: usize);

/// Cross-platform register-text-id handler signature. Called when a
/// `Text("content", "id")` is created so the platform UI code can map
/// the id → widget handle for later `setText` lookups.
pub type RegisterTextIdHandler =
    extern "C" fn(widget_handle: i64, id_ptr: *const u8, id_len: usize);

#[cfg(not(feature = "ohos-napi"))]
static SHOW_TOAST_HANDLER: AtomicPtr<()> = AtomicPtr::new(null_mut());
#[cfg(not(feature = "ohos-napi"))]
static SET_TEXT_HANDLER: AtomicPtr<()> = AtomicPtr::new(null_mut());
#[cfg(not(feature = "ohos-napi"))]
static REGISTER_TEXT_ID_HANDLER: AtomicPtr<()> = AtomicPtr::new(null_mut());

// --- Pending-call buffers ---
//
// Widget construction in user code happens at module-init time (before
// `app_run` calls our `js_register_*_handler` functions): every
// `Text("Count: 0", "counter")` immediately fires
// `perry_arkts_register_text_id(handle, id)`. If we discarded those
// calls when no handler was registered, the macOS-side id → handle map
// would never get populated and later `setText("counter", ...)` calls
// would silently no-op.
//
// Solution: queue each call when the handler slot is null. When the UI
// crate registers its handler at startup, the registration FFI drains
// the queue immediately, replaying every buffered call against the
// fresh handler. After drain, future calls go straight through.

static PENDING_TOASTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static PENDING_SET_TEXTS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static PENDING_REGISTER_IDS: Mutex<Vec<(i64, String)>> = Mutex::new(Vec::new());

#[cfg(not(feature = "ohos-napi"))]
#[no_mangle]
pub extern "C" fn js_register_show_toast_handler(f: ShowToastHandler) {
    SHOW_TOAST_HANDLER.store(f as *mut (), Ordering::Release);
    // Drain any toasts queued before the UI lib finished initialising.
    let drained: Vec<String> = match PENDING_TOASTS.lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(_) => return,
    };
    for s in drained {
        let bytes = s.as_bytes();
        f(bytes.as_ptr(), bytes.len());
    }
}

#[cfg(not(feature = "ohos-napi"))]
#[no_mangle]
pub extern "C" fn js_register_set_text_handler(f: SetTextHandler) {
    SET_TEXT_HANDLER.store(f as *mut (), Ordering::Release);
    let drained: Vec<(String, String)> = match PENDING_SET_TEXTS.lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(_) => return,
    };
    for (id, val) in drained {
        let id_b = id.as_bytes();
        let val_b = val.as_bytes();
        f(id_b.as_ptr(), id_b.len(), val_b.as_ptr(), val_b.len());
    }
}

#[cfg(not(feature = "ohos-napi"))]
#[no_mangle]
pub extern "C" fn js_register_text_id_handler(f: RegisterTextIdHandler) {
    REGISTER_TEXT_ID_HANDLER.store(f as *mut (), Ordering::Release);
    let drained: Vec<(i64, String)> = match PENDING_REGISTER_IDS.lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(_) => return,
    };
    for (handle, id) in drained {
        let id_b = id.as_bytes();
        f(handle, id_b.as_ptr(), id_b.len());
    }
}

// On harmonyos, `arkts_callbacks::perry_arkts_show_toast` and
// `arkts_callbacks::perry_arkts_set_text` provide the canonical
// drain-queue implementations. We stub the registration FFIs so cross-
// platform UI crates that try to register handlers compile cleanly even
// on harmonyos builds (the ArkUI path doesn't need them — but leaving
// them undefined would break the link if a UI crate tried to register).
#[cfg(feature = "ohos-napi")]
#[no_mangle]
pub extern "C" fn js_register_show_toast_handler(_f: ShowToastHandler) {
    // No-op on harmonyos — drain-queue path in arkts_callbacks owns it.
}

#[cfg(feature = "ohos-napi")]
#[no_mangle]
pub extern "C" fn js_register_set_text_handler(_f: SetTextHandler) {}

#[cfg(feature = "ohos-napi")]
#[no_mangle]
pub extern "C" fn js_register_text_id_handler(_f: RegisterTextIdHandler) {}

/// Cross-platform `perry_arkts_show_toast` stub. Only compiled when the
/// `ohos-napi` feature is OFF — when it's ON, `arkts_callbacks.rs`
/// provides the canonical drain-queue implementation and this stub is
/// gated out so there's no symbol collision.
///
/// Calls before a handler is registered (i.e. during widget-tree
/// construction at module-init time, before `app_run` runs the UI
/// crate's `js_register_show_toast_handler` call) are buffered into
/// `PENDING_TOASTS`. The handler-registration FFI drains the buffer.
#[cfg(not(feature = "ohos-napi"))]
#[no_mangle]
pub extern "C" fn perry_arkts_show_toast(msg_handle: f64) {
    let s = decode_jsvalue_string(msg_handle);
    let raw = SHOW_TOAST_HANDLER.load(Ordering::Acquire);
    if raw.is_null() {
        if let Ok(mut q) = PENDING_TOASTS.lock() {
            q.push(s);
        }
        return;
    }
    unsafe {
        let func: ShowToastHandler = std::mem::transmute(raw);
        let bytes = s.as_bytes();
        func(bytes.as_ptr(), bytes.len());
    }
}

/// Cross-platform `perry_arkts_set_text` stub. Same `ohos-napi` gating
/// + buffering shape as `perry_arkts_show_toast`.
#[cfg(not(feature = "ohos-napi"))]
#[no_mangle]
pub extern "C" fn perry_arkts_set_text(id_handle: f64, val_handle: f64) {
    let id = decode_jsvalue_string(id_handle);
    let val = decode_jsvalue_string(val_handle);
    let raw = SET_TEXT_HANDLER.load(Ordering::Acquire);
    if raw.is_null() {
        if let Ok(mut q) = PENDING_SET_TEXTS.lock() {
            q.push((id, val));
        }
        return;
    }
    unsafe {
        let func: SetTextHandler = std::mem::transmute(raw);
        let id_bytes = id.as_bytes();
        let val_bytes = val.as_bytes();
        func(
            id_bytes.as_ptr(),
            id_bytes.len(),
            val_bytes.as_ptr(),
            val_bytes.len(),
        );
    }
}

/// Cross-platform widget-id registration. Codegen at
/// `lower_call/native.rs` emits a call to this immediately after
/// `perry_ui_text_create` when the user wrote `Text("content", "id")`,
/// so the UI crate can map the id → widget handle for later `setText`
/// lookups.
///
/// Defined unconditionally (no `ohos-napi` gating) because the harmonyos
/// path uses a different mechanism — codegen-arkts emits the
/// `@State text_<id>: string = ...` declaration directly into the .ets
/// page, so the runtime never needs to track id → handle on harmonyos.
/// We still need the symbol to exist so non-arkts codegen can emit the
/// call without target-aware branching; it's just a no-op there.
///
/// Buffers calls before handler registration — see
/// `perry_arkts_show_toast` for the rationale.
#[no_mangle]
pub extern "C" fn perry_arkts_register_text_id(widget_handle: i64, id_handle: f64) {
    let id = decode_jsvalue_string(id_handle);
    #[cfg(feature = "ohos-napi")]
    {
        // ArkUI binds via @State decorators emitted by codegen-arkts; no
        // runtime registration needed. Drop the call.
        let _ = (widget_handle, id);
        return;
    }
    #[cfg(not(feature = "ohos-napi"))]
    {
        let raw = REGISTER_TEXT_ID_HANDLER.load(Ordering::Acquire);
        if raw.is_null() {
            if let Ok(mut q) = PENDING_REGISTER_IDS.lock() {
                q.push((widget_handle, id));
            }
            return;
        }
        unsafe {
            let func: RegisterTextIdHandler = std::mem::transmute(raw);
            let id_bytes = id.as_bytes();
            func(widget_handle, id_bytes.as_ptr(), id_bytes.len());
        }
    }
}
