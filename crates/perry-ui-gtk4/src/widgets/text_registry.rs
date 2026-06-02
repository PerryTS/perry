use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static TEXT_REGISTRY: RefCell<HashMap<String, i64>> = RefCell::new(HashMap::new());
}

/// Store widget handle under id so setText() can find it later.
pub fn register(id: &str, handle: i64) {
    TEXT_REGISTRY.with(|r| {
        r.borrow_mut().insert(id.to_string(), handle);
    });
}

/// Update the GtkLabel text for a previously registered id.
pub fn set_text_for_id(id: &str, value: &str) {
    let handle = TEXT_REGISTRY.with(|r| r.borrow().get(id).copied());
    if let Some(h) = handle {
        super::text::set_text_str(h, value);
    }
}

/// Cross-platform handler registered with `js_register_text_id_handler`
/// at app startup. Issue #535 Layer 1.
pub extern "C" fn register_text_id_handler(widget_handle: i64, id_ptr: *const u8, id_len: usize) {
    if id_ptr.is_null() || id_len == 0 {
        return;
    }
    let id = unsafe {
        let bytes = std::slice::from_raw_parts(id_ptr, id_len);
        String::from_utf8_lossy(bytes).into_owned()
    };
    register(&id, widget_handle);
}

/// Cross-platform setText handler. Issue #535 Layer 1.
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
    set_text_for_id(&id, &val);
}
