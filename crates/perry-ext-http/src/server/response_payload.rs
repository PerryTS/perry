//! Standalone responses are ordinary objects; only Rust response data lives
//! in their payload. The hidden JS state owns sockets and callback arrays.
use super::response::ResponseState;
use super::types::{PTR_MASK, TAG_UNDEFINED};
use perry_ffi::{native_payload as payload, native_stream, JsValue, TransientRootScope};
static VTABLE: native_stream::PayloadVTable = native_stream::payload_vtable::<ResponseState>(None);
static FAMILY: payload::PayloadFamily = payload::PayloadFamily::new::<ResponseState>(
    perry_ffi::native_class_ids::HTTP_SERVER_RESPONSE,
    "ServerResponse",
    false,
    &VTABLE,
);
const JS_STATE: &str = "#<perry:native-payload-js-state>";
fn value(handle: i64) -> f64 {
    f64::from_bits(0x7FFD_0000_0000_0000 | (handle as u64 & PTR_MASK))
}
pub(crate) fn is_response(handle: i64) -> bool {
    if (handle as usize) < perry_ffi::RECEIVER_HANDLE_FLOOR {
        return false;
    }
    unsafe {
        (*(handle as *const perry_ffi::ObjectHeader)).class_id
            == perry_ffi::native_class_ids::HTTP_SERVER_RESPONSE
    }
}
pub(crate) unsafe fn state(handle: i64) -> Option<&'static mut ResponseState> {
    payload::get_mut(value(handle), &FAMILY)
}
pub(crate) unsafe fn alloc(state: ResponseState) -> i64 {
    let proto = payload::prototype(&FAMILY, "http");
    let bytes = retained_bytes(&state);
    let owner = payload::alloc(&FAMILY, state, proto, bytes);
    (owner.to_bits() & PTR_MASK) as i64
}
fn retained_bytes(s: &ResponseState) -> usize {
    std::mem::size_of::<ResponseState>()
        + s.buffered_body.capacity()
        + s.headers
            .iter()
            .map(|(k, v)| k.capacity() + v.capacity())
            .sum::<usize>()
        + s.header_value_lists
            .iter()
            .map(|(k, v)| {
                k.capacity()
                    + v.capacity() * std::mem::size_of::<String>()
                    + v.iter().map(String::capacity).sum::<usize>()
            })
            .sum::<usize>()
        + s.raw_header_names
            .iter()
            .map(|(k, v)| k.capacity() + v.capacity())
            .sum::<usize>()
        + s.header_order.capacity() * std::mem::size_of::<String>()
        + s.header_order.iter().map(String::capacity).sum::<usize>()
        + s.trailers
            .iter()
            .map(|(k, v)| k.capacity() + v.capacity())
            .sum::<usize>()
        + s.raw_trailer_names
            .iter()
            .map(|(k, v)| k.capacity() + v.capacity())
            .sum::<usize>()
        + s.status_message.as_ref().map_or(0, String::capacity)
        + s.standalone_req_method.as_ref().map_or(0, String::capacity)
}
pub(crate) fn restate(handle: i64) {
    if let Some(s) = unsafe { state(handle) } {
        payload::set_external_bytes(value(handle), &VTABLE, retained_bytes(s));
    }
}
fn js_state(handle: i64) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let old = perry_ffi::object_field_by_name(JsValue::from_bits(owner.get().to_bits()), JS_STATE);
    if !old.is_undefined() {
        return f64::from_bits(old.bits());
    }
    let state = perry_ffi::alloc_null_proto_object(&[]);
    let state = roots.root_nanbox(f64::from_bits(state.bits()));
    payload::own(owner.get(), JS_STATE, state.get());
    state.get()
}
pub(crate) fn get(handle: i64, key: &str) -> f64 {
    f64::from_bits(
        perry_ffi::object_field_by_name(JsValue::from_bits(js_state(handle).to_bits()), key).bits(),
    )
}
pub(crate) fn set(handle: i64, key: &str, v: f64) {
    let roots = TransientRootScope::enter();
    let v = roots.root_nanbox(v);
    let state = js_state(handle);
    payload::own(state, key, v.get());
}
pub(crate) fn push(handle: i64, key: &str, callback: i64) {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let cb = roots.root_nanbox(value(callback));
    let old = get(handle, key);
    let arr = if old.to_bits() == TAG_UNDEFINED {
        unsafe { perry_ffi::js_array_alloc(0) }
    } else {
        (old.to_bits() & PTR_MASK) as *mut perry_ffi::ArrayHeader
    };
    let arr = unsafe { perry_ffi::js_array_push(arr, JsValue::from_bits(cb.get().to_bits())) };
    set(
        (owner.get().to_bits() & PTR_MASK) as i64,
        key,
        f64::from_bits(JsValue::from_object_ptr(arr.cast::<u8>()).bits()),
    );
}
pub(crate) fn callbacks(handle: i64, key: &str, take: bool) -> Vec<i64> {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let arr = get(handle, key);
    if arr.to_bits() == TAG_UNDEFINED {
        return Vec::new();
    }
    let arr = roots.root_nanbox(arr);
    if take {
        set(
            (owner.get().to_bits() & PTR_MASK) as i64,
            key,
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    let p = (arr.get().to_bits() & PTR_MASK) as *const perry_ffi::ArrayHeader;
    unsafe {
        (0..perry_ffi::js_array_length(p))
            .map(|i| (perry_ffi::js_array_get(p, i).bits() & PTR_MASK) as i64)
            .collect()
    }
}
pub(crate) fn listeners(handle: i64, event: &str) -> Vec<i64> {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let on = callbacks(handle, &format!("on:{event}"), false);
    let on = roots.root_addrs(&on);
    let mut once = callbacks(
        (owner.get().to_bits() & PTR_MASK) as i64,
        &format!("once:{event}"),
        true,
    );
    let mut out = on.iter().map(|r| r.get()).collect::<Vec<_>>();
    out.append(&mut once);
    out
}
/// Preserve observable terminal metadata in JS, then release native memory.
/// The object and its JS state survive explicit release; sweep is the backstop.
pub(crate) fn close(handle: i64) {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let Some(s) = (unsafe { state(handle) }) else {
        return;
    };
    let headers = serde_json::to_string(&s.headers).unwrap();
    let props = [
        ("statusCode", s.status_code as f64),
        ("headersSent", boolean(s.headers_sent)),
        ("writableEnded", boolean(s.writable_ended)),
        ("writableFinished", boolean(s.writable_finished)),
        ("finished", boolean(s.writable_ended)),
        ("destroyed", boolean(s.destroyed)),
        ("sendDate", boolean(s.send_date)),
        ("strictContentLength", boolean(s.strict_content_length)),
    ];
    let message = s.status_message.clone();
    for (key, v) in props {
        set((owner.get().to_bits() & PTR_MASK) as i64, key, v);
    }
    let h = perry_ffi::alloc_string(&headers);
    set(
        (owner.get().to_bits() & PTR_MASK) as i64,
        "headers",
        f64::from_bits(JsValue::from_string_ptr(h.as_raw()).bits()),
    );
    if let Some(m) = message {
        let m = perry_ffi::alloc_string(&m);
        set(
            (owner.get().to_bits() & PTR_MASK) as i64,
            "statusMessage",
            f64::from_bits(JsValue::from_string_ptr(m.as_raw()).bits()),
        );
    }
    payload::close(owner.get(), &FAMILY);
}
fn boolean(v: bool) -> f64 {
    f64::from_bits(JsValue::from_bool(v).bits())
}
pub(crate) fn closed_property(handle: i64, key: &str) -> Option<f64> {
    (is_response(handle) && unsafe { state(handle) }.is_none()).then(|| get(handle, key))
}
pub(crate) fn closed_headers(handle: i64) -> Option<std::collections::HashMap<String, String>> {
    let json = closed_property(handle, "headers")?;
    let json = JsValue::from_bits(json.to_bits()).to_owned_string()?;
    serde_json::from_str(&json).ok()
}

#[cfg(test)]
#[path = "response_payload_tests.rs"]
mod tests;
