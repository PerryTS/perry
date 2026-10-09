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
extern "C" {
    fn js_node_http_response_state_get(owner: f64) -> f64;
    fn js_node_http_response_state_set(owner: f64, state: f64);
    fn js_array_from_jsvalue(elements: *const u64, count: u32) -> *mut perry_ffi::ArrayHeader;
}
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
/// Fixed JS state slots in a private traced record.
#[derive(Clone, Copy)]
#[repr(u32)]
pub(crate) enum Key {
    Socket,
    WriteCallbacks,
    StatusCode,
    HeadersSent,
    WritableEnded,
    WritableFinished,
    Finished,
    Destroyed,
    SendDate,
    StrictContentLength,
    Headers,
    StatusMessage,
    Events,
    #[cfg(test)]
    ListenerCalls,
}
fn js_state(handle: i64) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let old = unsafe { js_node_http_response_state_get(owner.get()) };
    if old.to_bits() != TAG_UNDEFINED {
        return old;
    }
    let empty = [TAG_UNDEFINED; Key::Events as usize + 1 + cfg!(test) as usize];
    let state = unsafe { js_array_from_jsvalue(empty.as_ptr(), empty.len() as u32) };
    let state = roots.root_nanbox(f64::from_bits(
        JsValue::from_object_ptr(state.cast::<u8>()).bits(),
    ));
    #[cfg(test)]
    if std::env::var_os("PERRY_TEST_HTTP_STATE_BY_NAME").is_some() {
        payload::own(owner.get(), "#<perry:native-payload-js-state>", state.get());
        return state.get();
    }
    unsafe { js_node_http_response_state_set(owner.get(), state.get()) };
    state.get()
}
pub(crate) fn get(handle: i64, key: Key) -> f64 {
    let state = js_state(handle);
    unsafe {
        f64::from_bits(
            perry_ffi::js_array_get(
                (state.to_bits() & PTR_MASK) as *const perry_ffi::ArrayHeader,
                key as u32,
            )
            .bits(),
        )
    }
}
pub(crate) fn set(handle: i64, key: Key, v: f64) {
    let roots = TransientRootScope::enter();
    let v = roots.root_nanbox(v);
    let state = js_state(handle);
    unsafe {
        perry_ffi::js_array_set(
            (state.to_bits() & PTR_MASK) as *mut perry_ffi::ArrayHeader,
            key as u32,
            JsValue::from_bits(v.get().to_bits()),
        )
    };
}
pub(crate) fn push(handle: i64, key: Key, callback: i64) {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let cb = roots.root_nanbox(value(callback));
    let arr = append_callback(get(handle, key), cb.get());
    set((owner.get().to_bits() & PTR_MASK) as i64, key, arr);
}
fn append_callback(old: f64, cb: f64) -> f64 {
    let roots = TransientRootScope::enter();
    let cb = roots.root_nanbox(cb);
    let arr = if old.to_bits() == TAG_UNDEFINED {
        unsafe { perry_ffi::js_array_alloc(0) }
    } else {
        (old.to_bits() & PTR_MASK) as *mut perry_ffi::ArrayHeader
    };
    let arr = unsafe { perry_ffi::js_array_push(arr, JsValue::from_bits(cb.get().to_bits())) };
    f64::from_bits(JsValue::from_object_ptr(arr.cast::<u8>()).bits())
}
fn callback_values(arr: f64) -> Vec<i64> {
    if arr.to_bits() == TAG_UNDEFINED {
        return Vec::new();
    }
    let p = (arr.to_bits() & PTR_MASK) as *const perry_ffi::ArrayHeader;
    unsafe {
        (0..perry_ffi::js_array_length(p))
            .map(|i| (perry_ffi::js_array_get(p, i).bits() & PTR_MASK) as i64)
            .collect()
    }
}
pub(crate) fn callbacks(handle: i64, key: Key, take: bool) -> Vec<i64> {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let arr = roots.root_nanbox(get(handle, key));
    if take {
        set(
            (owner.get().to_bits() & PTR_MASK) as i64,
            key,
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    callback_values(arr.get())
}
// Event names are user input. Their callback arrays live in an ordinary
// null-prototype dictionary, reached through the fixed Events slot.
fn events(handle: i64) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let old = get(handle, Key::Events);
    if old.to_bits() != TAG_UNDEFINED {
        return old;
    }
    let events = perry_ffi::alloc_null_proto_object(&[]);
    let events = roots.root_nanbox(f64::from_bits(events.bits()));
    set(
        (owner.get().to_bits() & PTR_MASK) as i64,
        Key::Events,
        events.get(),
    );
    events.get()
}
pub(crate) fn push_event(handle: i64, key: &str, callback: i64) {
    let roots = TransientRootScope::enter();
    let cb = roots.root_nanbox(value(callback));
    let events = roots.root_nanbox(events(handle));
    let old = perry_ffi::object_field_by_name(JsValue::from_bits(events.get().to_bits()), key);
    let arr = append_callback(f64::from_bits(old.bits()), cb.get());
    payload::own(events.get(), key, arr);
}
fn event_callbacks(handle: i64, key: &str, take: bool) -> Vec<i64> {
    let roots = TransientRootScope::enter();
    let events = roots.root_nanbox(events(handle));
    let arr = perry_ffi::object_field_by_name(JsValue::from_bits(events.get().to_bits()), key);
    let arr = roots.root_nanbox(f64::from_bits(arr.bits()));
    if take {
        payload::own(events.get(), key, f64::from_bits(TAG_UNDEFINED));
    }
    callback_values(arr.get())
}
pub(crate) fn listeners(handle: i64, event: &str) -> Vec<i64> {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let on = event_callbacks(handle, &format!("on:{event}"), false);
    let on = roots.root_addrs(&on);
    let mut once = event_callbacks(
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
        (Key::StatusCode, s.status_code as f64),
        (Key::HeadersSent, boolean(s.headers_sent)),
        (Key::WritableEnded, boolean(s.writable_ended)),
        (Key::WritableFinished, boolean(s.writable_finished)),
        (Key::Finished, boolean(s.writable_ended)),
        (Key::Destroyed, boolean(s.destroyed)),
        (Key::SendDate, boolean(s.send_date)),
        (Key::StrictContentLength, boolean(s.strict_content_length)),
    ];
    let message = s.status_message.clone();
    for (key, v) in props {
        set((owner.get().to_bits() & PTR_MASK) as i64, key, v);
    }
    let h = perry_ffi::alloc_string(&headers);
    set(
        (owner.get().to_bits() & PTR_MASK) as i64,
        Key::Headers,
        f64::from_bits(JsValue::from_string_ptr(h.as_raw()).bits()),
    );
    if let Some(m) = message {
        let m = perry_ffi::alloc_string(&m);
        set(
            (owner.get().to_bits() & PTR_MASK) as i64,
            Key::StatusMessage,
            f64::from_bits(JsValue::from_string_ptr(m.as_raw()).bits()),
        );
    }
    payload::close(owner.get(), &FAMILY);
}
fn boolean(v: bool) -> f64 {
    f64::from_bits(JsValue::from_bool(v).bits())
}
pub(crate) fn closed_property(handle: i64, key: Key) -> Option<f64> {
    (is_response(handle) && unsafe { state(handle) }.is_none()).then(|| get(handle, key))
}
pub(crate) fn set_closed_property(handle: i64, key: Key, value: f64) -> bool {
    if is_response(handle) && unsafe { state(handle) }.is_none() {
        set(handle, key, value);
        true
    } else {
        false
    }
}
pub(crate) fn closed_headers(handle: i64) -> Option<std::collections::HashMap<String, String>> {
    let json = closed_property(handle, Key::Headers)?;
    let json = JsValue::from_bits(json.to_bits()).to_owned_string()?;
    serde_json::from_str(&json).ok()
}

#[cfg(test)]
#[path = "response_payload_tests.rs"]
mod tests;
