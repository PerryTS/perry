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
    fn js_is_truthy(value: f64) -> i32;
    fn js_node_http_response_state_get(owner: f64) -> f64;
    fn js_node_http_response_state_set(owner: f64, state: f64);
    fn js_array_from_jsvalue(elements: *const u64, count: u32) -> *mut perry_ffi::ArrayHeader;
    fn js_node_http_server_response_birth(proto: f64, req: f64) -> f64;
    fn js_node_http_server_response_init(this: f64, req: f64);
}
fn value(handle: i64) -> f64 {
    f64::from_bits(perry_ffi::JsValue::from_object_ptr(handle as usize as *mut u8).bits())
}
/// Is `handle` a heap object carrying a response payload (open or closed):
/// a direct `new ServerResponse(req)` or the `this` of
/// `ServerResponse.call(this, req)` / `super(req)`.
pub(crate) fn is_response(handle: i64) -> bool {
    (handle as usize) >= perry_ffi::RECEIVER_HANDLE_FLOOR
        && payload::has_attached(value(handle), &VTABLE)
}
pub(crate) unsafe fn state(handle: i64) -> Option<&'static mut ResponseState> {
    if (handle as usize) < perry_ffi::RECEIVER_HANDLE_FLOOR {
        return None;
    }
    payload::get_attached(value(handle), &VTABLE)
}
/// `new ServerResponse(req)`: born on node's response layout with node's
/// constructor fields (`js_node_http_server_response_birth`), then the
/// native state attached.
pub(crate) unsafe fn alloc(state: ResponseState, req: f64) -> i64 {
    let scope = TransientRootScope::enter();
    let req = scope.root_nanbox(req);
    let proto = payload::prototype(&FAMILY, "http");
    let owner = scope.root_nanbox(js_node_http_server_response_birth(proto, req.get()));
    let bytes = retained_bytes(&state);
    if !payload::attach(owner.get(), &FAMILY, state, bytes) {
        return 0;
    }
    (owner.get().to_bits() & PTR_MASK) as i64
}
/// `ServerResponse.call(this, req)` / `super(req)`: node's constructor runs
/// on `this`, so the native state is attached to `this` itself and node's
/// fields are stored on it.
pub(crate) unsafe fn attach_to(this: f64, state: ResponseState, req: f64) -> bool {
    let scope = TransientRootScope::enter();
    let this = scope.root_nanbox(this);
    let req = scope.root_nanbox(req);
    let bytes = retained_bytes(&state);
    if !payload::attach(this.get(), &FAMILY, state, bytes) {
        return false;
    }
    js_node_http_server_response_init(this.get(), req.get());
    true
}
fn retained_bytes(s: &ResponseState) -> usize {
    std::mem::size_of::<ResponseState>()
        + s.buffered_body.capacity()
        + s.header_fields.as_ref().map_or(0, |f| f.retained_bytes())
        + s.committed_headers.as_ref().map_or(0, |headers| {
            headers.capacity() * std::mem::size_of::<(String, String)>()
                + headers
                    .iter()
                    .map(|(k, v)| k.capacity() + v.capacity())
                    .sum::<usize>()
        })
        + s.native_headers
            .iter()
            .map(|(k, v)| k.capacity() + v.capacity())
            .sum::<usize>()
        + s.native_header_value_lists
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
/// Fixed JS state slots in a private traced record: the edges only the
/// native side holds. node's own fields live on the response object.
#[derive(Clone, Copy)]
#[repr(u32)]
pub(crate) enum Key {
    Socket,
    WriteCallbacks,
    /// `end(cb)` callbacks: run when the response finishes, or at its
    /// `close` when it never does (node's `kEndCallbacks`).
    EndCallbacks,
    /// Lowercase header directory retained when the native payload closes.
    HeaderNames,
}
pub(crate) fn js_state(handle: i64) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let old = unsafe { js_node_http_response_state_get(owner.get()) };
    if old.to_bits() != TAG_UNDEFINED {
        return old;
    }
    let empty = [TAG_UNDEFINED; Key::HeaderNames as usize + 1];
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
/// Header values occupy the variable tail after the fixed private fields.
pub(crate) const HEADER_VALUES_START: u32 = Key::HeaderNames as u32 + 1;
pub(crate) fn existing_state(handle: i64) -> f64 {
    unsafe { js_node_http_response_state_get(value(handle)) }
}
pub(crate) fn store_state(handle: i64, state: f64) {
    unsafe { js_node_http_response_state_set(value(handle), state) };
}
/// Read the private array without creating it. This only reads data slots;
/// callers have already classified and rooted the response.
pub(crate) fn get_existing(handle: i64, key: Key) -> f64 {
    let state = existing_state(handle);
    if state.to_bits() == TAG_UNDEFINED {
        return state;
    }
    unsafe {
        f64::from_bits(
            perry_ffi::js_array_get(JsValue::from_bits(state.to_bits()).as_pointer(), key as u32)
                .bits(),
        )
    }
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
/// Keep the headers readable after the native memory is released (node keeps
/// them on the object); the object and its fields survive explicit release,
/// sweep is the backstop.
pub(crate) fn close(handle: i64) {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    super::response::header_values::close_headers((owner.get().to_bits() & PTR_MASK) as i64);
    unsafe { payload::close_attached(owner.get(), &VTABLE) };
}
fn closed(handle: i64) -> bool {
    is_response(handle) && unsafe { state(handle) }.is_none()
}

// ---------------------------------------------------------------------------
// A heap response's node fields. `new ServerResponse(req)` and the `this` of
// `ServerResponse.call(this, req)` carry node's instance fields themselves;
// the native side writes them where node does (`writeHead` -> `statusMessage`,
// `statusCode`, `_header`; the flush -> `_headerSent`; `end` -> `finished`;
// `destroy` -> `destroyed`) and reads the ones a program may write
// (`statusCode`, `statusMessage`, `sendDate`, `strictContentLength`) when the
// head is rendered, as node's `_implicitHeader`/`_storeHeader` do. Events go
// through the object's own `emit`.
// ---------------------------------------------------------------------------

/// `value[name]` of a heap response, `None` for a live server's numeric handle.
pub(crate) fn field(handle: i64, name: &'static [u8]) -> Option<f64> {
    is_response(handle).then(|| unsafe { perry_ffi::runtime_get(value(handle), name) })
}
/// [`field`] once the response's native memory is released.
pub(crate) fn closed_field(handle: i64, name: &'static [u8]) -> Option<f64> {
    if closed(handle) {
        field(handle, name)
    } else {
        None
    }
}
/// `value[name] = v` on a heap response; false for a numeric handle.
pub(crate) fn set_field(handle: i64, name: &'static [u8], v: f64) -> bool {
    if !is_response(handle) {
        return false;
    }
    unsafe { perry_ffi::runtime_set(value(handle), name, v) };
    true
}
pub(crate) fn truthy(v: f64) -> bool {
    unsafe { js_is_truthy(v) != 0 }
}

fn string_value(text: &str) -> f64 {
    f64::from_bits(JsValue::from_string_ptr(perry_ffi::alloc_string(text).as_raw()).bits())
}
fn bool_value(b: bool) -> f64 {
    f64::from_bits(JsValue::from_bool(b).bits())
}
/// `res.on(event, cb)` / `res.once(...)` reached natively on a heap response:
/// node's emitter method of the object itself.
pub(crate) fn add_listener(handle: i64, method: &'static [u8], event: &str, callback: i64) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let cb = roots.root_nanbox(value(callback));
    let event = roots.root_nanbox(string_value(event));
    unsafe { perry_ffi::runtime_call(owner.get(), method, &[event.get(), cb.get()]) };
    owner.get()
}
/// `this.emit(event)`, as node's response does.
pub(crate) fn emit(handle: i64, event: &str) {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let event = roots.root_nanbox(string_value(event));
    unsafe { perry_ffi::runtime_call(owner.get(), b"emit", &[event.get()]) };
}
/// The fields a program may have written, read into the native state before
/// the head is rendered (node reads them in `_implicitHeader` and
/// `_storeHeader`).
pub(crate) fn sync_head_fields(handle: i64) {
    if !is_response(handle) {
        return;
    }
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let get = |name: &'static [u8]| unsafe { perry_ffi::runtime_get(owner.get(), name) };
    let code = JsValue::from_bits(get(b"statusCode").to_bits()).to_number();
    let message = JsValue::from_bits(get(b"statusMessage").to_bits()).to_owned_string();
    let send_date = truthy(get(b"sendDate"));
    let strict = truthy(get(b"strictContentLength"));
    if let Some(s) = unsafe { state((owner.get().to_bits() & PTR_MASK) as i64) } {
        if code.is_finite() && code > 0.0 && !s.header_committed {
            s.status_code = code as u16;
        }
        if !s.header_committed {
            s.status_message = message;
        }
        s.send_date = send_date;
        s.strict_content_length = strict;
    }
}
/// Explicit writeHead supplies the status and reads its current message in
/// write_head_fields. Only the rendering options need synchronizing here.
pub(crate) fn sync_head_options(handle: i64) {
    if !is_response(handle) {
        return;
    }
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let send_date = truthy(unsafe { perry_ffi::runtime_get(owner.get(), b"sendDate") });
    let strict = truthy(unsafe { perry_ffi::runtime_get(owner.get(), b"strictContentLength") });
    if let Some(s) = unsafe { state((owner.get().to_bits() & PTR_MASK) as i64) } {
        s.send_date = send_date;
        s.strict_content_length = strict;
    }
}
/// node's `writeHead(statusCode[, reason])` field writes: `statusMessage`
/// (the reason, or the current message, or the status text), then
/// `statusCode`.
pub(crate) fn write_head_fields(handle: i64, code: u16, reason: Option<String>) {
    if !is_response(handle) {
        return;
    }
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let message = match reason {
        Some(reason) => reason,
        None => {
            let current = unsafe { perry_ffi::runtime_get(owner.get(), b"statusMessage") };
            match JsValue::from_bits(current.to_bits()).to_owned_string() {
                Some(m) if !m.is_empty() => m,
                _ => http::StatusCode::from_u16(code)
                    .ok()
                    .and_then(|s| s.canonical_reason())
                    .unwrap_or("unknown")
                    .to_string(),
            }
        }
    };
    let message_value = roots.root_nanbox(string_value(&message));
    let h = (owner.get().to_bits() & PTR_MASK) as i64;
    set_field(h, b"statusMessage", message_value.get());
    set_field(h, b"statusCode", code as f64);
    if let Some(s) = unsafe { state(h) } {
        s.status_code = code;
        s.status_message = Some(message);
    }
}
/// `this._header = head`: the head the response renders (node's
/// `_storeHeader`).
pub(crate) fn set_header_text(handle: i64, head: &[u8]) {
    if !is_response(handle) {
        return;
    }
    let roots = TransientRootScope::enter();
    let text = roots.root_nanbox(string_value(&String::from_utf8_lossy(head)));
    set_field(handle, b"_header", text.get());
}
/// The fields node's `end` and first `_send` write.
pub(crate) fn mark_ended(handle: i64) {
    if !is_response(handle) {
        return;
    }
    set_field(handle, b"_headerSent", bool_value(true));
    set_field(handle, b"finished", bool_value(true));
}
/// node's `OutgoingMessage.destroy`: `destroyed`, then the socket's destroy
/// (whose `close` the response relays, see [`relay_socket_close`]).
pub(crate) fn destroy(handle: i64) -> bool {
    if !is_response(handle) {
        return false;
    }
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let h = |o: f64| (o.to_bits() & PTR_MASK) as i64;
    if field(h(owner.get()), b"destroyed").is_some_and(truthy) {
        return true;
    }
    set_field(h(owner.get()), b"destroyed", bool_value(true));
    let socket = roots.root_nanbox(get(h(owner.get()), Key::Socket));
    if JsValue::from_bits(socket.get().to_bits()).is_pointer() {
        unsafe { perry_ffi::runtime_call(socket.get(), b"destroy", &[]) };
    }
    true
}
extern "C" fn socket_close_relay(
    closure: *const perry_ffi::RawClosureHeader,
    _this: perry_ffi::JsThis,
) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(unsafe { perry_ffi::closure_capture_f64(closure.cast(), 0) });
    let h = (owner.get().to_bits() & PTR_MASK) as i64;
    set_field(h, b"_closed", bool_value(true));
    emit(h, "close");
    run_end_callbacks((owner.get().to_bits() & PTR_MASK) as i64);
    f64::from_bits(TAG_UNDEFINED)
}
fn run_end_callbacks(handle: i64) {
    let roots = TransientRootScope::enter();
    let cbs = roots.root_addrs(&callbacks(handle, Key::EndCallbacks, true));
    for cb in &cbs {
        super::response_end::call_closure0(cb.get());
    }
}
/// node's `assignSocket`: the socket's `close` becomes the response's
/// (`onServerResponseClose`).
pub(crate) fn relay_socket_close(handle: i64, socket: f64) {
    if !is_response(handle) || !JsValue::from_bits(socket.to_bits()).is_pointer() {
        return;
    }
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let socket = roots.root_nanbox(socket);
    let relay = perry_ffi::alloc_closure(perry_ffi::js_function_info!(socket_close_relay, 0), 1);
    unsafe { perry_ffi::set_closure_capture_f64(relay, 0, owner.get()) };
    let relay = roots.root_nanbox(f64::from_bits(JsValue::from_object_ptr(relay).bits()));
    let event = roots.root_nanbox(string_value("close"));
    unsafe { perry_ffi::runtime_call(socket.get(), b"on", &[event.get(), relay.get()]) };
}
extern "C" fn flushed(
    closure: *const perry_ffi::RawClosureHeader,
    _this: perry_ffi::JsThis,
) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(unsafe { perry_ffi::closure_capture_f64(closure.cast(), 0) });
    let h = || (owner.get().to_bits() & PTR_MASK) as i64;
    // A response destroyed before its socket took the last chunk never
    // finishes; its callbacks run at its `close`.
    if field(h(), b"destroyed").is_some_and(truthy) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    run_end_callbacks(h());
    emit(h(), "finish");
    f64::from_bits(TAG_UNDEFINED)
}
/// The heap response's final socket write: node finishes (the queued write
/// callbacks, the end callback, then `finish`) when the socket has taken the
/// last chunk, and a destroyed response runs those callbacks at its `close`.
pub(crate) fn finish_on_flush(handle: i64, socket: f64, write_cbs: &[i64], end_cb: i64) {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let socket = roots.root_nanbox(socket);
    let pending = roots.root_addrs(write_cbs);
    for cb in &pending {
        push(
            (owner.get().to_bits() & PTR_MASK) as i64,
            Key::EndCallbacks,
            cb.get(),
        );
    }
    if end_cb != 0 {
        push(
            (owner.get().to_bits() & PTR_MASK) as i64,
            Key::EndCallbacks,
            end_cb,
        );
    }
    let done = perry_ffi::alloc_closure(perry_ffi::js_function_info!(flushed, 0), 1);
    unsafe { perry_ffi::set_closure_capture_f64(done, 0, owner.get()) };
    let done = roots.root_nanbox(f64::from_bits(JsValue::from_object_ptr(done).bits()));
    let empty = roots.root_nanbox(string_value(""));
    unsafe { perry_ffi::runtime_call(socket.get(), b"write", &[empty.get(), done.get()]) };
}

#[cfg(test)]
#[path = "response_payload_tests.rs"]
mod tests;
