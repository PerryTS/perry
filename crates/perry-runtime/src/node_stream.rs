//! Node `stream` module — `new Readable(opts)`, `new Writable(opts)`,
//! `new Duplex(opts)`, `new Transform(opts)`, `new PassThrough(opts)`,
//! and `Readable.from(iterable)`. Closes #631.
//!
//! Pre-fix, these constructors fell through to the generic `Expr::New`
//! placeholder (an empty `ObjectHeader`), so `r.on`, `r.pipe`, `.read`
//! etc. were all `undefined`. Any downstream code that touched stream
//! methods crashed with `(undefined).x is not a function`.
//!
//! This module mirrors the closure-fields pattern used by fs streams
//! (`crates/perry-runtime/src/fs.rs::build_stream_object`): allocate
//! an `ObjectHeader` keyed by method names whose values are NaN-boxed
//! closure pointers. Each closure captures the host object pointer in
//! slot 0, so chained calls like `.on(...).on(...).pipe(...)` return
//! `this` and the chain doesn't lose identity.
//!
//! Method semantics are minimal stubs — Node's stream surface (full
//! EventEmitter pump, backpressure, async iteration) is far beyond
//! the scope of this issue. The acceptance criterion (#631) is
//! byte-identical typeof output: every method name reports
//! `"function"`, and chained calls don't crash. Real data flow
//! through `read`/`write`/`pipe` is left for a dedicated streams
//! runtime rewrite.

use crate::closure::{js_closure_alloc, js_closure_get_capture_ptr, ClosureHeader};
use crate::object::{
    js_object_alloc_with_shape, js_object_get_field_by_name_f64, js_object_set_field,
    js_object_set_field_by_name, ObjectHeader,
};
use crate::value::JSValue;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

// Shape ids — pick a band well clear of fs streams (`STREAM_SHAPE_ID =
// 0x7FFF_FE40` + method_count). The base ids are spaced 0x40 (64
// slots) apart so each constructor's `base + method_count` lands in
// its own band and stays a unique shape-cache key — Readable's method
// set grew to 28 with the #1558 iterator helpers, so the historical
// 16-slot spacing no longer left enough headroom.
const READABLE_SHAPE_ID: u32 = 0x7FFF_FE60;
const WRITABLE_SHAPE_ID: u32 = 0x7FFF_FEA0;
const DUPLEX_SHAPE_ID: u32 = 0x7FFF_FEE0;
const READABLE_CHUNKS_KEY: &[u8] = b"__perryReadableChunks";
const READABLE_ERROR_KEY: &[u8] = b"__perryReadableError";
const READABLE_SIGNAL_KEY: &[u8] = b"__perryReadableSignal";
const READABLE_READ_KEY: &[u8] = b"__perryReadableRead";
const READABLE_READ_INVOKED_KEY: &[u8] = b"__perryReadableReadInvoked";
const STREAM_ENDED_KEY: &[u8] = b"__perryStreamEnded";
const WRITABLE_WRITE_KEY: &[u8] = b"__perryWritableWrite";

// ─────────────────────────────────────────────────────────────────
// Stub method bodies. Each receives the closure pointer (slot 0
// holds the host object's NaN-boxed bits cast to i64) plus its
// argument list. Bodies return either `this`, `null`, `true`, or
// `false`, matching the most useful subset of Node's contract for
// chained no-ops.
// ─────────────────────────────────────────────────────────────────

#[inline]
fn this_value(closure: *const ClosureHeader) -> f64 {
    // Slot 0 was set by `build_object` to the NaN-boxed bits of the
    // host object value cast to i64; reverse the cast.
    if !closure.is_null() {
        let bits = js_closure_get_capture_ptr(closure, 0) as u64;
        return f64::from_bits(bits);
    }
    crate::object::js_implicit_this_get()
}

extern "C" fn ns_chain0(closure: *const ClosureHeader) -> f64 {
    this_value(closure)
}
extern "C" fn ns_chain1(closure: *const ClosureHeader, _a: f64) -> f64 {
    this_value(closure)
}
extern "C" fn ns_chain2(closure: *const ClosureHeader, _a: f64, _b: f64) -> f64 {
    this_value(closure)
}
extern "C" fn ns_chain3(closure: *const ClosureHeader, _a: f64, _b: f64, _c: f64) -> f64 {
    this_value(closure)
}

extern "C" fn ns_emit2(closure: *const ClosureHeader, event: f64, arg: f64) -> f64 {
    if string_value_eq(event, b"error") {
        set_hidden_value(this_value(closure), hidden_error_key(), arg);
        return f64::from_bits(TAG_TRUE);
    }
    f64::from_bits(TAG_FALSE)
}
extern "C" fn ns_resume0(closure: *const ClosureHeader) -> f64 {
    let stream = this_value(closure);
    set_hidden_value(stream, hidden_ended_key(), f64::from_bits(TAG_TRUE));
    stream
}

