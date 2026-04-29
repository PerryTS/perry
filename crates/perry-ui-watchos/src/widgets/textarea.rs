//! TextArea widget for watchOS.
//!
//! Uses the data-driven tree model: creates a `NodeKind::TextArea` node with
//! placeholder text and an on-change closure. The Swift side renders this as a
//! SwiftUI `TextEditor` and calls `perry_watchos_textarea_changed` when the
//! user edits the content.

use crate::cstring_from_header;
use crate::tree::{self, NodeData, NodeKind};

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
}

/// Create a TextArea node with a placeholder string and an on-change callback.
/// Returns a 1-based widget handle.
pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    let mut node = NodeData::new(NodeKind::TextArea);
    node.text = cstring_from_header(placeholder_ptr);
    node.on_change_closure = Some(on_change);
    tree::register_node(node)
}

/// Set the text content of a TextArea node.
pub fn set_string(handle: i64, text_ptr: *const u8) {
    let text = cstring_from_header(text_ptr);
    tree::with_node_mut(handle, |node| {
        node.text = text;
    });
}

/// Get the text content of a TextArea node as a `StringHeader` pointer.
///
/// Returns a pointer allocated via `js_string_from_bytes` — the runtime owns
/// the memory and the GC will collect it.
pub fn get_string(handle: i64) -> *const u8 {
    let text = tree::with_node(handle, |node| {
        node.text.as_ref().map(|cs| {
            let bytes = cs.as_bytes();
            (bytes.as_ptr(), bytes.len())
        })
    })
    .flatten();

    if let Some((ptr, len)) = text {
        unsafe { js_string_from_bytes(ptr, len as i64) }
    } else {
        unsafe { js_string_from_bytes(std::ptr::null(), 0) }
    }
}
