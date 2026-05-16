//! Web Streams API (issue #237).
//!
//! Implements `ReadableStream` / `WritableStream` / `TransformStream` plus
//! the matching default reader / writer pair, and the per-controller
//! enqueue / close / error / write / abort surface. Wires `blob.stream()`
//! and `response.body` so the consumers in the issue's acceptance
//! criteria all work end-to-end.
//!
//! Handles use the same numeric f64 ABI as `BLOB_REGISTRY` /
//! `FETCH_RESPONSES` (registry id cast to f64). Codegen's `module ==
//! "readable_stream"` / `"reader"` / `"writable_stream"` / `"writer"` /
//! `"transform_stream"` arms in `lower_call.rs` route methods through
//! these FFIs.
//!
//! Buffered model: `blob.stream()` and `response.body` produce a
//! single-chunk readable stream over the body bytes that are already
//! resident in memory. True chunk-by-chunk streaming from
//! `reqwest::Response::chunk()` is a separate followup — the existing
//! fetch path eagerly buffers the whole response anyway, so the user-
//! visible contract is identical for the consumers we expose here.
//!
//! Stubs: BYOB readers, custom `QueuingStrategy` size functions, and
//! `ReadableStream.from(asyncIterable)` throw via
//! `js_streams_throw_not_implemented` — see the inline comment on each
//! site.

use perry_runtime::{
    js_array_alloc, js_array_push, js_closure_call0, js_closure_call1, js_closure_call2,
    js_object_alloc, js_object_get_field_by_name, js_object_set_field, js_object_set_field_by_name,
    js_object_set_keys, js_promise_new, js_promise_reject, js_promise_resolve,
    js_string_from_bytes, ClosureHeader, JSValue, ObjectHeader, Promise,
};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadableState {
    Readable,
    Closed,
    Errored,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum WritableState {
    Writable,
    Closing,
    Closed,
    Errored,
}

struct ReadableStreamData {
    state: ReadableState,
    /// Queued chunks as NaN-boxed pointers (typically Uint8Array via POINTER_TAG).
    chunks: VecDeque<u64>,
    /// FIFO of read() promises waiting for a chunk.
    pending_reads: VecDeque<*mut Promise>,
    start_cb: i64,
    pull_cb: i64,
    cancel_cb: i64,
    high_water_mark: f64,
    pulling: bool,
    started: bool,
    reader_handle: Option<usize>,
    error_value: u64,
    /// Per-controller cancel reason captured when `cancel()` is called.
    canceled: bool,
}

#[allow(dead_code)]
struct WritableStreamData {
    state: WritableState,
    write_cb: i64,
    close_cb: i64,
    abort_cb: i64,
    /// Backlog of writes when `in_flight` is true. Reserved for the
    /// async-write path tracked as a #237 followup; today every write
    /// runs synchronously through the user `write` callback.
    write_queue: VecDeque<(u64, *mut Promise)>,
    in_flight: bool,
    high_water_mark: f64,
    writer_handle: Option<usize>,
    error_value: u64,
    /// Resolved when the stream becomes ready for more writes (i.e. queue drains).
    ready_promise: *mut Promise,
    /// Resolved when the stream finishes / rejects on error.
    closed_promise: *mut Promise,
}

struct TransformStreamData {
    readable_handle: usize,
    writable_handle: usize,
    transform_cb: i64,
    flush_cb: i64,
}

struct ReaderData {
    stream_handle: usize,
    locked: bool,
    closed_promise: *mut Promise,
}

struct WriterData {
    stream_handle: usize,
    locked: bool,
    closed_promise: *mut Promise,
    ready_promise: *mut Promise,
}

unsafe impl Send for ReadableStreamData {}
unsafe impl Send for WritableStreamData {}
unsafe impl Send for ReaderData {}
unsafe impl Send for WriterData {}

lazy_static::lazy_static! {
    static ref READABLE_STREAMS: Mutex<HashMap<usize, ReadableStreamData>> = Mutex::new(HashMap::new());
    static ref NEXT_RS_ID: Mutex<usize> = Mutex::new(1);
    static ref WRITABLE_STREAMS: Mutex<HashMap<usize, WritableStreamData>> = Mutex::new(HashMap::new());
    static ref NEXT_WS_ID: Mutex<usize> = Mutex::new(1);
    static ref TRANSFORM_STREAMS: Mutex<HashMap<usize, TransformStreamData>> = Mutex::new(HashMap::new());
    static ref NEXT_TS_ID: Mutex<usize> = Mutex::new(1);
    static ref READERS: Mutex<HashMap<usize, ReaderData>> = Mutex::new(HashMap::new());
    static ref NEXT_READER_ID: Mutex<usize> = Mutex::new(1);
    static ref WRITERS: Mutex<HashMap<usize, WriterData>> = Mutex::new(HashMap::new());
    static ref NEXT_WRITER_ID: Mutex<usize> = Mutex::new(1);
}

static GC_REGISTERED: std::sync::Once = std::sync::Once::new();

/// Register the streams GC root scanner once. Closures held by user-
/// supplied `start` / `pull` / `cancel` / `write` / `close` / `abort` /
/// `transform` / `flush` callbacks live in the registry maps below; the
/// runtime GC mark phase wouldn't see them otherwise and a sweep
/// between registration and dispatch would free the closure body. Same
/// shape as `ws.rs::ensure_gc_scanner_registered`.
fn ensure_gc_registered() {
    GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_root_scanner(scan_stream_roots);
    });
}