extern "C" fn ns_read1(closure: *const ClosureHeader, _n: f64) -> f64 {
    set_hidden_value(
        this_value(closure),
        hidden_ended_key(),
        f64::from_bits(TAG_TRUE),
    );
    f64::from_bits(TAG_NULL)
}
extern "C" fn ns_pipe1(_closure: *const ClosureHeader, dest: f64) -> f64 {
    // Node's `Readable.pipe(dest)` returns `dest` to allow `r.pipe(a).pipe(b)`.
    dest
}
extern "C" fn writable_write_callback_noop(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_write2(closure: *const ClosureHeader, chunk: f64, enc: f64) -> f64 {
    let stream = this_value(closure);
    if let Some(write) = writable_hidden_write(stream) {
        let cb = js_closure_alloc(writable_write_callback_noop as *const u8, 0);
        let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
        let args = [chunk, enc, cb_value];
        let prev_this = crate::object::js_implicit_this_set(stream);
        unsafe {
            let _ = crate::closure::js_native_call_value(write, args.as_ptr(), args.len());
        }
        crate::object::js_implicit_this_set(prev_this);
    }
    f64::from_bits(TAG_TRUE)
}
extern "C" fn ns_end1(closure: *const ClosureHeader, chunk: f64) -> f64 {
    if !JSValue::from_bits(chunk.to_bits()).is_undefined() {
        let _ = ns_write2(closure, chunk, f64::from_bits(TAG_UNDEFINED));
    }
    let stream = this_value(closure);
    set_hidden_value(stream, hidden_ended_key(), f64::from_bits(TAG_TRUE));
    stream
}

fn stream_value_from_handle(stream_handle: i64) -> f64 {
    if stream_handle == 0 {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        f64::from_bits(JSValue::pointer(stream_handle as *const u8).bits())
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_emit(stream_handle: i64, event: f64, arg: f64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    if string_value_eq(event, b"error") {
        set_hidden_value(stream, hidden_error_key(), arg);
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_read(stream_handle: i64, _n: f64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    set_hidden_value(stream, hidden_ended_key(), f64::from_bits(TAG_TRUE));
    f64::from_bits(TAG_NULL)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_resume(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    set_hidden_value(stream, hidden_ended_key(), f64::from_bits(TAG_TRUE));
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_write(stream_handle: i64, chunk: f64, enc: f64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    if let Some(write) = writable_hidden_write(stream) {
        let cb = js_closure_alloc(writable_write_callback_noop as *const u8, 0);
        let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
        let args = [chunk, enc, cb_value];
        let prev_this = crate::object::js_implicit_this_set(stream);
        unsafe {
            let _ = crate::closure::js_native_call_value(write, args.as_ptr(), args.len());
        }
        crate::object::js_implicit_this_set(prev_this);
    }
    f64::from_bits(TAG_TRUE)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_end(stream_handle: i64, chunk: f64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    if !JSValue::from_bits(chunk.to_bits()).is_undefined() {
        let _ = js_node_stream_method_write(stream_handle, chunk, f64::from_bits(TAG_UNDEFINED));
    }
    set_hidden_value(stream, hidden_ended_key(), f64::from_bits(TAG_TRUE));
    stream
}
extern "C" fn ns_listener_count(_closure: *const ClosureHeader, _e: f64) -> f64 {
    0.0
}
extern "C" fn ns_listeners(_closure: *const ClosureHeader, _e: f64) -> f64 {
    let arr = crate::array::js_array_alloc(0);
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}
extern "C" fn ns_undefined0(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

// ─────────────────────────────────────────────────────────────────
// #1558: Readable async iterator helpers (Node 17+).
//
// `map` / `filter` / `flatMap` / `take` / `drop` are lazy in Node —
// they return a new Readable — while `toArray` / `reduce` / `forEach`
// / `find` / `some` / `every` consume the stream and return a
// Promise. Perry's stub Readable already retains its source chunks in
// the hidden `__perryReadableChunks` array (see `Readable.from`), so
// these operate on that snapshot eagerly: the transforming helpers
// build a fresh chunk array wrapped in a new Readable (so chains like
// `r.map(f).filter(g).toArray()` keep working), and the consuming
// helpers compute the value and hand back an already-resolved Promise
// so `await` unwraps the expected result. A Readable with no retained
// chunks (a bare `new Readable()`) is treated as an empty source.
// ─────────────────────────────────────────────────────────────────

/// Extract the callback's closure pointer, or null when the argument
/// isn't a heap pointer (e.g. a missing/undefined callback).
#[inline]
fn callback_closure(value: f64) -> *const ClosureHeader {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 {
        std::ptr::null()
    } else {
        raw as *const ClosureHeader
    }
}

/// The readable's retained chunk list as an `ArrayHeader*`, or null
/// when it has no array-backed chunk storage.
#[inline]
fn readable_chunks_array(this: f64) -> *const crate::array::ArrayHeader {
    match readable_hidden_chunks(this) {
        Some(chunks) if is_array_like_value(chunks) => {
            raw_ptr_from_value(chunks) as *const crate::array::ArrayHeader
        }
        _ => std::ptr::null(),
    }
}

/// Wrap `value` in an already-fulfilled Promise, NaN-boxed.
#[inline]
fn resolved_promise(value: f64) -> f64 {
    let promise = crate::promise::js_promise_resolved(value);
    box_pointer(promise as *const u8)
}

/// Build a fresh Readable whose retained chunks are `chunks`.
#[inline]
fn readable_from_chunks(chunks: *const crate::array::ArrayHeader) -> f64 {
    js_node_stream_readable_from(box_pointer(chunks as *const u8))
}

/// NaN-box a freshly-allocated string.
#[inline]
fn string_value(bytes: &[u8]) -> f64 {
    let ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

/// Build the rejection reason used when an operation is aborted — a
/// plain `{ name: "AbortError", message }` object. Node rejects with a
/// DOMException whose `.name` is `"AbortError"`; callers only inspect
/// `.name`, so a plain object is byte-equivalent for parity.
fn abort_error() -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    js_object_set_field_by_name(obj, hidden_key(b"name"), string_value(b"AbortError"));
    js_object_set_field_by_name(
        obj,
        hidden_key(b"message"),
        string_value(b"The operation was aborted"),
    );
    box_pointer(obj as *const u8)
}

/// A rejected Promise carrying `reason`, NaN-boxed.
#[inline]
fn rejected_promise(reason: f64) -> f64 {
    box_pointer(crate::promise::js_promise_rejected(reason) as *const u8)
}

#[inline]
fn hidden_signal_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_SIGNAL_KEY)
}

/// The `AbortSignal` carried in `opts.signal`, if any.
fn options_signal(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"signal"))
}

/// The `AbortSignal` a lazy helper propagated onto this stream.
fn readable_stored_signal(this: f64) -> Option<f64> {
    get_hidden_value(this, hidden_signal_key())
}

/// The signal governing an operation on `this` with call `opts` — the
/// call's own `{ signal }` wins, otherwise one inherited from an
/// upstream lazy helper.
fn effective_signal(this: f64, opts: f64) -> Option<f64> {
    options_signal(opts).or_else(|| readable_stored_signal(this))
}

/// True when `signal` is an `AbortSignal` whose `aborted` flag is set.
fn signal_is_aborted(signal: f64) -> bool {
    match get_hidden_value(signal, hidden_key(b"aborted")) {
        Some(v) => crate::value::js_is_truthy(v) != 0,
        None => false,
    }
}

/// Recover a NaN-boxed Promise pointer from a closure capture slot.
#[inline]
fn promise_from_capture(closure: *const ClosureHeader, idx: u32) -> *mut crate::promise::Promise {
    let bits = js_closure_get_capture_ptr(closure, idx) as u64;
    crate::value::js_nanbox_get_pointer(f64::from_bits(bits)) as *mut crate::promise::Promise
}

/// Abort-listener body: reject the captured Promise with an AbortError.
extern "C" fn ns_abort_reject(closure: *const ClosureHeader) -> f64 {
    let p = promise_from_capture(closure, 0);
    if !p.is_null() {
        crate::promise::js_promise_reject(p, abort_error());
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Deferred-resolve body: fulfill the captured Promise (slot 0) with the
/// captured value (slot 1) on the next microtask — a no-op if an abort
/// already rejected it.
extern "C" fn ns_deferred_resolve(closure: *const ClosureHeader) -> f64 {
    let p = promise_from_capture(closure, 0);
    let value = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    if !p.is_null() {
        crate::promise::js_promise_resolve(p, value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Build a pending Promise for a consuming helper running under a
/// not-yet-aborted signal: an abort listener rejects it with an
/// AbortError, while a queued microtask fulfills it with `value` if no
/// abort fires first. This matches Node's async timing — the operation
/// is in flight when a synchronous `controller.abort()` lands before
/// the awaiter resumes.
fn deferred_promise(signal: f64, value: f64) -> f64 {
    let promise = crate::promise::js_promise_new();
    let promise_box = box_pointer(promise as *const u8);

    if let Some(sig_obj) = object_ptr_from_value(signal) {
        let reject_cl = js_closure_alloc(ns_abort_reject as *const u8, 1);
        crate::closure::js_closure_set_capture_ptr(reject_cl, 0, promise_box.to_bits() as i64);
        crate::url::js_abort_signal_add_listener(
            sig_obj,
            string_value(b"abort"),
            box_pointer(reject_cl as *const u8),
        );
    }

    let resolve_cl = js_closure_alloc(ns_deferred_resolve as *const u8, 2);
    crate::closure::js_closure_set_capture_ptr(resolve_cl, 0, promise_box.to_bits() as i64);
    crate::closure::js_closure_set_capture_ptr(resolve_cl, 1, value.to_bits() as i64);
    crate::builtins::js_queue_microtask(resolve_cl as i64);

    promise_box
}

/// Settle a consuming helper's result under any governing signal: reject
/// now if already aborted, defer if a signal is pending, else resolve.
fn settle_consuming(this: f64, opts: f64, value: f64) -> f64 {
    if let Some(err) = readable_hidden_error(this) {
        return rejected_promise(err);
    }
    match effective_signal(this, opts) {
        Some(sig) if signal_is_aborted(sig) => rejected_promise(abort_error()),
        Some(sig) => deferred_promise(sig, value),
        None => resolved_promise(value),
    }
}

/// Carry a lazy helper's source error and governing signal onto its
/// freshly-built result stream so a downstream consuming helper can
/// observe an abort or error that happens later in the chain.
fn propagate_stream_state(this: f64, opts: f64, result: f64) {
    if let Some(err) = readable_hidden_error(this) {
        set_hidden_value(result, hidden_error_key(), err);
    }
    if let Some(sig) = effective_signal(this, opts) {
        set_hidden_value(result, hidden_signal_key(), sig);
    }
}

/// Resolve a callback result that may be a Promise (an async mapper /
/// predicate) by draining microtasks until it settles, then reading the
/// fulfilled value. Bounded so a never-settling promise can't hang the
/// stub; an unresolved or rejected promise yields the original value.
fn settle(value: f64) -> f64 {
    if crate::promise::js_value_is_promise(value) == 0 {
        return value;
    }
    let p = crate::value::js_nanbox_get_pointer(value) as *mut crate::promise::Promise;
    if p.is_null() {
        return value;
    }
    for _ in 0..10_000 {
        if unsafe { (*p).state } != crate::promise::PromiseState::Pending {
            break;
        }
        if crate::promise::js_promise_run_microtasks() == 0 {
            break;
        }
    }
    unsafe {
        if (*p).state == crate::promise::PromiseState::Fulfilled {
            (*p).value
        } else {
            value
        }
    }
}

/// Invoke a single-argument stream callback and settle an async result.
#[inline]
fn call_settled(cb: *const ClosureHeader, arg: f64) -> f64 {
    settle(crate::closure::js_closure_call1(cb, arg))
}

/// Coerce a `take(n)` / `drop(n)` count argument to a clamped element
/// count (negative / NaN → 0, matching Node's normalization).
#[inline]
fn count_arg(value: f64) -> u32 {
    let n = JSValue::from_bits(value.to_bits()).to_number();
    if n.is_nan() || n <= 0.0 {
        0
    } else if n >= u32::MAX as f64 {
        u32::MAX
    } else {
        n as u32
    }
}

/// Append every element of array `arr` to `out`, returning the
/// possibly-reallocated `out`.
#[inline]
fn extend_with_array(
    mut out: *mut crate::array::ArrayHeader,
    arr: *const crate::array::ArrayHeader,
) -> *mut crate::array::ArrayHeader {
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        out = crate::array::js_array_push_f64(out, crate::array::js_array_get_f64(arr, i));
    }
    out
}

extern "C" fn ns_iter_to_array(closure: *const ClosureHeader, opts: f64) -> f64 {
    let this = this_value(closure);
    let arr = readable_chunks_array(this);
    let mut out = crate::array::js_array_alloc(0);
    if !arr.is_null() {
        out = extend_with_array(out, arr);
    }
    settle_consuming(this, opts, box_pointer(out as *const u8))
}

extern "C" fn ns_iter_map(closure: *const ClosureHeader, mapper: f64, opts: f64) -> f64 {
    let this = this_value(closure);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(mapper);
    let mut out = crate::array::js_array_alloc(0);
    if !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            out = crate::array::js_array_push_f64(out, call_settled(cb, el));
        }
    }
    let result = readable_from_chunks(out);
    propagate_stream_state(this, opts, result);
    result
}

extern "C" fn ns_iter_filter(closure: *const ClosureHeader, predicate: f64, opts: f64) -> f64 {
    let this = this_value(closure);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(predicate);
    let mut out = crate::array::js_array_alloc(0);
    if !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            if crate::value::js_is_truthy(call_settled(cb, el)) != 0 {
                out = crate::array::js_array_push_f64(out, el);
            }
        }
    }
    let result = readable_from_chunks(out);
    propagate_stream_state(this, opts, result);
    result
}

extern "C" fn ns_iter_reduce(
    closure: *const ClosureHeader,
    reducer: f64,
    initial: f64,
    opts: f64,
) -> f64 {
    let this = this_value(closure);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(reducer);
    let len = if arr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr)
    };
    let has_initial = initial.to_bits() != TAG_UNDEFINED;
    let (mut acc, start) = if has_initial {
        (initial, 0)
    } else if len > 0 {
        (crate::array::js_array_get_f64(arr, 0), 1)
    } else {
        // Node throws "Reduce of empty stream with no initial value";
        // the stub resolves undefined rather than crash.
        return settle_consuming(this, opts, f64::from_bits(TAG_UNDEFINED));
    };
    if !cb.is_null() {
        for i in start..len {
            let el = crate::array::js_array_get_f64(arr, i);
            // Node's stream reducer is (accumulator, current) — no index.
            acc = settle(crate::closure::js_closure_call2(cb, acc, el));
        }
    }
    settle_consuming(this, opts, acc)
}

extern "C" fn ns_iter_for_each(closure: *const ClosureHeader, action: f64, opts: f64) -> f64 {
    let this = this_value(closure);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(action);
    if !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            let _ = call_settled(cb, el);
        }
    }
    settle_consuming(this, opts, f64::from_bits(TAG_UNDEFINED))
}

extern "C" fn ns_iter_find(closure: *const ClosureHeader, predicate: f64, opts: f64) -> f64 {
    let this = this_value(closure);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(predicate);
    let mut found = f64::from_bits(TAG_UNDEFINED);
    if !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            if crate::value::js_is_truthy(call_settled(cb, el)) != 0 {
                found = el;
                break;
            }
        }
    }
    settle_consuming(this, opts, found)
}

