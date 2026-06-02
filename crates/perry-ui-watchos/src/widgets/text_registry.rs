//! watchOS text-id → tree-handle registry for `setText(id, value)`.
//!
//! Issue #535 Layer 1: `state(initial)` desugars to `Text(initial, "synth_id")`
//! plus `__state_set("synth_id", v) → js_state_set → perry_arkts_set_text`.
//! On watchOS the SwiftUI host re-evaluates the data tree on every
//! `perry_watchos_tree_version` bump, so updating a node's `text` field via
//! `tree::with_node_mut` is enough to re-render the bound Text view.

use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Mutex;

use crate::tree;

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
    let Ok(cstr) = CString::new(val) else {
        return;
    };
    tree::with_node_mut(handle, |node| {
        node.text = Some(cstr);
    });
}