fn scan_stream_roots(mark: &mut dyn FnMut(f64)) {
    let mark_cb = |cb: i64, mark: &mut dyn FnMut(f64)| {
        if cb != 0 {
            let boxed = f64::from_bits(POINTER_TAG | (cb as u64 & POINTER_MASK));
            mark(boxed);
        }
    };
    let mark_promise = |p: *mut Promise, mark: &mut dyn FnMut(f64)| {
        let raw = p as i64;
        if raw != 0 {
            let boxed = f64::from_bits(POINTER_TAG | (raw as u64 & POINTER_MASK));
            mark(boxed);
        }
    };
    let mark_chunk = |bits: u64, mark: &mut dyn FnMut(f64)| {
        let top = bits >> 48;
        if top == 0x7FFD || top == 0x7FFF {
            mark(f64::from_bits(bits));
        }
    };

    if let Ok(map) = READABLE_STREAMS.lock() {
        for s in map.values() {
            mark_cb(s.start_cb, mark);
            mark_cb(s.pull_cb, mark);
            mark_cb(s.cancel_cb, mark);
            for &c in s.chunks.iter() {
                mark_chunk(c, mark);
            }
            for &p in s.pending_reads.iter() {
                mark_promise(p, mark);
            }
            if s.state == ReadableState::Errored {
                mark_chunk(s.error_value, mark);
            }
        }
    }
    if let Ok(map) = WRITABLE_STREAMS.lock() {
        for s in map.values() {
            mark_cb(s.write_cb, mark);
            mark_cb(s.close_cb, mark);
            mark_cb(s.abort_cb, mark);
            for (chunk, p) in s.write_queue.iter() {
                mark_chunk(*chunk, mark);
                mark_promise(*p, mark);
            }
            mark_promise(s.ready_promise, mark);
            mark_promise(s.closed_promise, mark);
            if s.state == WritableState::Errored {
                mark_chunk(s.error_value, mark);
            }
        }
    }
    if let Ok(map) = TRANSFORM_STREAMS.lock() {
        for t in map.values() {
            mark_cb(t.transform_cb, mark);
            mark_cb(t.flush_cb, mark);
        }
    }
    if let Ok(map) = READERS.lock() {
        for r in map.values() {
            mark_promise(r.closed_promise, mark);
        }
    }
    if let Ok(map) = WRITERS.lock() {
        for w in map.values() {
            mark_promise(w.closed_promise, mark);
            mark_promise(w.ready_promise, mark);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────

fn next_id(slot: &Mutex<usize>) -> usize {
    let mut guard = slot.lock().unwrap();
    let id = *guard;
    *guard += 1;
    id
}

unsafe fn closure_from_bits(bits: u64) -> i64 {
    if bits == TAG_UNDEFINED || bits == TAG_NULL || bits == 0 {
        return 0;
    }
    let top = bits >> 48;
    if top >= 0x7FF8 {
        (bits & POINTER_MASK) as i64
    } else {
        0
    }
}

unsafe fn build_iter_result(value_bits: u64, done: bool) -> u64 {
    let obj = js_object_alloc(0, 2);
    let keys = js_array_alloc(2);
    let k_value = js_string_from_bytes(b"value".as_ptr(), 5);
    let k_done = js_string_from_bytes(b"done".as_ptr(), 4);
    js_array_push(keys, JSValue::string_ptr(k_value));
    js_array_push(keys, JSValue::string_ptr(k_done));
    js_object_set_field(obj, 0, JSValue::from_bits(value_bits));
    let done_bits = if done { TAG_TRUE } else { TAG_FALSE };
    js_object_set_field(obj, 1, JSValue::from_bits(done_bits));
    js_object_set_keys(obj, keys);
    JSValue::object_ptr(obj as *mut u8).bits()
}

unsafe fn alloc_uint8array_from_bytes(bytes: &[u8]) -> u64 {
    let buf = perry_runtime::buffer::buffer_alloc(bytes.len() as u32);
    (*buf).length = bytes.len() as u32;
    if !bytes.is_empty() {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            perry_runtime::buffer::buffer_data_mut(buf),
            bytes.len(),
        );
    }
    JSValue::object_ptr(buf as *mut u8).bits()
}

unsafe fn read_bytes_from_chunk(chunk_bits: u64) -> Option<Vec<u8>> {
    let top = chunk_bits >> 48;
    if top != 0x7FFD {
        return None;
    }
    let ptr = (chunk_bits & POINTER_MASK) as *mut perry_runtime::buffer::BufferHeader;
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).length as usize;
    let data = perry_runtime::buffer::buffer_data_mut(ptr) as *const u8;
    Some(std::slice::from_raw_parts(data, len).to_vec())
}

unsafe fn make_error_with_message(msg: &str) -> u64 {
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = perry_runtime::error::js_error_new_with_message(s);
    JSValue::pointer(err as *const u8).bits()
}

fn alloc_readable(start_cb: i64, pull_cb: i64, cancel_cb: i64, hwm: f64) -> usize {
    let id = next_id(&NEXT_RS_ID);
    READABLE_STREAMS.lock().unwrap().insert(
        id,
        ReadableStreamData {
            state: ReadableState::Readable,
            chunks: VecDeque::new(),
            pending_reads: VecDeque::new(),
            start_cb,
            pull_cb,
            cancel_cb,
            high_water_mark: if hwm.is_nan() || hwm <= 0.0 { 1.0 } else { hwm },
            pulling: false,
            started: false,
            reader_handle: None,
            error_value: 0,
            canceled: false,
        },
    );
    id
}

fn alloc_writable(write_cb: i64, close_cb: i64, abort_cb: i64, hwm: f64) -> usize {
    let id = next_id(&NEXT_WS_ID);
    let ready = js_promise_new();
    let closed = js_promise_new();
    js_promise_resolve(ready, f64::from_bits(TAG_UNDEFINED));
    WRITABLE_STREAMS.lock().unwrap().insert(
        id,
        WritableStreamData {
            state: WritableState::Writable,
            write_cb,
            close_cb,
            abort_cb,
            write_queue: VecDeque::new(),
            in_flight: false,
            high_water_mark: if hwm.is_nan() || hwm <= 0.0 { 1.0 } else { hwm },
            writer_handle: None,
            error_value: 0,
            ready_promise: ready,
            closed_promise: closed,
        },
    );
    id
}

unsafe fn invoke_start(stream_id: usize) {
    let (cb, controller) = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) if !s.started => {
                s.started = true;
                (s.start_cb, stream_id as f64)
            }
            _ => return,
        }
    };
    if cb != 0 {
        js_closure_call1(cb as *const ClosureHeader, controller);
    }
}