extern "C" fn ns_iter_some(closure: *const ClosureHeader, predicate: f64, opts: f64) -> f64 {
    let this = this_value(closure);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(predicate);
    let mut result = f64::from_bits(TAG_FALSE);
    if !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            if crate::value::js_is_truthy(call_settled(cb, el)) != 0 {
                result = f64::from_bits(TAG_TRUE);
                break;
            }
        }
    }
    settle_consuming(this, opts, result)
}

extern "C" fn ns_iter_every(closure: *const ClosureHeader, predicate: f64, opts: f64) -> f64 {
    let this = this_value(closure);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(predicate);
    let mut result = f64::from_bits(TAG_TRUE);
    if !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            if crate::value::js_is_truthy(call_settled(cb, el)) == 0 {
                result = f64::from_bits(TAG_FALSE);
                break;
            }
        }
    }
    settle_consuming(this, opts, result)
}

extern "C" fn ns_iter_flat_map(closure: *const ClosureHeader, mapper: f64, opts: f64) -> f64 {
    let this = this_value(closure);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(mapper);
    let mut out = crate::array::js_array_alloc(0);
    if !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            let mapped = call_settled(cb, el);
            // flatMap flattens one level: an array result is spread, a
            // Readable result contributes its retained chunks, anything
            // else is appended as a single chunk. (A bare async-generator
            // mapper isn't flattened yet — tracked separately.)
            if is_array_like_value(mapped) {
                out = extend_with_array(out, raw_ptr_from_value(mapped) as *const _);
            } else if let Some(inner) = readable_hidden_chunks(mapped) {
                if is_array_like_value(inner) {
                    out = extend_with_array(out, raw_ptr_from_value(inner) as *const _);
                } else {
                    out = crate::array::js_array_push_f64(out, mapped);
                }
            } else {
                out = crate::array::js_array_push_f64(out, mapped);
            }
        }
    }
    let result = readable_from_chunks(out);
    propagate_stream_state(this, opts, result);
    result
}

