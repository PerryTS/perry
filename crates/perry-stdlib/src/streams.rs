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
//! Stubs: BYOB readers, full custom `QueuingStrategy` size accounting, and
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

mod pipe;
use self::pipe::js_readable_stream_pipe_to;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
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
    strategy_size_cb: i64,
    high_water_mark: f64,
    is_byte_stream: bool,
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
    static ref WRITABLE_STREAMS: Mutex<HashMap<usize, WritableStreamData>> = Mutex::new(HashMap::new());
    static ref TRANSFORM_STREAMS: Mutex<HashMap<usize, TransformStreamData>> = Mutex::new(HashMap::new());
    static ref READERS: Mutex<HashMap<usize, ReaderData>> = Mutex::new(HashMap::new());
    static ref WRITERS: Mutex<HashMap<usize, WriterData>> = Mutex::new(HashMap::new());
    // #1545: ONE id counter shared across all five Web Streams registries.
    // Two reasons: (1) ids are globally unique across readable/writable/
    // transform/reader/writer, so the runtime handle dispatcher can tell which
    // registry a handle belongs to unambiguously; (2) the high base (0x40000)
    // puts stream handles well above every other handle subsystem's id range
    // (commander/fastify/net/... all start at 1 and never approach it), so a
    // stream handle never collides with another subsystem's handle while still
    // staying under the 0x100000 small-handle detection threshold. This is what
    // lets `js_handle_method_dispatch` safely route stream methods at runtime
    // for receivers whose static type the codegen lost (e.g.
    // `src.pipeThrough(ts).getReader()`, `ts.readable.getReader()`).
    static ref NEXT_STREAM_ID: Mutex<usize> = Mutex::new(0x40000);
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
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:streams",
            scan_stream_roots_mut,
        );
        perry_runtime::node_submodules::js_register_stream_consumer_callbacks(
            js_readable_stream_get_reader,
            js_reader_read,
        );
    });
}

#[allow(dead_code)]
fn scan_stream_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = perry_runtime::gc::RuntimeRootVisitor::for_copy(mark);
    scan_stream_roots_mut(&mut visitor);
}

fn visit_stream_value_slot(
    visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>,
    slot: &mut u64,
) {
    let top = *slot >> 48;
    if top == 0x7FFD || top == 0x7FFF {
        visitor.visit_nanbox_u64_slot(slot);
    }
}

fn scan_stream_roots_mut(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    if let Ok(mut map) = READABLE_STREAMS.lock() {
        for s in map.values_mut() {
            visitor.visit_i64_slot(&mut s.start_cb);
            visitor.visit_i64_slot(&mut s.pull_cb);
            visitor.visit_i64_slot(&mut s.cancel_cb);
            visitor.visit_i64_slot(&mut s.strategy_size_cb);
            for c in s.chunks.iter_mut() {
                visit_stream_value_slot(visitor, c);
            }
            for p in s.pending_reads.iter_mut() {
                visitor.visit_raw_mut_ptr_slot(p);
            }
            if s.state == ReadableState::Errored {
                visit_stream_value_slot(visitor, &mut s.error_value);
            }
        }
    }
    if let Ok(mut map) = WRITABLE_STREAMS.lock() {
        for s in map.values_mut() {
            visitor.visit_i64_slot(&mut s.write_cb);
            visitor.visit_i64_slot(&mut s.close_cb);
            visitor.visit_i64_slot(&mut s.abort_cb);
            for (chunk, p) in s.write_queue.iter_mut() {
                visit_stream_value_slot(visitor, chunk);
                visitor.visit_raw_mut_ptr_slot(p);
            }
            visitor.visit_raw_mut_ptr_slot(&mut s.ready_promise);
            visitor.visit_raw_mut_ptr_slot(&mut s.closed_promise);
            if s.state == WritableState::Errored {
                visit_stream_value_slot(visitor, &mut s.error_value);
            }
        }
    }
    if let Ok(mut map) = TRANSFORM_STREAMS.lock() {
        for t in map.values_mut() {
            visitor.visit_i64_slot(&mut t.transform_cb);
            visitor.visit_i64_slot(&mut t.flush_cb);
        }
    }
    if let Ok(mut map) = READERS.lock() {
        for r in map.values_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut r.closed_promise);
        }
    }
    if let Ok(mut map) = WRITERS.lock() {
        for w in map.values_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut w.closed_promise);
            visitor.visit_raw_mut_ptr_slot(&mut w.ready_promise);
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

