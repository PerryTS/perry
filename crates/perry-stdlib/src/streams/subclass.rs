// ─────────────────────────────────────────────────────────────────────
// Public helpers used by other crates / tests
// ─────────────────────────────────────────────────────────────────────

use super::*;

// ─────────────────────────────────────────────────────────────────────
// Subclass support (issue #562)
//
// User classes extending `WritableStream` / `ReadableStream` /
// `TransformStream` get an underlying-stream registry handle allocated
// at `super({ ... })` time and stashed on `this` under the hidden field
// `__perry_stream_handle__`. The dispatch arms in `lower_call.rs` route
// the receiver / destination through `js_stream_unwrap_handle` before
// the FFI call so subclass instances and bare handles are
// interchangeable.
// ─────────────────────────────────────────────────────────────────────

/// Hidden field name used to stash the underlying-stream registry id on
/// a subclass instance. Read by `js_stream_unwrap_handle`, written by
/// the three `*_subclass_init` helpers below.
const SUBCLASS_HANDLE_FIELD: &[u8] = b"__perry_stream_handle__";

unsafe fn subclass_handle_key() -> *const perry_runtime::StringHeader {
    js_string_from_bytes(
        SUBCLASS_HANDLE_FIELD.as_ptr(),
        SUBCLASS_HANDLE_FIELD.len() as u32,
    )
}

unsafe fn this_object_ptr(this_bits: f64) -> Option<*mut ObjectHeader> {
    let bits = this_bits.to_bits();
    let top16 = bits >> 48;
    if top16 != 0x7FFD {
        return None;
    }
    let raw = (bits & POINTER_MASK) as *mut ObjectHeader;
    if raw.is_null() || (raw as usize) < 0x10000 {
        return None;
    }
    Some(raw)
}

unsafe fn attach_handle_to_this(this_bits: f64, handle_id: usize) {
    if let Some(obj) = this_object_ptr(this_bits) {
        let key = subclass_handle_key();
        // Stored as plain f64 numeric — same ABI the rest of the stream
        // FFIs use for handles. `js_stream_unwrap_handle` reads it back.
        js_object_set_field_by_name(obj, key, handle_id as f64);
    }
}

/// Resolve a stream receiver / argument to a numeric registry handle.
///
/// For raw numeric handles (the value `js_writable_stream_new` etc.
/// return) the input is returned unchanged. For NaN-boxed pointer-tagged
/// JS objects (subclass instances), reads the hidden
/// `__perry_stream_handle__` field. Falls back to the input when the
/// field is missing — caller's downstream FFI will then no-op on a
/// 0-or-bogus handle exactly as it did pre-#562.
#[no_mangle]
pub unsafe extern "C" fn js_stream_unwrap_handle(value: f64) -> f64 {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    if top16 != 0x7FFD {
        return value;
    }
    let Some(obj) = this_object_ptr(value) else {
        return value;
    };
    let key = subclass_handle_key();
    let result = js_object_get_field_by_name(obj, key);
    let result_bits = result.bits();
    if result_bits == TAG_UNDEFINED || result_bits == TAG_NULL {
        return value;
    }
    f64::from_bits(result_bits)
}

// Pair conversion can call user getters and allocate its hidden-field key.
// Keep that scoped rooting local to pipeThrough instead of legacy dispatch.
#[cold]
#[inline(never)]
pub(super) unsafe fn unwrap_pair_stream_handle(value: f64) -> f64 {
    if this_object_ptr(value).is_none() {
        return value;
    }
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(value);
    unwrap_rooted_pair_stream_handle(receiver)
}

// The caller owns this root for the entire conversion, including allocations
// and getter calls. Borrow it rather than parking a second copy in a new scope.
#[inline]
pub(super) unsafe fn unwrap_rooted_pair_stream_handle(
    receiver: perry_runtime::gc::RuntimeHandle<'_>,
) -> f64 {
    if this_object_ptr(receiver.get_nanbox_f64()).is_none() {
        return receiver.get_nanbox_f64();
    }
    let key = subclass_handle_key();
    let obj = js_nanbox_get_pointer(receiver.get_nanbox_f64()) as *const ObjectHeader;
    let result = js_object_get_field_by_name(obj, key);
    let result_bits = result.bits();
    if result_bits == TAG_UNDEFINED || result_bits == TAG_NULL {
        return receiver.get_nanbox_f64();
    }
    f64::from_bits(result_bits)
}