extern "C" fn ns_iter_take(closure: *const ClosureHeader, count: f64) -> f64 {
    let this = this_value(closure);
    let arr = readable_chunks_array(this);
    let mut out = crate::array::js_array_alloc(0);
    if !arr.is_null() {
        let len = crate::array::js_array_length(arr);
        let take = count_arg(count).min(len);
        for i in 0..take {
            out = crate::array::js_array_push_f64(out, crate::array::js_array_get_f64(arr, i));
        }
    }
    let result = readable_from_chunks(out);
    propagate_stream_state(this, f64::from_bits(TAG_UNDEFINED), result);
    result
}

extern "C" fn ns_iter_drop(closure: *const ClosureHeader, count: f64) -> f64 {
    let this = this_value(closure);
    let arr = readable_chunks_array(this);
    let mut out = crate::array::js_array_alloc(0);
    if !arr.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in count_arg(count).min(len)..len {
            out = crate::array::js_array_push_f64(out, crate::array::js_array_get_f64(arr, i));
        }
    }
    let result = readable_from_chunks(out);
    propagate_stream_state(this, f64::from_bits(TAG_UNDEFINED), result);
    result
}

type StubFn = unsafe extern "C" fn();

#[allow(clippy::missing_transmute_annotations)]
fn cast0(f: extern "C" fn(*const ClosureHeader) -> f64) -> StubFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
fn cast1(f: extern "C" fn(*const ClosureHeader, f64) -> f64) -> StubFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
fn cast2(f: extern "C" fn(*const ClosureHeader, f64, f64) -> f64) -> StubFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
fn cast3(f: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64) -> StubFn {
    unsafe { std::mem::transmute(f) }
}

// ─────────────────────────────────────────────────────────────────
// Build the host object: allocate an ObjectHeader sized to the
// method set, then fill each slot with a closure that captures the
// host object's NaN-boxed value (so `this` chains return identity).
// ─────────────────────────────────────────────────────────────────

fn build_object(methods: &[(&str, StubFn)], shape_id: u32) -> *mut ObjectHeader {
    register_stub_arities();

    // Pack the method names as a NUL-separated byte sequence, matching
    // the layout `js_object_alloc_with_shape` parses for shape keys.
    let mut packed: Vec<u8> = Vec::new();
    for (name, _) in methods {
        packed.extend_from_slice(name.as_bytes());
        packed.push(0);
    }
    let field_count = methods.len() as u32;
    let obj =
        js_object_alloc_with_shape(shape_id, field_count, packed.as_ptr(), packed.len() as u32);

    // NaN-box the object pointer — we'll capture it (as raw bits) in each
    // closure's slot 0 so the stub `this_value` helper can reconstruct
    // the f64 form for `return this` semantics.
    let this_bits = JSValue::pointer(obj as *const u8).bits();

    for (i, (_name, func)) in methods.iter().enumerate() {
        let closure = js_closure_alloc(*func as *const u8, 1);
        // Reuse `set_capture_ptr` (i64 payload). We only need 64 bits
        // and the NaN-boxed pattern fits cleanly when reinterpreted.
        crate::closure::js_closure_set_capture_ptr(closure, 0, this_bits as i64);
        let val = JSValue::pointer(closure as *const u8);
        js_object_set_field(obj, i as u32, val);
    }
    obj
}

fn register_stub_arities() {
    let register = |func: *const u8, arity: u32| {
        crate::closure::js_register_closure_arity(func, arity);
    };
    register(ns_chain0 as *const u8, 0);
    register(ns_chain1 as *const u8, 1);
    register(ns_chain2 as *const u8, 2);
    register(ns_chain3 as *const u8, 3);
    register(ns_emit2 as *const u8, 2);
    register(ns_resume0 as *const u8, 0);
    register(ns_read1 as *const u8, 1);
    register(ns_pipe1 as *const u8, 1);
    register(writable_write_callback_noop as *const u8, 0);
    register(ns_write2 as *const u8, 2);
    register(ns_end1 as *const u8, 1);
    register(ns_listener_count as *const u8, 1);
    register(ns_listeners as *const u8, 1);
    register(ns_undefined0 as *const u8, 0);
}

#[inline]
fn box_pointer(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

#[inline]
#[cfg(test)]
fn box_string(ptr: *mut crate::string::StringHeader) -> f64 {
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

#[inline]
fn raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() || jsval.is_string() || jsval.is_bigint() {
        return (bits & crate::value::POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

#[inline]
unsafe fn gc_type_for_ptr(raw: usize) -> Option<u8> {
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let gc_type = (*header).obj_type;
    if gc_type <= crate::gc::GC_TYPE_MAX {
        Some(gc_type)
    } else {
        None
    }
}

#[inline]
fn hidden_chunks_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_CHUNKS_KEY)
}

#[inline]
fn hidden_error_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_ERROR_KEY)
}

#[inline]
fn hidden_read_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_READ_KEY)
}

#[inline]
fn hidden_read_invoked_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_READ_INVOKED_KEY)
}

#[inline]
fn hidden_ended_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_ENDED_KEY)
}

#[inline]
fn hidden_write_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_WRITE_KEY)
}

fn hidden_key(bytes: &[u8]) -> *mut crate::string::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn string_value_eq(value: f64, expected: &[u8]) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        return false;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return false;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        if len != expected.len() {
            return false;
        }
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::slice::from_raw_parts(data, len) == expected
    }
}

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_OBJECT) {
            return None;
        }
    }
    Some(raw as *mut ObjectHeader)
}

fn get_hidden_value(value: f64, key: *mut crate::string::StringHeader) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, key);
    if value.to_bits() == TAG_UNDEFINED {
        None
    } else {
        Some(value)
    }
}

fn set_hidden_value(value: f64, key: *mut crate::string::StringHeader, field_value: f64) {
    if let Some(obj) = object_ptr_from_value(value) {
        js_object_set_field_by_name(obj, key, field_value);
    }
}