unsafe fn maybe_pull(stream_id: usize) {
    let (cb, controller, should_pull) = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) if s.state == ReadableState::Readable && !s.pulling && s.started => {
                let need = s.chunks.is_empty() || (s.chunks.len() as f64) < s.high_water_mark;
                if need && s.pull_cb != 0 {
                    s.pulling = true;
                    (s.pull_cb, stream_id as f64, true)
                } else {
                    (0, 0.0, false)
                }
            }
            _ => (0, 0.0, false),
        }
    };
    if !should_pull {
        return;
    }
    js_closure_call1(cb as *const ClosureHeader, controller);
    if let Some(s) = READABLE_STREAMS.lock().unwrap().get_mut(&stream_id) {
        s.pulling = false;
    }
}

unsafe fn close_pending(stream_id: usize) {
    let promises: Vec<*mut Promise> = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => s.pending_reads.drain(..).collect(),
            None => Vec::new(),
        }
    };
    for p in promises {
        let result = build_iter_result(TAG_UNDEFINED, true);
        js_promise_resolve(p, f64::from_bits(result));
    }
}

unsafe fn error_pending(stream_id: usize, reason_bits: u64) {
    let promises: Vec<*mut Promise> = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => s.pending_reads.drain(..).collect(),
            None => Vec::new(),
        }
    };
    for p in promises {
        js_promise_reject(p, f64::from_bits(reason_bits));
    }
}

// ─────────────────────────────────────────────────────────────────────
// ReadableStream FFI
// ─────────────────────────────────────────────────────────────────────

/// `new ReadableStream({ start, pull, cancel })` — `start_cb` / `pull_cb`
/// / `cancel_cb` are NaN-boxed `*ClosureHeader` bits (or undefined). The
/// new stream's controller is the stream handle itself; user code calls
/// `controller.enqueue(c)` etc. to drive it.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_new(
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
    invoke_start(id);
    maybe_pull(id);
    id as f64
}

