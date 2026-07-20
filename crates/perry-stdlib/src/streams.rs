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
//! BYOB readers (`getReader({ mode: "byob" })`, `read(view)`,
//! `controller.byobRequest.respond/respondWithNewView`) and real
//! `QueuingStrategy` size accounting (per-chunk `size()` results summed
//! into `desiredSize`) live in `streams/byob.rs` and the queue helpers on
//! `ReadableStreamData` (#4915).

use perry_runtime::{
    js_array_alloc, js_array_push, js_closure_call0, js_closure_call1, js_closure_call2,
    js_nanbox_get_pointer, js_object_alloc, js_object_get_field_by_name, js_object_set_field,
    js_object_set_field_by_name, js_object_set_keys, js_promise_mark_internally_handled,
    js_promise_new, js_promise_reject, js_promise_resolve, js_string_from_bytes, ClosureHeader,
    JSValue, ObjectHeader, Promise,
};
use std::collections::{HashMap, VecDeque};
use std::os::raw::c_int;
use std::sync::Mutex;

/// Allocate a promise the stream machinery owns and observes internally — the
/// reader/writer `closed`, writer `ready`, and `[[closeRequest]]` promises.
/// Node marks these `markPromiseAsHandled` so that an abort / error / cancel
/// that rejects them is never surfaced as an unhandled rejection (#1545).
pub(crate) fn internal_promise() -> *mut Promise {
    let p = js_promise_new();
    js_promise_mark_internally_handled(p);
    p
}

mod byob;
mod expando;
mod idalloc;
mod pipe;
mod strategy;
mod subclass;
mod tee;
#[cfg(test)]
mod tests;
mod transform;
mod writable;

pub use tee::js_readable_stream_tee;
use tee::{tee_branches_of, tee_error_branches, tee_source_of};

pub use self::byob::{
    js_readable_stream_controller_byob_request, js_readable_stream_get_byob_reader,
    js_reader_read_with_view,
};
pub(crate) use self::strategy::parse_strategy_value;
pub use self::strategy::{
    js_byte_length_queuing_strategy_new, js_count_queuing_strategy_new,
    js_streams_strategy_high_water_mark,
};
use self::strategy::{read_high_water_mark, read_queuing_strategy_size};

pub(crate) use self::expando::stream_expando_get;
use self::pipe::js_readable_stream_pipe_to;
use self::subclass::{box_promise, js_stream_unwrap_handle};
pub(crate) use self::subclass::{dispatch_stream_method, dispatch_stream_property};
pub use self::subclass::{
    drain_readable_into_bytes, js_readable_stream_subclass_init, js_stream_handle_is_registered,
    js_stream_handle_kind, js_transform_stream_subclass_init, js_writable_stream_subclass_init,
};
pub use self::transform::{
    js_stream_web_compression_stream_new, js_stream_web_decompression_stream_new,
    js_stream_web_text_decoder_stream_new, js_stream_web_text_encoder_stream_new,
    js_transform_stream_new, js_transform_stream_new_from_transformer_object,
    js_transform_stream_readable, js_transform_stream_writable,
};
use self::transform::{
    transform_close, transform_writable_for_readable, transform_write, TRANSFORM_PAIRS,
};
pub use self::writable::{
    js_writable_stream_abort, js_writable_stream_close, js_writable_stream_get_writer,
    js_writable_stream_locked, js_writable_stream_new, js_writable_stream_new_from_sink_object,
    js_writable_stream_new_with_sink_type, js_writable_stream_throw_invalid_sink, js_writer_abort,
    js_writer_close, js_writer_closed, js_writer_desired_size, js_writer_ready,
    js_writer_release_lock, js_writer_write,
};
use self::writable::{js_writable_stream_abort_inner, writable_stream_write};

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
    /// Per-chunk strategy sizes, parallel to `chunks` (#4915). The size of a
    /// chunk is `strategy.size(chunk)` when a custom strategy is installed,
    /// `chunk.byteLength` on byte streams, else 1 — computed once at enqueue
    /// time per spec. Mutate the queue only through `push_chunk` /
    /// `pop_chunk` / `clear_chunks` / `drain_chunks` so the running
    /// `queue_total_size` stays in sync.
    chunk_sizes: VecDeque<f64>,
    /// Running sum of `chunk_sizes` — what `desiredSize` subtracts from the
    /// highWaterMark (real ByteLengthQueuingStrategy accounting, #4915).
    queue_total_size: f64,
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

