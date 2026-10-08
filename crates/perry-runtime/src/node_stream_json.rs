//! node:stream — JSON serialization of Readable/Writable stub objects. Split
//! out of node_stream_readwrite.rs for the 2000-line file-size gate (#1987).
//! Shares the parent module's constants, hidden-key accessors and state
//! primitives via `use super::*`.
use super::*;
use crate::object::ObjectHeader;

pub(super) fn push_json_number(buf: &mut String, value: f64) {
    if value.is_nan() || value.is_infinite() {
        buf.push_str("null");
    } else if value.fract() == 0.0 && value.abs() < crate::builtins::INT_EXACT_FASTPATH_LIMIT {
        let mut itoa_buf = itoa::Buffer::new();
        buf.push_str(itoa_buf.format(value as i64));
    } else if value.fract() == 0.0 {
        // #6127: a large integer (`>= 2^53`) must print its shortest round-trip
        // decimal in POSITIONAL notation per ECMAScript Number::toString, not the
        // exact integer and not `ryu`'s scientific form (`2.88…e17`). The shared
        // JS formatter handles the exponent thresholds (`1e21` → `1e+21`).
        buf.push_str(&crate::string::js_format_f64(value));
    } else {
        let mut ryu_buf = ryu::Buffer::new();
        buf.push_str(ryu_buf.format(value));
    }
}

pub(crate) unsafe fn try_stringify_node_stream_json(ptr: *const u8, buf: &mut String) -> bool {
    if ptr.is_null() {
        return false;
    }
    let obj = ptr as *const ObjectHeader;
    // This probe runs for EVERY object `JSON.stringify` serializes, so it
    // must stay cheap (#6009). A stream answers from its state record (a few
    // loads; an ordinary object has none). A state view is found by its one
    // internal key, in one raw pass over the dense keys array.
    let value = crate::value::js_nanbox_pointer(obj as i64);
    let readable = read_slot(value, READABLE_FLAG_KEY).is_some();
    let writable = read_slot(value, WRITABLE_FLAG_KEY).is_some();
    if !readable && !writable {
        // #11197: a `_readableState` / `_writableState` view serializes as
        // Node's state object does, and never walks its back-pointer to the
        // stream (which would be a cycle: stream → view → stream).
        return match state_view_owner_slot(obj) {
            Some(i) => {
                stringify_state_view_json(obj, crate::object::js_object_get_field(obj, i), buf)
            }
            None => false,
        };
    }
    if readable == writable {
        return false;
    }

    buf.push_str(r#"{"_events":{},"#);
    if readable {
        buf.push_str(r#""_readableState":"#);
        push_readable_state_json(obj, buf);
    } else {
        buf.push_str(r#""_writableState":"#);
        push_writable_state_json(obj, buf);
    }
    buf.push('}');
    true
}

unsafe fn state_view_owner_slot(obj: *const ObjectHeader) -> Option<u32> {
    let keys_view = crate::object::object_keys(obj);
    let keys = keys_view.arr();
    let keys_ptr = keys as usize;
    if keys.is_null() || keys_ptr < 0x10000 {
        return None;
    }
    if gc_type_for_ptr(keys_ptr) != Some(crate::gc::GC_TYPE_ARRAY) {
        return None;
    }
    let key_count = keys_view.count() as usize;
    if key_count > 65_536 || key_count > (*keys).capacity as usize {
        return None;
    }
    let elements =
        crate::array::array_elements_ptr(keys as *const crate::ArrayHeader) as *const f64;
    (0..key_count)
        .find(|&i| {
            let stored = JSValue::from_bits((*elements.add(i)).to_bits());
            crate::string::js_string_key_matches_bytes(stored, STREAM_STATE_OWNER_KEY)
        })
        .map(|i| i as u32)
}

unsafe fn stringify_state_view_json(
    view: *const ObjectHeader,
    owner: JSValue,
    buf: &mut String,
) -> bool {
    let Some(stream) = object_ptr_from_value(f64::from_bits(owner.bits())) else {
        return false;
    };
    let view_bits = crate::value::js_nanbox_pointer(view as i64).to_bits();
    let is_readable_view = own_field_by_key_bytes(stream, b"_readableState")
        .is_some_and(|value| value.to_bits() == view_bits);
    if is_readable_view {
        push_readable_state_json(stream, buf);
    } else {
        push_writable_state_json(stream, buf);
    }
    true
}

unsafe fn push_readable_state_json(stream: *const ObjectHeader, buf: &mut String) {
    let stream_value = crate::value::js_nanbox_pointer(stream as i64);
    let hwm = read_slot(stream_value, READABLE_HWM_KEY).unwrap_or_else(|| default_hwm(false));
    let length = read_slot(stream_value, READABLE_BUFFERED_KEY).unwrap_or(0.0);
    buf.push_str(r#"{"highWaterMark":"#);
    push_json_number(buf, hwm);
    buf.push_str(r#","buffer":[],"bufferIndex":0,"length":"#);
    push_json_number(buf, length);
    buf.push_str(r#","pipes":[],"awaitDrainWriters":null}"#);
}

unsafe fn push_writable_state_json(stream: *const ObjectHeader, buf: &mut String) {
    let hwm = own_field_by_key_bytes(stream, b"writableHighWaterMark")
        .unwrap_or_else(|| default_hwm(false));
    let length = 0.0;
    let corked = read_slot(crate::value::js_nanbox_pointer(stream as i64), WRITABLE_CORKED_KEY)
        .unwrap_or(0.0);
    buf.push_str(r#"{"highWaterMark":"#);
    push_json_number(buf, hwm);
    buf.push_str(r#","length":"#);
    push_json_number(buf, length);
    buf.push_str(r#","corked":"#);
    push_json_number(buf, corked);
    buf.push_str(r#","writelen":0,"bufferedIndex":0,"pendingcb":0}"#);
}
