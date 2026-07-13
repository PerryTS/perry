// ─────────────────────────────────────────────────────────────────────
// ReadableStreamBYOBReader + ReadableByteStreamController.byobRequest
// (#4915 — Web Streams residuals after #237/#1545)
// ─────────────────────────────────────────────────────────────────────
//
// `getReader({ mode: "byob" })` / `new ReadableStreamBYOBReader(stream)`
// mint a reader whose `read(view)` fills the caller-supplied
// TypedArray / DataView / Buffer in place. While a BYOB read is
// outstanding, the byte-stream controller exposes `byobRequest` —
// `{ view, respond(bytesWritten), respondWithNewView(view) }` — so pull
// sources can write straight into the caller's buffer instead of
// allocating an intermediate chunk.
//
// Perry's typed arrays own their storage inline (no detachable
// ArrayBuffer aliasing), so the spec's buffer-transfer dance is
// simplified: the caller's view is mutated in place and the fulfilled
// `value` is a fresh view of the same element kind holding exactly the
// bytes read. The common consumer patterns —
// `const { value } = await reader.read(new Uint8Array(n))` and
// pull-sources driving `byobRequest.respond(n)` — observe Node-shaped
// results either way.

use super::*;
use std::collections::HashMap;

/// One outstanding `read(view)` — the read promise plus the caller's view.
struct ByobPending {
    promise: *mut Promise,
    view_bits: u64,
}

unsafe impl Send for ByobPending {}

lazy_static::lazy_static! {
    static ref BYOB_PENDING: Mutex<HashMap<usize, VecDeque<ByobPending>>> =
        Mutex::new(HashMap::new());
}

/// True while at least one `read(view)` is parked on this stream — feeds
/// the ShouldCallPull check in `maybe_pull`.
pub(super) fn has_pending(stream_id: usize) -> bool {
    BYOB_PENDING
        .lock()
        .map(|m| m.get(&stream_id).map(|q| !q.is_empty()).unwrap_or(false))
        .unwrap_or(false)
}

pub(super) fn scan_byob_roots(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    if let Ok(mut map) = BYOB_PENDING.lock() {
        for queue in map.values_mut() {
            for pending in queue.iter_mut() {
                visitor.visit_raw_mut_ptr_slot(&mut pending.promise);
                visit_stream_value_slot(visitor, &mut pending.view_bits);
            }
        }
    }
}

/// A writable window over a caller-supplied view: TypedArray (any kind),
/// DataView, or Node Buffer. `kind` is the typed-array element kind used
/// to mint the fulfilled value (Uint8 for DataView/Buffer receivers).
struct ViewInfo {
    data: *mut u8,
    byte_len: usize,
    kind: u8,
    elem_size: usize,
}

unsafe fn view_info(view_bits: u64) -> Option<ViewInfo> {
    let addr = raw_pointer_addr(view_bits)?;
    if addr < 0x1000 {
        return None;
    }
    if let Some(kind) = perry_runtime::typedarray::lookup_typed_array_kind(addr) {
        let ta = addr as *mut perry_runtime::typedarray::TypedArrayHeader;
        let bytes = perry_runtime::typedarray::typed_array_bytes_mut(ta)?;
        return Some(ViewInfo {
            data: bytes.as_mut_ptr(),
            byte_len: bytes.len(),
            kind,
            elem_size: perry_runtime::typedarray::elem_size_for_kind(kind).max(1),
        });
    }
    if perry_runtime::buffer::is_registered_buffer(addr)
        && !perry_runtime::buffer::is_any_array_buffer(addr)
    {
        // DataView and Uint8Array/Buffer registrations both carry their
        // byte storage in a BufferHeader.
        let buf = addr as *mut perry_runtime::buffer::BufferHeader;
        let len = (*buf).length as usize;
        return Some(ViewInfo {
            data: perry_runtime::buffer::buffer_data_mut(buf),
            byte_len: len,
            kind: 0, // KIND_U8 — fulfilled values surface as Uint8Array
            elem_size: 1,
        });
    }
    None
}

/// `chunk.byteLength` for desiredSize accounting on byte streams; 1.0 for
/// values whose byte length can't be derived (matches the count fallback).
/// Clone a byte chunk as a fresh Uint8Array (spec `CloneAsUint8Array`, used by
/// the byte-stream tee so branches never share a mutable buffer). Non-byte
/// values pass through unchanged.
pub(super) unsafe fn clone_byte_chunk(chunk_bits: u64) -> u64 {
    match read_bytes_from_chunk(chunk_bits) {
        Some(bytes) => alloc_uint8array_from_bytes(&bytes),
        None => chunk_bits,
    }
}