impl ReadableStreamData {
    fn push_chunk(&mut self, bits: u64, size: f64) {
        self.chunks.push_back(bits);
        self.chunk_sizes.push_back(size);
        self.queue_total_size += size;
    }

    fn pop_chunk(&mut self) -> Option<u64> {
        let bits = self.chunks.pop_front()?;
        let size = self.chunk_sizes.pop_front().unwrap_or(1.0);
        self.queue_total_size = (self.queue_total_size - size).max(0.0);
        Some(bits)
    }

    fn clear_chunks(&mut self) {
        self.chunks.clear();
        self.chunk_sizes.clear();
        self.queue_total_size = 0.0;
    }

    fn drain_chunks(&mut self) -> Vec<u64> {
        self.chunk_sizes.clear();
        self.queue_total_size = 0.0;
        self.chunks.drain(..).collect()
    }
}

#[allow(dead_code)]
struct WritableStreamData {
    state: WritableState,
    write_cb: i64,
    close_cb: i64,
    abort_cb: i64,
    /// Custom `strategy.size(chunk)` callback (#4915). 0 when absent; each
    /// chunk then counts as 1 toward `desiredSize`.
    strategy_size_cb: i64,
    /// Backlog of writes while the sink's previous `write()` Promise is
    /// pending: `(chunk_bits, write_promise, strategy_size)`.
    write_queue: VecDeque<(u64, *mut Promise, f64)>,
    /// Strategy size of the chunk currently in flight (0.0 when idle).
    in_flight_size: f64,
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
    /// True for readers minted by `getReader({ mode: "byob" })` /
    /// `new ReadableStreamBYOBReader(stream)` (#4915). BYOB readers route
    /// `read(view)` through `byob::js_reader_read_with_view`.
    is_byob: bool,
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

// Band boundaries owned by `perry_runtime::value::addr_class` (the runtime's
// finite-number stream probes classify against the same range).
pub(crate) const STREAM_HANDLE_ID_START: usize =
    perry_runtime::value::addr_class::STREAM_ID_BAND_START;
pub(crate) const STREAM_HANDLE_ID_END: usize = perry_runtime::value::addr_class::STREAM_ID_BAND_END;

lazy_static::lazy_static! {
    static ref READABLE_STREAMS: Mutex<HashMap<usize, ReadableStreamData>> = Mutex::new(HashMap::new());
    static ref WRITABLE_STREAMS: Mutex<HashMap<usize, WritableStreamData>> = Mutex::new(HashMap::new());
    static ref TRANSFORM_STREAMS: Mutex<HashMap<usize, TransformStreamData>> = Mutex::new(HashMap::new());
    static ref READERS: Mutex<HashMap<usize, ReaderData>> = Mutex::new(HashMap::new());
    static ref WRITERS: Mutex<HashMap<usize, WriterData>> = Mutex::new(HashMap::new());
}

// #1545: ONE id allocator shared across all five Web Streams registries.
// Stream handles are raw numeric f64 values, not POINTER_TAG small handles,
// so they live just above the runtime's `< 0x100000` small-handle band. That
// keeps them disjoint from Fetch/native/proxy pointer-tagged ids while the
// runtime's finite-number stream probes still route dynamic calls like
// `src.pipeThrough(ts).getReader()`. #6602: ids of terminal objects recycle
// through `idalloc` so the finite band survives long-running servers.
use self::idalloc::next_stream_id;

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
        unsafe {
            perry_runtime::object::js_register_stream_expando_set(expando::stream_expando_set_hook);
        }
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
    expando::scan_expando_roots(visitor);
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
    byob::scan_byob_roots(visitor);
    if let Ok(mut map) = WRITABLE_STREAMS.lock() {
        for s in map.values_mut() {
            visitor.visit_i64_slot(&mut s.write_cb);
            visitor.visit_i64_slot(&mut s.close_cb);
            visitor.visit_i64_slot(&mut s.abort_cb);
            visitor.visit_i64_slot(&mut s.strategy_size_cb);
            for (chunk, p, _size) in s.write_queue.iter_mut() {
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
    // Parked/deferred transform promises are held only as raw addresses in
    // these maps (an unawaited write/close has no other root).
    if let Ok(mut map) = transform::TRANSFORM_WRITE_RELEASES.lock() {
        for promises in map.values_mut() {
            for slot in promises.iter_mut() {
                let mut p = *slot as *mut Promise;
                visitor.visit_raw_mut_ptr_slot(&mut p);
                *slot = p as usize;
            }
        }
    }
    if let Ok(mut map) = transform::TRANSFORM_PENDING_CLOSE.lock() {
        for slot in map.values_mut() {
            let mut p = *slot as *mut Promise;
            visitor.visit_raw_mut_ptr_slot(&mut p);
            *slot = p as usize;
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
    let id = next_stream_id();
    READABLE_STREAMS.lock().unwrap().insert(
        id,
        ReadableStreamData {
            state: ReadableState::Readable,
            chunks: VecDeque::new(),
            chunk_sizes: VecDeque::new(),
            queue_total_size: 0.0,
            pending_reads: VecDeque::new(),
            start_cb,
            pull_cb,
            cancel_cb,
            strategy_size_cb,
            // Byte streams default to highWaterMark 0 (per spec — no eager
            // pull at construction; the first pull fires when a read
            // request arrives, #4915). Default streams keep the legacy
            // clamp-to-1 behavior.
            high_water_mark: if hwm.is_nan() {
                if is_byte_stream {
                    0.0
                } else {
                    1.0
                }
            } else if !is_byte_stream && hwm <= 0.0 {
                1.0
            } else {
                hwm.max(0.0)
            },
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
    alloc_writable_with_strategy(write_cb, close_cb, abort_cb, hwm, 0)
}

fn alloc_writable_with_strategy(
    write_cb: i64,
    close_cb: i64,
    abort_cb: i64,
    hwm: f64,
    strategy_size_cb: i64,
) -> usize {
    let id = next_stream_id();
    let ready = internal_promise();
    let closed = internal_promise();
    js_promise_resolve(ready, f64::from_bits(TAG_UNDEFINED));
    WRITABLE_STREAMS.lock().unwrap().insert(
        id,
        WritableStreamData {
            state: WritableState::Writable,
            write_cb,
            close_cb,
            abort_cb,
            strategy_size_cb,
            write_queue: VecDeque::new(),
            in_flight_size: 0.0,
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
            // A Web Streams `pull()` that throws must ERROR the stream (its
            // pending reads reject with the exception) — it must NOT `longjmp`
            // past this Rust microtask frame. Without the local trap the throw
            // unwinds to the microtask loop, skipping the `pulling = false`
            // reset (and any live frames), leaving the stream wedged and, under
            // concurrency, crashing the process. Catch it here and route to the
            // spec error path so `reader.read()` rejects and the consumer's own
            // handler surfaces the error, exactly as Node does.
            let pull_outcome = perry_runtime::exception::js_call_catching(|| {
                if pull_returns_byte_chunk {
                    pull_deferred_byte_chunk(stream_id, cb);
                } else {
                    js_closure_call1(cb as *const ClosureHeader, stream_id as f64);
                }
                f64::from_bits(TAG_UNDEFINED)
            });
            if let Some(s) = READABLE_STREAMS.lock().unwrap().get_mut(&stream_id) {
                s.pulling = false;
            }
            if let Err(exc) = pull_outcome {
                error_readable_stream(stream_id, exc.to_bits());
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) unsafe fn maybe_pull(stream_id: usize) {
    // #5989: a read on a tee branch pulls its SOURCE — the branch carries no
    // `pull_cb` of its own; the source's enqueue fans the chunk out to both
    // branches. `force` so a highWaterMark-0 flight producer actually pulls.
    if let Some(source) = tee_source_of(stream_id) {
        // Tick parity: a demand-initiated pull travels TWO microtask cycles
        // (Node's sourceReader.read() resolution + .then(fanout) reaction);
        // buffered chunks + close discovery included. A live producer is
        // driven from the pull job.
        tee::tee_schedule_pull_demand(source);
        return;
    }
    maybe_pull_inner(stream_id, false);
}

/// `maybe_pull` that pulls whenever the source can produce more, ignoring the
/// highWaterMark / read-request gate. Used by the synchronous body drain
/// (`new Response(stream)` / `new Request(url, { body: stream })`): that drain
/// has no parked reader, so for a `highWaterMark: 0` stream the normal
/// `maybe_pull` would refuse to pull and the body would drain EMPTY (CodeRabbit
/// #5776). Forcing the pull when the queue is empty drives such a stream to
/// completion regardless of its strategy.
pub(super) unsafe fn maybe_pull_force(stream_id: usize) {
    maybe_pull_inner(stream_id, true);
}

unsafe fn maybe_pull_inner(stream_id: usize, force: bool) {
    // ShouldCallPull (#4915): a parked read request always justifies a
    // pull (this is what drives byte streams with highWaterMark 0 — the
    // pull only fires once a `read()` / `read(view)` is waiting);
    // otherwise pull while the queue is under the highWaterMark. `force`
    // (drain path) pulls whenever the queue is empty regardless of strategy.
    let has_byob_pending = byob::has_pending(stream_id);
    let (cb, controller, should_pull, pull_returns_byte_chunk) = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) if s.state == ReadableState::Readable && !s.pulling && s.started => {
                let has_read_request = !s.pending_reads.is_empty() || has_byob_pending;
                let need = (force && s.chunks.is_empty())
                    || has_read_request
                    || (s.chunks.is_empty() && s.high_water_mark > 0.0)
                    || (!s.chunks.is_empty() && s.queue_total_size < s.high_water_mark);
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
    byob::close_pending_byob(stream_id);
    // #5437: stream is done — drop any expando entries so the table doesn't
    // grow one row per stream over the server's lifetime.
    expando::stream_expando_clear(stream_id);
    idalloc::retire_readable_terminal(stream_id);
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
    byob::error_pending_byob(stream_id, reason_bits);
    // #5437: stream errored — drop any expando entries (see close_pending).
    expando::stream_expando_clear(stream_id);
    idalloc::retire_readable_terminal(stream_id);
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
                s.push_chunk(chunk_bits, 1.0);
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
    if std::env::var("PERRY_JSON_PATH").as_deref() == Ok("1") {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 40 {
            eprintln!("[STREAM] get_reader stream={:#x}", stream_handle.to_bits());
        }
    }
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
            let reader_id = next_stream_id();
            let closed_p = internal_promise();
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
                    is_byob: byob_requested,
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
                    s.clear_chunks();
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
// referenced below only exist behind the `web-fetch` feature, so the
// streams-only auto-optimize build (`bundled-streams` alone, no
// `web-fetch`) failed to compile. Gate the two blob/response constructors
// on `web-fetch` and provide no-op stubs when it's off — anything that
// actually needs a Blob/Response went through `web-fetch` anyway. (#5174:
// these track `fetch.rs`, which moved from `http-client` to `web-fetch`.)
#[cfg(feature = "web-fetch")]
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_blob(blob_id: f64) -> f64 {
    let bytes = crate::fetch::blob_bytes_clone(blob_id as usize).unwrap_or_default();
    alloc_readable_from_bytes(bytes) as f64
}

#[cfg(not(feature = "web-fetch"))]
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_blob(_blob_id: f64) -> f64 {
    alloc_readable_from_bytes(Vec::new()) as f64
}

#[cfg(feature = "web-fetch")]
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_response(resp_id: f64) -> f64 {
    let bytes = crate::fetch::response_bytes_clone(resp_id as usize).unwrap_or_default();
    alloc_readable_from_bytes(bytes) as f64
}

#[cfg(not(feature = "web-fetch"))]
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
            for bits in source.chunks {
                s.push_chunk(bits, 1.0);
            }
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
                s.clear_chunks();
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
    if std::env::var("PERRY_JSON_PATH").as_deref() == Ok("1") {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 40 {
            eprintln!(
                "[STREAM] enqueue stream={:#x} chunk_bits={:#018x}",
                (stream_handle as usize),
                chunk.to_bits()
            );
        }
    }
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
    // #5989 + Node tick parity: a tee'd source's enqueue stays in the SOURCE
    // queue; branches receive chunks only through the demand-driven pull
    // cycle (see `tee::tee_source_enqueue`).
    if tee::tee_source_enqueue(id, chunk, chunk_bits, is_byte_stream) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // A pending BYOB read (byte streams only) takes the chunk before the
    // default-read queue: the bytes land directly in the caller's view.
    if is_byte_stream && byob::service_pending_with_chunk(id, chunk_bits) {
        return f64::from_bits(TAG_UNDEFINED);
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
        // Per spec the strategy's size(chunk) runs once at enqueue time; the
        // result is the chunk's contribution to desiredSize accounting.
        let size = if strategy_size_cb != 0 {
            let size = readable_strategy_size_to_number(js_closure_call1(
                strategy_size_cb as *const ClosureHeader,
                chunk,
            ));
            if size.is_nan() || size < 0.0 || size.is_infinite() {
                throw_invalid_readable_strategy_size(id, size);
            }
            size
        } else if is_byte_stream {
            byob::chunk_byte_length(chunk_bits)
        } else {
            1.0
        };
        let mut g = READABLE_STREAMS.lock().unwrap();
        if let Some(s) = g.get_mut(&id) {
            if s.state == ReadableState::Readable {
                s.push_chunk(chunk_bits, size);
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_controller_close(stream_handle: f64) -> f64 {
    let id = stream_handle as usize;
    if std::env::var("PERRY_JSON_PATH").as_deref() == Ok("1") {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 40 {
            eprintln!("[STREAM] controller_close stream={id:#x}");
        }
    }
    // #5989 + Node tick parity: closing a tee'd source marks it Closed; the
    // branch close is DISCOVERED by the pull cycle draining the source queue
    // empty (byte tee: prompt; default tee: nextTick-deferred) — never
    // delivered eagerly ahead of undrained chunks.
    if tee_branches_of(id).is_some() {
        {
            let mut g = READABLE_STREAMS.lock().unwrap();
            if let Some(s) = g.get_mut(&id) {
                if s.state == ReadableState::Readable {
                    s.state = ReadableState::Closed;
                }
            }
        }
        tee::tee_schedule_pull(id);
        return f64::from_bits(TAG_UNDEFINED);
    }
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
    // #5989: erroring a tee'd source errors both branches.
    if tee_branches_of(id).is_some() {
        tee_error_branches(id, reason.to_bits());
        return f64::from_bits(TAG_UNDEFINED);
    }
    error_readable_stream(id, reason.to_bits());
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_controller_desired_size(stream_handle: f64) -> f64 {
    let id = stream_handle as usize;
    let g = READABLE_STREAMS.lock().unwrap();
    match g.get(&id) {
        Some(s) if s.state == ReadableState::Readable => s.high_water_mark - s.queue_total_size,
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
    if std::env::var("PERRY_JSON_PATH").as_deref() == Ok("1") {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 40 {
            eprintln!("[STREAM] read RESOLVED value_bits={value_bits:#018x}");
        }
    }
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
    if std::env::var("PERRY_JSON_PATH").as_deref() == Ok("1") {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 40 {
            eprintln!("[STREAM] reader_read reader_id={reader_id:#x}");
        }
    }
    let stream_id = match READERS.lock().unwrap().get(&reader_id) {
        Some(r) if r.locked => r.stream_handle,
        Some(_) => {
            reject_type_error(promise, "Reader is no longer locked to a stream");
            return promise;
        }
        None => {
            reject_type_error(promise, "Invalid reader");
            return promise;
        }
    };
    let mut closed_rejection: Option<(usize, u64)> = None;
    let mut closed_resolution: Option<usize> = None;
    let outcome: Option<(u64, bool, bool)> = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => {
                if let Some(c) = s.pop_chunk() {
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
    // Consumer progress: if this readable is a transform's output and its
    // queue just drained (pop emptied it, or the read parked on an empty
    // queue), release write promises parked on backpressure.
    transform::transform_release_writes(stream_id);
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
    // #5437: the reader instance is done — drop any expando entries keyed by
    // its handle id so the table doesn't retain them for the process lifetime.
    expando::stream_expando_clear(reader_id);
    idalloc::retire_reader_if_released(reader_id);
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
