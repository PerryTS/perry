//! Web Streams API native bindings (issue #237).
//!
//! Implements `ReadableStream` / `WritableStream` / `TransformStream`
//! plus the matching default reader / writer pair, and the per-
//! controller enqueue / close / error / write / abort surface. This
//! is the perry-ffi port of perry-stdlib's `streams.rs` (#466 Phase 5)
//! — the on-disk surface (FFI symbols + observable behaviour) is
//! identical; the dependency edge is now perry-ffi-only.
//!
//! # Architecture
//!
//! Each stream variant owns a per-process `HashMap<usize, …>` keyed
//! by a numeric handle (registry id cast to `f64`, the same ABI
//! perry-stdlib used). User-supplied `start` / `pull` / `cancel` /
//! `write` / `close` / `abort` / `transform` / `flush` callbacks are
//! stored as raw `i64` closure pointers and pinned across GC cycles
//! by a single registered root scanner — same #35 / `events` pattern
//! used by every other in-tree wrapper that owns closures.
//!
//! # Punted gaps (issue #237 followups)
//!
//! - **`blob.stream()` / `response.body`** — perry-stdlib's
//!   `js_readable_stream_from_blob` / `_from_response` reach into
//!   perry-stdlib's `fetch` module to clone the blob bytes by handle.
//!   That cross-wrapper bridge has no perry-ffi surface yet, and
//!   perry-ext-fetch's `js_blob_stream` is currently a stub that
//!   returns the blob handle as-is. A v0.6.0 followup needs a
//!   cross-wrapper handle-bytes exchange (e.g. perry-ffi
//!   `wrapper_get_bytes(symbol, handle)`) to wire them up.
//! - **BYOB readers** — `getReader({ mode: "byob" })`. Throws via
//!   `js_streams_throw_byob_not_implemented`.
//! - **`ByteLengthQueuingStrategy`** — custom `size()` callbacks per
//!   chunk. Throws via `js_streams_throw_byte_length_not_implemented`.
//! - **`ReadableStream.from(asyncIterable)`** — needs cooperative
//!   async iteration which the spawn-blocking-only perry-ffi v0.5.x
//!   surface can't express; throws.
//! - **Concurrent-reader `tee()` semantics** — today's
//!   `tee()` drains the source eagerly into per-branch queues at
//!   call time. Lazily-pulled streams that produce chunks after
//!   `tee()` only see the pre-tee chunks. Same trade-off Node's
//!   `Readable.from([...]).tee()` makes for buffered iterables.
//! - **Backpressure / async writes** — `WritableStream`'s
//!   `write_queue` field is reserved for the queued-writes path;
//!   today every write runs synchronously through the user
//!   callback.
//!
//! # Cargo features
//!
//! No optional features — this crate links unconditionally when the
//! well-known bindings table flips `import 'streams'` to it.

use lazy_static::lazy_static;
use perry_ffi::{
    alloc_buffer, alloc_string, build_object_shape, gc_register_mutable_root_scanner,
    js_array_alloc, js_array_push, js_object_alloc_with_shape, js_object_set_field, ArrayHeader,
    BufferHeader, GcRootVisitor, JsClosure, JsValue, ObjectHeader, Promise, RawClosureHeader,
    StringHeader,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, Once};

// NaN-box tag values. Mirror perry-runtime's stable JSValue tags
// (documented in perry-runtime/src/value.rs and committed across
// the v0.5.x cycle). perry-ffi exports JsValue::UNDEFINED etc. as
// safe constructors; the raw `u64` tags are still useful in the
// scanner / chunk-decoder paths where we work with the raw bits
// returned by the runtime.
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
#[allow(dead_code)]
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
#[cfg(test)]
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
#[cfg(test)]
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

// Runtime exit points used by the deferred-feature stubs. perry-ffi
// doesn't expose an error/throw helper today — these are the same
// stable extern "C" exports perry-stdlib uses, declared here
// directly so the BYOB / queuing-strategy / async-iterable stubs
// produce node-shaped TypeError exceptions instead of returning
// undefined silently. Should be lifted into perry-ffi once a second
// wrapper needs to throw.
extern "C" {
    fn js_error_new_with_message(message: *mut StringHeader) -> *const u8;
    fn js_throw(value: f64) -> !;
}