pub(super) unsafe fn chunk_byte_length(chunk_bits: u64) -> f64 {
    match read_bytes_from_chunk(chunk_bits) {
        Some(bytes) => bytes.len() as f64,
        None => 1.0,
    }
}

/// Build the fulfilled `value` for a BYOB read: a fresh view of the same
/// element kind holding `bytes` (already copied into the caller's view).
unsafe fn alloc_view_of_kind(kind: u8, elem_size: usize, bytes: &[u8]) -> u64 {
    if kind == 0 {
        return alloc_uint8array_from_bytes(bytes);
    }
    let elems = bytes.len() / elem_size.max(1);
    let ta = perry_runtime::typedarray::typed_array_alloc(kind, elems as u32);
    if ta.is_null() {
        return TAG_UNDEFINED;
    }
    if let Some(dst) = perry_runtime::typedarray::typed_array_bytes_mut(ta) {
        let n = dst.len().min(bytes.len());
        dst[..n].copy_from_slice(&bytes[..n]);
    }
    JSValue::pointer(ta as *const u8).bits()
}

/// Copy queued chunk bytes into `view`, returning the number of bytes
/// filled. Partially-consumed chunks are put back at the queue head as a
/// fresh Uint8Array remainder.
unsafe fn fill_view_from_queue(stream_id: usize, view: &ViewInfo) -> usize {
    let mut filled = 0usize;
    // Round capacity down to a whole number of elements.
    let cap = (view.byte_len / view.elem_size) * view.elem_size;
    while filled < cap {
        let chunk = {
            let mut g = READABLE_STREAMS.lock().unwrap();
            match g.get_mut(&stream_id) {
                Some(s) => match s.pop_chunk() {
                    Some(c) => c,
                    None => break,
                },
                None => break,
            }
        };
        let bytes = match read_bytes_from_chunk(chunk) {
            Some(b) => b,
            None => continue, // non-byte chunk on a byte stream — skip
        };
        let take = bytes.len().min(cap - filled);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), view.data.add(filled), take);
        filled += take;
        if take < bytes.len() {
            let remainder = alloc_uint8array_from_bytes(&bytes[take..]);
            let mut g = READABLE_STREAMS.lock().unwrap();
            if let Some(s) = g.get_mut(&stream_id) {
                let size = (bytes.len() - take) as f64;
                s.chunks.push_front(remainder);
                s.chunk_sizes.push_front(size);
                s.queue_total_size += size;
            }
            break;
        }
    }
    filled
}

unsafe fn reject_with_type_error(promise: *mut Promise, message: &str) -> *mut Promise {
    reject_type_error(promise, message);
    promise
}

/// `getReader({ mode: "byob" })` routed through a direct
/// `new ReadableStreamBYOBReader(stream)` construction (#4915).
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_get_byob_reader(stream_handle: f64) -> f64 {
    let stream_handle = js_stream_unwrap_handle(stream_handle);
    let id = stream_handle as usize;
    let is_byte_stream = {
        let g = READABLE_STREAMS.lock().unwrap();
        match g.get(&id) {
            Some(s) => s.is_byte_stream,
            None => {
                throw_type_error(
                    "ReadableStreamBYOBReader constructor requires a ReadableStream argument",
                );
            }
        }
    };
    if !is_byte_stream {
        throw_type_error(
            "Cannot construct a ReadableStreamBYOBReader for a stream not constructed with a byte source",
        );
    }
    // Reuse the lock/closed-promise bookkeeping in the shared path; the
    // mode check above already proved this is a byte stream.
    let reader = get_reader_for_stream(id, true);
    reader as f64
}