fn is_array_like_value(value: f64) -> bool {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return false;
    }
    unsafe {
        matches!(
            gc_type_for_ptr(raw),
            Some(crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY)
        )
    }
}

fn readable_hidden_chunks(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_chunks_key())
}

fn readable_hidden_error(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_error_key())
}

fn stream_hidden_ended(value: f64) -> bool {
    get_hidden_value(value, hidden_ended_key()).is_some_and(|v| crate::value::js_is_truthy(v) != 0)
}

fn writable_hidden_write(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_write_key())
}

fn rebind_callback_this(callback: f64, stream: f64) -> f64 {
    f64::from_bits(crate::closure::clone_closure_rebind_this(
        callback.to_bits(),
        stream,
    ))
}

fn read_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"read"))
}

fn write_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"write"))
}

fn invoke_read_once(stream: f64) {
    let Some(read) = get_hidden_value(stream, hidden_read_key()) else {
        return;
    };
    if get_hidden_value(stream, hidden_read_invoked_key()).is_some() {
        return;
    }
    set_hidden_value(stream, hidden_read_invoked_key(), f64::from_bits(TAG_TRUE));
    let prev_this = crate::object::js_implicit_this_set(stream);
    unsafe {
        let _ = crate::closure::js_native_call_value(read, std::ptr::null(), 0);
    }
    crate::object::js_implicit_this_set(prev_this);
}

fn is_single_chunk_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_any_string() {
        return true;
    }
    let raw = raw_ptr_from_value(value);
    raw >= 0x10000 && crate::buffer::is_registered_buffer(raw)
}

fn uint8array_byte_chunks(raw: usize) -> f64 {
    let arr = crate::array::js_array_alloc(0);
    if raw < 0x10000 || !crate::buffer::is_registered_buffer(raw) {
        return box_pointer(arr as *const u8);
    }
    unsafe {
        let buf = raw as *const crate::buffer::BufferHeader;
        let len = (*buf).length as usize;
        let data = crate::buffer::buffer_data(buf);
        let mut out = arr;
        for i in 0..len {
            out = crate::array::js_array_push_f64(out, *data.add(i) as f64);
        }
        box_pointer(out as *const u8)
    }
}

fn normalize_readable_from_input(iterable: f64) -> f64 {
    if let Some(chunks) = readable_hidden_chunks(iterable) {
        return chunks;
    }
    let raw = raw_ptr_from_value(iterable);
    if raw >= 0x10000
        && crate::buffer::is_registered_buffer(raw)
        && crate::buffer::is_uint8array_buffer(raw)
        && !crate::buffer::is_array_buffer(raw)
    {
        return uint8array_byte_chunks(raw);
    }
    if is_array_like_value(iterable) {
        return iterable;
    }

    let arr = crate::array::js_array_alloc(1);
    if is_single_chunk_value(iterable) {
        let arr = crate::array::js_array_push_f64(arr, iterable);
        return box_pointer(arr as *const u8);
    }
    box_pointer(arr as *const u8)
}

fn append_string_bytes(value: f64, out: &mut Vec<u8>) {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    append_string_ptr_bytes(ptr, out);
}

fn append_string_ptr_bytes(ptr: *const crate::StringHeader, out: &mut Vec<u8>) {
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        out.extend_from_slice(std::slice::from_raw_parts(data, len));
    }
}

fn append_buffer_bytes(raw: usize, out: &mut Vec<u8>) {
    if raw < 0x10000 || !crate::buffer::is_registered_buffer(raw) {
        return;
    }
    unsafe {
        let buf = raw as *const crate::buffer::BufferHeader;
        let len = (*buf).length as usize;
        let data = crate::buffer::buffer_data(buf);
        out.extend_from_slice(std::slice::from_raw_parts(data, len));
    }
}

fn append_array_chunks(raw: usize, out: &mut Vec<u8>, depth: u8) {
    if raw < 0x10000 {
        return;
    }
    let arr = raw as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        let chunk = crate::array::js_array_get_f64(arr, i);
        append_chunk_bytes(chunk, out, depth + 1);
    }
}

fn append_chunk_bytes(value: f64, out: &mut Vec<u8>, depth: u8) {
    if depth > 8 {
        return;
    }
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_any_string() {
        append_string_bytes(value, out);
        return;
    }
    if jsval.is_int32() {
        out.extend_from_slice(jsval.as_int32().to_string().as_bytes());
        return;
    }
    if jsval.is_number() && value.is_finite() {
        let text = if value.fract() == 0.0 {
            (value as i64).to_string()
        } else {
            value.to_string()
        };
        out.extend_from_slice(text.as_bytes());
        return;
    }

    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 {
        return;
    }
    if crate::buffer::is_registered_buffer(raw) {
        append_buffer_bytes(raw, out);
        return;
    }

    unsafe {
        match gc_type_for_ptr(raw) {
            Some(crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY) => {
                append_array_chunks(raw, out, depth);
            }
            Some(crate::gc::GC_TYPE_OBJECT) => {
                if let Some(chunks) = readable_hidden_chunks(value) {
                    append_chunk_bytes(chunks, out, depth + 1);
                }
            }
            Some(crate::gc::GC_TYPE_STRING) => {
                append_string_ptr_bytes(raw as *const crate::StringHeader, out);
            }
            _ => {}
        }
    }
}

fn push_chunk_values(value: f64, out: &mut Vec<f64>, depth: u8) {
    if depth > 8 {
        return;
    }
    if let Some(chunks) = readable_hidden_chunks(value) {
        push_chunk_values(chunks, out, depth + 1);
        return;
    }
    if is_array_like_value(value) {
        let raw = raw_ptr_from_value(value);
        if raw < 0x10000 {
            return;
        }
        let arr = raw as *const crate::array::ArrayHeader;
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            out.push(crate::array::js_array_get_f64(arr, i));
        }
        return;
    }
    if is_single_chunk_value(value) {
        out.push(value);
    }
}

/// Drain the chunk storage Perry attaches in `Readable.from(iterable)`.
///
/// This intentionally handles only the current stream stub's concrete shapes:
/// arrays of strings/Buffers/Uint8Arrays/ArrayBuffers plus direct single
/// string/binary chunks. It gives `node:stream/consumers` useful data without
/// pretending Perry has a full Node stream pump yet.
pub fn js_node_stream_collect_bytes(stream: f64) -> Vec<u8> {
    js_node_stream_collect_bytes_result(stream).unwrap_or_default()
}

pub fn js_node_stream_collect_chunks_result(stream: f64) -> Option<Result<f64, f64>> {
    invoke_read_once(stream);
    if let Some(err) = readable_hidden_error(stream) {
        return Some(Err(err));
    }
    if let Some(chunks) = readable_hidden_chunks(stream) {
        return Some(Ok(chunks));
    }
    if is_array_like_value(stream) {
        return Some(Ok(stream));
    }
    if is_single_chunk_value(stream) {
        let mut arr = crate::array::js_array_alloc(1);
        arr = crate::array::js_array_push_f64(arr, stream);
        return Some(Ok(box_pointer(arr as *const u8)));
    }
    if get_hidden_value(stream, hidden_read_key()).is_some() {
        let arr = crate::array::js_array_alloc(0);
        return Some(Ok(box_pointer(arr as *const u8)));
    }
    None
}