/// Internal helper: build a single-chunk readable stream from an owned
/// byte buffer. Used by `blob.stream()` and `response.body`.
pub fn alloc_readable_from_bytes(bytes: Vec<u8>) -> usize {
    ensure_gc_registered();
    let id = alloc_readable(0, 0, 0, 1.0);
    unsafe {
        let chunk_bits = alloc_uint8array_from_bytes(&bytes);
        let mut g = READABLE_STREAMS.lock().unwrap();
        if let Some(s) = g.get_mut(&id) {
            s.started = true;
            if !bytes.is_empty() {
                s.chunks.push_back(chunk_bits);
            }
            s.state = ReadableState::Closed;
        }
    }
    id
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_get_reader(stream_handle: f64) -> f64 {
    ensure_gc_registered();
    let id = stream_handle as usize;
    {
        let mut g = READABLE_STREAMS.lock().unwrap();
        let s = match g.get_mut(&id) {
            Some(s) => s,
            None => return f64::from_bits(TAG_UNDEFINED),
        };
        if s.reader_handle.is_some() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let reader_id = next_id(&NEXT_READER_ID);
        let closed_p = js_promise_new();
        if s.state == ReadableState::Closed {
            js_promise_resolve(closed_p, f64::from_bits(TAG_UNDEFINED));
        } else if s.state == ReadableState::Errored {
            js_promise_reject(closed_p, f64::from_bits(s.error_value));
        }
        s.reader_handle = Some(reader_id);
        READERS.lock().unwrap().insert(
            reader_id,
            ReaderData {
                stream_handle: id,
                locked: true,
                closed_promise: closed_p,
            },
        );
        reader_id as f64
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_locked(stream_handle: f64) -> f64 {
    let id = stream_handle as usize;
    let g = READABLE_STREAMS.lock().unwrap();
    let locked = g
        .get(&id)
        .map(|s| s.reader_handle.is_some())
        .unwrap_or(false);
    f64::from_bits(if locked { TAG_TRUE } else { TAG_FALSE })
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_cancel(
    stream_handle: f64,
    reason: f64,
) -> *mut Promise {
    let promise = js_promise_new();
    let id = stream_handle as usize;
    let cb = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) => {
                s.canceled = true;
                s.state = ReadableState::Closed;
                s.chunks.clear();
                s.cancel_cb
            }
            None => 0,
        }
    };
    if cb != 0 {
        js_closure_call1(cb as *const ClosureHeader, reason);
    }
    close_pending(id);
    js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    promise
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_blob(blob_id: f64) -> f64 {
    let bytes = crate::fetch::blob_bytes_clone(blob_id as usize).unwrap_or_default();
    alloc_readable_from_bytes(bytes) as f64
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_response(resp_id: f64) -> f64 {
    let bytes = crate::fetch::response_bytes_clone(resp_id as usize).unwrap_or_default();
    alloc_readable_from_bytes(bytes) as f64
}

// `ReadableStream.from(asyncIterable)` — deferred (issue #237 followup).
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_iterable(_value: f64) -> f64 {
    let err = make_error_with_message(
        "ReadableStream.from(asyncIterable) is not yet implemented (issue #237 followup)",
    );
    perry_runtime::exception::js_throw(f64::from_bits(err));
}

// ─────────────────────────────────────────────────────────────────────
// ReadableStreamDefaultController FFI (controller is the stream handle)
// ─────────────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_controller_enqueue(
    stream_handle: f64,
    chunk: f64,
) -> f64 {
    let id = stream_handle as usize;
    let chunk_bits = chunk.to_bits();
    let popped = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) if s.state == ReadableState::Readable => {
                if let Some(p) = s.pending_reads.pop_front() {
                    Some(p)
                } else {
                    s.chunks.push_back(chunk_bits);
                    None
                }
            }
            _ => return f64::from_bits(TAG_UNDEFINED),
        }
    };
    if let Some(p) = popped {
        let result = build_iter_result(chunk_bits, false);
        js_promise_resolve(p, f64::from_bits(result));
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_controller_close(stream_handle: f64) -> f64 {
    let id = stream_handle as usize;
    {
        let mut g = READABLE_STREAMS.lock().unwrap();
        if let Some(s) = g.get_mut(&id) {
            if s.state == ReadableState::Readable {
                s.state = ReadableState::Closed;
            }
        }
    }
    // Reader.closed promise resolves when stream closes and queue empties.
    let (queue_empty, reader_id) = {
        let g = READABLE_STREAMS.lock().unwrap();
        match g.get(&id) {
            Some(s) => (s.chunks.is_empty(), s.reader_handle),
            None => (true, None),
        }
    };
    if queue_empty {
        if let Some(rid) = reader_id {
            let p = READERS.lock().unwrap().get(&rid).map(|r| r.closed_promise);
            if let Some(p) = p {
                js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            }
        }
        close_pending(id);
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_controller_error(
    stream_handle: f64,
    reason: f64,
) -> f64 {
    let id = stream_handle as usize;
    let reason_bits = reason.to_bits();
    let reader_id = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) => {
                s.state = ReadableState::Errored;
                s.error_value = reason_bits;
                s.chunks.clear();
                s.reader_handle
            }
            None => return f64::from_bits(TAG_UNDEFINED),
        }
    };
    error_pending(id, reason_bits);
    if let Some(rid) = reader_id {
        let p = READERS.lock().unwrap().get(&rid).map(|r| r.closed_promise);
        if let Some(p) = p {
            js_promise_reject(p, reason);
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_controller_desired_size(stream_handle: f64) -> f64 {
    let id = stream_handle as usize;
    let g = READABLE_STREAMS.lock().unwrap();
    match g.get(&id) {
        Some(s) if s.state == ReadableState::Readable => {
            (s.high_water_mark - s.chunks.len() as f64).max(0.0)
        }
        Some(s) if s.state == ReadableState::Errored => f64::NAN,
        _ => 0.0,
    }
}

// ─────────────────────────────────────────────────────────────────────
// ReadableStreamDefaultReader FFI
// ─────────────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn js_reader_read(reader_handle: f64) -> *mut Promise {
    let promise = js_promise_new();
    let reader_id = reader_handle as usize;
    let stream_id = match READERS.lock().unwrap().get(&reader_id) {
        Some(r) if r.locked => r.stream_handle,
        Some(_) => {
            let err = make_error_with_message("Reader is no longer locked to a stream");
            js_promise_reject(promise, f64::from_bits(err));
            return promise;
        }
        None => {
            let err = make_error_with_message("Invalid reader");
            js_promise_reject(promise, f64::from_bits(err));
            return promise;
        }
    };
    let outcome: Option<(u64, bool, bool)> = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => {
                if let Some(c) = s.chunks.pop_front() {
                    Some((c, false, false))
                } else if s.state == ReadableState::Closed {
                    Some((TAG_UNDEFINED, true, false))
                } else if s.state == ReadableState::Errored {
                    Some((s.error_value, false, true))
                } else {
                    s.pending_reads.push_back(promise);
                    None
                }
            }
            None => Some((TAG_UNDEFINED, true, false)),
        }
    };
    match outcome {
        Some((value, _, true)) => {
            js_promise_reject(promise, f64::from_bits(value));
        }
        Some((value, done, false)) => {
            let result = build_iter_result(value, done);
            js_promise_resolve(promise, f64::from_bits(result));
        }
        None => {}
    }
    maybe_pull(stream_id);
    promise
}