/// Shared reader-minting path used by `js_readable_stream_get_reader_with_options`
/// (via the byob_requested flag) and the BYOB constructor above.
pub(super) unsafe fn get_reader_for_stream(stream_id: usize, is_byob: bool) -> usize {
    ensure_gc_registered();
    let mut g = READABLE_STREAMS.lock().unwrap();
    let s = match g.get_mut(&stream_id) {
        Some(s) => s,
        None => return 0,
    };
    if s.reader_handle.is_some() {
        drop(g);
        throw_type_error("ReadableStream is locked");
    }
    let reader_id = next_id(&NEXT_STREAM_ID);
    let closed_p = internal_promise();
    if s.state == ReadableState::Closed {
        js_promise_resolve(closed_p, f64::from_bits(TAG_UNDEFINED));
    } else if s.state == ReadableState::Errored {
        js_promise_reject(closed_p, f64::from_bits(s.error_value));
    }
    s.reader_handle = Some(reader_id);
    drop(g);
    READERS.lock().unwrap().insert(
        reader_id,
        ReaderData {
            stream_handle: stream_id,
            locked: true,
            closed_promise: closed_p,
            is_byob,
        },
    );
    reader_id
}

/// `reader.read(view)` — BYOB read into a caller-supplied buffer.
/// Default readers ignore the argument and behave like `read()`.
#[no_mangle]
pub unsafe extern "C" fn js_reader_read_with_view(reader_handle: f64, view: f64) -> *mut Promise {
    let unwrapped = js_stream_unwrap_handle(reader_handle);
    let reader_id = unwrapped as usize;
    let (stream_id, is_byob) = match READERS.lock().unwrap().get(&reader_id) {
        Some(r) if r.locked => (r.stream_handle, r.is_byob),
        Some(_) => {
            let promise = js_promise_new();
            return reject_with_type_error(promise, "Reader is no longer locked to a stream");
        }
        None => {
            let promise = js_promise_new();
            return reject_with_type_error(promise, "Invalid reader");
        }
    };
    let view_bits = view.to_bits();
    if !is_byob || view_bits == TAG_UNDEFINED {
        // Default reader: `read()` semantics (extra args ignored).
        return js_reader_read(reader_handle);
    }
    let promise = js_promise_new();
    let info = match view_info(view_bits) {
        Some(info) => info,
        None => {
            return reject_with_type_error(
                promise,
                "ReadableStreamBYOBReader.read(view) requires a TypedArray or DataView",
            );
        }
    };
    if info.byte_len == 0 {
        return reject_with_type_error(
            promise,
            "ReadableStreamBYOBReader.read(view) view cannot be zero-length",
        );
    }

    enum Outcome {
        Errored(u64),
        Closed,
        TryFill,
    }
    let outcome = {
        let g = READABLE_STREAMS.lock().unwrap();
        match g.get(&stream_id) {
            Some(s) => match s.state {
                ReadableState::Errored => Outcome::Errored(s.error_value),
                ReadableState::Closed if s.chunks.is_empty() => Outcome::Closed,
                _ => Outcome::TryFill,
            },
            None => Outcome::Closed,
        }
    };
    match outcome {
        Outcome::Errored(e) => {
            js_promise_reject(promise, f64::from_bits(e));
            return promise;
        }
        Outcome::Closed => {
            let empty = alloc_view_of_kind(info.kind, info.elem_size, &[]);
            let result = build_iter_result(empty, true);
            js_promise_resolve(promise, f64::from_bits(result));
            return promise;
        }
        Outcome::TryFill => {}
    }

    let filled = fill_view_from_queue(stream_id, &info);
    // A BYOB drain (or an about-to-park read on an empty queue) is consumer
    // progress on this readable — release transform writes parked on
    // backpressure, mirroring the default-reader path in `js_reader_read`.
    super::transform::transform_release_writes(stream_id);
    if filled > 0 {
        let bytes = std::slice::from_raw_parts(info.data, filled);
        let value = alloc_view_of_kind(info.kind, info.elem_size, bytes);
        let (closed_now, reader) = {
            let g = READABLE_STREAMS.lock().unwrap();
            match g.get(&stream_id) {
                Some(s) => (
                    s.state == ReadableState::Closed && s.chunks.is_empty(),
                    s.reader_handle,
                ),
                None => (false, None),
            }
        };
        let result = build_iter_result(value, false);
        js_promise_resolve(promise, f64::from_bits(result));
        if closed_now {
            if let Some(rid) = reader {
                let p = READERS.lock().unwrap().get(&rid).map(|r| r.closed_promise);
                if let Some(p) = p {
                    js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
                }
            }
        }
        maybe_pull(stream_id);
        return promise;
    }

    // Nothing buffered: park the read so the controller's byobRequest /
    // enqueue can service it, then ask the source to pull.
    BYOB_PENDING
        .lock()
        .unwrap()
        .entry(stream_id)
        .or_default()
        .push_back(ByobPending { promise, view_bits });
    maybe_pull(stream_id);
    promise
}