unsafe fn raw_pointer_addr(bits: u64) -> Option<usize> {
    let top = bits >> 48;
    if top == 0x7FFD || top == 0x7FFF {
        Some((bits & POINTER_MASK) as usize)
    } else if top == 0 && bits >= 0x10000 {
        Some(bits as usize)
    } else {
        None
    }
}

unsafe fn is_byte_stream_enqueue_chunk(chunk_bits: u64) -> bool {
    let Some(addr) = raw_pointer_addr(chunk_bits) else {
        return false;
    };
    perry_runtime::typedarray::lookup_typed_array_kind(addr).is_some()
        || perry_runtime::buffer::is_data_view(addr)
        || perry_runtime::buffer::is_uint8array_buffer(addr)
        || (perry_runtime::buffer::is_registered_buffer(addr)
            && !perry_runtime::buffer::is_any_array_buffer(addr))
}

unsafe fn make_error_with_message(msg: &str) -> u64 {
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = perry_runtime::error::js_error_new_with_message(s);
    JSValue::pointer(err as *const u8).bits()
}

unsafe fn make_type_error_with_message(msg: &str) -> u64 {
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = perry_runtime::error::js_typeerror_new(s);
    JSValue::pointer(err as *const u8).bits()
}

unsafe fn make_range_error_with_code(message: &str, code: &'static str) -> u64 {
    let s = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(s, code);
    let err = perry_runtime::error::js_rangeerror_new(s);
    JSValue::pointer(err as *const u8).bits()
}

unsafe fn throw_type_error(message: &str) -> ! {
    let err = make_type_error_with_message(message);
    perry_runtime::exception::js_throw(f64::from_bits(err))
}

unsafe fn throw_range_error_with_code(message: &str, code: &'static str) -> ! {
    let err = make_range_error_with_code(message, code);
    perry_runtime::exception::js_throw(f64::from_bits(err))
}

unsafe fn reject_type_error(promise: *mut Promise, message: &str) {
    let err = make_type_error_with_message(message);
    js_promise_reject(promise, f64::from_bits(err));
}

unsafe fn throw_invalid_arg_type(message: &str) -> ! {
    let s = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = perry_runtime::error::js_typeerror_new(s);
    perry_runtime::node_submodules::register_error_code_pub(s, "ERR_INVALID_ARG_TYPE");
    perry_runtime::exception::js_throw(perry_runtime::value::js_nanbox_pointer(err as i64))
}

fn alloc_readable(start_cb: i64, pull_cb: i64, cancel_cb: i64, hwm: f64) -> usize {
    alloc_readable_with_type(start_cb, pull_cb, cancel_cb, hwm, false)
}

fn alloc_readable_with_type(
    start_cb: i64,
    pull_cb: i64,
    cancel_cb: i64,
    hwm: f64,
    is_byte_stream: bool,
) -> usize {
    alloc_readable_with_strategy(start_cb, pull_cb, cancel_cb, hwm, is_byte_stream, 0)
}