#[inline]
pub(super) fn box_promise(p: *mut Promise) -> f64 {
    f64::from_bits(JSValue::pointer(p as *const u8).bits())
}

/// #1545: probe used by `js_native_call_method` to recognise a numeric receiver
/// as a live Web Streams handle (readable/writable/reader/writer). Only ids in
/// the reserved stream range that are present in a registry qualify.
#[no_mangle]
pub extern "C" fn js_stream_handle_is_registered(id: usize) -> bool {
    js_stream_handle_kind(id) != 0
}

/// #1545: classify a numeric Web Streams handle for `instanceof`, dispatch,
/// and `Object.prototype.toString` tags.
/// 0 = not a stream, 1 = ReadableStream, 2 = WritableStream, 3 = reader,
/// 4 = writer, 5 = TransformStream.
#[no_mangle]
pub extern "C" fn js_stream_handle_kind(id: usize) -> u8 {
    if !(STREAM_HANDLE_ID_START..STREAM_HANDLE_ID_END).contains(&id) {
        return 0;
    }
    if READABLE_STREAMS
        .lock()
        .map(|m| m.contains_key(&id))
        .unwrap_or(false)
    {
        return 1;
    }
    if WRITABLE_STREAMS
        .lock()
        .map(|m| m.contains_key(&id))
        .unwrap_or(false)
    {
        return 2;
    }
    if READERS.lock().map(|m| m.contains_key(&id)).unwrap_or(false) {
        return 3;
    }
    if WRITERS.lock().map(|m| m.contains_key(&id)).unwrap_or(false) {
        return 4;
    }
    if TRANSFORM_STREAMS
        .lock()
        .map(|m| m.contains_key(&id))
        .unwrap_or(false)
    {
        return 5;
    }
    0
}

