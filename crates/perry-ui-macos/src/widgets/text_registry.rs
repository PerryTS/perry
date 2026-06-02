//! macOS text-id → widget-handle registry for `setText(id, value)`
//! (Phase 2 v3.3).
//!
//! When user TS authors `Text("Count: 0", "counter")`, the codegen:
//!
//! 1. Calls `perry_ui_text_create(content_ptr)` and gets a widget handle.
//! 2. Immediately calls `perry_arkts_register_text_id(handle, id)` (the
//!    cross-platform symbol defined in `perry-runtime/ui_text_registry.rs`).
//! 3. The runtime forwards to the registered `RegisterTextIdHandler` —
//!    `register_text_id_handler` in this file.
//! 4. We store `id → handle` in `TEXT_IDS`.
//!
//! Later when user TS calls `setText("counter", "Count: 5")`, the codegen
//! emits `perry_arkts_set_text(id, val)`; the runtime forwards to
//! `set_text_handler` here, which looks up the handle and calls the
//! existing `widgets::text::set_text_str` to update the NSTextField via
//! `setStringValue:`.
//!
//! The registry is process-global (Mutex-guarded for thread-safety,
//! though all UI calls should be on the main thread anyway). Calls to
//! `setText` for an unregistered id are silently ignored — matches the
//! HarmonyOS semantics from the v0.5.405 entry.

use std::collections::HashMap;
use std::sync::Mutex;

static TEXT_IDS: Mutex<Option<HashMap<String, i64>>> = Mutex::new(None);

fn with_registry<F, R>(f: F) -> R
where
    F: FnOnce(&mut HashMap<String, i64>) -> R,
{
    let mut guard = TEXT_IDS.lock().expect("TEXT_IDS poisoned");
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    f(guard.as_mut().unwrap())
}

/// Cross-platform handler entry point. Registered with
/// `js_register_text_id_handler` at app startup.
pub extern "C" fn register_text_id_handler(widget_handle: i64, id_ptr: *const u8, id_len: usize) {
    if id_ptr.is_null() || id_len == 0 {
        return;
    }
    let id = unsafe {
        let bytes = std::slice::from_raw_parts(id_ptr, id_len);
        String::from_utf8_lossy(bytes).into_owned()
    };
    with_registry(|map| {
        map.insert(id, widget_handle);
    });
}

/// Cross-platform setText handler. Looks up `id` and updates the
/// matching widget's text via `widgets::text::set_text_str`.
pub extern "C" fn set_text_handler(
    id_ptr: *const u8,
    id_len: usize,
    val_ptr: *const u8,
    val_len: usize,
) {
    if id_ptr.is_null() || id_len == 0 {
        return;
    }
    let id = unsafe {
        let bytes = std::slice::from_raw_parts(id_ptr, id_len);
        String::from_utf8_lossy(bytes).into_owned()
    };
    let val = if val_ptr.is_null() {
        String::new()
    } else {
        unsafe {
            let bytes = std::slice::from_raw_parts(val_ptr, val_len);
            String::from_utf8_lossy(bytes).into_owned()
        }
    };
    let handle = with_registry(|map| map.get(&id).copied());
    let Some(handle) = handle else {
        // Unregistered id — silently ignore. Matches harmonyos semantics
        // (the v0.5.405 entry covers the equivalent behavior on ArkUI).
        return;
    };
    super::text::set_text_str(handle, &val);
}