fn alloc_readable_with_strategy(
    start_cb: i64,
    pull_cb: i64,
    cancel_cb: i64,
    hwm: f64,
    is_byte_stream: bool,
    strategy_size_cb: i64,
) -> usize {
    let id = next_id(&NEXT_STREAM_ID);
    READABLE_STREAMS.lock().unwrap().insert(
        id,
        ReadableStreamData {
            state: ReadableState::Readable,
            chunks: VecDeque::new(),
            pending_reads: VecDeque::new(),
            start_cb,
            pull_cb,
            cancel_cb,
            strategy_size_cb,
            high_water_mark: if hwm.is_nan() || hwm <= 0.0 { 1.0 } else { hwm },
            is_byte_stream,
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
    let id = next_id(&NEXT_STREAM_ID);
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
    js_readable_stream_new_with_source_type(
        start_bits,
        pull_bits,
        cancel_bits,
        hwm,
        f64::from_bits(TAG_UNDEFINED),
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_new_with_source_type(
    start_bits: f64,
    pull_bits: f64,
    cancel_bits: f64,
    hwm: f64,
    source_type: f64,
) -> f64 {
    ensure_gc_registered();
    let id = alloc_readable_with_type(
        closure_from_bits(start_bits.to_bits()),
        closure_from_bits(pull_bits.to_bits()),
        closure_from_bits(cancel_bits.to_bits()),
        hwm,
        value_string_equals(source_type, b"bytes"),
    );
    invoke_start(id);
    maybe_pull(id);
    id as f64
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_new_with_strategy_and_source_type(
    start_bits: f64,
    pull_bits: f64,
    cancel_bits: f64,
    strategy: f64,
    source_type: f64,
) -> f64 {
    ensure_gc_registered();
    let is_byte_stream = value_string_equals(source_type, b"bytes");
    let size_cb = if is_byte_stream {
        0
    } else {
        read_queuing_strategy_size(strategy)
    };
    let id = alloc_readable_with_strategy(
        closure_from_bits(start_bits.to_bits()),
        closure_from_bits(pull_bits.to_bits()),
        closure_from_bits(cancel_bits.to_bits()),
        f64::from_bits(read_high_water_mark(strategy)),
        is_byte_stream,
        size_cb,
    );
    invoke_start(id);
    maybe_pull(id);
    id as f64
}

// ── #1545: node:stream/web QueuingStrategy classes ──────────────────────
//
// `new CountQueuingStrategy({ highWaterMark })` and
// `new ByteLengthQueuingStrategy({ highWaterMark })` produce plain objects
// with a numeric `highWaterMark` field and a `size` method, matching the
// WHATWG built-ins. CountQueuingStrategy.size always returns 1 (chunks are
// counted one-by-one); ByteLengthQueuingStrategy.size returns
// `chunk.byteLength`. Both are surfaced through codegen's builtin-`new`
// dispatch (lower_call/builtin.rs); the import binding lives in
// node_submodules.

/// `CountQueuingStrategy.prototype.size` — every chunk counts as 1.
extern "C" fn count_queuing_strategy_size(_c: *const ClosureHeader, _chunk: f64) -> f64 {
    1.0
}

/// `ByteLengthQueuingStrategy.prototype.size` — `chunk.byteLength`.
extern "C" fn byte_length_queuing_strategy_size(_c: *const ClosureHeader, chunk: f64) -> f64 {
    // Mirror Node's `return chunk.byteLength`: the generic property getter
    // resolves `.byteLength` for both registered buffers/typed arrays and
    // plain `{ byteLength }` objects.
    unsafe { perry_runtime::value::js_get_property(chunk, b"byteLength".as_ptr() as i64, 10) }
}

/// Build a `{ highWaterMark, size }` object for a queuing strategy. `hwm_bits`
/// is the raw JSValue bits read from the caller's options object.
unsafe fn build_queuing_strategy(
    hwm_bits: u64,
    size_fn: extern "C" fn(*const ClosureHeader, f64) -> f64,
) -> f64 {
    let obj = js_object_alloc(0, 2);
    let keys = js_array_alloc(2);
    let k_hwm = js_string_from_bytes(b"highWaterMark".as_ptr(), 13);
    let k_size = js_string_from_bytes(b"size".as_ptr(), 4);
    js_array_push(keys, JSValue::string_ptr(k_hwm));
    js_array_push(keys, JSValue::string_ptr(k_size));
    js_object_set_field(obj, 0, JSValue::from_bits(hwm_bits));
    // `size` is a 1-arg native function value. Register the arity so closure
    // dispatch pads/forwards the single `chunk` argument correctly.
    let fn_ptr = size_fn as *const u8;
    perry_runtime::closure::js_register_closure_arity(fn_ptr, 1);
    let closure = perry_runtime::closure::js_closure_alloc(fn_ptr, 0);
    js_object_set_field(obj, 1, JSValue::pointer(closure as *const u8));
    js_object_set_keys(obj, keys);
    f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits())
}

/// Read `opts.highWaterMark` (raw JSValue bits) from a strategy's options
/// object; undefined when absent (matches `new CountQueuingStrategy({})`).
unsafe fn read_high_water_mark(opts: f64) -> u64 {
    perry_runtime::value::js_get_property(opts, b"highWaterMark".as_ptr() as i64, 13).to_bits()
}

unsafe fn read_queuing_strategy_size(strategy: f64) -> i64 {
    let size = perry_runtime::value::js_get_property(strategy, b"size".as_ptr() as i64, 4);
    closure_from_bits(size.to_bits())
}

/// `new CountQueuingStrategy({ highWaterMark })`.
#[no_mangle]
pub unsafe extern "C" fn js_count_queuing_strategy_new(opts: f64) -> f64 {
    let hwm = read_high_water_mark(opts);
    build_queuing_strategy(hwm, count_queuing_strategy_size)
}

/// `new ByteLengthQueuingStrategy({ highWaterMark })`.
#[no_mangle]
pub unsafe extern "C" fn js_byte_length_queuing_strategy_new(opts: f64) -> f64 {
    let hwm = read_high_water_mark(opts);
    build_queuing_strategy(hwm, byte_length_queuing_strategy_size)
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
    js_readable_stream_get_reader_with_options(stream_handle, f64::from_bits(TAG_UNDEFINED))
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_get_reader_with_options(
    stream_handle: f64,
    options: f64,
) -> f64 {
    let byob_requested = option_string_equals(options, b"mode", b"byob");
    let id = stream_handle as usize;
    if byob_requested {
        let is_non_byte_stream = {
            let g = READABLE_STREAMS.lock().unwrap();
            g.get(&id).map(|s| !s.is_byte_stream).unwrap_or(false)
        };
        if is_non_byte_stream {
            throw_type_error("ReadableStream BYOB reader requires a byte stream");
        }
    }

    ensure_gc_registered();
    let id = stream_handle as usize;
    let was_locked = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        let s = match g.get_mut(&id) {
            Some(s) => s,
            None => return f64::from_bits(TAG_UNDEFINED),
        };
        if s.reader_handle.is_some() {
            true
        } else {
            let reader_id = next_id(&NEXT_STREAM_ID);
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
            return reader_id as f64;
        }
    };
    if was_locked {
        throw_type_error("ReadableStream is locked");
    }
    f64::from_bits(TAG_UNDEFINED)
}

unsafe fn option_string_equals(options: f64, name: &[u8], expected: &[u8]) -> bool {
    let value =
        perry_runtime::value::js_get_property(options, name.as_ptr() as i64, name.len() as i64);
    value_string_equals(value, expected)
}

unsafe fn value_string_equals(value: f64, expected: &[u8]) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        return false;
    }

    let ptr = perry_runtime::value::js_get_string_pointer_unified(value)
        as *const perry_runtime::StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return false;
    }

    let len = (*ptr).byte_len as usize;
    if len != expected.len() {
        return false;
    }

    let data = (ptr as *const u8).add(std::mem::size_of::<perry_runtime::StringHeader>());
    std::slice::from_raw_parts(data, len) == expected
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
    js_readable_stream_cancel_inner(stream_handle, reason, false)
}

unsafe fn js_readable_stream_cancel_inner(
    stream_handle: f64,
    reason: f64,
    allow_locked: bool,
) -> *mut Promise {
    let promise = js_promise_new();
    let id = stream_handle as usize;
    let mut locked_reject = false;
    let cb = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) => {
                if !allow_locked && s.reader_handle.is_some() {
                    locked_reject = true;
                    0
                } else {
                    s.canceled = true;
                    s.state = ReadableState::Closed;
                    s.chunks.clear();
                    s.cancel_cb
                }
            }
            None => 0,
        }
    };
    if locked_reject {
        reject_type_error(promise, "ReadableStream is locked");
        return promise;
    }
    if cb != 0 {
        js_closure_call1(cb as *const ClosureHeader, reason);
    }
    close_pending(id);
    js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    promise
}