pub fn js_node_stream_collect_bytes_result(stream: f64) -> Result<Vec<u8>, f64> {
    invoke_read_once(stream);
    if let Some(err) = readable_hidden_error(stream) {
        return Err(err);
    }
    let mut out = Vec::new();
    append_chunk_bytes(stream, &mut out, 0);
    if let Some(err) = readable_hidden_error(stream) {
        return Err(err);
    }
    Ok(out)
}

pub(crate) fn js_node_stream_hidden_error_after_read(stream: f64) -> Option<f64> {
    invoke_read_once(stream);
    readable_hidden_error(stream)
}

pub(crate) fn js_node_stream_is_stub_ended_after_read(stream: f64) -> bool {
    invoke_read_once(stream);
    stream_hidden_ended(stream)
}

#[cfg(test)]
pub(crate) fn test_set_hidden_error(stream: f64, err: f64) {
    set_hidden_value(stream, hidden_error_key(), err);
}

pub(crate) fn js_node_stream_readable_chunks_result(stream: f64) -> Result<Option<Vec<f64>>, f64> {
    invoke_read_once(stream);
    if let Some(err) = readable_hidden_error(stream) {
        return Err(err);
    }
    let Some(chunks) = readable_hidden_chunks(stream) else {
        return Ok(None);
    };
    let mut out = Vec::new();
    push_chunk_values(chunks, &mut out, 0);
    if let Some(err) = readable_hidden_error(stream) {
        return Err(err);
    }
    Ok(Some(out))
}

// ─────────────────────────────────────────────────────────────────
// Method tables. Order is locked in — it determines the shape's
// packed-keys order. Each method set's length is a unique
// shape-cache key when added to its base shape id, so e.g. Readable's
// 28 methods at READABLE_SHAPE_ID don't collide with Writable's at
// WRITABLE_SHAPE_ID.
// ─────────────────────────────────────────────────────────────────