// ─────────────────────────────────────────────────────────────────────
// State
// ─────────────────────────────────────────────────────────────────────

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

// SAFETY: Promise / closure pointers cross the FFI boundary as raw
// integers. The runtime guarantees their lifetimes via GC roots
// (registered below); the registry's `Send + Sync` bound is what
// lets these structs live behind the perry-ffi handle registry on
// other wrappers, but here we keep our own per-process maps that
// were the original streams.rs design — no behavioural change.
unsafe impl Send for ReadableStreamData {}
unsafe impl Send for WritableStreamData {}
unsafe impl Send for ReaderData {}
unsafe impl Send for WriterData {}

lazy_static! {
    static ref READABLE_STREAMS: Mutex<HashMap<usize, ReadableStreamData>> =
        Mutex::new(HashMap::new());
    static ref NEXT_RS_ID: Mutex<usize> = Mutex::new(1);
    static ref WRITABLE_STREAMS: Mutex<HashMap<usize, WritableStreamData>> =
        Mutex::new(HashMap::new());
    static ref NEXT_WS_ID: Mutex<usize> = Mutex::new(1);
    static ref TRANSFORM_STREAMS: Mutex<HashMap<usize, TransformStreamData>> =
        Mutex::new(HashMap::new());
    static ref NEXT_TS_ID: Mutex<usize> = Mutex::new(1);
    static ref READERS: Mutex<HashMap<usize, ReaderData>> = Mutex::new(HashMap::new());
    static ref NEXT_READER_ID: Mutex<usize> = Mutex::new(1);
    static ref WRITERS: Mutex<HashMap<usize, WriterData>> = Mutex::new(HashMap::new());
    static ref NEXT_WRITER_ID: Mutex<usize> = Mutex::new(1);
    /// Maps a writable-side handle for a TransformStream to its
    /// owning TransformStream id. Lookup is what makes
    /// `pipeTo(transform.writable)` / `writer.write` route through
    /// the user's `transform(chunk, controller)` callback instead of
    /// a missing `write_cb`.
    static ref TRANSFORM_PAIRS: Mutex<HashMap<usize, usize>> = Mutex::new(HashMap::new());
}

static GC_REGISTERED: Once = Once::new();

/// Register the streams GC root scanner once. Closures held by user-
/// supplied `start` / `pull` / `cancel` / `write` / `close` / `abort` /
/// `transform` / `flush` callbacks live in the registry maps below; the
/// runtime GC mark phase wouldn't see them otherwise and a sweep
/// between registration and dispatch would free the closure body. Same
/// shape as perry-ext-events / perry-ext-http.
fn ensure_gc_registered() {
    GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner(scan_stream_roots);
    });
}

