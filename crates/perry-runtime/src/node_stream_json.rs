//! node:stream — JSON serialization of Readable/Writable stub objects. Split
//! out of node_stream_readwrite.rs for the 2000-line file-size gate (#1987).
//! Shares the parent module's constants, hidden-key accessors and state
//! primitives via `use super::*`.
#![allow(unused_imports)]
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
    // This probe runs for EVERY object `JSON.stringify` serializes, so the
    // flag detection must stay cheap (#6009): one raw pass over the dense
    // keys array for BOTH flag keys, instead of two exported-getter scans
    // (`own_field_by_key_bytes` × 2) whose per-element `js_array_get` +
    // SSO-materializing compare dominated small-object stringify profiles.
    let keys = (*obj).keys_array;
    let keys_ptr = keys as usize;
    if keys.is_null() || keys_ptr < 0x10000 {
        return false;
    }
    if gc_type_for_ptr(keys_ptr) != Some(crate::gc::GC_TYPE_ARRAY) {
        return false;
    }
    let key_count = (*keys).length as usize;
    if key_count > 65_536 || key_count > (*keys).capacity as usize {
        return false;
    }
    let elements = (keys as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let mut readable_idx: Option<u32> = None;
    let mut writable_idx: Option<u32> = None;
    for i in 0..key_count {
        let stored = JSValue::from_bits((*elements.add(i)).to_bits());
        if crate::string::js_string_key_matches_bytes(stored, READABLE_FLAG_KEY) {
            readable_idx = Some(i as u32);
        } else if crate::string::js_string_key_matches_bytes(stored, WRITABLE_FLAG_KEY) {
            writable_idx = Some(i as u32);
        }
    }
    let flag_defined = |idx: Option<u32>| -> bool {
        idx.is_some_and(|i| {
            crate::object::js_object_get_field(obj, i).bits() != crate::value::TAG_UNDEFINED
        })
    };
    let readable = flag_defined(readable_idx);
    let writable = flag_defined(writable_idx);
    if readable == writable {
        return false;
    }

    buf.push_str(r#"{"_events":{},"#);
    if readable {
        let hwm =
            own_field_by_key_bytes(obj, READABLE_HWM_KEY).unwrap_or_else(|| default_hwm(false));
        let length = own_field_by_key_bytes(obj, READABLE_BUFFERED_KEY).unwrap_or(0.0);
        buf.push_str(r#""_readableState":{"highWaterMark":"#);
        push_json_number(buf, hwm);
        buf.push_str(r#","buffer":[],"bufferIndex":0,"length":"#);
        push_json_number(buf, length);
        buf.push_str(r#","pipes":[],"awaitDrainWriters":null}}"#);
    } else {
        let hwm = own_field_by_key_bytes(obj, b"writableHighWaterMark")
            .unwrap_or_else(|| default_hwm(false));
        let length = 0.0;
        let corked = own_field_by_key_bytes(obj, WRITABLE_CORKED_KEY).unwrap_or(0.0);
        buf.push_str(r#""_writableState":{"highWaterMark":"#);
        push_json_number(buf, hwm);
        buf.push_str(r#","length":"#);
        push_json_number(buf, length);
        buf.push_str(r#","corked":"#);
        push_json_number(buf, corked);
        buf.push_str(r#","writelen":0,"bufferedIndex":0,"pendingcb":0}}"#);
    }
    true
}