// Refs #915 (effect smoke fallback path): the `crate::fetch::*` helpers
// referenced below only exist behind the `http-client` feature, so the
// streams-only auto-optimize build (`bundled-streams` alone, no
// `http-client`) failed to compile. Gate the two blob/response constructors
// on `http-client` and provide no-op stubs when it's off — anything that
// actually needs a Blob/Response went through `http-client` anyway.
#[cfg(feature = "http-client")]
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_blob(blob_id: f64) -> f64 {
    let bytes = crate::fetch::blob_bytes_clone(blob_id as usize).unwrap_or_default();
    alloc_readable_from_bytes(bytes) as f64
}

#[cfg(not(feature = "http-client"))]
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_blob(_blob_id: f64) -> f64 {
    alloc_readable_from_bytes(Vec::new()) as f64
}

#[cfg(feature = "http-client")]
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_response(resp_id: f64) -> f64 {
    let bytes = crate::fetch::response_bytes_clone(resp_id as usize).unwrap_or_default();
    alloc_readable_from_bytes(bytes) as f64
}

#[cfg(not(feature = "http-client"))]
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_response(_resp_id: f64) -> f64 {
    alloc_readable_from_bytes(Vec::new()) as f64
}

/// `ReadableStream.from(iterable)` (Node 20+, #1645) — build a Web
/// ReadableStream pre-loaded with the iterable's items, then closed. Today we
/// handle the synchronous-array case (the overwhelmingly common form: a literal
/// array, a spread, `[...set]`, etc.); each element becomes one chunk so
/// `getReader().read()` / `for await` yield them in order, then `done`.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_iterable(value: f64) -> f64 {
    ensure_gc_registered();
    let bits = value.to_bits();
    let top = bits >> 48;
    let ptr_addr = if top == 0x7FFD || top == 0x7FFF {
        Some((bits & POINTER_MASK) as usize)
    } else if top == 0 && bits >= 0x10000 {
        Some(bits as usize)
    } else {
        None
    };

    let chunks: Vec<u64> = if perry_runtime::array::js_array_is_array(value).to_bits() == TAG_TRUE {
        let arr_ptr = ptr_addr.unwrap_or(0) as *const perry_runtime::ArrayHeader;
        let len = perry_runtime::array::js_array_length(arr_ptr);
        (0..len)
            .map(|i| perry_runtime::array::js_array_get(arr_ptr, i).bits())
            .collect()
    } else if let Some(addr) = ptr_addr {
        if perry_runtime::typedarray::lookup_typed_array_kind(addr).is_some() {
            let ta = addr as *const perry_runtime::typedarray::TypedArrayHeader;
            let len = perry_runtime::typedarray::js_typed_array_length(ta).max(0);
            (0..len)
                .map(|i| perry_runtime::typedarray::js_typed_array_get(ta, i).to_bits())
                .collect()
        } else if perry_runtime::buffer::is_registered_buffer(addr)
            && !perry_runtime::buffer::is_any_array_buffer(addr)
            && !perry_runtime::buffer::is_data_view(addr)
        {
            let buf = addr as *const perry_runtime::buffer::BufferHeader;
            let len = (*buf).length as usize;
            let data = perry_runtime::buffer::buffer_data(buf);
            (0..len).map(|i| (*data.add(i) as f64).to_bits()).collect()
        } else {
            throw_type_error("ReadableStream.from requires an iterable");
        }
    } else {
        throw_type_error("ReadableStream.from requires an iterable");
    };

    let id = alloc_readable(0, 0, 0, 1.0);
    {
        let mut g = READABLE_STREAMS.lock().unwrap();
        if let Some(s) = g.get_mut(&id) {
            s.chunks.extend(chunks);
            s.started = true;
            s.state = ReadableState::Closed;
        }
    }
    id as f64
}