fn scan_stream_roots(visitor: &mut GcRootVisitor<'_>) {
    let visit_chunk = |bits: &mut u64, visitor: &mut GcRootVisitor<'_>| {
        let top = *bits >> 48;
        if top == 0x7FFD || top == 0x7FFF {
            visitor.visit_nanbox_u64_slot(bits);
        }
    };

    if let Ok(mut map) = READABLE_STREAMS.lock() {
        for s in map.values_mut() {
            visitor.visit_i64_slot(&mut s.start_cb);
            visitor.visit_i64_slot(&mut s.pull_cb);
            visitor.visit_i64_slot(&mut s.cancel_cb);
            for c in s.chunks.iter_mut() {
                visit_chunk(c, visitor);
            }
            for p in s.pending_reads.iter_mut() {
                visitor.visit_raw_mut_ptr_slot(p);
            }
            if s.state == ReadableState::Errored {
                visit_chunk(&mut s.error_value, visitor);
            }
        }
    }
    if let Ok(mut map) = WRITABLE_STREAMS.lock() {
        for s in map.values_mut() {
            visitor.visit_i64_slot(&mut s.write_cb);
            visitor.visit_i64_slot(&mut s.close_cb);
            visitor.visit_i64_slot(&mut s.abort_cb);
            for (chunk, p) in s.write_queue.iter_mut() {
                visit_chunk(chunk, visitor);
                visitor.visit_raw_mut_ptr_slot(p);
            }
            visitor.visit_raw_mut_ptr_slot(&mut s.ready_promise);
            visitor.visit_raw_mut_ptr_slot(&mut s.closed_promise);
            if s.state == WritableState::Errored {
                visit_chunk(&mut s.error_value, visitor);
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

/// Build a `{ value, done }` IteratorResult object. The runtime's
/// shape-aware allocator clusters allocations of the same shape so
/// hot read paths don't churn the shape table.
unsafe fn build_iter_result(value_bits: u64, done: bool) -> u64 {
    let (packed, shape_id) = build_object_shape(&["value", "done"]);
    let obj: *mut ObjectHeader =
        js_object_alloc_with_shape(shape_id, 2, packed.as_ptr(), packed.len() as u32);
    if obj.is_null() {
        return TAG_UNDEFINED;
    }
    js_object_set_field(obj, 0, JsValue::from_bits(value_bits));
    let done_value = if done {
        JsValue::from_bits(TAG_TRUE)
    } else {
        JsValue::from_bits(TAG_FALSE)
    };
    js_object_set_field(obj, 1, done_value);
    JsValue::from_object_ptr(obj as *mut u8).bits()
}

unsafe fn alloc_uint8array_from_bytes(bytes: &[u8]) -> u64 {
    let buf: *mut BufferHeader = alloc_buffer(bytes);
    JsValue::from_object_ptr(buf as *mut u8).bits()
}

unsafe fn read_bytes_from_chunk(chunk_bits: u64) -> Option<Vec<u8>> {
    let top = chunk_bits >> 48;
    if top != 0x7FFD {
        return None;
    }
    let ptr = (chunk_bits & POINTER_MASK) as *const BufferHeader;
    perry_ffi::read_buffer_bytes(ptr).map(|s| s.to_vec())
}

/// Allocate a node-shaped `Error` and throw it via the runtime's
/// stable `js_throw` exit. Used by the `not_implemented` stubs only.
unsafe fn throw_with_message(msg: &str) -> ! {
    let s = alloc_string(msg);
    let err = js_error_new_with_message(s.as_raw());
    let bits = JsValue::from_object_ptr(err as *mut u8).bits();
    js_throw(f64::from_bits(bits))
}

// ── Promise helpers ──────────────────────────────────────────────────

/// Allocate a fresh, unresolved promise. Wraps perry-ffi's
/// `JsPromise::new()` and returns the raw pointer for storage in
/// the per-stream maps (the actual resolution path uses the bits
/// helpers below since `JsPromise` consumes itself on resolution).
fn promise_new() -> *mut Promise {
    perry_ffi::JsPromise::new().as_raw()
}

unsafe fn promise_resolve_bits(p: *mut Promise, bits: u64) {
    if !p.is_null() {
        // SAFETY: `p` was returned from `perry_ffi::JsPromise::new()`
        // exactly once; resolving via a fresh `JsPromise` wrapper
        // delivers the value to the awaiter without dropping the
        // promise twice.
        let pr = perry_ffi::JsPromise::from_raw(p);
        pr.resolve(JsValue::from_bits(bits));
    }
}

unsafe fn promise_reject_bits(p: *mut Promise, bits: u64) {
    if !p.is_null() {
        let pr = perry_ffi::JsPromise::from_raw(p);
        pr.reject(JsValue::from_bits(bits));
    }
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
    let ready = promise_new();
    let closed = promise_new();
    unsafe {
        promise_resolve_bits(ready, TAG_UNDEFINED);
    }
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
        let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
        let _ = closure.call1(controller);
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
    let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
    let _ = closure.call1(controller);
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
        promise_resolve_bits(p, result);
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
        promise_reject_bits(p, reason_bits);
    }
}

// ─────────────────────────────────────────────────────────────────────
// ReadableStream FFI
// ─────────────────────────────────────────────────────────────────────

/// `new ReadableStream({ start, pull, cancel })` — `start_cb` /
/// `pull_cb` / `cancel_cb` are NaN-boxed `*ClosureHeader` bits (or
/// undefined). The new stream's controller is the stream handle
/// itself; user code calls `controller.enqueue(c)` etc. to drive it.
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

/// Build a single-chunk readable stream from an owned byte buffer.
/// Used internally by the (currently-stubbed) blob/response
/// constructors and exposed publicly so other wrappers porting onto
/// perry-ffi can build pre-buffered streams without duplicating the
/// state-machine wiring.
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
    let mut g = READABLE_STREAMS.lock().unwrap();
    let s = match g.get_mut(&id) {
        Some(s) => s,
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    if s.reader_handle.is_some() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let reader_id = next_id(&NEXT_READER_ID);
    let closed_p = promise_new();
    if s.state == ReadableState::Closed {
        promise_resolve_bits(closed_p, TAG_UNDEFINED);
    } else if s.state == ReadableState::Errored {
        promise_reject_bits(closed_p, s.error_value);
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
    let promise = promise_new();
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
        let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
        let _ = closure.call1(reason);
    }
    close_pending(id);
    promise_resolve_bits(promise, TAG_UNDEFINED);
    promise
}

/// Single-chunk readable stream from raw bytes, public for any
/// other wrapper (e.g. a future cross-wrapper bridge between
/// perry-ext-fetch's blob handles and this crate). The current
/// `js_readable_stream_from_blob` / `js_readable_stream_from_response`
/// surfaces would be implemented here in a v0.6.0 followup — for
/// today, this entry exists so the well-known flip's symbol
/// surface stays compatible with perry-stdlib's older copy.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_blob(_blob_id: f64) -> f64 {
    // Blob bytes live in perry-ext-fetch's BLOB_REGISTRY; perry-ffi
    // doesn't yet expose a cross-wrapper handle-bytes accessor. See
    // the punted-gaps section of this module's docs.
    let id = alloc_readable_from_bytes(Vec::new());
    id as f64
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_response(_resp_id: f64) -> f64 {
    let id = alloc_readable_from_bytes(Vec::new());
    id as f64
}

/// `ReadableStream.from(asyncIterable)` — deferred (issue #237 followup).
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_from_iterable(_value: f64) -> f64 {
    throw_with_message(
        "ReadableStream.from(asyncIterable) is not yet implemented (issue #237 followup)",
    );
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
        promise_resolve_bits(p, result);
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
                promise_resolve_bits(p, TAG_UNDEFINED);
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
            promise_reject_bits(p, reason_bits);
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
    let promise = promise_new();
    let reader_id = reader_handle as usize;
    let stream_id = match READERS.lock().unwrap().get(&reader_id) {
        Some(r) if r.locked => r.stream_handle,
        Some(_) => {
            let s = alloc_string("Reader is no longer locked to a stream");
            let err = js_error_new_with_message(s.as_raw());
            promise_reject_bits(promise, JsValue::from_object_ptr(err as *mut u8).bits());
            return promise;
        }
        None => {
            let s = alloc_string("Invalid reader");
            let err = js_error_new_with_message(s.as_raw());
            promise_reject_bits(promise, JsValue::from_object_ptr(err as *mut u8).bits());
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
            promise_reject_bits(promise, value);
        }
        Some((value, done, false)) => {
            let result = build_iter_result(value, done);
            promise_resolve_bits(promise, result);
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
            let p = promise_new();
            unsafe { promise_resolve_bits(p, TAG_UNDEFINED) };
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
            let p = promise_new();
            promise_resolve_bits(p, TAG_UNDEFINED);
            return p;
        }
    };
    js_readable_stream_cancel(stream_id as f64, reason)
}

// ─────────────────────────────────────────────────────────────────────
// tee / pipeTo / pipeThrough
// ─────────────────────────────────────────────────────────────────────

/// `stream.tee()` — returns an array of two new ReadableStreams.
/// Both branches drain the SOURCE eagerly into separate per-branch
/// queues at tee time. Correct for the buffered consumers Perry
/// exposes (`blob.stream()` / `response.body` are pre-buffered) and
/// the "user source already enqueued everything synchronously in
/// start" pattern. Lazy producers post-tee see only the chunks
/// present at tee time — same trade-off Node's
/// `Readable.from([...]).tee()` makes for already-buffered
/// iterables.
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

    let mut arr: *mut ArrayHeader = js_array_alloc(2);
    arr = js_array_push(arr, JsValue::from_number(id_a as f64));
    arr = js_array_push(arr, JsValue::from_number(id_b as f64));
    f64::from_bits(JsValue::from_object_ptr(arr as *mut u8).bits())
}

/// `readable.pipeTo(writable)` — drives the readable into the
/// writable synchronously chunk-by-chunk. Returns a Promise that
/// resolves when the writable closes cleanly, or rejects on error.
/// Synchronous because the buffered model has all bytes resident
/// already; an async loop here would just queue tasks against an
/// empty event loop.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_pipe_to(
    readable_handle: f64,
    writable_handle: f64,
) -> *mut Promise {
    let promise = promise_new();
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
                        promise_reject_bits(promise, e);
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
                        let closure = JsClosure::from_raw(write_cb as *const RawClosureHeader);
                        let _ = closure.call1(f64::from_bits(chunk));
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
            let closure = JsClosure::from_raw(close_cb as *const RawClosureHeader);
            let _ = closure.call0();
        }
        if let Some(w) = WRITABLE_STREAMS.lock().unwrap().get_mut(&w_id) {
            w.state = WritableState::Closed;
            let cp = w.closed_promise;
            promise_resolve_bits(cp, TAG_UNDEFINED);
        }
    }
    promise_resolve_bits(promise, TAG_UNDEFINED);
    promise
}