#[no_mangle]
pub unsafe extern "C" fn js_reader_release_lock(reader_handle: f64) -> f64 {
    let reader_id = reader_handle as usize;
    let stream_id = {
        let mut g = READERS.lock().unwrap();
        match g.get_mut(&reader_id) {
            Some(r) => {
                r.locked = false;
                r.stream_handle
            }
            None => return f64::from_bits(TAG_UNDEFINED),
        }
    };
    if let Some(s) = READABLE_STREAMS.lock().unwrap().get_mut(&stream_id) {
        s.reader_handle = None;
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_reader_closed(reader_handle: f64) -> *mut Promise {
    let reader_id = reader_handle as usize;
    match READERS.lock().unwrap().get(&reader_id) {
        Some(r) => r.closed_promise,
        None => {
            let p = js_promise_new();
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            p
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_reader_cancel(reader_handle: f64, reason: f64) -> *mut Promise {
    let reader_id = reader_handle as usize;
    let stream_id = match READERS.lock().unwrap().get(&reader_id) {
        Some(r) => r.stream_handle,
        None => {
            let p = js_promise_new();
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            return p;
        }
    };
    js_readable_stream_cancel(stream_id as f64, reason)
}

// ─────────────────────────────────────────────────────────────────────
// tee / pipeTo / pipeThrough
// ─────────────────────────────────────────────────────────────────────

/// `stream.tee()` — returns an array of two new ReadableStreams. Both
/// branches drain the SOURCE eagerly into separate per-branch queues at
/// tee time. This is correct for the buffered consumers Perry exposes
/// (`blob.stream()` / `response.body` are pre-buffered) and the "user
/// source already enqueued everything synchronously in start" pattern.
/// Streams that lazily produce chunks via `pull` after tee will only see
/// chunks present at the tee call — the same trade-off Node's
/// `Readable.from([...]).tee()` makes for already-buffered iterables.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_tee(stream_handle: f64) -> f64 {
    let id = stream_handle as usize;
    let chunks: Vec<u64> = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) if s.reader_handle.is_none() => {
                let drained: Vec<u64> = s.chunks.drain(..).collect();
                s.state = ReadableState::Closed;
                drained
            }
            _ => Vec::new(),
        }
    };

    let id_a = next_id(&NEXT_RS_ID);
    let id_b = next_id(&NEXT_RS_ID);
    {
        let mut g = READABLE_STREAMS.lock().unwrap();
        for new_id in [id_a, id_b] {
            g.insert(
                new_id,
                ReadableStreamData {
                    state: ReadableState::Closed,
                    chunks: chunks.iter().copied().collect(),
                    pending_reads: VecDeque::new(),
                    start_cb: 0,
                    pull_cb: 0,
                    cancel_cb: 0,
                    high_water_mark: 1.0,
                    pulling: false,
                    started: true,
                    reader_handle: None,
                    error_value: 0,
                    canceled: false,
                },
            );
        }
    }

    let arr = js_array_alloc(2);
    js_array_push(arr, JSValue::from_bits(f64::to_bits(id_a as f64)));
    js_array_push(arr, JSValue::from_bits(f64::to_bits(id_b as f64)));
    f64::from_bits(JSValue::object_ptr(arr as *mut u8).bits())
}

/// `readable.pipeTo(writable)` — drives the readable into the writable
/// synchronously chunk-by-chunk. Returns a Promise that resolves when
/// the writable closes cleanly, or rejects on error. Synchronous because
/// our buffered model has all bytes resident already; an async loop here
/// would just queue tasks against an empty event loop.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_pipe_to(
    readable_handle: f64,
    writable_handle: f64,
) -> *mut Promise {
    let promise = js_promise_new();
    let r_id = readable_handle as usize;
    let w_id = writable_handle as usize;

    loop {
        let chunk_or_done: Result<u64, bool> = {
            let mut g = READABLE_STREAMS.lock().unwrap();
            match g.get_mut(&r_id) {
                Some(s) => {
                    if let Some(c) = s.chunks.pop_front() {
                        Ok(c)
                    } else if s.state == ReadableState::Closed {
                        Err(true)
                    } else if s.state == ReadableState::Errored {
                        let e = s.error_value;
                        js_promise_reject(promise, f64::from_bits(e));
                        return promise;
                    } else {
                        Err(true)
                    }
                }
                None => Err(true),
            }
        };
        match chunk_or_done {
            Ok(chunk) => {
                // TransformStream's writable side has write_cb=0 — route
                // through transform_write so the user transform fn runs.
                if TRANSFORM_PAIRS.lock().unwrap().contains_key(&w_id) {
                    let _ = transform_write(w_id, f64::from_bits(chunk));
                } else {
                    let write_cb = WRITABLE_STREAMS
                        .lock()
                        .unwrap()
                        .get(&w_id)
                        .map(|w| w.write_cb)
                        .unwrap_or(0);
                    if write_cb != 0 {
                        js_closure_call1(write_cb as *const ClosureHeader, f64::from_bits(chunk));
                    }
                }
            }
            Err(_done) => break,
        }
    }

    // Close downstream — TransformStream routes through transform_close
    // so flush_cb runs and the readable side is closed.
    if TRANSFORM_PAIRS.lock().unwrap().contains_key(&w_id) {
        let _ = transform_close(w_id);
    } else {
        let close_cb = WRITABLE_STREAMS
            .lock()
            .unwrap()
            .get(&w_id)
            .map(|w| w.close_cb)
            .unwrap_or(0);
        if close_cb != 0 {
            js_closure_call0(close_cb as *const ClosureHeader);
        }
        if let Some(w) = WRITABLE_STREAMS.lock().unwrap().get_mut(&w_id) {
            w.state = WritableState::Closed;
            let cp = w.closed_promise;
            js_promise_resolve(cp, f64::from_bits(TAG_UNDEFINED));
        }
    }
    js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    promise
}