/// #1671: `renderToReadableStream` stream backend. Builds a closed
/// single-chunk readable stream carrying the already-rendered HTML string, so
/// hono's `renderToReadableStream(<App/>)` returns a real Web stream that
/// `getReader()` / `for await` can drain. Registered into the runtime via
/// `js_register_jsx_render_stream` from `js_stdlib_init_dispatch` when
/// `bundled-streams` is linked; absent that, the runtime thunk returns the
/// rendered HTML node directly (degraded but usable).
#[no_mangle]
pub unsafe extern "C" fn js_jsx_render_stream_from_value(html_value: f64) -> f64 {
    let mut arr = perry_runtime::array::js_array_alloc(1);
    arr = perry_runtime::array::js_array_push_f64(arr, html_value);
    let arr_f64 = f64::from_bits(perry_runtime::JSValue::pointer(arr as *const u8).bits());
    js_readable_stream_from_iterable(arr_f64)
}

fn invalid_size_message(size: f64) -> String {
    let received = if size.is_nan() {
        "NaN".to_string()
    } else if size == f64::INFINITY {
        "Infinity".to_string()
    } else if size == f64::NEG_INFINITY {
        "-Infinity".to_string()
    } else {
        format!("{}", size)
    };
    format!("The argument 'size' is invalid. Received {}", received)
}