/// `readable.pipeThrough({readable, writable})` — pipeTo into the
/// transform's writable side, return its readable side. Caller
/// already destructured the TransformStream into its readable /
/// writable handles.
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
    let promise = promise_new();
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
        let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
        let _ = closure.call0();
    }
    if !closed_p.is_null() {
        promise_resolve_bits(closed_p, TAG_UNDEFINED);
    }
    promise_resolve_bits(promise, TAG_UNDEFINED);
    promise
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_abort(stream_handle: f64, reason: f64) -> *mut Promise {
    let promise = promise_new();
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
        let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
        let _ = closure.call1(reason);
    }
    if !closed_p.is_null() {
        promise_reject_bits(closed_p, reason_bits);
    }
    promise_resolve_bits(promise, TAG_UNDEFINED);
    promise
}

// ─────────────────────────────────────────────────────────────────────
// WritableStreamDefaultWriter FFI
// ─────────────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn js_writer_write(writer_handle: f64, chunk: f64) -> *mut Promise {
    let promise = promise_new();
    let writer_id = writer_handle as usize;
    let stream_id = match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) if w.locked => w.stream_handle,
        _ => {
            let s = alloc_string("Writer is no longer locked to a stream");
            let err = js_error_new_with_message(s.as_raw());
            promise_reject_bits(promise, JsValue::from_object_ptr(err as *mut u8).bits());
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
            promise_reject_bits(promise, e);
            return promise;
        }
        _ => {
            let s = alloc_string("Stream is closed or closing");
            let err = js_error_new_with_message(s.as_raw());
            promise_reject_bits(promise, JsValue::from_object_ptr(err as *mut u8).bits());
            return promise;
        }
    };
    if cb != 0 {
        let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
        let _ = closure.call1(chunk);
    }
    promise_resolve_bits(promise, TAG_UNDEFINED);
    promise
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_close(writer_handle: f64) -> *mut Promise {
    let writer_id = writer_handle as usize;
    let stream_id = match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) => w.stream_handle,
        None => {
            let p = promise_new();
            promise_resolve_bits(p, TAG_UNDEFINED);
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
            let p = promise_new();
            promise_resolve_bits(p, TAG_UNDEFINED);
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
            let p = promise_new();
            unsafe { promise_resolve_bits(p, TAG_UNDEFINED) };
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
            let p = promise_new();
            unsafe { promise_resolve_bits(p, TAG_UNDEFINED) };
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
    let ready = promise_new();
    let closed = promise_new();
    promise_resolve_bits(ready, TAG_UNDEFINED);
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

/// Replacement `writer.write` for the writable side of a
/// TransformStream — invokes the user transform with (chunk,
/// transformController) where the transformController is the
/// readable-side stream handle (so `controller.enqueue(...)` reuses
/// the readable controller path).
unsafe fn transform_write(writable_id: usize, chunk: f64) -> *mut Promise {
    let promise = promise_new();
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
        let closure = JsClosure::from_raw(transform_cb as *const RawClosureHeader);
        let _ = closure.call2(chunk, readable_id as f64);
    } else {
        // Identity transform — pass-through.
        js_readable_stream_controller_enqueue(readable_id as f64, chunk);
    }
    promise_resolve_bits(promise, TAG_UNDEFINED);
    promise
}