/// #1545: runtime method dispatch for Web Streams handles whose static type
/// the codegen could not track. The static `module == "readable_stream"` /
/// `"reader"` / … NativeMethodCall arms only fire when the receiver is a local
/// whose inferred type is the stream class. Chained / member results lose that
/// type — e.g. `src.pipeThrough(ts).getReader()`, `ts.readable.getReader()`,
/// `rs.tee()[0].getReader()`, `const r = rs.getReader(); r.read()` — and lower
/// to a generic method call that reaches `js_native_call_method` →
/// `js_handle_method_dispatch` with a bare numeric handle.
///
/// Because every Web Streams handle now lives in one shared id space based at
/// `STREAM_HANDLE_ID_START` (see `idalloc::next_stream_id`), the handle is (a)
/// recognisable by range and (b) present in exactly one of the five registries,
/// so routing by
/// `(registry-membership, method-name)` is unambiguous and can never collide
/// with another handle subsystem. Returns `None` when the handle isn't a stream
/// handle or the method isn't a stream method, so the generic dispatcher falls
/// through to the next arm unchanged.
pub(crate) unsafe fn dispatch_stream_method(
    handle: f64,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    let id = handle as usize;
    if !(STREAM_HANDLE_ID_START..STREAM_HANDLE_ID_END).contains(&id) {
        return None;
    }
    let arg0 = args
        .first()
        .copied()
        .unwrap_or(f64::from_bits(TAG_UNDEFINED));
    let arg1 = args
        .get(1)
        .copied()
        .unwrap_or(f64::from_bits(TAG_UNDEFINED));

    // IDs have one registry owner; eviction clears every registry before reuse.
    // Skip registries that cannot handle this method, dropping each guard
    // before the FFI call re-locks it. Controllers alias only readable IDs.
    let is_reader = matches!(method, "read" | "releaseLock" | "cancel")
        && READERS.lock().unwrap().contains_key(&id);
    if is_reader {
        match method {
            // BYOB readers fill the caller-supplied view (#4915); default
            // readers ignore the argument inside read_with_view.
            "read" if !args.is_empty() => {
                return Some(box_promise(super::byob::js_reader_read_with_view(
                    handle, arg0,
                )))
            }
            "read" => return Some(box_promise(js_reader_read(handle))),
            "releaseLock" => return Some(js_reader_release_lock(handle)),
            "cancel" => return Some(box_promise(js_reader_cancel(handle, arg0))),
            _ => return None,
        }
    }
    let is_writer = matches!(method, "write" | "close" | "abort" | "releaseLock")
        && WRITERS.lock().unwrap().contains_key(&id);
    if is_writer {
        match method {
            "write" => return Some(box_promise(js_writer_write(handle, arg0))),
            "close" => return Some(box_promise(js_writer_close(handle))),
            "abort" => return Some(box_promise(js_writer_abort(handle, arg0))),
            "releaseLock" => return Some(js_writer_release_lock(handle)),
            _ => return None,
        }
    }
    let is_readable = matches!(
        method,
        "getReader"
            | "values"
            | "@@asyncIterator"
            | "cancel"
            | "tee"
            | "pipeTo"
            | "pipeThrough"
            | "enqueue"
            | "close"
            | "terminate"
            | "error"
    ) && READABLE_STREAMS.lock().unwrap().contains_key(&id);
    if is_readable {
        match method {
            "getReader" => return Some(js_readable_stream_get_reader_with_options(handle, arg0)),
            "values" | "@@asyncIterator" => return Some(js_readable_stream_values(handle)),
            "cancel" => return Some(box_promise(js_readable_stream_cancel(handle, arg0))),
            "tee" => return Some(js_readable_stream_tee(handle)),
            "pipeTo" => return Some(box_promise(js_readable_stream_pipe_to(handle, arg0, arg1))),
            "pipeThrough" => {
                // Non-object pairs with default options need no scoped roots
                // or getter conversion. Preserve the legacy endpoint sequence;
                // the registry lookups and validation reject invalid handles.
                if arg1.to_bits() == TAG_UNDEFINED
                    && !JSValue::from_bits(arg0.to_bits()).is_pointer()
                {
                    let transform = js_stream_unwrap_handle(arg0);
                    let writable = js_transform_stream_writable(transform);
                    let readable = js_transform_stream_readable(transform);
                    let output =
                        js_readable_stream_pipe_through_validate(handle, writable, readable, arg1);
                    let pipe = js_readable_stream_pipe_to(handle, writable, arg1);
                    js_promise_mark_internally_handled(pipe);
                    return Some(output);
                }
                return Some(js_readable_stream_pipe_through_pair(handle, arg0, arg1));
            }
            // #1644: a readable handle is also its own controller. The
            // start/transform/flush callbacks receive it as `controller`, so
            // `controller.enqueue/close/error/terminate` dispatch here when the
            // controller param is generically typed.
            "enqueue" => return Some(js_readable_stream_controller_enqueue(handle, arg0)),
            "close" => return Some(js_readable_stream_controller_close(handle)),
            "terminate" => {
                let result = js_readable_stream_controller_close(handle);
                if let Some(writable_id) = transform_writable_for_readable(id) {
                    let reason = f64::from_bits(make_type_error_with_message(
                        "Invalid state: TransformStream has been terminated",
                    ));
                    let _ = js_writable_stream_abort_inner(writable_id as f64, reason, true);
                }
                return Some(result);
            }
            "error" => return Some(js_readable_stream_controller_error(handle, arg0)),
            _ => return None,
        }
    }
    let is_writable = matches!(method, "getWriter" | "abort" | "close")
        && WRITABLE_STREAMS.lock().unwrap().contains_key(&id);
    if is_writable {
        match method {
            "getWriter" => return Some(js_writable_stream_get_writer(handle)),
            "abort" => return Some(box_promise(js_writable_stream_abort(handle, arg0))),
            "close" => return Some(box_promise(js_writable_stream_close(handle))),
            _ => return None,
        }
    }
    None
}

