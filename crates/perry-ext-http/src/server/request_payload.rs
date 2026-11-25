//! An HTTP/1 server request is an ordinary object born on
//! `IncomingMessage.prototype` with node's fields (`url`, `method`,
//! `rawHeaders`, `headers` built once through `_addHeaderLine`); its Rust
//! `IncomingMessage` rides along as the native payload for the transport
//! data no field carries (peer, TLS, the buffered body). The handle the
//! native side passes around is the object pointer.
use super::request::IncomingMessage;
pub(crate) use super::types::PTR_MASK;
use super::types::TAG_UNDEFINED;
use perry_ffi::{native_payload as payload, native_stream, JsValue, TransientRootScope};
static VTABLE: native_stream::PayloadVTable =
    native_stream::payload_vtable::<IncomingMessage>(None);
static FAMILY: payload::PayloadFamily = payload::PayloadFamily::new::<IncomingMessage>(
    perry_ffi::native_class_ids::HTTP_SERVER_INCOMING_MESSAGE,
    "IncomingMessage",
    false,
    &VTABLE,
);
extern "C" {
    fn js_node_http_incoming_message_create(proto: f64) -> f64;
    fn js_node_http_incoming_message_birth(
        this: f64,
        socket: f64,
        raw_headers: f64,
        major: f64,
        minor: f64,
        version: f64,
        url: f64,
        method: f64,
    ) -> f64;
    fn js_node_http_incoming_message_abort_signal(this: f64);
}
pub(crate) fn value(handle: i64) -> f64 {
    f64::from_bits(JsValue::from_object_ptr(handle as usize as *mut u8).bits())
}
/// Is `handle` a heap request carrying its payload (open or closed).
pub(crate) fn is_request(handle: i64) -> bool {
    (handle as usize) >= perry_ffi::RECEIVER_HANDLE_FLOOR
        && payload::has_attached(value(handle), &VTABLE)
}
pub(crate) unsafe fn state(handle: i64) -> Option<&'static mut IncomingMessage> {
    if (handle as usize) < perry_ffi::RECEIVER_HANDLE_FLOOR {
        return None;
    }
    payload::get_attached(value(handle), &VTABLE)
}
fn string_value(text: &str) -> f64 {
    f64::from_bits(JsValue::from_string_ptr(perry_ffi::alloc_string(text).as_raw()).bits())
}
/// node's parser: `new IncomingMessage(socket)`, then the head's fields, then
/// the payload. Returns the object word. Allocates and runs the constructor:
/// the caller holds no borrow and roots what it keeps.
pub(crate) unsafe fn alloc(im: IncomingMessage, socket: f64) -> f64 {
    let roots = TransientRootScope::enter();
    let socket = roots.root_nanbox(socket);
    let mut raw = roots.root_nanbox(f64::from_bits(
        JsValue::from_object_ptr(
            perry_ffi::js_array_alloc((im.raw_headers.len() * 2) as u32).cast::<u8>(),
        )
        .bits(),
    ));
    for (name, v) in &im.raw_headers {
        for text in [name, v] {
            let s = roots.root_nanbox(string_value(text));
            let arr = perry_ffi::js_array_push(
                (raw.get().to_bits() & PTR_MASK) as *mut perry_ffi::ArrayHeader,
                JsValue::from_bits(s.get().to_bits()),
            );
            raw = roots.root_nanbox(f64::from_bits(
                JsValue::from_object_ptr(arr.cast::<u8>()).bits(),
            ));
        }
    }
    let (major, minor) = match im.http_version.split_once('.') {
        Some((a, b)) => (a.parse().unwrap_or(1.0), b.parse().unwrap_or(1.0)),
        None => (1.0, 1.0),
    };
    let version = roots.root_nanbox(string_value(&im.http_version));
    let url = roots.root_nanbox(string_value(&im.url));
    let method = roots.root_nanbox(string_value(&im.method));
    let owner = roots.root_nanbox(js_node_http_incoming_message_create(f64::from_bits(
        TAG_UNDEFINED,
    )));
    let bytes = retained_bytes(&im);
    if !payload::attach(owner.get(), &FAMILY, im, bytes) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    js_node_http_incoming_message_birth(
        owner.get(),
        socket.get(),
        raw.get(),
        major,
        minor,
        version.get(),
        url.get(),
        method.get(),
    );
    owner.get()
}
fn retained_bytes(im: &IncomingMessage) -> usize {
    std::mem::size_of::<IncomingMessage>()
        + im.method.capacity()
        + im.url.capacity()
        + im.body_bytes.capacity()
        + im.raw_headers
            .iter()
            .map(|(k, v)| k.capacity() + v.capacity())
            .sum::<usize>()
        + im.headers
            .iter()
            .map(|(k, v)| k.capacity() + v.capacity())
            .sum::<usize>()
}
fn truthy(v: f64) -> bool {
    super::response_payload::truthy(v)
}
fn bool_value(b: bool) -> f64 {
    f64::from_bits(JsValue::from_bool(b).bits())
}
/// The body as node's parser delivers it once the handler has run
/// (`parserOnBody` then `parserOnMessageComplete`): `push(body)` when there
/// is one, `complete = true`, `push(null)`.
pub(crate) fn deliver_body(handle: i64) {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    // Once per request (`data_emitted` latches the delivery).
    let body = match unsafe { state(handle) } {
        Some(im) if !im.data_emitted => {
            im.data_emitted = true;
            std::mem::take(&mut im.body_bytes)
        }
        _ => return,
    };
    if !body.is_empty() {
        let buf = perry_ffi::alloc_buffer(&body);
        let chunk = roots.root_nanbox(f64::from_bits(
            JsValue::from_object_ptr(buf as usize as *mut u8).bits(),
        ));
        unsafe { perry_ffi::runtime_call(owner.get(), b"push", &[chunk.get()]) };
    }
    unsafe {
        perry_ffi::runtime_set(owner.get(), b"complete", bool_value(true));
        perry_ffi::runtime_call(
            owner.get(),
            b"push",
            &[f64::from_bits(perry_ffi::JsValue::NULL.bits())],
        );
    }
}
/// The response finished or the connection went away (node's `resOnFinish`
/// / `abortIncoming`): the signal aborts; an unread body is dumped
/// (`_dump`), so the stream ends and closes.
pub(crate) fn close(handle: i64) {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    unsafe { js_node_http_incoming_message_abort_signal(owner.get()) };
    dump_unread((owner.get().to_bits() & PTR_MASK) as i64);
}
/// node's `resOnFinish` check: `_dump()` unless a reader consumes the body
/// or a `resume()` is scheduled.
pub(crate) fn dump_unread(handle: i64) {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let get = |name: &'static [u8]| unsafe { perry_ffi::runtime_get(owner.get(), name) };
    let state = roots.root_nanbox(get(b"_readableState"));
    let resume_scheduled = JsValue::from_bits(state.get().to_bits()).is_pointer()
        && truthy(unsafe { perry_ffi::runtime_get(state.get(), b"resumeScheduled") });
    if !truthy(get(b"_consuming")) && !resume_scheduled && !truthy(get(b"destroyed")) {
        unsafe { perry_ffi::runtime_call(owner.get(), b"_dump", &[]) };
    }
}
/// The connection died before the response completed: node's `'aborted'`.
pub(crate) fn abort(handle: i64) -> bool {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    if truthy(unsafe { perry_ffi::runtime_get(owner.get(), b"aborted") }) {
        return false;
    }
    unsafe {
        perry_ffi::runtime_set(owner.get(), b"aborted", bool_value(true));
        let event = roots.root_nanbox(string_value("aborted"));
        perry_ffi::runtime_call(owner.get(), b"emit", &[event.get()]);
    }
    true
}
/// node's `IncomingMessage.prototype` methods on a heap request.
pub(crate) unsafe fn method(handle: i64, name: &str, args: &[f64]) -> Option<f64> {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(value(handle));
    let undef = f64::from_bits(TAG_UNDEFINED);
    match name {
        // `_read`: the first read marks the message consumed; the body is
        // already buffered, so there is nothing to start.
        "_read" => {
            if !truthy(perry_ffi::runtime_get(owner.get(), b"_consuming")) {
                let state =
                    roots.root_nanbox(perry_ffi::runtime_get(owner.get(), b"_readableState"));
                if JsValue::from_bits(state.get().to_bits()).is_pointer() {
                    perry_ffi::runtime_set(state.get(), b"readingMore", bool_value(false));
                }
                perry_ffi::runtime_set(owner.get(), b"_consuming", bool_value(true));
            }
            Some(undef)
        }
        // `_destroy(err, cb)`: an incomplete message is aborted, then cb(err).
        "_destroy" => {
            let err = roots.root_nanbox(args.first().copied().unwrap_or(undef));
            let cb = roots.root_nanbox(args.get(1).copied().unwrap_or(undef));
            if !truthy(perry_ffi::runtime_get(owner.get(), b"readableEnded"))
                || !truthy(perry_ffi::runtime_get(owner.get(), b"complete"))
            {
                abort((owner.get().to_bits() & PTR_MASK) as i64);
            }
            if JsValue::from_bits(cb.get().to_bits()).is_pointer() {
                perry_ffi::runtime_call(cb.get(), b"call", &[undef, err.get()]);
            }
            Some(undef)
        }
        // `_dump`: drop the body without a reader.
        "_dump" | "_dumpAndCloseReadable" => {
            if !truthy(perry_ffi::runtime_get(owner.get(), b"_dumped")) {
                perry_ffi::runtime_set(owner.get(), b"_dumped", bool_value(true));
                let data = roots.root_nanbox(string_value("data"));
                perry_ffi::runtime_call(owner.get(), b"removeAllListeners", &[data.get()]);
                perry_ffi::runtime_call(owner.get(), b"resume", &[]);
            }
            Some(undef)
        }
        // `setTimeout(msecs, cb)`: the socket's, then `this`.
        "setTimeout" => {
            let socket = roots.root_nanbox(perry_ffi::runtime_get(owner.get(), b"socket"));
            if JsValue::from_bits(socket.get().to_bits()).is_pointer() {
                let a = roots.root_nanbox(args.first().copied().unwrap_or(undef));
                let b = roots.root_nanbox(args.get(1).copied().unwrap_or(undef));
                perry_ffi::runtime_call(socket.get(), b"setTimeout", &[a.get(), b.get()]);
            }
            Some(owner.get())
        }
        _ => None,
    }
}
// ---------------------------------------------------------------------------
// Typed entries (`req.url` on a receiver the compiler knows is an
// IncomingMessage) reach a heap request as the object's own [[Get]] /
// [[Call]]: its fields are the truth, a registry message is served natively.
// ---------------------------------------------------------------------------

