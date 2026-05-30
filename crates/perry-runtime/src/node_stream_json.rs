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
    } else if value.fract() == 0.0 && value.abs() < (i64::MAX as f64) {
        let mut itoa_buf = itoa::Buffer::new();
        buf.push_str(itoa_buf.format(value as i64));
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
    let readable = own_field_by_key_bytes(obj, READABLE_FLAG_KEY).is_some();
    let writable = own_field_by_key_bytes(obj, WRITABLE_FLAG_KEY).is_some();
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