unsafe fn transform_close(writable_id: usize) -> *mut Promise {
    let promise = promise_new();
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
        let closure = JsClosure::from_raw(flush_cb as *const RawClosureHeader);
        let _ = closure.call1(readable_id as f64);
    }
    if readable_id != 0 {
        js_readable_stream_controller_close(readable_id as f64);
    }
    if let Some(s) = WRITABLE_STREAMS.lock().unwrap().get_mut(&writable_id) {
        s.state = WritableState::Closed;
        let cp = s.closed_promise;
        promise_resolve_bits(cp, TAG_UNDEFINED);
    }
    promise_resolve_bits(promise, TAG_UNDEFINED);
    promise
}

// ─────────────────────────────────────────────────────────────────────
// Stubs for deferred surface (issue #237 followups)
// ─────────────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn js_streams_throw_byob_not_implemented() -> f64 {
    throw_with_message("BYOB readers are not yet implemented (issue #237 followup)");
}

#[no_mangle]
pub unsafe extern "C" fn js_streams_throw_byte_length_not_implemented() -> f64 {
    throw_with_message("ByteLengthQueuingStrategy is not yet implemented (issue #237 followup)");
}

// ─────────────────────────────────────────────────────────────────────
// Public helpers
// ─────────────────────────────────────────────────────────────────────