/// #1670: property reads on a numeric Web Streams handle that reached the
/// generic field-get path (e.g. inline `res.body.locked`, where the
/// intermediate stream id never became a typed local). Returns the WHATWG
/// getter property value, a bound-method closure for callable members (so
/// `typeof rs.getReader === "function"` and a subsequent call routes back
/// through `js_native_call_method`'s #1545 stream branch → `dispatch_stream_method`),
/// or `undefined` for any other property. Crucially this NEVER dereferences
/// the float id as a pointer — the pre-#1670 generic field-get segfaulted on
/// `res.body.locked`. Gated by the caller on stream-registry membership.
pub(crate) unsafe fn dispatch_stream_property(handle: f64, name: &str) -> f64 {
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let id = handle as usize;
    // Kind: 1=ReadableStream, 2=WritableStream, 3=reader, 4=writer.
    let kind = js_stream_handle_kind(id);
    if kind == 0 {
        return undefined;
    }
    // WHATWG getter properties (the rest fall through to bound-method /
    // undefined). `locked` is the one #1670 exercises (`res.body.locked`).
    match (kind, name) {
        (1, "locked") => return js_readable_stream_locked(handle),
        (1, "desiredSize") => return js_readable_stream_controller_desired_size(handle),
        // Non-null only while a BYOB read is parked on this byte stream (#4915).
        (1, "byobRequest") => {
            return super::byob::js_readable_stream_controller_byob_request(handle)
        }
        (2, "locked") => return js_writable_stream_locked(handle),
        (3, "closed") => return box_promise(js_reader_closed(handle)),
        (4, "closed") => return box_promise(js_writer_closed(handle)),
        (4, "ready") => return box_promise(js_writer_ready(handle)),
        (4, "desiredSize") => return js_writer_desired_size(handle),
        (5, "readable") => return js_transform_stream_readable(handle),
        (5, "writable") => return js_transform_stream_writable(handle),
        _ => {}
    }
    // #5437: expando properties stored via `stream.prop = v` (React's
    // renderToReadableStream reads back `stream.allReady`).
    if let Some(v) = super::stream_expando_get(id, name) {
        return v;
    }
    // Callable members → bound-method closure so `typeof` reports
    // "function". The name must be a `&'static [u8]` because
    // `js_class_method_bind` stores the raw pointer in the closure.
    // The receiver is the raw float handle (not NaN-boxed) so that when the
    // bound method is called, `js_native_call_method`'s stream branch fires.
    let method: Option<&'static [u8]> = match (kind, name) {
        (1, "getReader") => Some(b"getReader"),
        (1, "cancel") => Some(b"cancel"),
        (1, "tee") => Some(b"tee"),
        (1, "pipeTo") => Some(b"pipeTo"),
        (1, "pipeThrough") => Some(b"pipeThrough"),
        (2, "getWriter") => Some(b"getWriter"),
        (2, "abort") => Some(b"abort"),
        (2, "close") => Some(b"close"),
        (3, "read") => Some(b"read"),
        (3, "releaseLock") => Some(b"releaseLock"),
        (3, "cancel") => Some(b"cancel"),
        (4, "write") => Some(b"write"),
        (4, "close") => Some(b"close"),
        (4, "abort") => Some(b"abort"),
        (4, "releaseLock") => Some(b"releaseLock"),
        _ => None,
    };
    if let Some(name_bytes) = method {
        extern "C" {
            fn js_class_method_bind(
                instance: f64,
                method_name_ptr: *const u8,
                method_name_len: usize,
            ) -> f64;
        }
        return js_class_method_bind(handle, name_bytes.as_ptr(), name_bytes.len());
    }
    undefined
}

/// `super({ start, pull, cancel })` dispatch for `class X extends ReadableStream`.
/// Allocates the underlying readable handle, stashes it on `this`, runs
/// the user `start` callback synchronously (mirrors `js_readable_stream_new`).
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_subclass_init(
    this_bits: f64,
    start_bits: f64,
    pull_bits: f64,
    cancel_bits: f64,
    hwm: f64,
) -> f64 {
    ensure_gc_registered();
    let id = alloc_readable(
        closure_from_bits(start_bits.to_bits()),
        closure_from_bits(pull_bits.to_bits()),
        closure_from_bits(cancel_bits.to_bits()),
        hwm,
    );
    attach_handle_to_this(this_bits, id);
    invoke_start(id);
    maybe_pull(id);
    f64::from_bits(TAG_UNDEFINED)
}

/// `super({ write, close, abort })` dispatch for `class X extends WritableStream`.
#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_subclass_init(
    this_bits: f64,
    write_bits: f64,
    close_bits: f64,
    abort_bits: f64,
    hwm: f64,
) -> f64 {
    ensure_gc_registered();
    let id = alloc_writable(
        closure_from_bits(write_bits.to_bits()),
        closure_from_bits(close_bits.to_bits()),
        closure_from_bits(abort_bits.to_bits()),
        hwm,
    );
    attach_handle_to_this(this_bits, id);
    f64::from_bits(TAG_UNDEFINED)
}