/// `readable.pipeThrough({readable, writable})` — pipeTo into the
/// transform's writable side, return its readable side. Caller already
/// destructured the TransformStream into its readable / writable
/// handles.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_pipe_through(
    readable_handle: f64,
    transform_writable_handle: f64,
    transform_readable_handle: f64,
) -> f64 {
    let _ = js_readable_stream_pipe_to(readable_handle, transform_writable_handle);
    transform_readable_handle
}

// ─────────────────────────────────────────────────────────────────────
// WritableStream FFI
// ─────────────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_new(
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
    id as f64
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_get_writer(stream_handle: f64) -> f64 {
    ensure_gc_registered();
    let id = stream_handle as usize;
    let mut g = WRITABLE_STREAMS.lock().unwrap();
    let s = match g.get_mut(&id) {
        Some(s) => s,
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    if s.writer_handle.is_some() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let writer_id = next_id(&NEXT_WRITER_ID);
    s.writer_handle = Some(writer_id);
    let closed_p = s.closed_promise;
    let ready_p = s.ready_promise;
    drop(g);
    WRITERS.lock().unwrap().insert(
        writer_id,
        WriterData {
            stream_handle: id,
            locked: true,
            closed_promise: closed_p,
            ready_promise: ready_p,
        },
    );
    writer_id as f64
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_locked(stream_handle: f64) -> f64 {
    let id = stream_handle as usize;
    let g = WRITABLE_STREAMS.lock().unwrap();
    let locked = g
        .get(&id)
        .map(|s| s.writer_handle.is_some())
        .unwrap_or(false);
    f64::from_bits(if locked { TAG_TRUE } else { TAG_FALSE })
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_close(stream_handle: f64) -> *mut Promise {
    let promise = js_promise_new();
    let id = stream_handle as usize;
    let (cb, closed_p) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) => {
                s.state = WritableState::Closed;
                (s.close_cb, s.closed_promise)
            }
            None => (0, std::ptr::null_mut()),
        }
    };
    if cb != 0 {
        js_closure_call0(cb as *const ClosureHeader);
    }
    if !closed_p.is_null() {
        js_promise_resolve(closed_p, f64::from_bits(TAG_UNDEFINED));
    }
    js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    promise
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_abort(stream_handle: f64, reason: f64) -> *mut Promise {
    let promise = js_promise_new();
    let id = stream_handle as usize;
    let reason_bits = reason.to_bits();
    let (cb, closed_p) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) => {
                s.state = WritableState::Errored;
                s.error_value = reason_bits;
                (s.abort_cb, s.closed_promise)
            }
            None => (0, std::ptr::null_mut()),
        }
    };
    if cb != 0 {
        js_closure_call1(cb as *const ClosureHeader, reason);
    }
    if !closed_p.is_null() {
        js_promise_reject(closed_p, reason);
    }
    js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    promise
}