/// Read every queued chunk into a Vec<u8>, draining the stream.
/// Used by `new Response(stream)` / `new Request(url, { body:
/// stream })` patterns — drain the buffered chunks at construction
/// time so the resulting body bytes match what a real serializer
/// would produce. Currently no users in-tree (perry-ext-fetch's
/// Request/Response constructors don't yet bridge to streams);
/// kept exported so a future cross-wrapper integration can pick it
/// up without an ABI bump.
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

// ─────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // Tests deliberately exercise pure-Rust paths (no FFI through
    // `promise_new` / `js_array_alloc` / `alloc_buffer`), because the
    // perry-ext-streams test binary is linked without the perry-stdlib
    // archive that defines `perry_ffi_promise_new` etc. (perry-stdlib
    // gates those shims behind `async-runtime`). Live FFI exercise
    // happens at the wrapper-level smoke step in the well-known flip
    // sweep — same model perry-ext-bcrypt / perry-ext-argon2 /
    // perry-ext-mongodb use.
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct GcTestGuard {
        frame: u64,
        _lock: MutexGuard<'static, ()>,
    }

    impl GcTestGuard {
        fn new() -> Self {
            let lock = GC_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            perry_runtime::gc::js_gc_write_barriers_emitted(1);
            let frame = perry_runtime::gc::js_shadow_frame_push(0);
            Self { frame, _lock: lock }
        }
    }

    impl Drop for GcTestGuard {
        fn drop(&mut self) {
            perry_runtime::gc::js_shadow_frame_pop(self.frame);
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
        }
    }

    fn young_gc_root() -> i64 {
        perry_runtime::arena::arena_alloc_gc(32, 8, perry_runtime::gc::GC_TYPE_STRING) as i64
    }

    fn young_nanbox_root(tag: u64) -> u64 {
        let ptr =
            perry_runtime::arena::arena_alloc_gc(32, 8, perry_runtime::gc::GC_TYPE_STRING) as u64;
        tag | (ptr & POINTER_MASK)
    }

    fn young_promise_root() -> *mut Promise {
        let ptr = perry_runtime::arena::arena_alloc_gc(
            std::mem::size_of::<perry_runtime::Promise>(),
            std::mem::align_of::<perry_runtime::Promise>(),
            perry_runtime::gc::GC_TYPE_PROMISE,
        );
        unsafe {
            std::ptr::write_bytes(ptr, 0, std::mem::size_of::<perry_runtime::Promise>());
        }
        ptr as *mut Promise
    }

    fn assert_rewritten_addr(before: usize, after: usize) {
        assert_ne!(after, before);
        assert!(perry_runtime::arena::pointer_in_nursery(after));
    }

    fn assert_rewritten_i64(before: i64, after: i64) {
        assert_rewritten_addr(before as usize, after as usize);
    }

    fn assert_rewritten_bits(before: u64, after: u64) {
        assert_eq!(after & !POINTER_MASK, before & !POINTER_MASK);
        assert_rewritten_addr(
            (before & POINTER_MASK) as usize,
            (after & POINTER_MASK) as usize,
        );
    }

    fn assert_rewritten_ptr<T>(before: *mut T, after: *mut T) {
        assert_rewritten_addr(before as usize, after as usize);
    }

    #[test]
    fn gc_scanner_registers_idempotently() {
        // Calling `ensure_gc_registered` repeatedly must not panic
        // and must register the scanner exactly once (Once
        // guarantees). Other wrappers may already have registered
        // their own scanners; no cross-talk expected.
        ensure_gc_registered();
        ensure_gc_registered();
        ensure_gc_registered();
    }

    #[test]
    fn gc_mutable_scanner_rewrites_all_stream_root_surfaces() {
        let _guard = GcTestGuard::new();
        perry_ffi::gc_register_mutable_root_scanner(scan_stream_roots);

        let readable_id = usize::MAX - 9_100;
        let readable_start = young_gc_root();
        let readable_pull = young_gc_root();
        let readable_cancel = young_gc_root();
        let readable_chunk = young_nanbox_root(POINTER_TAG);
        let readable_error = young_nanbox_root(STRING_TAG);
        let readable_pending = young_promise_root();
        let mut readable_chunks = VecDeque::new();
        readable_chunks.push_back(readable_chunk);
        let mut pending_reads = VecDeque::new();
        pending_reads.push_back(readable_pending);
        READABLE_STREAMS.lock().unwrap().insert(
            readable_id,
            ReadableStreamData {
                state: ReadableState::Errored,
                chunks: readable_chunks,
                pending_reads,
                start_cb: readable_start,
                pull_cb: readable_pull,
                cancel_cb: readable_cancel,
                high_water_mark: 1.0,
                pulling: false,
                started: true,
                reader_handle: None,
                error_value: readable_error,
                canceled: false,
            },
        );

        let writable_id = usize::MAX - 9_101;
        let writable_write = young_gc_root();
        let writable_close = young_gc_root();
        let writable_abort = young_gc_root();
        let write_queue_chunk = young_nanbox_root(POINTER_TAG);
        let write_queue_promise = young_promise_root();
        let writable_ready = young_promise_root();
        let writable_closed = young_promise_root();
        let writable_error = young_nanbox_root(STRING_TAG);
        let mut write_queue = VecDeque::new();
        write_queue.push_back((write_queue_chunk, write_queue_promise));
        WRITABLE_STREAMS.lock().unwrap().insert(
            writable_id,
            WritableStreamData {
                state: WritableState::Errored,
                write_cb: writable_write,
                close_cb: writable_close,
                abort_cb: writable_abort,
                write_queue,
                in_flight: false,
                high_water_mark: 1.0,
                writer_handle: None,
                error_value: writable_error,
                ready_promise: writable_ready,
                closed_promise: writable_closed,
            },
        );

        let transform_id = usize::MAX - 9_102;
        let transform_cb = young_gc_root();
        let flush_cb = young_gc_root();
        TRANSFORM_STREAMS.lock().unwrap().insert(
            transform_id,
            TransformStreamData {
                readable_handle: readable_id,
                writable_handle: writable_id,
                transform_cb,
                flush_cb,
            },
        );

        let reader_id = usize::MAX - 9_103;
        let reader_closed = young_promise_root();
        READERS.lock().unwrap().insert(
            reader_id,
            ReaderData {
                stream_handle: readable_id,
                locked: true,
                closed_promise: reader_closed,
            },
        );

        let writer_id = usize::MAX - 9_104;
        let writer_closed = young_promise_root();
        let writer_ready = young_promise_root();
        WRITERS.lock().unwrap().insert(
            writer_id,
            WriterData {
                stream_handle: writable_id,
                locked: true,
                closed_promise: writer_closed,
                ready_promise: writer_ready,
            },
        );

        let _ = perry_runtime::gc::gc_collect_minor();

        {
            let readable = READABLE_STREAMS.lock().unwrap();
            let s = readable
                .get(&readable_id)
                .expect("readable stream should remain live");
            assert_rewritten_i64(readable_start, s.start_cb);
            assert_rewritten_i64(readable_pull, s.pull_cb);
            assert_rewritten_i64(readable_cancel, s.cancel_cb);
            assert_rewritten_bits(readable_chunk, s.chunks[0]);
            assert_rewritten_ptr(readable_pending, s.pending_reads[0]);
            assert_rewritten_bits(readable_error, s.error_value);
        }
        {
            let writable = WRITABLE_STREAMS.lock().unwrap();
            let s = writable
                .get(&writable_id)
                .expect("writable stream should remain live");
            assert_rewritten_i64(writable_write, s.write_cb);
            assert_rewritten_i64(writable_close, s.close_cb);
            assert_rewritten_i64(writable_abort, s.abort_cb);
            assert_rewritten_bits(write_queue_chunk, s.write_queue[0].0);
            assert_rewritten_ptr(write_queue_promise, s.write_queue[0].1);
            assert_rewritten_ptr(writable_ready, s.ready_promise);
            assert_rewritten_ptr(writable_closed, s.closed_promise);
            assert_rewritten_bits(writable_error, s.error_value);
        }
        {
            let transforms = TRANSFORM_STREAMS.lock().unwrap();
            let t = transforms
                .get(&transform_id)
                .expect("transform stream should remain live");
            assert_rewritten_i64(transform_cb, t.transform_cb);
            assert_rewritten_i64(flush_cb, t.flush_cb);
        }
        {
            let readers = READERS.lock().unwrap();
            let r = readers.get(&reader_id).expect("reader should remain live");
            assert_rewritten_ptr(reader_closed, r.closed_promise);
        }
        {
            let writers = WRITERS.lock().unwrap();
            let w = writers.get(&writer_id).expect("writer should remain live");
            assert_rewritten_ptr(writer_closed, w.closed_promise);
            assert_rewritten_ptr(writer_ready, w.ready_promise);
        }

        READABLE_STREAMS.lock().unwrap().remove(&readable_id);
        WRITABLE_STREAMS.lock().unwrap().remove(&writable_id);
        TRANSFORM_STREAMS.lock().unwrap().remove(&transform_id);
        READERS.lock().unwrap().remove(&reader_id);
        WRITERS.lock().unwrap().remove(&writer_id);
    }

    #[test]
    fn closure_from_bits_decoding() {
        // TAG_UNDEFINED / TAG_NULL / 0 → 0 (no closure).
        unsafe {
            assert_eq!(closure_from_bits(TAG_UNDEFINED), 0);
            assert_eq!(closure_from_bits(TAG_NULL), 0);
            assert_eq!(closure_from_bits(0), 0);
            // Pointer-tagged bits → masked pointer.
            let ptr_bits = POINTER_TAG | 0xCAFE_BABE;
            assert_eq!(closure_from_bits(ptr_bits), 0xCAFE_BABE_i64);
        }
    }

    #[test]
    fn drain_unknown_returns_empty() {
        // Draining a never-allocated stream id returns an empty vec
        // (error path used by the cross-wrapper bridge).
        let bytes = drain_readable_into_bytes(usize::MAX);
        assert!(bytes.is_empty());
    }

    #[test]
    fn next_id_is_monotonic() {
        // Smoke test for the per-table id allocator the FFI exports
        // route through. Distinct slots advance independently.
        let slot = Mutex::new(100usize);
        let a = next_id(&slot);
        let b = next_id(&slot);
        let c = next_id(&slot);
        assert_eq!(a, 100);
        assert_eq!(b, 101);
        assert_eq!(c, 102);
    }

    #[test]
    fn read_bytes_from_chunk_skips_non_pointer_bits() {
        // A non-pointer-tagged chunk (raw double, undefined, …)
        // returns None — the GC scanner can't honor it as a buffer
        // either, but `drain_readable_into_bytes` shouldn't crash.
        unsafe {
            assert!(read_bytes_from_chunk(TAG_UNDEFINED).is_none());
            // Number bits (raw f64=42.0) — top 16 ≠ 0x7FFD.
            assert!(read_bytes_from_chunk(42.0_f64.to_bits()).is_none());
        }
    }
}