/// `super({ transform, flush })` dispatch for `class X extends TransformStream`.
/// Allocates the transform-stream pair (readable + writable + the
/// dispatcher row in `TRANSFORM_PAIRS`) — same shape as
/// `js_transform_stream_new` — and stashes the transform handle id on
/// `this`. `pipeThrough(subclass)` then calls `js_transform_stream_writable`
/// / `_readable` after `js_stream_unwrap_handle`, finding the same
/// readable / writable sub-handles.
#[no_mangle]
pub unsafe extern "C" fn js_transform_stream_subclass_init(
    this_bits: f64,
    transform_bits: f64,
    flush_bits: f64,
    hwm: f64,
) -> f64 {
    // #1644: subclass `super({...})` doesn't thread a `start` hook through this
    // path (the #562 subclass shim only forwards transform/flush); pass undefined.
    let handle = js_transform_stream_new(
        f64::from_bits(TAG_UNDEFINED),
        transform_bits,
        flush_bits,
        hwm,
        hwm,
    );
    attach_handle_to_this(this_bits, handle as usize);
    f64::from_bits(TAG_UNDEFINED)
}

/// A Fetch body chunk's bytes. Unlike `read_bytes_from_chunk`, a string chunk
/// (valid in an async-iterable body) is UTF-8 encoded rather than dropped.
pub(super) unsafe fn body_chunk_bytes(chunk_bits: u64) -> Option<Vec<u8>> {
    if JSValue::from_bits(chunk_bits).is_any_string() {
        let mut scratch = [0u8; perry_runtime::value::SHORT_STRING_MAX_LEN];
        let (ptr, len) = perry_runtime::string::str_bytes_from_jsvalue(
            f64::from_bits(chunk_bits),
            &mut scratch,
        )?;
        return Some(if ptr.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(ptr, len as usize).to_vec()
        });
    }
    read_bytes_from_chunk(chunk_bits)
}

/// Consume a body through the default-reader protocol. A real parked read is
/// required for tee branches and transforms to produce their next chunk.
/// Each delivered chunk loses its stream-queue owner before the next pull.
#[doc(hidden)]
pub fn drain_readable_into_bytes(stream_id: usize) -> Vec<u8> {
    unsafe {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let reader = js_readable_stream_get_reader(stream_id as f64);
        let done_key = scope.root_string_ptr(js_string_from_bytes(b"done".as_ptr(), 4));
        let value_key = scope.root_string_ptr(js_string_from_bytes(b"value".as_ptr(), 5));
        let mut out = Vec::new();
        for _ in 0..1_000_000 {
            // Keep only the current promise/result alive across producer
            // callbacks; previous chunks have already become native bytes.
            let pull_scope = perry_runtime::gc::RuntimeHandleScope::new();
            let promise = pull_scope.root_raw_mut_ptr(js_reader_read(reader));
            for _ in 0..100_000 {
                if js_promise_state(promise.get_raw_mut_ptr()) != 0 {
                    break;
                }
                if js_promise_run_microtasks() == 0 {
                    break;
                }
            }
            // Retain the synchronous materializer's existing live-I/O/error
            // boundary. Ordinary buffered, tee and microtask pull sources
            // are driven by the parked read above.
            if js_promise_state(promise.get_raw_mut_ptr()) != 1 {
                break;
            }
            let result = pull_scope.root_nanbox_f64(js_promise_value(promise.get_raw_mut_ptr()));
            let obj = js_nanbox_get_pointer(result.get_nanbox_f64()) as *const ObjectHeader;
            let done = js_object_get_field_by_name(obj, done_key.get_raw_const_ptr());
            if perry_runtime::value::js_is_truthy(f64::from_bits(done.bits())) != 0 {
                break;
            }
            let obj = js_nanbox_get_pointer(result.get_nanbox_f64()) as *const ObjectHeader;
            let chunk = js_object_get_field_by_name(obj, value_key.get_raw_const_ptr());
            if let Some(bytes) = body_chunk_bytes(chunk.bits()) {
                out.extend_from_slice(&bytes);
            }
        }
        let _ = js_reader_release_lock(reader);
        out
    }
}

pub(crate) fn readable_body_state(stream_id: usize) -> (bool, bool) {
    READABLE_STREAMS
        .lock()
        .unwrap()
        .get(&stream_id)
        .map(|s| (s.reader_handle.is_some(), s.disturbed))
        .unwrap_or((false, false))
}