fn readable_methods() -> [(&'static str, StubFn); 28] {
    [
        ("on", cast2(ns_chain2)),
        ("once", cast2(ns_chain2)),
        ("off", cast2(ns_chain2)),
        ("addListener", cast2(ns_chain2)),
        ("removeListener", cast2(ns_chain2)),
        ("removeAllListeners", cast1(ns_chain1)),
        ("emit", cast2(ns_emit2)),
        ("listenerCount", cast1(ns_listener_count)),
        ("listeners", cast1(ns_listeners)),
        ("read", cast1(ns_read1)),
        ("pipe", cast1(ns_pipe1)),
        ("unpipe", cast1(ns_chain1)),
        ("pause", cast0(ns_chain0)),
        ("resume", cast0(ns_resume0)),
        ("destroy", cast1(ns_chain1)),
        ("setEncoding", cast1(ns_chain1)),
        ("isPaused", cast0(ns_undefined0)),
        // #1558 — async iterator helpers. The consuming helpers accept a
        // trailing `{ signal }` options arg; the lazy transforms accept one
        // too (Node's signature). Arities are registered in
        // `register_iter_helper_arities` so under-supplied calls pad the
        // missing trailing args with `undefined`.
        ("toArray", cast1(ns_iter_to_array)),
        ("map", cast2(ns_iter_map)),
        ("filter", cast2(ns_iter_filter)),
        ("reduce", cast3(ns_iter_reduce)),
        ("forEach", cast2(ns_iter_for_each)),
        ("find", cast2(ns_iter_find)),
        ("some", cast2(ns_iter_some)),
        ("every", cast2(ns_iter_every)),
        ("flatMap", cast2(ns_iter_flat_map)),
        ("take", cast1(ns_iter_take)),
        ("drop", cast1(ns_iter_drop)),
    ]
}

fn writable_methods() -> [(&'static str, StubFn); 16] {
    [
        ("on", cast2(ns_chain2)),
        ("once", cast2(ns_chain2)),
        ("off", cast2(ns_chain2)),
        ("addListener", cast2(ns_chain2)),
        ("removeListener", cast2(ns_chain2)),
        ("removeAllListeners", cast1(ns_chain1)),
        ("emit", cast2(ns_emit2)),
        ("listenerCount", cast1(ns_listener_count)),
        ("listeners", cast1(ns_listeners)),
        ("write", cast2(ns_write2)),
        ("end", cast1(ns_end1)),
        ("cork", cast0(ns_chain0)),
        ("uncork", cast0(ns_chain0)),
        ("destroy", cast1(ns_chain1)),
        ("setDefaultEncoding", cast1(ns_chain1)),
        ("_write", cast3(ns_chain3)),
    ]
}

fn duplex_methods() -> [(&'static str, StubFn); 22] {
    // Union of readable + writable, deduped (`on/once/off/addListener/
    // removeListener/removeAllListeners/emit/listenerCount/listeners/
    // destroy` appear once each).
    [
        ("on", cast2(ns_chain2)),
        ("once", cast2(ns_chain2)),
        ("off", cast2(ns_chain2)),
        ("addListener", cast2(ns_chain2)),
        ("removeListener", cast2(ns_chain2)),
        ("removeAllListeners", cast1(ns_chain1)),
        ("emit", cast2(ns_emit2)),
        ("listenerCount", cast1(ns_listener_count)),
        ("listeners", cast1(ns_listeners)),
        ("read", cast1(ns_read1)),
        ("pipe", cast1(ns_pipe1)),
        ("unpipe", cast1(ns_chain1)),
        ("pause", cast0(ns_chain0)),
        ("resume", cast0(ns_resume0)),
        ("setEncoding", cast1(ns_chain1)),
        ("isPaused", cast0(ns_undefined0)),
        ("write", cast2(ns_write2)),
        ("end", cast1(ns_end1)),
        ("cork", cast0(ns_chain0)),
        ("uncork", cast0(ns_chain0)),
        ("destroy", cast1(ns_chain1)),
        ("setDefaultEncoding", cast1(ns_chain1)),
    ]
}

// ─────────────────────────────────────────────────────────────────
// Public entry points — wired up by codegen's lower_builtin_new
// (`Readable`, `Writable`, `Duplex`, `Transform`, `PassThrough` arms)
// and by the `stream.from` NATIVE_MODULE_TABLE row for
// `Readable.from(iterable)`.
//
// Each takes a single `_opts` argument (NaN-boxed) for ABI parity
// with Node's constructor signature. The stub reads only the small
// option callbacks Perry can honor today (`read` and `write`), keeping
// the wider stream state machine out of this compatibility layer.
// ─────────────────────────────────────────────────────────────────

thread_local! {
    static ITER_HELPER_ARITIES_REGISTERED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// Register declared arities for the iterator-helper stubs (once per
/// thread) so the closure dispatcher pads missing trailing args with
/// `undefined` instead of reading register garbage. `reduce` strictly
/// needs it — `reduce(fn)` omits the initial value — and registering
/// the single-arg helpers makes a missing-callback call (`map()`)
/// degrade to a no-op rather than dereference junk.
fn register_iter_helper_arities() {
    if ITER_HELPER_ARITIES_REGISTERED.with(|c| c.replace(true)) {
        return;
    }
    let entries: &[(StubFn, u32)] = &[
        (cast1(ns_iter_to_array), 1),
        (cast2(ns_iter_map), 2),
        (cast2(ns_iter_filter), 2),
        (cast3(ns_iter_reduce), 3),
        (cast2(ns_iter_for_each), 2),
        (cast2(ns_iter_find), 2),
        (cast2(ns_iter_some), 2),
        (cast2(ns_iter_every), 2),
        (cast2(ns_iter_flat_map), 2),
        (cast1(ns_iter_take), 1),
        (cast1(ns_iter_drop), 1),
    ];
    for (f, arity) in entries {
        crate::closure::js_register_closure_arity(*f as *const u8, *arity);
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_readable_new(opts: f64) -> f64 {
    register_iter_helper_arities();
    let methods = readable_methods();
    let obj = build_object(&methods, READABLE_SHAPE_ID + methods.len() as u32);
    let readable = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    if let Some(read) = read_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_read_key(), rebind_callback_this(read, readable));
    }
    readable
}

#[no_mangle]
pub extern "C" fn js_node_stream_writable_new(opts: f64) -> f64 {
    let methods = writable_methods();
    let obj = build_object(&methods, WRITABLE_SHAPE_ID + methods.len() as u32);
    let writable = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    if let Some(write) = write_callback_from_options(opts) {
        js_object_set_field_by_name(
            obj,
            hidden_write_key(),
            rebind_callback_this(write, writable),
        );
    }
    writable
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_new(opts: f64) -> f64 {
    let methods = duplex_methods();
    let obj = build_object(&methods, DUPLEX_SHAPE_ID + methods.len() as u32);
    let duplex = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    if let Some(write) = write_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_write_key(), rebind_callback_this(write, duplex));
    }
    duplex
}

/// Transform / PassThrough share Duplex's surface for the stub — the
/// `transform`/`flush` callbacks aren't wired through.
#[no_mangle]
pub extern "C" fn js_node_stream_transform_new(_opts: f64) -> f64 {
    js_node_stream_duplex_new(_opts)
}

#[no_mangle]
pub extern "C" fn js_node_stream_passthrough_new(_opts: f64) -> f64 {
    js_node_stream_duplex_new(_opts)
}

/// `Readable.from(iterable)` — Node's static factory. Returns a
/// Readable object and retains simple iterable chunks so
/// `node:stream/consumers` can drain the current stub stream surface.
#[no_mangle]
pub extern "C" fn js_node_stream_readable_from(iterable: f64) -> f64 {
    let readable = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let raw = raw_ptr_from_value(readable);
    if raw >= 0x10000 {
        let chunks = normalize_readable_from_input(iterable);
        js_object_set_field_by_name(raw as *mut ObjectHeader, hidden_chunks_key(), chunks);
    }
    readable
}

// ─────────────────────────────────────────────────────────────────
// #1534: static introspection helpers `Readable.isDisturbed(s)` and
// `Readable.isErrored(s)`. Node returns booleans reflecting the
// stream's internal state machine; Perry's stream stubs don't track
// any of that state yet, so both return `false` — which is the
// correct answer for a freshly-constructed, untouched stream. The
// directional helpers `isReadable` / `isWritable` aren't here
// because Node's answer depends on the stream's actual direction
// (Readable returns `true` for isReadable + `null` for isWritable
// and so on); a uniform stub would lie for at least one case, so
// they're deferred until Perry's stream stub tracks direction.
// ─────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn js_node_stream_is_disturbed(_stream: f64) -> f64 {
    f64::from_bits(TAG_FALSE)
}

#[no_mangle]
pub extern "C" fn js_node_stream_is_errored(stream: f64) -> f64 {
    if readable_hidden_error(stream).is_some() {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// #1541: `stream.addAbortSignal(signal, stream)` — Node wires the
/// AbortSignal so aborting it destroys the stream, and returns the
/// stream for chaining. Perry's stream stubs don't implement the
/// destroy / abort propagation yet, so the helper just returns the
/// stream verbatim and ignores the signal. Caller chains
/// (`r = addAbortSignal(s, r)`) keep working with the same stream
/// reference they passed in.
#[no_mangle]
pub extern "C" fn js_node_stream_add_abort_signal(_signal: f64, stream: f64) -> f64 {
    stream
}

/// #1539: `stream.compose(...streams)` chains a sequence of streams
/// into one composite Duplex (data flows through them in order).
/// Perry's stream stubs don't propagate data through chains, so the
/// helper returns a fresh Duplex — the typeof / instanceof checks
/// callers do (`compose(a, b) instanceof Duplex`) hold, and the
/// reads/writes are stubbed at the Duplex layer same as a bare
/// `new Duplex()`. The variadic `...streams` arg list is ignored;
/// real composition is tracked separately.
#[no_mangle]
pub extern "C" fn js_node_stream_compose(_streams_array: f64) -> f64 {
    js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED))
}

/// #1539: `stream.duplexPair([options])` returns a two-element array
/// `[Duplex, Duplex]` where writes to one show up as reads on the
/// other and vice versa. Perry's stubs return a pair of unrelated
/// Duplex stubs so the shape (`const [a, b] = duplexPair()`,
/// `a instanceof Duplex`) matches; cross-stream piping is the real
/// missing piece, tracked separately.
#[no_mangle]
pub extern "C" fn js_node_stream_duplex_pair(_opts: f64) -> f64 {
    let a = js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED));
    let b = js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED));
    let arr = crate::array::js_array_alloc(2);
    crate::array::js_array_push(arr, JSValue::from_bits(a.to_bits()));
    crate::array::js_array_push(arr, JSValue::from_bits(b.to_bits()));
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

// ─────────────────────────────────────────────────────────────────
// #1540: Web-stream interop. Node exposes static helpers on the
// stream classes for converting between Node streams and WHATWG
// streams:
//   - `Readable.toWeb(nodeReadable)` → WHATWG ReadableStream
//   - `Readable.fromWeb(webStream)` → Node Readable
//   - `Writable.toWeb(nodeWritable)` → WHATWG WritableStream
//   - `Writable.fromWeb(webStream)` → Node Writable
//
// Perry's stubs return a Node stream of the appropriate direction
// for all four (data isn't actually forwarded between the two
// universes yet). That's the closest shape match: consumers that
// branch on `typeof toWeb(s) === "object"` or destructure with
// `const w = Readable.fromWeb(...)` get a non-null object back and
// don't crash. Real bidirectional adapters are tracked separately.
// ─────────────────────────────────────────────────────────────────

/// `Readable.toWeb` / `Writable.toWeb` — Perry returns a fresh
/// Duplex stub for either direction. It's not a real WHATWG
/// ReadableStream / WritableStream, but typeof / truthy / method
/// existence checks (`.pipeTo`, etc. via duplex_methods) pass.
#[no_mangle]
pub extern "C" fn js_node_stream_to_web(_node_stream: f64) -> f64 {
    js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED))
}

