use super::*;

/// Record that a consumer received a chunk — Node's
/// `_readableState.dataEmitted`, which `readableDidRead` returns and
/// `stream.isDisturbed()` reads (#1534). Only consumer-side delivery sets it:
/// a `'data'` emission or a `read()` that returns data. Producer-side `push()`
/// / `unshift()` and a bare `resume()` / `pipe()` / iterator attach do not
/// (#11212): undici's `body.text()` rejects a body it only pushed into as
/// "unusable" when this lies.
pub(in super::super) fn mark_disturbed(stream: f64) {
    set_hidden_value(stream, hidden_disturbed_key(), f64::from_bits(TAG_TRUE));
    set_visible_readable_did_read(stream, true);
}

pub(in super::super) unsafe fn own_field_by_key_bytes(
    obj: *const ObjectHeader,
    key: &[u8],
) -> Option<f64> {
    if obj.is_null() {
        return None;
    }
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
    if key_count > 65_536 {
        return None;
    }
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if string_value_eq(f64::from_bits(key_val.bits()), key) {
            let value = crate::object::key_attrs::object_slot_data(obj, i as u32);
            return if value.bits() == TAG_UNDEFINED {
                None
            } else {
                Some(f64::from_bits(value.bits()))
            };
        }
    }
    None
}

/// Canonical trie keys share the runtime's existing atom and GC machinery.
/// Taking a key cannot collect: callers may already hold an unrooted receiver.
pub(in super::super) fn hidden_key(bytes: &'static [u8]) -> *mut crate::string::StringHeader {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
    #[cfg(test)]
    if std::env::var_os("PERRY_TEST_STREAM_LITERAL_KEY").is_some() {
        return crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    }
    // The canonical-keys trie adopts these same atoms (Appended::atomized).
    // Resolve text identity at its source; traversing a singleton trie list
    // here would add the trie's edge HashMap probe to every construction.
    crate::string::js_string_pool_atom(bytes.as_ptr(), bytes.len() as u32, hash, 0)
}

#[cfg(test)]
pub(crate) fn hidden_key_for_test(bytes: &'static [u8]) -> *mut crate::string::StringHeader {
    hidden_key(bytes)
}

pub(in super::super) fn string_value_eq(value: f64, expected: &[u8]) -> bool {
    // SSO-aware byte compare (heap strings AND inline short strings) with no
    // heap materialization. The previous implementation routed SSO values
    // through `js_get_string_pointer_unified`, which allocates a heap copy of
    // the inline bytes per call — a fresh allocation for every SSO key
    // compared, on every object `JSON.stringify` probes for the stream flag
    // keys (#6009).
    let jsval = JSValue::from_bits(value.to_bits());
    unsafe { crate::string::js_string_key_matches_bytes(jsval, expected) }
}

pub(in super::super) fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let raw = raw_ptr_from_value(value);
    // The handle band (EventEmitter ids sit at 0x38000..0x40000,
    // widget/stream handles lower) is never a heap object. The old 0x10000
    // floor let an EventEmitter handle through to the GcHeader probe at
    // raw-8, which is unmapped memory (#4633 SIGSEGV in
    // events.on(emitter, name, { signal }) target validation).
    if crate::value::addr_class::is_handle_band(raw) || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_OBJECT) {
            return None;
        }
    }
    Some(raw as *mut ObjectHeader)
}
