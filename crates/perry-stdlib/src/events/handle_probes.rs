//! Heap-handle probe helpers shared by the EventEmitter dispatch paths.
//!
//! Kept in a sibling module so `events.rs` stays under the 2000-line ceiling.

use super::{Handle, MAX_HEAP_POINTER, MIN_HEAP_POINTER, TAG_NULL_F64_BITS};
use perry_runtime::js_nanbox_pointer;

/// Returns the NaN-boxed value for a node:stream handle, or `None` when the
/// handle is not an 8-byte-aligned heap pointer backing a readable/writable
/// stream.
pub(super) fn stream_value_from_handle(handle: Handle) -> Option<f64> {
    let addr = handle as u64;
    if !(MIN_HEAP_POINTER..=MAX_HEAP_POINTER).contains(&addr) || addr & 0x7 != 0 {
        return None;
    }
    let value = js_nanbox_pointer(handle);
    let readable = perry_runtime::node_stream::js_node_stream_is_readable(value);
    let writable = perry_runtime::node_stream::js_node_stream_is_writable(value);
    if readable.to_bits() == TAG_NULL_F64_BITS && writable.to_bits() == TAG_NULL_F64_BITS {
        None
    } else {
        Some(value)
    }
}