/// Controller property `byobRequest` — non-null while a BYOB read is
/// outstanding on this byte stream.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_controller_byob_request(stream_handle: f64) -> f64 {
    let id = js_stream_unwrap_handle(stream_handle) as usize;
    let view_bits = {
        let g = BYOB_PENDING.lock().unwrap();
        match g.get(&id).and_then(|q| q.front()) {
            Some(p) => p.view_bits,
            None => return f64::from_bits(TAG_NULL),
        }
    };

    let obj = js_object_alloc(0, 3);
    let keys = js_array_alloc(3);
    let k_view = js_string_from_bytes(b"view".as_ptr(), 4);
    let k_respond = js_string_from_bytes(b"respond".as_ptr(), 7);
    let k_rwnv = js_string_from_bytes(b"respondWithNewView".as_ptr(), 18);
    js_array_push(keys, JSValue::string_ptr(k_view));
    js_array_push(keys, JSValue::string_ptr(k_respond));
    js_array_push(keys, JSValue::string_ptr(k_rwnv));
    js_object_set_field(obj, 0, JSValue::from_bits(view_bits));

    let respond_fn = byob_request_respond as *const u8;
    perry_runtime::closure::js_register_closure_arity(respond_fn, 1);
    let respond = perry_runtime::closure::js_closure_alloc(respond_fn, 1);
    perry_runtime::closure::js_closure_set_capture_f64(respond, 0, id as f64);
    js_object_set_field(obj, 1, JSValue::pointer(respond as *const u8));

    let rwnv_fn = byob_request_respond_with_new_view as *const u8;
    perry_runtime::closure::js_register_closure_arity(rwnv_fn, 1);
    let rwnv = perry_runtime::closure::js_closure_alloc(rwnv_fn, 1);
    perry_runtime::closure::js_closure_set_capture_f64(rwnv, 0, id as f64);
    js_object_set_field(obj, 2, JSValue::pointer(rwnv as *const u8));

    js_object_set_keys(obj, keys);
    f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits())
}