// ─────────────────────────────────────────────────────────────────────
// WritableStreamDefaultWriter FFI
// ─────────────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn js_writer_write(writer_handle: f64, chunk: f64) -> *mut Promise {
    let promise = js_promise_new();
    let writer_id = writer_handle as usize;
    let stream_id = match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) if w.locked => w.stream_handle,
        _ => {
            let err = make_error_with_message("Writer is no longer locked to a stream");
            js_promise_reject(promise, f64::from_bits(err));
            return promise;
        }
    };
    if TRANSFORM_PAIRS.lock().unwrap().contains_key(&stream_id) {
        return transform_write(stream_id, chunk);
    }
    let cb = match WRITABLE_STREAMS.lock().unwrap().get(&stream_id) {
        Some(s) if s.state == WritableState::Writable => s.write_cb,
        Some(s) if s.state == WritableState::Errored => {
            let e = s.error_value;
            js_promise_reject(promise, f64::from_bits(e));
            return promise;
        }
        _ => {
            let err = make_error_with_message("Stream is closed or closing");
            js_promise_reject(promise, f64::from_bits(err));
            return promise;
        }
    };
    if cb != 0 {
        js_closure_call1(cb as *const ClosureHeader, chunk);
    }
    js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    promise
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_close(writer_handle: f64) -> *mut Promise {
    let writer_id = writer_handle as usize;
    let stream_id = match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) => w.stream_handle,
        None => {
            let p = js_promise_new();
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            return p;
        }
    };
    if TRANSFORM_PAIRS.lock().unwrap().contains_key(&stream_id) {
        return transform_close(stream_id);
    }
    js_writable_stream_close(stream_id as f64)
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_abort(writer_handle: f64, reason: f64) -> *mut Promise {
    let writer_id = writer_handle as usize;
    let stream_id = match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) => w.stream_handle,
        None => {
            let p = js_promise_new();
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            return p;
        }
    };
    js_writable_stream_abort(stream_id as f64, reason)
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_release_lock(writer_handle: f64) -> f64 {
    let writer_id = writer_handle as usize;
    let stream_id = {
        let mut g = WRITERS.lock().unwrap();
        match g.get_mut(&writer_id) {
            Some(w) => {
                w.locked = false;
                w.stream_handle
            }
            None => return f64::from_bits(TAG_UNDEFINED),
        }
    };
    if let Some(s) = WRITABLE_STREAMS.lock().unwrap().get_mut(&stream_id) {
        s.writer_handle = None;
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_closed(writer_handle: f64) -> *mut Promise {
    let writer_id = writer_handle as usize;
    match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) => w.closed_promise,
        None => {
            let p = js_promise_new();
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            p
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_ready(writer_handle: f64) -> *mut Promise {
    let writer_id = writer_handle as usize;
    match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) => w.ready_promise,
        None => {
            let p = js_promise_new();
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            p
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_desired_size(writer_handle: f64) -> f64 {
    let writer_id = writer_handle as usize;
    let stream_id = match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) => w.stream_handle,
        None => return 0.0,
    };
    let g = WRITABLE_STREAMS.lock().unwrap();
    match g.get(&stream_id) {
        Some(s) if s.state == WritableState::Writable => s.high_water_mark,
        Some(s) if s.state == WritableState::Errored => f64::NAN,
        _ => 0.0,
    }
}

// ─────────────────────────────────────────────────────────────────────
// TransformStream FFI
// ─────────────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn js_transform_stream_new(
    transform_bits: f64,
    flush_bits: f64,
    hwm: f64,
) -> f64 {
    ensure_gc_registered();
    let transform_cb = closure_from_bits(transform_bits.to_bits());
    let flush_cb = closure_from_bits(flush_bits.to_bits());

    // Allocate the readable side empty (controller is its own handle).
    let readable_id = alloc_readable(0, 0, 0, hwm);
    {
        let mut g = READABLE_STREAMS.lock().unwrap();
        if let Some(s) = g.get_mut(&readable_id) {
            s.started = true;
        }
    }

    // Allocate writable side; its write_cb is synthesized via the
    // dispatcher table below to invoke transform(chunk, controller).
    let writable_id = next_id(&NEXT_WS_ID);
    let ready = js_promise_new();
    let closed = js_promise_new();
    js_promise_resolve(ready, f64::from_bits(TAG_UNDEFINED));
    WRITABLE_STREAMS.lock().unwrap().insert(
        writable_id,
        WritableStreamData {
            state: WritableState::Writable,
            // Sentinel: write_cb=0, close_cb=0 — the dispatcher checks
            // TRANSFORM_PAIRS first and routes through the user transform_cb /
            // flush_cb instead.
            write_cb: 0,
            close_cb: 0,
            abort_cb: 0,
            write_queue: VecDeque::new(),
            in_flight: false,
            high_water_mark: if hwm.is_nan() || hwm <= 0.0 { 1.0 } else { hwm },
            writer_handle: None,
            error_value: 0,
            ready_promise: ready,
            closed_promise: closed,
        },
    );

    let id = next_id(&NEXT_TS_ID);
    TRANSFORM_STREAMS.lock().unwrap().insert(
        id,
        TransformStreamData {
            readable_handle: readable_id,
            writable_handle: writable_id,
            transform_cb,
            flush_cb,
        },
    );
    TRANSFORM_PAIRS.lock().unwrap().insert(writable_id, id);
    id as f64
}

#[no_mangle]
pub unsafe extern "C" fn js_transform_stream_readable(handle: f64) -> f64 {
    let id = handle as usize;
    TRANSFORM_STREAMS
        .lock()
        .unwrap()
        .get(&id)
        .map(|t| t.readable_handle as f64)
        .unwrap_or(f64::from_bits(TAG_UNDEFINED))
}

#[no_mangle]
pub unsafe extern "C" fn js_transform_stream_writable(handle: f64) -> f64 {
    let id = handle as usize;
    TRANSFORM_STREAMS
        .lock()
        .unwrap()
        .get(&id)
        .map(|t| t.writable_handle as f64)
        .unwrap_or(f64::from_bits(TAG_UNDEFINED))
}

lazy_static::lazy_static! {
    static ref TRANSFORM_PAIRS: Mutex<HashMap<usize, usize>> = Mutex::new(HashMap::new());
}

/// Replacement `writer.write` for the writable side of a TransformStream
/// — invokes the user transform with (chunk, transformController) where
/// the transformController is the readable-side stream handle (so
/// `controller.enqueue(...)` reuses the readable controller path).
unsafe fn transform_write(writable_id: usize, chunk: f64) -> *mut Promise {
    let promise = js_promise_new();
    let (transform_cb, readable_id) = {
        let pairs = TRANSFORM_PAIRS.lock().unwrap();
        match pairs.get(&writable_id) {
            Some(t_id) => {
                let g = TRANSFORM_STREAMS.lock().unwrap();
                match g.get(t_id) {
                    Some(t) => (t.transform_cb, t.readable_handle),
                    None => (0, 0),
                }
            }
            None => (0, 0),
        }
    };
    if transform_cb != 0 && readable_id != 0 {
        js_closure_call2(
            transform_cb as *const ClosureHeader,
            chunk,
            readable_id as f64,
        );
    } else {
        // Identity transform — pass-through.
        js_readable_stream_controller_enqueue(readable_id as f64, chunk);
    }
    js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    promise
}

unsafe fn transform_close(writable_id: usize) -> *mut Promise {
    let promise = js_promise_new();
    let (flush_cb, readable_id) = {
        let pairs = TRANSFORM_PAIRS.lock().unwrap();
        match pairs.get(&writable_id) {
            Some(t_id) => {
                let g = TRANSFORM_STREAMS.lock().unwrap();
                match g.get(t_id) {
                    Some(t) => (t.flush_cb, t.readable_handle),
                    None => (0, 0),
                }
            }
            None => (0, 0),
        }
    };
    if flush_cb != 0 && readable_id != 0 {
        js_closure_call1(flush_cb as *const ClosureHeader, readable_id as f64);
    }
    if readable_id != 0 {
        js_readable_stream_controller_close(readable_id as f64);
    }
    if let Some(s) = WRITABLE_STREAMS.lock().unwrap().get_mut(&writable_id) {
        s.state = WritableState::Closed;
        let cp = s.closed_promise;
        js_promise_resolve(cp, f64::from_bits(TAG_UNDEFINED));
    }
    js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    promise
}

// ─────────────────────────────────────────────────────────────────────
// Stubs for deferred surface (issue #237 followups)
// ─────────────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn js_streams_throw_byob_not_implemented() -> f64 {
    let err = make_error_with_message("BYOB readers are not yet implemented (issue #237 followup)");
    perry_runtime::exception::js_throw(f64::from_bits(err));
}