/// `Readable.fromWeb` / `Writable.fromWeb` — Perry returns a fresh
/// Duplex stub for either direction. Real bidirectional adapters
/// are tracked separately.
#[no_mangle]
pub extern "C" fn js_node_stream_from_web(_web_stream: f64) -> f64 {
    js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static WRITE_CAPTURED: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    }

    fn string_value(s: &str) -> f64 {
        let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        box_string(ptr)
    }

    fn buffer_value(bytes: &[u8]) -> f64 {
        let buf = crate::buffer::buffer_alloc(bytes.len() as u32);
        unsafe {
            (*buf).length = bytes.len() as u32;
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                crate::buffer::buffer_data_mut(buf),
                bytes.len(),
            );
        }
        box_pointer(buf as *const u8)
    }

    extern "C" fn write_capture(
        _closure: *const ClosureHeader,
        chunk: f64,
        _enc: f64,
        cb: f64,
    ) -> f64 {
        let readable = js_node_stream_readable_from(chunk);
        let bytes = js_node_stream_collect_bytes(readable);
        WRITE_CAPTURED.with(|captured| captured.borrow_mut().push(bytes));
        unsafe {
            let _ = crate::closure::js_native_call_value(cb, std::ptr::null(), 0);
        }
        f64::from_bits(TAG_UNDEFINED)
    }

    extern "C" fn read_records_this(closure: *const ClosureHeader) -> f64 {
        let stream = crate::closure::js_closure_get_capture_f64(closure, 0);
        set_hidden_value(stream, hidden_error_key(), string_value("from-read"));
        f64::from_bits(TAG_UNDEFINED)
    }

    #[test]
    fn readable_from_retains_string_chunks_for_consumers() {
        let mut arr = crate::array::js_array_alloc(2);
        arr = crate::array::js_array_push_f64(arr, string_value("he"));
        arr = crate::array::js_array_push_f64(arr, string_value("llo"));

        let readable = js_node_stream_readable_from(box_pointer(arr as *const u8));

        assert_eq!(js_node_stream_collect_bytes(readable), b"hello");
    }

    #[test]
    fn readable_from_retains_buffer_chunks_for_consumers() {
        let mut arr = crate::array::js_array_alloc(2);
        arr = crate::array::js_array_push_f64(arr, buffer_value(b"ab"));
        arr = crate::array::js_array_push_f64(arr, buffer_value(b"cd"));

        let readable = js_node_stream_readable_from(box_pointer(arr as *const u8));

        assert_eq!(js_node_stream_collect_bytes(readable), b"abcd");
    }

    #[test]
    fn writable_options_write_callback_is_invoked_by_stub_write() {
        WRITE_CAPTURED.with(|captured| captured.borrow_mut().clear());
        let opts = crate::object::js_object_alloc(0, 1);
        let closure = js_closure_alloc(write_capture as *const u8, 0);
        crate::closure::js_register_closure_arity(write_capture as *const u8, 3);
        js_object_set_field_by_name(
            opts,
            hidden_key(b"write"),
            f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
        );

        let writable = js_node_stream_writable_new(box_pointer(opts as *const u8));
        let write = js_object_get_field_by_name_f64(
            raw_ptr_from_value(writable) as *const ObjectHeader,
            hidden_key(b"write"),
        );
        let args = [string_value("chunk"), f64::from_bits(TAG_UNDEFINED)];
        unsafe {
            let _ = crate::closure::js_native_call_value(write, args.as_ptr(), args.len());
        }

        WRITE_CAPTURED.with(|captured| {
            assert_eq!(captured.borrow().as_slice(), &[b"chunk".to_vec()]);
        });
    }

    #[test]
    fn readable_options_read_callback_this_is_rebound_to_stream() {
        let opts = crate::object::js_object_alloc(0, 1);
        let closure = js_closure_alloc(
            read_records_this as *const u8,
            crate::closure::CAPTURES_THIS_FLAG | 1,
        );
        crate::closure::js_register_closure_arity(read_records_this as *const u8, 0);
        crate::closure::js_closure_set_capture_f64(closure, 0, box_pointer(opts as *const u8));
        js_object_set_field_by_name(
            opts,
            hidden_key(b"read"),
            f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
        );

        let readable = js_node_stream_readable_new(box_pointer(opts as *const u8));

        let err = js_node_stream_hidden_error_after_read(readable).expect("stream error");
        assert!(string_value_eq(err, b"from-read"));
        assert!(readable_hidden_error(box_pointer(opts as *const u8)).is_none());
    }

    #[test]
    fn stream_methods_use_implicit_this_without_closure_capture() {
        let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
        let prev_this = crate::object::js_implicit_this_set(stream);
        let _ = ns_end1(std::ptr::null(), f64::from_bits(TAG_UNDEFINED));
        crate::object::js_implicit_this_set(prev_this);

        assert!(js_node_stream_is_stub_ended_after_read(stream));
    }

    #[test]
    fn stream_method_closure_capture_wins_over_stale_implicit_this() {
        let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
        let other = box_pointer(crate::object::js_object_alloc(0, 0) as *const u8);
        let end = js_object_get_field_by_name_f64(
            raw_ptr_from_value(stream) as *const ObjectHeader,
            hidden_key(b"end"),
        );

        let prev_this = crate::object::js_implicit_this_set(other);
        unsafe {
            let _ = crate::closure::js_native_call_value(end, std::ptr::null(), 0);
        }
        crate::object::js_implicit_this_set(prev_this);

        assert!(js_node_stream_is_stub_ended_after_read(stream));
        assert!(!stream_hidden_ended(other));
    }

    #[test]
    fn stream_methods_dispatch_through_dynamic_method_call() {
        let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
        unsafe {
            let _ = crate::object::js_native_call_method(
                stream,
                b"end".as_ptr() as *const i8,
                3,
                std::ptr::null(),
                0,
            );
        }

        assert!(js_node_stream_is_stub_ended_after_read(stream));
    }

    #[test]
    fn stream_native_receiver_methods_update_hidden_state() {
        let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
        let handle = raw_ptr_from_value(stream) as i64;
        let err = string_value("boom");

        assert_eq!(
            js_node_stream_method_emit(handle, string_value("error"), err).to_bits(),
            TAG_TRUE
        );
        assert!(js_node_stream_hidden_error_after_read(stream).is_some());

        let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
        let handle = raw_ptr_from_value(stream) as i64;
        let _ = js_node_stream_method_end(handle, f64::from_bits(TAG_UNDEFINED));
        assert!(js_node_stream_is_stub_ended_after_read(stream));
    }

    #[test]
    fn stream_stub_arities_are_registered_per_thread() {
        let _ = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
        assert_eq!(
            crate::closure::lookup_closure_arity(ns_end1 as *const u8),
            Some(1)
        );

        std::thread::spawn(|| {
            let _ = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
            assert_eq!(
                crate::closure::lookup_closure_arity(ns_end1 as *const u8),
                Some(1)
            );
            assert_eq!(
                crate::closure::lookup_closure_arity(ns_write2 as *const u8),
                Some(2)
            );
        })
        .join()
        .expect("stream arity registration thread should not panic");
    }
}
