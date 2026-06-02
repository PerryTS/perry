//! iOS text-id → widget-handle registry for `setText(id, value)`
//! (Phase 2 v3.3). Direct port of the macOS counterpart; UILabel.text
//! is the UIKit analogue of NSTextField.setStringValue:.

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

/// Cross-platform handler registered with `js_register_text_id_handler`
/// at app startup.
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

/// Cross-platform setText handler. Looks up `id` and calls
/// `widgets::text::set_text_str` to update the UILabel via `setText:`.
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
        return;
    };
    super::text::set_text_str(handle, &val);
}