#[no_mangle]
pub unsafe extern "C" fn js_streams_throw_byte_length_not_implemented() -> f64 {
    let err = make_error_with_message(
        "ByteLengthQueuingStrategy is not yet implemented (issue #237 followup)",
    );
    perry_runtime::exception::js_throw(f64::from_bits(err));
}

// ─────────────────────────────────────────────────────────────────────
// Public helpers used by other crates / tests
// ─────────────────────────────────────────────────────────────────────

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
    let handle = js_transform_stream_new(transform_bits, flush_bits, hwm);
    attach_handle_to_this(this_bits, handle as usize);
    f64::from_bits(TAG_UNDEFINED)
}

/// Read every queued chunk into a Vec<u8>, draining the stream. Used by
/// `new Response(stream)` / `new Request(url, { body: stream })` — we
/// drain the buffered chunks at construction time so the resulting
/// Response.body bytes match what a real serializer would produce.
#[doc(hidden)]
pub fn drain_readable_into_bytes(stream_id: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let chunks: Vec<u64> = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => {
                let drained: Vec<u64> = s.chunks.drain(..).collect();
                s.state = ReadableState::Closed;
                drained
            }
            None => return out,
        }
    };
    for chunk in chunks {
        unsafe {
            if let Some(bytes) = read_bytes_from_chunk(chunk) {
                out.extend_from_slice(&bytes);
            }
        }
    }
    out
}