unsafe fn readable_strategy_size_to_number(value: f64) -> f64 {
    JSValue::from_bits(value.to_bits()).to_number()
}

unsafe fn error_readable_stream(stream_id: usize, reason_bits: u64) {
    let reader_id = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => {
                s.state = ReadableState::Errored;
                s.error_value = reason_bits;
                s.chunks.clear();
                s.reader_handle
            }
            None => return,
        }
    };
    error_pending(stream_id, reason_bits);
    if let Some(rid) = reader_id {
        let p = READERS.lock().unwrap().get(&rid).map(|r| r.closed_promise);
        if let Some(p) = p {
            js_promise_reject(p, f64::from_bits(reason_bits));
        }
    }
}

unsafe fn throw_invalid_readable_strategy_size(stream_id: usize, size: f64) -> ! {
    let message = invalid_size_message(size);
    let err = make_range_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
    error_readable_stream(stream_id, err);
    perry_runtime::exception::js_throw(f64::from_bits(err))
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
    let is_byte_stream = {
        let g = READABLE_STREAMS.lock().unwrap();
        g.get(&id).map(|s| s.is_byte_stream).unwrap_or(false)
    };
    if is_byte_stream && !is_byte_stream_enqueue_chunk(chunk_bits) {
        throw_invalid_arg_type(
            "The \"buffer\" argument must be an instance of Buffer, TypedArray, or DataView",
        );
    }
    let (popped, strategy_size_cb) = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) if s.state == ReadableState::Readable => {
                if let Some(p) = s.pending_reads.pop_front() {
                    (Some(p), 0)
                } else {
                    (None, s.strategy_size_cb)
                }
            }
            _ => return f64::from_bits(TAG_UNDEFINED),
        }
    };
    if let Some(p) = popped {
        let result = build_iter_result(chunk_bits, false);
        js_promise_resolve(p, f64::from_bits(result));
    } else {
        if strategy_size_cb != 0 {
            let size = readable_strategy_size_to_number(js_closure_call1(
                strategy_size_cb as *const ClosureHeader,
                chunk,
            ));
            if size.is_nan() || size < 0.0 || size.is_infinite() {
                throw_invalid_readable_strategy_size(id, size);
            }
        }
        let mut g = READABLE_STREAMS.lock().unwrap();
        if let Some(s) = g.get_mut(&id) {
            if s.state == ReadableState::Readable {
                s.chunks.push_back(chunk_bits);
            }
        }
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
    error_readable_stream(id, reason.to_bits());
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

fn resolved_done_promise() -> f64 {
    unsafe {
        let promise = js_promise_new();
        let result = build_iter_result(TAG_UNDEFINED, true);
        js_promise_resolve(promise, f64::from_bits(result));
        box_promise(promise)
    }
}

fn closure_capture_value(
    func: extern "C" fn(*const ClosureHeader) -> f64,
    value: f64,
) -> *mut ClosureHeader {
    let fn_ptr = func as *const u8;
    perry_runtime::closure::js_register_closure_arity(fn_ptr, 0);
    let closure = perry_runtime::closure::js_closure_alloc(fn_ptr, 1);
    perry_runtime::closure::js_closure_set_capture_ptr(closure, 0, value.to_bits() as i64);
    closure
}

fn closure_capture_value_get(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let bits = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as u64;
    f64::from_bits(bits)
}

extern "C" fn readable_stream_iterator_next(closure: *const ClosureHeader) -> f64 {
    let reader = closure_capture_value_get(closure);
    if reader.to_bits() == TAG_UNDEFINED {
        return resolved_done_promise();
    }
    unsafe { box_promise(js_reader_read(reader)) }
}

extern "C" fn readable_stream_iterator_return(closure: *const ClosureHeader) -> f64 {
    let reader = closure_capture_value_get(closure);
    if reader.to_bits() != TAG_UNDEFINED {
        unsafe {
            let _ = js_reader_release_lock(reader);
        }
    }
    resolved_done_promise()
}

extern "C" fn readable_stream_iterator_self(closure: *const ClosureHeader) -> f64 {
    closure_capture_value_get(closure)
}

unsafe fn build_readable_stream_iterator(stream_handle: f64) -> f64 {
    let reader = js_readable_stream_get_reader(stream_handle);
    let obj = js_object_alloc(0, 2);
    let keys = js_array_alloc(2);
    let k_next = js_string_from_bytes(b"next".as_ptr(), 4);
    let k_return = js_string_from_bytes(b"return".as_ptr(), 6);
    js_array_push(keys, JSValue::string_ptr(k_next));
    js_array_push(keys, JSValue::string_ptr(k_return));
    js_object_set_field(
        obj,
        0,
        JSValue::pointer(closure_capture_value(readable_stream_iterator_next, reader) as *const u8),
    );
    js_object_set_field(
        obj,
        1,
        JSValue::pointer(
            closure_capture_value(readable_stream_iterator_return, reader) as *const u8,
        ),
    );
    js_object_set_keys(obj, keys);
    let iterator = f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits());

    let async_iterator = perry_runtime::symbol::well_known_symbol("asyncIterator");
    if !async_iterator.is_null() {
        let self_closure = closure_capture_value(readable_stream_iterator_self, iterator);
        let symbol_value = f64::from_bits(JSValue::pointer(async_iterator as *const u8).bits());
        let closure_value = f64::from_bits(JSValue::pointer(self_closure as *const u8).bits());
        perry_runtime::symbol::js_object_set_symbol_property(iterator, symbol_value, closure_value);
    }

    iterator
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_values(stream_handle: f64) -> f64 {
    build_readable_stream_iterator(stream_handle)
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
    js_readable_stream_cancel_inner(stream_id as f64, reason, true)
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
    let mut was_locked = false;
    let mut is_byte_stream = false;
    let chunks: Vec<u64> = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) if s.reader_handle.is_none() => {
                is_byte_stream = s.is_byte_stream;
                let drained: Vec<u64> = s.chunks.drain(..).collect();
                s.state = ReadableState::Closed;
                drained
            }
            Some(_) => {
                was_locked = true;
                Vec::new()
            }
            None => Vec::new(),
        }
    };
    if was_locked {
        throw_type_error("ReadableStream is locked");
    }

    let id_a = next_id(&NEXT_STREAM_ID);
    let id_b = next_id(&NEXT_STREAM_ID);
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
                    strategy_size_cb: 0,
                    high_water_mark: 1.0,
                    is_byte_stream,
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
    let _ = js_readable_stream_pipe_to(
        readable_handle,
        transform_writable_handle,
        f64::from_bits(TAG_UNDEFINED),
    );
    transform_readable_handle
}