/// `req[name]` of a heap request, `None` for a registry message.
pub(crate) fn get(handle: i64, name: &'static [u8]) -> Option<f64> {
    is_request(handle).then(|| unsafe { perry_ffi::runtime_get(value(handle), name) })
}
/// [`get`] as a string entry's result (`String(v)` for a non-string).
pub(crate) fn get_str(handle: i64, name: &'static [u8]) -> Option<*mut perry_ffi::StringHeader> {
    let v = get(handle, name)?;
    let js = JsValue::from_bits(v.to_bits());
    if js.is_string() {
        return Some(js.as_string_ptr() as *mut perry_ffi::StringHeader);
    }
    let text = super::types::jsvalue_to_owned_string(v).unwrap_or_default();
    Some(perry_ffi::alloc_string(&text).as_raw())
}
/// [`get`] as a flag entry's result.
pub(crate) fn get_flag(handle: i64, name: &'static [u8]) -> Option<i32> {
    get(handle, name).map(|v| truthy(v) as i32)
}
/// `req[name](...args)` on a heap request.
pub(crate) fn call(handle: i64, name: &'static [u8], args: &[f64]) -> Option<f64> {
    if !is_request(handle) {
        return None;
    }
    let roots = TransientRootScope::enter();
    let rooted: Vec<_> = args.iter().map(|a| roots.root_nanbox(*a)).collect();
    let owner = roots.root_nanbox(value(handle));
    let args: Vec<f64> = rooted.iter().map(|r| r.get()).collect();
    Some(unsafe { perry_ffi::runtime_call(owner.get(), name, &args) })
}
/// A string argument of a typed entry as a JS value.
pub(crate) fn string_arg(ptr: *const perry_ffi::StringHeader) -> f64 {
    if ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    f64::from_bits(JsValue::from_string_ptr(ptr as *mut perry_ffi::StringHeader).bits())
}
/// A closure argument of a typed entry as a JS value.
pub(crate) fn closure_arg(cb: i64) -> f64 {
    if cb == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    value(cb)
}
