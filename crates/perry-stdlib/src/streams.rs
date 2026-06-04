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
//! Stubs: BYOB readers and full custom `QueuingStrategy` size accounting —
//! see the inline comment on each site.

use flate2::read::{
    DeflateDecoder, DeflateEncoder, GzDecoder, GzEncoder, ZlibDecoder, ZlibEncoder,
};
use flate2::Compression;
use perry_runtime::{
    js_array_alloc, js_array_push, js_closure_call0, js_closure_call1, js_closure_call2,
    js_nanbox_get_pointer, js_object_alloc, js_object_get_field_by_name, js_object_set_field,
    js_object_set_field_by_name, js_object_set_keys, js_promise_new, js_promise_reject,
    js_promise_resolve, js_string_from_bytes, ClosureHeader, JSValue, ObjectHeader, Promise,
};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::os::raw::c_int;
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
    /// Internal source used by runtime APIs that hand back byte chunks instead
    /// of receiving a Web Streams controller.
    pull_returns_byte_chunk: bool,
    pulling: bool,
    started: bool,
    reader_handle: Option<usize>,
    error_value: u64,
    pending_error_after_chunks: Option<u64>,
    /// Per-controller cancel reason captured when `cancel()` is called.
    canceled: bool,
}

#[allow(dead_code)]
struct WritableStreamData {
    state: WritableState,
    write_cb: i64,
    close_cb: i64,
    abort_cb: i64,
    /// Backlog of writes while the sink's previous `write()` Promise is pending.
    write_queue: VecDeque<(u64, *mut Promise)>,
    in_flight: bool,
    high_water_mark: f64,
    writer_handle: Option<usize>,
    error_value: u64,
    /// Resolved when the stream becomes ready for more writes (i.e. queue drains).
    ready_promise: *mut Promise,
    /// Resolved when the stream finishes / rejects on error.
    closed_promise: *mut Promise,
    /// Pending `close()` request, if close is waiting for queued writes.
    close_request_promise: *mut Promise,
    close_started: bool,
}