extern "C" fn byob_request_respond(closure: *const ClosureHeader, bytes_written: f64) -> f64 {
    unsafe {
        let stream_id = perry_runtime::closure::js_closure_get_capture_f64(closure, 0) as usize;
        let n = JSValue::from_bits(bytes_written.to_bits()).to_number();
        if n.is_nan() || n < 0.0 || n.fract() != 0.0 {
            throw_range_error_with_code(
                "bytesWritten must be a non-negative integer",
                "ERR_OUT_OF_RANGE",
            );
        }
        let pending = match BYOB_PENDING
            .lock()
            .unwrap()
            .get_mut(&stream_id)
            .and_then(|q| q.pop_front())
        {
            Some(p) => p,
            None => {
                throw_type_error("There is no pending BYOB request to respond to");
            }
        };
        let info = match view_info(pending.view_bits) {
            Some(info) => info,
            None => {
                reject_type_error(pending.promise, "BYOB request view is no longer valid");
                return f64::from_bits(TAG_UNDEFINED);
            }
        };
        let n = n as usize;
        if n > info.byte_len {
            // Put the request back so the source can retry with a valid count.
            BYOB_PENDING
                .lock()
                .unwrap()
                .entry(stream_id)
                .or_default()
                .push_front(pending);
            throw_range_error_with_code(
                "bytesWritten exceeds the BYOB view's byteLength",
                "ERR_OUT_OF_RANGE",
            );
        }
        let stream_closed = {
            let g = READABLE_STREAMS.lock().unwrap();
            g.get(&stream_id)
                .map(|s| s.state != ReadableState::Readable)
                .unwrap_or(true)
        };
        if n == 0 {
            if !stream_closed {
                BYOB_PENDING
                    .lock()
                    .unwrap()
                    .entry(stream_id)
                    .or_default()
                    .push_front(pending);
                throw_type_error("bytesWritten must be positive while the stream is readable");
            }
            let empty = alloc_view_of_kind(info.kind, info.elem_size, &[]);
            let result = build_iter_result(empty, true);
            js_promise_resolve(pending.promise, f64::from_bits(result));
            return f64::from_bits(TAG_UNDEFINED);
        }
        let bytes = std::slice::from_raw_parts(info.data, n);
        let value = alloc_view_of_kind(info.kind, info.elem_size, bytes);
        let result = build_iter_result(value, false);
        js_promise_resolve(pending.promise, f64::from_bits(result));
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn byob_request_respond_with_new_view(closure: *const ClosureHeader, view: f64) -> f64 {
    unsafe {
        let stream_id = perry_runtime::closure::js_closure_get_capture_f64(closure, 0) as usize;
        let view_bits = view.to_bits();
        let info = match view_info(view_bits) {
            Some(info) => info,
            None => {
                throw_type_error("respondWithNewView requires a TypedArray or DataView argument");
            }
        };
        let pending = match BYOB_PENDING
            .lock()
            .unwrap()
            .get_mut(&stream_id)
            .and_then(|q| q.pop_front())
        {
            Some(p) => p,
            None => {
                throw_type_error("There is no pending BYOB request to respond to");
            }
        };
        let stream_closed = {
            let g = READABLE_STREAMS.lock().unwrap();
            g.get(&stream_id)
                .map(|s| s.state != ReadableState::Readable)
                .unwrap_or(true)
        };
        let done = info.byte_len == 0;
        if done && !stream_closed {
            BYOB_PENDING
                .lock()
                .unwrap()
                .entry(stream_id)
                .or_default()
                .push_front(pending);
            throw_type_error(
                "respondWithNewView requires a non-empty view while the stream is readable",
            );
        }
        let result = build_iter_result(view_bits, done);
        js_promise_resolve(pending.promise, f64::from_bits(result));
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Controller `enqueue(chunk)` on a byte stream while a BYOB read is
/// parked: copy the chunk straight into the caller's view (overflow goes
/// back on the queue). Returns true when the chunk was consumed.
pub(super) unsafe fn service_pending_with_chunk(stream_id: usize, chunk_bits: u64) -> bool {
    let pending = match BYOB_PENDING
        .lock()
        .unwrap()
        .get_mut(&stream_id)
        .and_then(|q| q.pop_front())
    {
        Some(p) => p,
        None => return false,
    };
    let info = match view_info(pending.view_bits) {
        Some(info) => info,
        None => {
            reject_type_error(pending.promise, "BYOB request view is no longer valid");
            return true;
        }
    };
    let bytes = match read_bytes_from_chunk(chunk_bits) {
        Some(b) => b,
        None => {
            // Shouldn't happen (enqueue validated the chunk); requeue the read.
            BYOB_PENDING
                .lock()
                .unwrap()
                .entry(stream_id)
                .or_default()
                .push_front(pending);
            return false;
        }
    };
    let cap = (info.byte_len / info.elem_size) * info.elem_size;
    let take = bytes.len().min(cap);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), info.data, take);
    if take < bytes.len() {
        let remainder = alloc_uint8array_from_bytes(&bytes[take..]);
        let mut g = READABLE_STREAMS.lock().unwrap();
        if let Some(s) = g.get_mut(&stream_id) {
            if s.state == ReadableState::Readable {
                s.push_chunk(remainder, (bytes.len() - take) as f64);
            }
        }
    }
    let filled = std::slice::from_raw_parts(info.data, take);
    let value = alloc_view_of_kind(info.kind, info.elem_size, filled);
    let result = build_iter_result(value, false);
    js_promise_resolve(pending.promise, f64::from_bits(result));
    true
}

/// Stream closed: outstanding BYOB reads settle `{ value: <empty view>,
/// done: true }`.
pub(super) unsafe fn close_pending_byob(stream_id: usize) {
    let drained: Vec<ByobPending> = match BYOB_PENDING.lock().unwrap().get_mut(&stream_id) {
        Some(q) => q.drain(..).collect(),
        None => return,
    };
    for pending in drained {
        let (kind, elem_size) = match view_info(pending.view_bits) {
            Some(info) => (info.kind, info.elem_size),
            None => (0, 1),
        };
        let empty = alloc_view_of_kind(kind, elem_size, &[]);
        let result = build_iter_result(empty, true);
        js_promise_resolve(pending.promise, f64::from_bits(result));
    }
}

/// Stream errored: outstanding BYOB reads reject with the stored reason.
pub(super) unsafe fn error_pending_byob(stream_id: usize, reason_bits: u64) {
    let drained: Vec<ByobPending> = match BYOB_PENDING.lock().unwrap().get_mut(&stream_id) {
        Some(q) => q.drain(..).collect(),
        None => return,
    };
    for pending in drained {
        js_promise_reject(pending.promise, f64::from_bits(reason_bits));
    }
}