// ─────────────────────────────────────────────────────────────────────
// WritableStream FFI
// ─────────────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_new(
    start_bits: f64,
    write_bits: f64,
    close_bits: f64,
    abort_bits: f64,
    hwm: f64,
) -> f64 {
    js_writable_stream_new_with_sink_type(
        start_bits,
        write_bits,
        close_bits,
        abort_bits,
        hwm,
        f64::from_bits(TAG_UNDEFINED),
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_new_with_sink_type(
    start_bits: f64,
    write_bits: f64,
    close_bits: f64,
    abort_bits: f64,
    hwm: f64,
    sink_type: f64,
) -> f64 {
    if sink_type.to_bits() != TAG_UNDEFINED {
        throw_range_error_with_code(
            "The argument 'type' is invalid. Received a non-undefined value",
            "ERR_INVALID_ARG_VALUE",
        );
    }

    ensure_gc_registered();
    let id = alloc_writable(
        closure_from_bits(write_bits.to_bits()),
        closure_from_bits(close_bits.to_bits()),
        closure_from_bits(abort_bits.to_bits()),
        hwm,
    );
    // #1545: WritableStream `start(controller)` fires synchronously at
    // construction (before any write), matching the WHATWG order
    // start → write → close. The controller arg is the stream handle.
    let start_cb = closure_from_bits(start_bits.to_bits());
    if start_cb != 0 {
        js_closure_call1(start_cb as *const ClosureHeader, id as f64);
    }
    id as f64
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_throw_invalid_sink() -> f64 {
    throw_invalid_arg_type("The \"sink\" argument must be of type object")
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
        drop(g);
        throw_type_error("WritableStream is locked");
    }
    let writer_id = next_id(&NEXT_STREAM_ID);
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
    start_bits: f64,
    transform_bits: f64,
    flush_bits: f64,
    hwm: f64,
) -> f64 {
    ensure_gc_registered();
    let start_cb = closure_from_bits(start_bits.to_bits());
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
    let writable_id = next_id(&NEXT_STREAM_ID);
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

    let id = next_id(&NEXT_STREAM_ID);
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

    // #1644: TransformStream `start(controller)` fires synchronously at
    // construction. The controller is the readable-side handle (same value the
    // transform/flush callbacks receive), so `controller.enqueue(c)` /
    // `controller.terminate()` / `controller.error(e)` act on the readable.
    if start_cb != 0 {
        js_closure_call1(start_cb as *const ClosureHeader, readable_id as f64);
    }
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

#[inline]
fn box_promise(p: *mut Promise) -> f64 {
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
    if !(0x40000..0x100000).contains(&id) {
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
/// `0x40000` (see `NEXT_STREAM_ID`), the handle is (a) recognisable by range
/// and (b) present in exactly one of the five registries, so routing by
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
    if !(0x40000..0x100000).contains(&id) {
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

    // Probe each registry for membership first (dropping the guard before we
    // call the FFI, which re-locks the same registry).
    let is_reader = READERS.lock().unwrap().contains_key(&id);
    if is_reader {
        match method {
            "read" => return Some(box_promise(js_reader_read(handle))),
            "releaseLock" => return Some(js_reader_release_lock(handle)),
            "cancel" => return Some(box_promise(js_reader_cancel(handle, arg0))),
            _ => return None,
        }
    }
    let is_writer = WRITERS.lock().unwrap().contains_key(&id);
    if is_writer {
        match method {
            "write" => return Some(box_promise(js_writer_write(handle, arg0))),
            "close" => return Some(box_promise(js_writer_close(handle))),
            "abort" => return Some(box_promise(js_writer_abort(handle, arg0))),
            "releaseLock" => return Some(js_writer_release_lock(handle)),
            _ => return None,
        }
    }
    let is_readable = READABLE_STREAMS.lock().unwrap().contains_key(&id);
    if is_readable {
        match method {
            "getReader" => return Some(js_readable_stream_get_reader_with_options(handle, arg0)),
            "values" | "@@asyncIterator" => return Some(js_readable_stream_values(handle)),
            "cancel" => return Some(box_promise(js_readable_stream_cancel(handle, arg0))),
            "tee" => return Some(js_readable_stream_tee(handle)),
            "pipeTo" => return Some(box_promise(js_readable_stream_pipe_to(handle, arg0, arg1))),
            // #1644: a readable handle is also its own controller. The
            // start/transform/flush callbacks receive it as `controller`, so
            // `controller.enqueue/close/error/terminate` dispatch here when the
            // controller param is generically typed. `terminate()` ends the
            // readable side (TransformStreamDefaultController.terminate).
            "enqueue" => return Some(js_readable_stream_controller_enqueue(handle, arg0)),
            "close" | "terminate" => return Some(js_readable_stream_controller_close(handle)),
            "error" => return Some(js_readable_stream_controller_error(handle, arg0)),
            _ => return None,
        }
    }
    let is_writable = WRITABLE_STREAMS.lock().unwrap().contains_key(&id);
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
        (2, "locked") => return js_writable_stream_locked(handle),
        _ => {}
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
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_scanner_emits_callbacks_chunks_and_promises() {
        {
            let mut readable = READABLE_STREAMS.lock().unwrap();
            readable.clear();
            readable.insert(
                1,
                ReadableStreamData {
                    state: ReadableState::Errored,
                    chunks: VecDeque::from([0x7FFD_0000_0000_1234]),
                    pending_reads: VecDeque::from([0x2345_6780 as *mut Promise]),
                    start_cb: 0x3456_7890,
                    pull_cb: 0,
                    cancel_cb: 0,
                    high_water_mark: 1.0,
                    strategy_size_cb: 0,
                    is_byte_stream: false,
                    pulling: false,
                    started: false,
                    reader_handle: None,
                    error_value: 0x7FFF_0000_0000_4567,
                    canceled: false,
                },
            );
        }

        let mut emitted = Vec::new();
        scan_stream_roots(&mut |value| emitted.push(value.to_bits()));

        assert!(emitted.contains(&0x7FFD_0000_0000_1234));
        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x2345_6780)));
        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x3456_7890)));
        assert!(emitted.contains(&0x7FFF_0000_0000_4567));
        READABLE_STREAMS.lock().unwrap().clear();
    }
}