struct TransformStreamData {
    readable_handle: usize,
    writable_handle: usize,
    transform_cb: i64,
    flush_cb: i64,
    native: Option<NativeTransformKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WebCompressionFormat {
    Gzip,
    Deflate,
    DeflateRaw,
    Brotli,
}

enum NativeTransformKind {
    TextEncoder,
    TextDecoder {
        fatal: bool,
        pending: Vec<u8>,
    },
    Compression {
        format: WebCompressionFormat,
        decompress: bool,
        chunks: Vec<u8>,
    },
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

pub(crate) const STREAM_HANDLE_ID_START: usize = 0x100000;
pub(crate) const STREAM_HANDLE_ID_END: usize = 0x200000;

lazy_static::lazy_static! {
    static ref READABLE_STREAMS: Mutex<HashMap<usize, ReadableStreamData>> = Mutex::new(HashMap::new());
    static ref WRITABLE_STREAMS: Mutex<HashMap<usize, WritableStreamData>> = Mutex::new(HashMap::new());
    static ref TRANSFORM_STREAMS: Mutex<HashMap<usize, TransformStreamData>> = Mutex::new(HashMap::new());
    static ref READERS: Mutex<HashMap<usize, ReaderData>> = Mutex::new(HashMap::new());
    static ref WRITERS: Mutex<HashMap<usize, WriterData>> = Mutex::new(HashMap::new());
    // #1545: ONE id counter shared across all five Web Streams registries.
    // Stream handles are raw numeric f64 values, not POINTER_TAG small handles,
    // so they live just above the runtime's `< 0x100000` small-handle band.
    // That keeps them disjoint from Fetch/native/proxy pointer-tagged ids while
    // the runtime's finite-number stream probes still route dynamic calls like
    // `src.pipeThrough(ts).getReader()`.
    static ref NEXT_STREAM_ID: Mutex<usize> = Mutex::new(STREAM_HANDLE_ID_START);
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
            if let Some(error) = &mut s.pending_error_after_chunks {
                visit_stream_value_slot(visitor, error);
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
            visitor.visit_raw_mut_ptr_slot(&mut s.close_request_promise);
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
    if id >= STREAM_HANDLE_ID_END {
        panic!("Web Streams handle id range exhausted");
    }
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

unsafe fn stream_object_field(object: f64, name: &[u8]) -> f64 {
    let value = JSValue::from_bits(object.to_bits());
    if !value.is_pointer() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let obj = js_nanbox_get_pointer(object) as *const ObjectHeader;
    if obj.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    f64::from_bits(js_object_get_field_by_name(obj, key).bits())
}

unsafe fn stream_object_closure(object: f64, name: &[u8]) -> i64 {
    closure_from_bits(stream_object_field(object, name).to_bits())
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
    perry_runtime::buffer::mark_as_uint8array(buf as usize);
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
    let addr = if top == 0x7FFD || top == 0x7FFF {
        (chunk_bits & POINTER_MASK) as usize
    } else if top == 0 && chunk_bits >= 0x10000 {
        chunk_bits as usize
    } else {
        return None;
    };
    if addr < 0x1000 {
        return None;
    }
    if perry_runtime::typedarray::lookup_typed_array_kind(addr).is_some() {
        let ta = addr as *const perry_runtime::typedarray::TypedArrayHeader;
        return perry_runtime::typedarray::typed_array_bytes(ta).map(|bytes| bytes.to_vec());
    }
    if !perry_runtime::buffer::is_registered_buffer(addr) {
        return None;
    }
    let ptr = addr as *const perry_runtime::buffer::BufferHeader;
    let len = (*ptr).length as usize;
    let data = perry_runtime::buffer::buffer_data(ptr);
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

unsafe fn make_type_error_with_code(message: &str, code: &'static str) -> u64 {
    let s = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(s, code);
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

unsafe fn throw_type_error_with_code(message: &str, code: &'static str) -> ! {
    let err = make_type_error_with_code(message, code);
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
            pull_returns_byte_chunk: false,
            pulling: false,
            started: false,
            reader_handle: None,
            error_value: 0,
            pending_error_after_chunks: None,
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
            close_request_promise: std::ptr::null_mut(),
            close_started: false,
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

extern "C" fn readable_pull_microtask(closure: *const ClosureHeader) -> f64 {
    unsafe {
        let stream_bits = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as u64;
        let stream_id = f64::from_bits(stream_bits) as usize;
        let cb = perry_runtime::closure::js_closure_get_capture_ptr(closure, 1);
        let pull_returns_byte_chunk =
            perry_runtime::closure::js_closure_get_capture_ptr(closure, 2) != 0;
        let should_pull = {
            let mut g = READABLE_STREAMS.lock().unwrap();
            match g.get_mut(&stream_id) {
                Some(s) if s.state == ReadableState::Readable && s.pulling => true,
                Some(s) => {
                    s.pulling = false;
                    false
                }
                None => false,
            }
        };
        if should_pull {
            if pull_returns_byte_chunk {
                pull_deferred_byte_chunk(stream_id, cb);
            } else {
                js_closure_call1(cb as *const ClosureHeader, stream_id as f64);
            }
            if let Some(s) = READABLE_STREAMS.lock().unwrap().get_mut(&stream_id) {
                s.pulling = false;
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) unsafe fn maybe_pull(stream_id: usize) {
    let (cb, controller, should_pull, pull_returns_byte_chunk) = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) if s.state == ReadableState::Readable && !s.pulling && s.started => {
                let need = s.chunks.is_empty() || (s.chunks.len() as f64) < s.high_water_mark;
                if need && s.pull_cb != 0 {
                    s.pulling = true;
                    (s.pull_cb, stream_id as f64, true, s.pull_returns_byte_chunk)
                } else {
                    (0, 0.0, false, false)
                }
            }
            _ => (0, 0.0, false, false),
        }
    };
    if !should_pull {
        return;
    }
    let pull_fn = readable_pull_microtask as *const u8;
    perry_runtime::closure::js_register_closure_arity(pull_fn, 0);
    let pull = perry_runtime::closure::js_closure_alloc(pull_fn, 3);
    perry_runtime::closure::js_closure_set_capture_ptr(pull, 0, controller.to_bits() as i64);
    perry_runtime::closure::js_closure_set_capture_ptr(pull, 1, cb);
    perry_runtime::closure::js_closure_set_capture_ptr(
        pull,
        2,
        if pull_returns_byte_chunk { 1 } else { 0 },
    );
    perry_runtime::builtins::js_queue_microtask(pull as i64);
}

unsafe fn pull_deferred_byte_chunk(stream_id: usize, cb: i64) {
    let chunk = js_closure_call0(cb as *const ClosureHeader);
    if chunk.to_bits() == TAG_UNDEFINED {
        let _ = js_readable_stream_controller_close(stream_id as f64);
    } else {
        let _ = js_readable_stream_controller_enqueue(stream_id as f64, chunk);
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

/// Runtime registration target for APIs like
/// `fs.promises.FileHandle.readableWebStream()`: create a byte-oriented
/// ReadableStream whose pull callback returns one Uint8Array chunk per call
/// and `undefined` at EOF. Unlike the public constructor path, this does not
/// run an eager initial pull, so file positions are not advanced at stream
/// creation time.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_deferred_byte_source(
    pull_bits: f64,
    cancel_bits: f64,
) -> f64 {
    ensure_gc_registered();
    let id = alloc_readable_with_type(
        0,
        closure_from_bits(pull_bits.to_bits()),
        closure_from_bits(cancel_bits.to_bits()),
        1.0,
        true,
    );
    if let Some(s) = READABLE_STREAMS.lock().unwrap().get_mut(&id) {
        s.started = true;
        s.pull_returns_byte_chunk = true;
    }
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

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_new_from_source_object(
    source: f64,
    strategy: f64,
) -> f64 {
    ensure_gc_registered();
    let source_type = stream_object_field(source, b"type");
    let is_byte_stream = value_string_equals(source_type, b"bytes");
    let size_cb = if is_byte_stream {
        0
    } else {
        read_queuing_strategy_size(strategy)
    };
    let id = alloc_readable_with_strategy(
        stream_object_closure(source, b"start"),
        stream_object_closure(source, b"pull"),
        stream_object_closure(source, b"cancel"),
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

#[no_mangle]
pub unsafe extern "C" fn js_streams_strategy_high_water_mark(strategy: f64) -> f64 {
    f64::from_bits(read_high_water_mark(strategy))
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
    let stream_handle = js_stream_unwrap_handle(stream_handle);
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

unsafe fn js_string_value_to_string(value: f64, coerce: bool) -> Option<String> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !coerce && !jsval.is_any_string() {
        return None;
    }
    let ptr = if coerce {
        perry_runtime::value::js_jsvalue_to_string(value) as *const perry_runtime::StringHeader
    } else {
        perry_runtime::value::js_get_string_pointer_unified(value)
            as *const perry_runtime::StringHeader
    };
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<perry_runtime::StringHeader>());
    Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
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
    if let Some(writable_id) = transform_writable_for_readable(id) {
        let _ = js_writable_stream_abort_inner(writable_id as f64, reason, true);
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

fn ptr_addr_from_nanbox(value: f64) -> Option<usize> {
    let bits = value.to_bits();
    let top = bits >> 48;
    if top == 0x7FFD || top == 0x7FFF {
        Some((bits & POINTER_MASK) as usize)
    } else if top == 0 && bits >= 0x10000 {
        Some(bits as usize)
    } else {
        None
    }
}

unsafe fn chunks_from_array_ptr(arr_ptr: *const perry_runtime::ArrayHeader) -> Vec<u64> {
    let len = perry_runtime::array::js_array_length(arr_ptr);
    (0..len)
        .map(|i| perry_runtime::array::js_array_get(arr_ptr, i).bits())
        .collect()
}

unsafe fn chunks_from_sync_iterable(value: f64) -> Option<Vec<u64>> {
    let iter = perry_runtime::symbol::js_get_iterator(value);
    if iter.to_bits() == value.to_bits() {
        return None;
    }
    let arr = perry_runtime::array::js_iterator_to_array(iter);
    Some(chunks_from_array_ptr(arr))
}

struct ReadableFromSource {
    chunks: Vec<u64>,
    error: Option<u64>,
}

impl ReadableFromSource {
    fn closed(chunks: Vec<u64>) -> Self {
        Self {
            chunks,
            error: None,
        }
    }
}

enum SettledValue {
    Fulfilled(f64),
    Rejected(u64),
    Pending,
}

unsafe fn is_callable_value(value: f64) -> bool {
    let raw = js_nanbox_get_pointer(value);
    raw >= 0x10000 && perry_runtime::closure::is_closure_ptr(raw as usize)
}

unsafe fn call_symbol_async_iterator(value: f64) -> Option<f64> {
    let sym = perry_runtime::symbol::well_known_symbol("asyncIterator");
    if sym.is_null() {
        return None;
    }
    let sym_value = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
    let method = perry_runtime::symbol::js_object_get_symbol_property(value, sym_value);
    if !is_callable_value(method) {
        return None;
    }
    let prev_this = perry_runtime::object::js_implicit_this_set(value);
    let iterator = perry_runtime::closure::js_native_call_value(method, std::ptr::null(), 0);
    perry_runtime::object::js_implicit_this_set(prev_this);
    if iterator.to_bits() == TAG_UNDEFINED {
        None
    } else {
        Some(iterator)
    }
}

unsafe fn has_iterator_next(value: f64) -> bool {
    let ptr = js_nanbox_get_pointer(value);
    if ptr == 0 {
        return false;
    }
    let obj = ptr as *const ObjectHeader;
    let next_key = js_string_from_bytes(b"next".as_ptr(), 4);
    let next = js_object_get_field_by_name(obj, next_key);
    is_callable_value(f64::from_bits(next.bits()))
}

unsafe fn await_maybe_promise(value: f64) -> SettledValue {
    if perry_runtime::promise::js_value_is_promise(value) == 0 {
        return SettledValue::Fulfilled(value);
    }
    let promise = js_nanbox_get_pointer(value) as *mut Promise;
    if promise.is_null() {
        return SettledValue::Fulfilled(value);
    }

    for _ in 0..100_000 {
        if perry_runtime::promise::js_promise_state(promise) != 0 {
            break;
        }
        if perry_runtime::promise::js_promise_run_microtasks() == 0 {
            break;
        }
    }

    match perry_runtime::promise::js_promise_state(promise) {
        1 => SettledValue::Fulfilled(perry_runtime::promise::js_promise_value(promise)),
        2 => SettledValue::Rejected(perry_runtime::promise::js_promise_reason(promise).to_bits()),
        _ => SettledValue::Pending,
    }
}

unsafe fn call_iterator_next(iterator: f64) -> Option<f64> {
    let iter_ptr = js_nanbox_get_pointer(iterator);
    if iter_ptr == 0 {
        return None;
    }
    let iter_obj = iter_ptr as *const ObjectHeader;
    let next_key = js_string_from_bytes(b"next".as_ptr(), 4);
    let next_val = js_object_get_field_by_name(iter_obj, next_key);
    let next = f64::from_bits(next_val.bits());
    if is_callable_value(next) {
        let prev_this = perry_runtime::object::js_implicit_this_set(iterator);
        let result = perry_runtime::closure::js_native_call_value(next, std::ptr::null(), 0);
        perry_runtime::object::js_implicit_this_set(prev_this);
        Some(result)
    } else {
        Some(perry_runtime::object::js_native_call_method(
            iterator,
            b"next".as_ptr() as *const i8,
            4,
            std::ptr::null(),
            0,
        ))
    }
}

unsafe fn try_call_iterator_next(iterator: f64) -> Result<Option<f64>, u64> {
    let trap_buf = perry_runtime::exception::js_try_push();
    let jumped = perry_runtime::ffi::setjmp::setjmp(trap_buf as *mut c_int);
    if jumped == 0 {
        let step = call_iterator_next(iterator);
        perry_runtime::exception::js_try_end();
        Ok(step)
    } else {
        let err = perry_runtime::exception::js_get_exception();
        perry_runtime::exception::js_clear_exception();
        perry_runtime::exception::js_try_end();
        Err(err.to_bits())
    }
}

unsafe fn chunks_from_async_iterable(value: f64) -> Option<ReadableFromSource> {
    let iterator = if let Some(iterator) = call_symbol_async_iterator(value) {
        iterator
    } else if has_iterator_next(value) {
        value
    } else {
        return None;
    };
    let done_key = js_string_from_bytes(b"done".as_ptr(), 4);
    let value_key = js_string_from_bytes(b"value".as_ptr(), 5);
    let mut chunks = Vec::new();

    for _ in 0..100_000 {
        let step = match try_call_iterator_next(iterator) {
            Ok(Some(step)) => step,
            Ok(None) => break,
            Err(reason) => {
                return Some(ReadableFromSource {
                    chunks,
                    error: Some(reason),
                });
            }
        };
        let step_result = match await_maybe_promise(step) {
            SettledValue::Fulfilled(result) => result,
            SettledValue::Rejected(reason) => {
                return Some(ReadableFromSource {
                    chunks,
                    error: Some(reason),
                });
            }
            SettledValue::Pending => break,
        };
        let result_ptr = js_nanbox_get_pointer(step_result);
        if result_ptr == 0 {
            break;
        }
        let result_obj = result_ptr as *const ObjectHeader;
        let done_val = js_object_get_field_by_name(result_obj, done_key);
        let done = f64::from_bits(done_val.bits());
        if perry_runtime::value::js_is_truthy(done) != 0 {
            break;
        }
        let item = js_object_get_field_by_name(result_obj, value_key);
        chunks.push(item.bits());
    }

    Some(ReadableFromSource::closed(chunks))
}

/// `ReadableStream.from(iterable)` (Node 20+, #1645) — build a Web
/// ReadableStream pre-loaded with the iterable's items, then closed. Async
/// iterators preserve a terminal rejection after any chunks already yielded.
/// Each element becomes one chunk so `getReader().read()` / `for await` yield
/// them in order, then `done`.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_iterable(value: f64) -> f64 {
    ensure_gc_registered();
    let ptr_addr = ptr_addr_from_nanbox(value);

    let source = if let Some(source) = chunks_from_async_iterable(value) {
        source
    } else if perry_runtime::array::js_array_is_array(value).to_bits() == TAG_TRUE {
        let arr_ptr = ptr_addr.unwrap_or(0) as *const perry_runtime::ArrayHeader;
        ReadableFromSource::closed(chunks_from_array_ptr(arr_ptr))
    } else if let Some(addr) = ptr_addr {
        if perry_runtime::typedarray::lookup_typed_array_kind(addr).is_some() {
            let ta = addr as *const perry_runtime::typedarray::TypedArrayHeader;
            let len = perry_runtime::typedarray::js_typed_array_length(ta).max(0);
            let chunks = (0..len)
                .map(|i| perry_runtime::typedarray::js_typed_array_get(ta, i).to_bits())
                .collect();
            ReadableFromSource::closed(chunks)
        } else if perry_runtime::buffer::is_registered_buffer(addr)
            && !perry_runtime::buffer::is_any_array_buffer(addr)
            && !perry_runtime::buffer::is_data_view(addr)
        {
            let buf = addr as *const perry_runtime::buffer::BufferHeader;
            let len = (*buf).length as usize;
            let data = perry_runtime::buffer::buffer_data(buf);
            let chunks = (0..len).map(|i| (*data.add(i) as f64).to_bits()).collect();
            ReadableFromSource::closed(chunks)
        } else if let Some(chunks) = chunks_from_sync_iterable(value) {
            ReadableFromSource::closed(chunks)
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
            s.chunks.extend(source.chunks);
            s.started = true;
            if let Some(error) = source.error {
                if s.chunks.is_empty() {
                    s.state = ReadableState::Errored;
                    s.error_value = error;
                } else {
                    s.state = ReadableState::Readable;
                    s.pending_error_after_chunks = Some(error);
                }
            } else {
                s.state = ReadableState::Closed;
            }
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
                s.pending_error_after_chunks = None;
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
                s.pending_error_after_chunks = None;
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

extern "C" fn readable_from_chunk_fulfilled(closure: *const ClosureHeader, value: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let promise = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    unsafe {
        let result = build_iter_result(value.to_bits(), false);
        js_promise_resolve(promise, f64::from_bits(result));
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn readable_from_chunk_rejected(closure: *const ClosureHeader, reason: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let promise = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    js_promise_reject(promise, reason);
    f64::from_bits(TAG_UNDEFINED)
}

unsafe fn resolve_reader_read_value(promise: *mut Promise, value_bits: u64) {
    let value = f64::from_bits(value_bits);
    if perry_runtime::promise::js_value_is_promise(value) == 0 {
        let result = build_iter_result(value_bits, false);
        js_promise_resolve(promise, f64::from_bits(result));
        return;
    }

    let inner = js_nanbox_get_pointer(value) as *mut Promise;
    if inner.is_null() {
        let result = build_iter_result(value_bits, false);
        js_promise_resolve(promise, f64::from_bits(result));
        return;
    }

    match perry_runtime::promise::js_promise_state(inner) {
        1 => {
            let value = perry_runtime::promise::js_promise_value(inner);
            let result = build_iter_result(value.to_bits(), false);
            js_promise_resolve(promise, f64::from_bits(result));
        }
        2 => {
            js_promise_reject(promise, perry_runtime::promise::js_promise_reason(inner));
        }
        _ => {
            let fulfill = perry_runtime::closure::js_closure_alloc(
                readable_from_chunk_fulfilled as *const u8,
                1,
            );
            let reject = perry_runtime::closure::js_closure_alloc(
                readable_from_chunk_rejected as *const u8,
                1,
            );
            perry_runtime::closure::js_closure_set_capture_ptr(fulfill, 0, promise as i64);
            perry_runtime::closure::js_closure_set_capture_ptr(reject, 0, promise as i64);
            let _ = perry_runtime::promise::js_promise_then(inner, fulfill, reject);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_reader_read(reader_handle: f64) -> *mut Promise {
    let promise = js_promise_new();
    let reader_handle = js_stream_unwrap_handle(reader_handle);
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
    let mut closed_rejection: Option<(usize, u64)> = None;
    let mut closed_resolution: Option<usize> = None;
    let outcome: Option<(u64, bool, bool)> = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => {
                if let Some(c) = s.chunks.pop_front() {
                    if s.chunks.is_empty() {
                        if let Some(error) = s.pending_error_after_chunks.take() {
                            s.state = ReadableState::Errored;
                            s.error_value = error;
                            if let Some(reader_id) = s.reader_handle {
                                closed_rejection = Some((reader_id, error));
                            }
                        } else if s.state == ReadableState::Closed {
                            if let Some(reader_id) = s.reader_handle {
                                closed_resolution = Some(reader_id);
                            }
                        }
                    }
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
    if let Some((reader_id, reason)) = closed_rejection {
        let p = READERS
            .lock()
            .unwrap()
            .get(&reader_id)
            .map(|r| r.closed_promise);
        if let Some(p) = p {
            js_promise_reject(p, f64::from_bits(reason));
        }
    }
    if let Some(reader_id) = closed_resolution {
        let p = READERS
            .lock()
            .unwrap()
            .get(&reader_id)
            .map(|r| r.closed_promise);
        if let Some(p) = p {
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
        }
        close_pending(stream_id);
    }
    match outcome {
        Some((value, _, true)) => {
            js_promise_reject(promise, f64::from_bits(value));
        }
        Some((value, done, false)) => {
            if done {
                let result = build_iter_result(value, true);
                js_promise_resolve(promise, f64::from_bits(result));
            } else {
                resolve_reader_read_value(promise, value);
            }
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
                    pull_returns_byte_chunk: false,
                    pulling: false,
                    started: true,
                    reader_handle: None,
                    error_value: 0,
                    pending_error_after_chunks: None,
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
pub unsafe extern "C" fn js_writable_stream_new_from_sink_object(sink: f64, hwm: f64) -> f64 {
    ensure_gc_registered();
    let sink_type = stream_object_field(sink, b"type");
    if sink_type.to_bits() != TAG_UNDEFINED {
        throw_range_error_with_code(
            "The argument 'type' is invalid. Received a non-undefined value",
            "ERR_INVALID_ARG_VALUE",
        );
    }

    let id = alloc_writable(
        stream_object_closure(sink, b"write"),
        stream_object_closure(sink, b"close"),
        stream_object_closure(sink, b"abort"),
        hwm,
    );
    let start_cb = stream_object_closure(sink, b"start");
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
    let start_close = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) => match s.state {
                WritableState::Writable => {
                    s.state = WritableState::Closing;
                    s.close_request_promise = promise;
                    !s.in_flight && s.write_queue.is_empty()
                }
                WritableState::Closing => {
                    if !s.close_request_promise.is_null() {
                        return s.close_request_promise;
                    }
                    s.close_request_promise = promise;
                    !s.in_flight && s.write_queue.is_empty()
                }
                WritableState::Closed => {
                    js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
                    return promise;
                }
                WritableState::Errored => {
                    js_promise_reject(promise, f64::from_bits(s.error_value));
                    return promise;
                }
            },
            None => {
                js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
                return promise;
            }
        }
    };
    if start_close {
        finish_writable_close(id);
    }
    promise
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_abort(stream_handle: f64, reason: f64) -> *mut Promise {
    js_writable_stream_abort_inner(stream_handle, reason, false)
}

unsafe fn js_writable_stream_abort_inner(
    stream_handle: f64,
    reason: f64,
    allow_locked: bool,
) -> *mut Promise {
    let promise = js_promise_new();
    let id = stream_handle as usize;
    let reason_bits = reason.to_bits();
    let mut locked_reject = false;
    let (cb, closed_p, close_request) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) => {
                if !allow_locked && s.writer_handle.is_some() {
                    locked_reject = true;
                    (0, std::ptr::null_mut(), std::ptr::null_mut())
                } else {
                    s.state = WritableState::Errored;
                    s.error_value = reason_bits;
                    let close_request = s.close_request_promise;
                    s.close_request_promise = std::ptr::null_mut();
                    s.close_started = false;
                    (s.abort_cb, s.closed_promise, close_request)
                }
            }
            None => (0, std::ptr::null_mut(), std::ptr::null_mut()),
        }
    };
    if locked_reject {
        reject_type_error(promise, "Invalid state: WritableStream is locked");
        return promise;
    }
    if cb != 0 {
        js_closure_call1(cb as *const ClosureHeader, reason);
    }
    if !closed_p.is_null() {
        js_promise_reject(closed_p, reason);
    }
    if !close_request.is_null() {
        js_promise_reject(close_request, reason);
    }
    js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    promise
}

// ─────────────────────────────────────────────────────────────────────
// WritableStreamDefaultWriter FFI
// ─────────────────────────────────────────────────────────────────────

fn writable_desired_size(s: &WritableStreamData) -> f64 {
    s.high_water_mark - if s.in_flight { 1.0 } else { 0.0 } - s.write_queue.len() as f64
}

fn sync_writer_ready_promise(stream_id: usize, writer_id: usize, ready: *mut Promise) {
    if let Some(w) = WRITERS.lock().unwrap().get_mut(&writer_id) {
        if w.stream_handle == stream_id {
            w.ready_promise = ready;
        }
    }
}

pub(super) unsafe fn install_writable_backpressure_ready(stream_id: usize, writer_id: usize) {
    let ready = js_promise_new();
    if let Some(s) = WRITABLE_STREAMS.lock().unwrap().get_mut(&stream_id) {
        s.ready_promise = ready;
    }
    sync_writer_ready_promise(stream_id, writer_id, ready);
}

fn writable_capture_usize(closure: *const ClosureHeader, idx: u32) -> usize {
    let bits = perry_runtime::closure::js_closure_get_capture_ptr(closure, idx) as u64;
    f64::from_bits(bits) as usize
}

fn writable_capture_promise(closure: *const ClosureHeader, idx: u32) -> *mut Promise {
    perry_runtime::closure::js_closure_get_capture_ptr(closure, idx) as *mut Promise
}

extern "C" fn writable_write_start_microtask(closure: *const ClosureHeader) -> f64 {
    unsafe {
        let stream_id = writable_capture_usize(closure, 0);
        let writer_id = writable_capture_usize(closure, 1);
        let cb = perry_runtime::closure::js_closure_get_capture_ptr(closure, 2);
        let chunk_bits = perry_runtime::closure::js_closure_get_capture_ptr(closure, 3) as u64;
        let write_promise = writable_capture_promise(closure, 4);
        run_writable_write(
            stream_id,
            writer_id,
            cb,
            f64::from_bits(chunk_bits),
            write_promise,
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn writable_write_fulfilled(closure: *const ClosureHeader, _value: f64) -> f64 {
    unsafe {
        let stream_id = writable_capture_usize(closure, 0);
        let writer_id = writable_capture_usize(closure, 1);
        let write_promise = writable_capture_promise(closure, 2);
        finish_writable_write_success(stream_id, writer_id, write_promise);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn writable_write_rejected(closure: *const ClosureHeader, reason: f64) -> f64 {
    unsafe {
        let stream_id = writable_capture_usize(closure, 0);
        let write_promise = writable_capture_promise(closure, 2);
        finish_writable_write_error(stream_id, write_promise, reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

unsafe fn attach_writable_write_handlers(
    stream_id: usize,
    writer_id: usize,
    write_promise: *mut Promise,
    sink_promise: *mut Promise,
) {
    let fulfilled_fn = writable_write_fulfilled as *const u8;
    let rejected_fn = writable_write_rejected as *const u8;
    perry_runtime::closure::js_register_closure_arity(fulfilled_fn, 1);
    perry_runtime::closure::js_register_closure_arity(rejected_fn, 1);

    let on_fulfilled = perry_runtime::closure::js_closure_alloc(fulfilled_fn, 3);
    perry_runtime::closure::js_closure_set_capture_ptr(
        on_fulfilled,
        0,
        (stream_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        on_fulfilled,
        1,
        (writer_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(on_fulfilled, 2, write_promise as i64);

    let on_rejected = perry_runtime::closure::js_closure_alloc(rejected_fn, 3);
    perry_runtime::closure::js_closure_set_capture_ptr(
        on_rejected,
        0,
        (stream_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        on_rejected,
        1,
        (writer_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(on_rejected, 2, write_promise as i64);

    let _ = perry_runtime::promise::js_promise_then(sink_promise, on_fulfilled, on_rejected);
}

unsafe fn try_call_writable_write(cb: i64, chunk: f64) -> Result<f64, u64> {
    let trap_buf = perry_runtime::exception::js_try_push();
    let jumped = perry_runtime::ffi::setjmp::setjmp(trap_buf as *mut c_int);
    if jumped == 0 {
        let result = js_closure_call1(cb as *const ClosureHeader, chunk);
        perry_runtime::exception::js_try_end();
        Ok(result)
    } else {
        let err = perry_runtime::exception::js_get_exception();
        perry_runtime::exception::js_clear_exception();
        perry_runtime::exception::js_try_end();
        Err(err.to_bits())
    }
}

unsafe fn try_call_writable_close(cb: i64) -> Result<f64, u64> {
    let trap_buf = perry_runtime::exception::js_try_push();
    let jumped = perry_runtime::ffi::setjmp::setjmp(trap_buf as *mut c_int);
    if jumped == 0 {
        let result = js_closure_call0(cb as *const ClosureHeader);
        perry_runtime::exception::js_try_end();
        Ok(result)
    } else {
        let err = perry_runtime::exception::js_get_exception();
        perry_runtime::exception::js_clear_exception();
        perry_runtime::exception::js_try_end();
        Err(err.to_bits())
    }
}

extern "C" fn writable_close_fulfilled(closure: *const ClosureHeader, _value: f64) -> f64 {
    unsafe {
        let stream_id = writable_capture_usize(closure, 0);
        let close_promise = writable_capture_promise(closure, 1);
        finish_writable_close_success(stream_id, close_promise);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn writable_close_rejected(closure: *const ClosureHeader, reason: f64) -> f64 {
    unsafe {
        let stream_id = writable_capture_usize(closure, 0);
        let close_promise = writable_capture_promise(closure, 1);
        finish_writable_close_error(stream_id, close_promise, reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

unsafe fn attach_writable_close_handlers(
    stream_id: usize,
    close_promise: *mut Promise,
    sink_promise: *mut Promise,
) {
    let fulfilled_fn = writable_close_fulfilled as *const u8;
    let rejected_fn = writable_close_rejected as *const u8;
    perry_runtime::closure::js_register_closure_arity(fulfilled_fn, 1);
    perry_runtime::closure::js_register_closure_arity(rejected_fn, 1);

    let on_fulfilled = perry_runtime::closure::js_closure_alloc(fulfilled_fn, 2);
    perry_runtime::closure::js_closure_set_capture_ptr(
        on_fulfilled,
        0,
        (stream_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(on_fulfilled, 1, close_promise as i64);

    let on_rejected = perry_runtime::closure::js_closure_alloc(rejected_fn, 2);
    perry_runtime::closure::js_closure_set_capture_ptr(
        on_rejected,
        0,
        (stream_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(on_rejected, 1, close_promise as i64);

    let _ = perry_runtime::promise::js_promise_then(sink_promise, on_fulfilled, on_rejected);
}

unsafe fn finish_writable_close(stream_id: usize) {
    let (cb, close_promise) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) if s.state == WritableState::Closing => {
                if s.in_flight || !s.write_queue.is_empty() || s.close_started {
                    return;
                }
                s.close_started = true;
                (s.close_cb, s.close_request_promise)
            }
            _ => return,
        }
    };

    if cb == 0 {
        finish_writable_close_success(stream_id, close_promise);
        return;
    }
    let result = match try_call_writable_close(cb) {
        Ok(result) => result,
        Err(reason) => {
            finish_writable_close_error(stream_id, close_promise, f64::from_bits(reason));
            return;
        }
    };
    let sink_promise = perry_runtime::promise::js_promise_resolved(result);
    if !sink_promise.is_null() {
        attach_writable_close_handlers(stream_id, close_promise, sink_promise);
        return;
    }
    finish_writable_close_success(stream_id, close_promise);
}

unsafe fn finish_writable_close_success(stream_id: usize, promise: *mut Promise) {
    let (ready, closed) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => {
                s.state = WritableState::Closed;
                s.close_request_promise = std::ptr::null_mut();
                s.close_started = false;
                (s.ready_promise, s.closed_promise)
            }
            None => (std::ptr::null_mut(), std::ptr::null_mut()),
        }
    };
    if !promise.is_null() {
        js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    }
    if !ready.is_null() {
        js_promise_resolve(ready, f64::from_bits(TAG_UNDEFINED));
    }
    if !closed.is_null() {
        js_promise_resolve(closed, f64::from_bits(TAG_UNDEFINED));
    }
}

unsafe fn finish_writable_close_error(stream_id: usize, promise: *mut Promise, reason: f64) {
    let (ready, closed) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => {
                s.state = WritableState::Errored;
                s.error_value = reason.to_bits();
                s.close_request_promise = std::ptr::null_mut();
                s.close_started = false;
                (s.ready_promise, s.closed_promise)
            }
            None => (std::ptr::null_mut(), std::ptr::null_mut()),
        }
    };
    if !promise.is_null() {
        js_promise_reject(promise, reason);
    }
    if !ready.is_null() {
        js_promise_reject(ready, reason);
    }
    if !closed.is_null() {
        js_promise_reject(closed, reason);
    }
}

pub(super) unsafe fn run_writable_write(
    stream_id: usize,
    writer_id: usize,
    cb: i64,
    chunk: f64,
    promise: *mut Promise,
) {
    if cb == 0 {
        finish_writable_write_success(stream_id, writer_id, promise);
        return;
    }
    let result = match try_call_writable_write(cb, chunk) {
        Ok(result) => result,
        Err(reason) => {
            finish_writable_write_error(stream_id, promise, f64::from_bits(reason));
            return;
        }
    };
    let sink_promise = perry_runtime::promise::js_promise_resolved(result);
    if !sink_promise.is_null() {
        attach_writable_write_handlers(stream_id, writer_id, promise, sink_promise);
        return;
    }
    finish_writable_write_success(stream_id, writer_id, promise);
}

pub(super) unsafe fn writable_stream_write(
    stream_id: usize,
    writer_id: usize,
    chunk: f64,
) -> *mut Promise {
    if TRANSFORM_PAIRS.lock().unwrap().contains_key(&stream_id) {
        return transform_write(stream_id, chunk);
    }
    let promise = js_promise_new();
    let mut start_write = None;
    let needs_pending_ready;
    {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        let s = match g.get_mut(&stream_id) {
            Some(s) if s.state == WritableState::Writable => s,
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
        let before = writable_desired_size(s);
        if s.in_flight {
            s.write_queue.push_back((chunk.to_bits(), promise));
        } else {
            s.in_flight = true;
            start_write = Some((s.write_cb, chunk, promise));
        }
        let after = writable_desired_size(s);
        needs_pending_ready = before > 0.0 && after <= 0.0;
    }
    if needs_pending_ready {
        install_writable_backpressure_ready(stream_id, writer_id);
    }
    if let Some((cb, chunk, write_promise)) = start_write {
        let start_fn = writable_write_start_microtask as *const u8;
        perry_runtime::closure::js_register_closure_arity(start_fn, 0);
        let start = perry_runtime::closure::js_closure_alloc(start_fn, 5);
        perry_runtime::closure::js_closure_set_capture_ptr(
            start,
            0,
            (stream_id as f64).to_bits() as i64,
        );
        perry_runtime::closure::js_closure_set_capture_ptr(
            start,
            1,
            (writer_id as f64).to_bits() as i64,
        );
        perry_runtime::closure::js_closure_set_capture_ptr(start, 2, cb);
        perry_runtime::closure::js_closure_set_capture_ptr(start, 3, chunk.to_bits() as i64);
        perry_runtime::closure::js_closure_set_capture_ptr(start, 4, write_promise as i64);
        perry_runtime::builtins::js_queue_microtask(start as i64);
    }
    promise
}

unsafe fn finish_writable_write_success(stream_id: usize, writer_id: usize, promise: *mut Promise) {
    if !promise.is_null() {
        js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    }

    let (next, ready, close_now) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => {
                s.in_flight = false;
                let next =
                    if s.state == WritableState::Writable || s.state == WritableState::Closing {
                        s.write_queue.pop_front().map(|(chunk, p)| {
                            s.in_flight = true;
                            (s.write_cb, f64::from_bits(chunk), p)
                        })
                    } else {
                        None
                    };
                let ready = if s.state == WritableState::Writable && writable_desired_size(s) > 0.0
                {
                    s.ready_promise
                } else {
                    std::ptr::null_mut()
                };
                let close_now = s.state == WritableState::Closing
                    && next.is_none()
                    && !s.in_flight
                    && s.write_queue.is_empty();
                (next, ready, close_now)
            }
            None => (None, std::ptr::null_mut(), false),
        }
    };

    if !ready.is_null() {
        js_promise_resolve(ready, f64::from_bits(TAG_UNDEFINED));
    }
    if let Some((cb, chunk, queued_promise)) = next {
        run_writable_write(stream_id, writer_id, cb, chunk, queued_promise);
    } else if close_now {
        finish_writable_close(stream_id);
    }
}

unsafe fn finish_writable_write_error(stream_id: usize, promise: *mut Promise, reason: f64) {
    let (ready, closed, close_request, queued) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => {
                s.in_flight = false;
                s.state = WritableState::Errored;
                s.error_value = reason.to_bits();
                let close_request = s.close_request_promise;
                s.close_request_promise = std::ptr::null_mut();
                s.close_started = false;
                let queued: Vec<*mut Promise> = s.write_queue.drain(..).map(|(_, p)| p).collect();
                (s.ready_promise, s.closed_promise, close_request, queued)
            }
            None => (
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                Vec::new(),
            ),
        }
    };

    if !promise.is_null() {
        js_promise_reject(promise, reason);
    }
    for queued_promise in queued {
        if !queued_promise.is_null() {
            js_promise_reject(queued_promise, reason);
        }
    }
    if !ready.is_null() {
        js_promise_reject(ready, reason);
    }
    if !close_request.is_null() {
        js_promise_reject(close_request, reason);
    }
    if !closed.is_null() {
        js_promise_reject(closed, reason);
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_write(writer_handle: f64, chunk: f64) -> *mut Promise {
    let writer_id = writer_handle as usize;
    let stream_id = match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) if w.locked => w.stream_handle,
        _ => {
            let promise = js_promise_new();
            let err = make_error_with_message("Writer is no longer locked to a stream");
            js_promise_reject(promise, f64::from_bits(err));
            return promise;
        }
    };
    writable_stream_write(stream_id, writer_id, chunk)
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
        Some(s) if s.state == WritableState::Writable => writable_desired_size(s),
        Some(s) if s.state == WritableState::Errored => f64::NAN,
        _ => 0.0,
    }
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
    js_writable_stream_abort_inner(stream_id as f64, reason, true)
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
    let start_cb = closure_from_bits(start_bits.to_bits());
    let transform_cb = closure_from_bits(transform_bits.to_bits());
    let flush_cb = closure_from_bits(flush_bits.to_bits());
    alloc_transform_stream(start_cb, transform_cb, flush_cb, None, hwm)
}

unsafe fn alloc_transform_stream(
    start_cb: i64,
    transform_cb: i64,
    flush_cb: i64,
    native: Option<NativeTransformKind>,
    hwm: f64,
) -> f64 {
    ensure_gc_registered();

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
            close_request_promise: std::ptr::null_mut(),
            close_started: false,
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
            native,
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
pub unsafe extern "C" fn js_transform_stream_new_from_transformer_object(
    transformer: f64,
    hwm: f64,
) -> f64 {
    ensure_gc_registered();
    js_transform_stream_new(
        stream_object_field(transformer, b"start"),
        stream_object_field(transformer, b"transform"),
        stream_object_field(transformer, b"flush"),
        hwm,
    )
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

fn transform_writable_for_readable(readable_id: usize) -> Option<usize> {
    TRANSFORM_STREAMS
        .lock()
        .unwrap()
        .values()
        .find_map(|t| (t.readable_handle == readable_id).then_some(t.writable_handle))
}

/// Replacement `writer.write` for the writable side of a TransformStream
/// — invokes the user transform with (chunk, transformController) where
/// the transformController is the readable-side stream handle (so
/// `controller.enqueue(...)` reuses the readable controller path).
pub(super) unsafe fn transform_write(writable_id: usize, chunk: f64) -> *mut Promise {
    let promise = js_promise_new();
    {
        let g = WRITABLE_STREAMS.lock().unwrap();
        match g.get(&writable_id) {
            Some(s) if s.state == WritableState::Writable => {}
            Some(s) if s.state == WritableState::Errored => {
                js_promise_reject(promise, f64::from_bits(s.error_value));
                return promise;
            }
            _ => {
                let err = make_error_with_message("Stream is closed or closing");
                js_promise_reject(promise, f64::from_bits(err));
                return promise;
            }
        }
    }
    let mut handled_native = false;
    let mut native_error = None;
    let (transform_cb, readable_id) = {
        let pairs = TRANSFORM_PAIRS.lock().unwrap();
        match pairs.get(&writable_id) {
            Some(t_id) => {
                let mut g = TRANSFORM_STREAMS.lock().unwrap();
                match g.get_mut(t_id) {
                    Some(t) => {
                        if let Some(native) = t.native.as_mut() {
                            handled_native = true;
                            if let Err(error_bits) =
                                native_transform_write(native, t.readable_handle, chunk)
                            {
                                native_error = Some(error_bits);
                            }
                        }
                        (t.transform_cb, t.readable_handle)
                    }
                    None => (0, 0),
                }
            }
            None => (0, 0),
        }
    };
    if let Some(error_bits) = native_error {
        if readable_id != 0 {
            js_readable_stream_controller_error(readable_id as f64, f64::from_bits(error_bits));
        }
        js_promise_reject(promise, f64::from_bits(error_bits));
        return promise;
    }
    if handled_native {
        js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
        return promise;
    }
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

pub(super) unsafe fn transform_close(writable_id: usize) -> *mut Promise {
    let promise = js_promise_new();
    let mut handled_native = false;
    let mut native_error = None;
    let (flush_cb, readable_id) = {
        let pairs = TRANSFORM_PAIRS.lock().unwrap();
        match pairs.get(&writable_id) {
            Some(t_id) => {
                let mut g = TRANSFORM_STREAMS.lock().unwrap();
                match g.get_mut(t_id) {
                    Some(t) => {
                        if let Some(native) = t.native.as_mut() {
                            handled_native = true;
                            if let Err(error_bits) =
                                native_transform_close(native, t.readable_handle)
                            {
                                native_error = Some(error_bits);
                            }
                        }
                        (t.flush_cb, t.readable_handle)
                    }
                    None => (0, 0),
                }
            }
            None => (0, 0),
        }
    };
    if let Some(error_bits) = native_error {
        if readable_id != 0 {
            js_readable_stream_controller_error(readable_id as f64, f64::from_bits(error_bits));
        }
        if let Some(s) = WRITABLE_STREAMS.lock().unwrap().get_mut(&writable_id) {
            s.state = WritableState::Errored;
            s.error_value = error_bits;
            let cp = s.closed_promise;
            js_promise_reject(cp, f64::from_bits(error_bits));
        }
        js_promise_reject(promise, f64::from_bits(error_bits));
        return promise;
    }
    if !handled_native && flush_cb != 0 && readable_id != 0 {
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

fn split_utf8_prefix(bytes: &[u8]) -> Result<(usize, bool), ()> {
    match std::str::from_utf8(bytes) {
        Ok(_) => Ok((bytes.len(), false)),
        Err(err) => {
            if err.error_len().is_none() {
                Ok((err.valid_up_to(), true))
            } else {
                Err(())
            }
        }
    }
}

unsafe fn enqueue_string(readable_id: usize, text: &str) {
    let ptr = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    js_readable_stream_controller_enqueue(
        readable_id as f64,
        f64::from_bits(JSValue::string_ptr(ptr).bits()),
    );
}

unsafe fn native_text_decoder_drain(
    pending: &mut Vec<u8>,
    fatal: bool,
    readable_id: usize,
    flush: bool,
) -> Result<(), u64> {
    if pending.is_empty() {
        return Ok(());
    }
    if fatal {
        let (valid_len, incomplete) = split_utf8_prefix(pending).map_err(|_| {
            make_type_error_with_code(
                "The encoded data was not valid for encoding utf-8",
                "ERR_ENCODING_INVALID_ENCODED_DATA",
            )
        })?;
        if valid_len > 0 {
            let text = std::str::from_utf8(&pending[..valid_len]).map_err(|_| {
                make_type_error_with_code(
                    "The encoded data was not valid for encoding utf-8",
                    "ERR_ENCODING_INVALID_ENCODED_DATA",
                )
            })?;
            enqueue_string(readable_id, text);
            pending.drain(..valid_len);
        }
        if flush && !pending.is_empty() {
            return Err(make_type_error_with_code(
                "The encoded data was not valid for encoding utf-8",
                "ERR_ENCODING_INVALID_ENCODED_DATA",
            ));
        }
        if !incomplete && !pending.is_empty() {
            return Err(make_type_error_with_code(
                "The encoded data was not valid for encoding utf-8",
                "ERR_ENCODING_INVALID_ENCODED_DATA",
            ));
        }
        return Ok(());
    }

    if flush {
        let text = String::from_utf8_lossy(pending).into_owned();
        pending.clear();
        if !text.is_empty() {
            enqueue_string(readable_id, &text);
        }
        return Ok(());
    }

    let (valid_len, incomplete) = split_utf8_prefix(pending).unwrap_or((pending.len(), false));
    let emit_len = if incomplete { valid_len } else { pending.len() };
    if emit_len > 0 {
        let text = String::from_utf8_lossy(&pending[..emit_len]).into_owned();
        enqueue_string(readable_id, &text);
        pending.drain(..emit_len);
    }
    Ok(())
}

fn run_web_compression_codec(
    format: WebCompressionFormat,
    decompress: bool,
    input: &[u8],
) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    match (format, decompress) {
        (WebCompressionFormat::Gzip, false) => {
            GzEncoder::new(input, Compression::default()).read_to_end(&mut out)?;
        }
        (WebCompressionFormat::Gzip, true) => {
            GzDecoder::new(input).read_to_end(&mut out)?;
        }
        (WebCompressionFormat::Deflate, false) => {
            ZlibEncoder::new(input, Compression::default()).read_to_end(&mut out)?;
        }
        (WebCompressionFormat::Deflate, true) => {
            ZlibDecoder::new(input).read_to_end(&mut out)?;
        }
        (WebCompressionFormat::DeflateRaw, false) => {
            DeflateEncoder::new(input, Compression::default()).read_to_end(&mut out)?;
        }
        (WebCompressionFormat::DeflateRaw, true) => {
            DeflateDecoder::new(input).read_to_end(&mut out)?;
        }
        (WebCompressionFormat::Brotli, false) => {
            let mut reader = brotli::CompressorReader::new(input, 4096, 11, 22);
            reader.read_to_end(&mut out)?;
        }
        (WebCompressionFormat::Brotli, true) => {
            let mut reader = brotli::Decompressor::new(input, 4096);
            reader.read_to_end(&mut out)?;
        }
    }
    Ok(out)
}

unsafe fn native_transform_write(
    native: &mut NativeTransformKind,
    readable_id: usize,
    chunk: f64,
) -> Result<(), u64> {
    match native {
        NativeTransformKind::TextEncoder => {
            let text = js_string_value_to_string(chunk, true).unwrap_or_default();
            let bytes = alloc_uint8array_from_bytes(text.as_bytes());
            js_readable_stream_controller_enqueue(readable_id as f64, f64::from_bits(bytes));
            Ok(())
        }
        NativeTransformKind::TextDecoder { fatal, pending } => {
            let bytes = read_bytes_from_chunk(chunk.to_bits()).ok_or_else(|| {
                make_type_error_with_code(
                    "The \"chunk\" argument must be an instance of Buffer, TypedArray, DataView, or ArrayBuffer",
                    "ERR_INVALID_ARG_TYPE",
                )
            })?;
            pending.extend_from_slice(&bytes);
            native_text_decoder_drain(pending, *fatal, readable_id, false)
        }
        NativeTransformKind::Compression { chunks, .. } => {
            let bytes = read_bytes_from_chunk(chunk.to_bits()).ok_or_else(|| {
                make_type_error_with_code(
                    "The \"chunk\" argument must be an instance of Buffer, TypedArray, DataView, or ArrayBuffer",
                    "ERR_INVALID_ARG_TYPE",
                )
            })?;
            chunks.extend_from_slice(&bytes);
            Ok(())
        }
    }
}

unsafe fn native_transform_close(
    native: &mut NativeTransformKind,
    readable_id: usize,
) -> Result<(), u64> {
    match native {
        NativeTransformKind::TextEncoder => Ok(()),
        NativeTransformKind::TextDecoder { fatal, pending } => {
            native_text_decoder_drain(pending, *fatal, readable_id, true)
        }
        NativeTransformKind::Compression {
            format,
            decompress,
            chunks,
        } => match run_web_compression_codec(*format, *decompress, chunks) {
            Ok(out) => {
                let chunk = alloc_uint8array_from_bytes(&out);
                js_readable_stream_controller_enqueue(readable_id as f64, f64::from_bits(chunk));
                chunks.clear();
                Ok(())
            }
            Err(err) => Err(make_type_error_with_code(&err.to_string(), "Z_DATA_ERROR")),
        },
    }
}

unsafe fn attach_stream_field(object_value: f64, name: &[u8], value: f64) {
    let ptr = js_nanbox_get_pointer(object_value) as *mut ObjectHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return;
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(ptr, key, value);
}

unsafe fn attach_stream_string_field(object_value: f64, name: &[u8], value: &[u8]) {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    attach_stream_field(
        object_value,
        name,
        f64::from_bits(JSValue::string_ptr(ptr).bits()),
    );
}

unsafe fn attach_stream_bool_field(object_value: f64, name: &[u8], value: bool) {
    attach_stream_field(
        object_value,
        name,
        f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE }),
    );
}

unsafe fn attach_transform_endpoints(object_value: f64, readable_id: usize, writable_id: usize) {
    attach_stream_field(object_value, b"readable", readable_id as f64);
    attach_stream_field(object_value, b"writable", writable_id as f64);
}

unsafe fn bool_option(options: f64, name: &[u8]) -> bool {
    let jsval = JSValue::from_bits(options.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let value =
        perry_runtime::value::js_get_property(options, name.as_ptr() as i64, name.len() as i64);
    perry_runtime::value::js_is_truthy(value) != 0
}

unsafe fn parse_text_decoder_stream_label(label: f64) {
    let jsval = JSValue::from_bits(label.to_bits());
    if jsval.is_undefined()
        || value_string_equals(label, b"utf-8")
        || value_string_equals(label, b"utf8")
    {
        return;
    }
    let label_text = js_string_value_to_string(label, true).unwrap_or_default();
    let message = format!("The \"{label_text}\" encoding is not supported");
    throw_range_error_with_code(&message, "ERR_ENCODING_NOT_SUPPORTED");
}

unsafe fn parse_web_compression_format(value: f64, constructor_name: &str) -> WebCompressionFormat {
    if value_string_equals(value, b"gzip") {
        return WebCompressionFormat::Gzip;
    }
    if value_string_equals(value, b"deflate") {
        return WebCompressionFormat::Deflate;
    }
    if value_string_equals(value, b"deflate-raw") {
        return WebCompressionFormat::DeflateRaw;
    }
    if value_string_equals(value, b"brotli") {
        return WebCompressionFormat::Brotli;
    }
    let received =
        js_string_value_to_string(value, true).unwrap_or_else(|| "undefined".to_string());
    let message = format!(
        "Failed to construct '{constructor_name}': 1st argument value '{received}' is not a valid enum value of type CompressionFormat."
    );
    throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
}

unsafe fn build_native_transform_object(object_value: f64, native: NativeTransformKind) -> f64 {
    let handle = alloc_transform_stream(0, 0, 0, Some(native), 1.0);
    let readable_id = js_transform_stream_readable(handle) as usize;
    let writable_id = js_transform_stream_writable(handle) as usize;
    attach_transform_endpoints(object_value, readable_id, writable_id);
    object_value
}

#[no_mangle]
pub unsafe extern "C" fn js_stream_web_text_encoder_stream_new() -> f64 {
    let object = perry_runtime::object::js_text_encoder_stream_new();
    attach_stream_string_field(object, b"encoding", b"utf-8");
    build_native_transform_object(object, NativeTransformKind::TextEncoder)
}

#[no_mangle]
pub unsafe extern "C" fn js_stream_web_text_decoder_stream_new(label: f64, options: f64) -> f64 {
    parse_text_decoder_stream_label(label);
    let fatal = bool_option(options, b"fatal");
    let ignore_bom = bool_option(options, b"ignoreBOM");
    let object = perry_runtime::object::js_text_decoder_stream_new();
    attach_stream_string_field(object, b"encoding", b"utf-8");
    attach_stream_bool_field(object, b"fatal", fatal);
    attach_stream_bool_field(object, b"ignoreBOM", ignore_bom);
    build_native_transform_object(
        object,
        NativeTransformKind::TextDecoder {
            fatal,
            pending: Vec::new(),
        },
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_stream_web_compression_stream_new(format: f64) -> f64 {
    let format = parse_web_compression_format(format, "CompressionStream");
    let object = perry_runtime::object::js_compression_stream_new();
    build_native_transform_object(
        object,
        NativeTransformKind::Compression {
            format,
            decompress: false,
            chunks: Vec::new(),
        },
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_stream_web_decompression_stream_new(format: f64) -> f64 {
    let format = parse_web_compression_format(format, "DecompressionStream");
    let object = perry_runtime::object::js_decompression_stream_new();
    build_native_transform_object(
        object,
        NativeTransformKind::Compression {
            format,
            decompress: true,
            chunks: Vec::new(),
        },
    )
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
/// `STREAM_HANDLE_ID_START` (see `NEXT_STREAM_ID`), the handle is (a)
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
            "pipeThrough" => {
                let transform = js_stream_unwrap_handle(arg0);
                let writable = js_transform_stream_writable(transform);
                let readable = js_transform_stream_readable(transform);
                return Some(js_readable_stream_pipe_through(handle, writable, readable));
            }
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
        (3, "closed") => return box_promise(js_reader_closed(handle)),
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
    fn stream_ids_live_outside_pointer_tag_small_handle_band() {
        let id = next_id(&NEXT_STREAM_ID);
        assert!(
            (STREAM_HANDLE_ID_START..STREAM_HANDLE_ID_END).contains(&id),
            "stream id {id:#x} must stay in the raw numeric stream band"
        );
        assert!(
            id >= 0x100000,
            "stream id {id:#x} must not overlap pointer-tagged small handles"
        );
    }

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
                    pull_returns_byte_chunk: false,
                    pulling: false,
                    started: false,
                    reader_handle: None,
                    error_value: 0x7FFF_0000_0000_4567,
                    pending_error_after_chunks: None,
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

    #[test]
    fn web_compression_formats_round_trip() {
        let input = b"hello stream/web compression";
        for format in [
            WebCompressionFormat::Gzip,
            WebCompressionFormat::Deflate,
            WebCompressionFormat::DeflateRaw,
            WebCompressionFormat::Brotli,
        ] {
            let compressed = run_web_compression_codec(format, false, input).unwrap();
            assert!(!compressed.is_empty());
            let decompressed = run_web_compression_codec(format, true, &compressed).unwrap();
            assert_eq!(decompressed, input);
        }
    }

    #[test]
    fn utf8_split_prefix_tracks_incomplete_sequence() {
        assert_eq!(split_utf8_prefix(&[0x68, 0xc3]).unwrap(), (1, true));
        assert_eq!(split_utf8_prefix(&[0xc3, 0xa9]).unwrap(), (2, false));
        assert!(split_utf8_prefix(&[0xff]).is_err());
    }
}
