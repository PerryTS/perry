//! node:stream — async-consume + iterator-helper machinery (map/filter/reduce/compose/...) (split out of node_stream.rs for the 2000-line
//! file-size gate, #1987). Shares the parent module's constants, hidden-key
//! accessors and state primitives via `use super::*`.
use super::*;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::object::{js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader};
use crate::value::JSValue;

pub(super) extern "C" fn ns_undefined0(_closure: *const ClosureHeader) -> f64 {
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
pub(super) fn callback_closure(value: f64) -> *const ClosureHeader {
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
pub(super) fn readable_chunks_array(this: f64) -> *const crate::array::ArrayHeader {
    match readable_hidden_chunks(this) {
        Some(chunks) if is_array_like_value(chunks) => {
            raw_ptr_from_value(chunks) as *const crate::array::ArrayHeader
        }
        _ => std::ptr::null(),
    }
}

/// Wrap `value` in an already-fulfilled Promise, NaN-boxed.
#[inline]
pub(super) fn resolved_promise(value: f64) -> f64 {
    let promise = crate::promise::js_promise_resolved(value);
    box_pointer(promise as *const u8)
}

/// Build a fresh Readable whose retained chunks are `chunks`.
#[inline]
pub(super) fn readable_from_chunks(chunks: *const crate::array::ArrayHeader) -> f64 {
    js_node_stream_readable_from(box_pointer(chunks as *const u8))
}

/// NaN-box a freshly-allocated string.
#[inline]
pub(super) fn string_value(bytes: &[u8]) -> f64 {
    let ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

/// Build the rejection reason used when an operation is aborted — a
/// plain `{ name: "AbortError", message }` object. Node rejects with a
/// DOMException whose `.name` is `"AbortError"`; callers only inspect
/// `.name`, so a plain object is byte-equivalent for parity.
pub(super) fn abort_error() -> f64 {
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
pub(super) fn rejected_promise(reason: f64) -> f64 {
    box_pointer(crate::promise::js_promise_rejected(reason) as *const u8)
}

pub(super) fn reduce_missing_initial_error() -> f64 {
    let message = b"Reduce of an empty stream requires an initial value";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "ERR_MISSING_ARGS");
    let err = crate::error::js_typeerror_new(msg);
    box_pointer(err as *const u8)
}

#[inline]
pub(super) fn hidden_signal_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_SIGNAL_KEY)
}

/// The `AbortSignal` carried in `opts.signal`, if any.
pub(super) fn options_signal(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"signal"))
}

/// The `AbortSignal` a lazy helper propagated onto this stream.
pub(super) fn readable_stored_signal(this: f64) -> Option<f64> {
    get_hidden_value(this, hidden_signal_key())
}

/// The signal governing an operation on `this` with call `opts` — the
/// call's own `{ signal }` wins, otherwise one inherited from an
/// upstream lazy helper.
pub(super) fn effective_signal(this: f64, opts: f64) -> Option<f64> {
    options_signal(opts).or_else(|| readable_stored_signal(this))
}

/// True when `signal` is an `AbortSignal` whose `aborted` flag is set.
pub(super) fn signal_is_aborted(signal: f64) -> bool {
    match get_hidden_value(signal, hidden_key(b"aborted")) {
        Some(v) => crate::value::js_is_truthy(v) != 0,
        None => false,
    }
}

/// Recover a NaN-boxed Promise pointer from a closure capture slot.
#[inline]
pub(super) fn promise_from_capture(
    closure: *const ClosureHeader,
    idx: u32,
) -> *mut crate::promise::Promise {
    let bits = js_closure_get_capture_ptr(closure, idx) as u64;
    crate::value::js_nanbox_get_pointer(f64::from_bits(bits)) as *mut crate::promise::Promise
}

/// Abort-listener body: reject the captured Promise with an AbortError.
pub(super) extern "C" fn ns_abort_reject(closure: *const ClosureHeader) -> f64 {
    let p = promise_from_capture(closure, 0);
    if !p.is_null() {
        crate::promise::js_promise_reject(p, abort_error());
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Deferred-resolve body: fulfill the captured Promise (slot 0) with the
/// captured value (slot 1) on the next microtask — a no-op if an abort
/// already rejected it.
pub(super) extern "C" fn ns_deferred_resolve(closure: *const ClosureHeader) -> f64 {
    let p = promise_from_capture(closure, 0);
    let value = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    if !p.is_null() {
        crate::promise::js_promise_resolve(p, value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn ns_stream_abort_listener(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    destroy_stream(stream, abort_error());
    f64::from_bits(TAG_UNDEFINED)
}

/// Build a pending Promise for a consuming helper running under a
/// not-yet-aborted signal: an abort listener rejects it with an
/// AbortError, while a queued microtask fulfills it with `value` if no
/// abort fires first. This matches Node's async timing — the operation
/// is in flight when a synchronous `controller.abort()` lands before
/// the awaiter resumes.
pub(super) fn deferred_promise(signal: f64, value: f64) -> f64 {
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
pub(super) fn settle_consuming(this: f64, opts: f64, value: f64) -> f64 {
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
pub(super) fn propagate_stream_state(this: f64, opts: f64, result: f64) {
    if let Some(err) = readable_hidden_error(this) {
        set_hidden_value(result, hidden_error_key(), err);
    }
    if let Some(sig) = effective_signal(this, opts) {
        set_hidden_value(result, hidden_signal_key(), sig);
    }
}

pub(super) fn drain_iter_helper_microtasks() {
    for _ in 0..10_000 {
        if crate::promise::js_promise_run_microtasks() == 0 {
            break;
        }
    }
}

pub(super) fn prepare_readable_for_iteration(stream: f64) {
    invoke_read_once(stream);
    drain_iter_helper_microtasks();
}

/// Resolve a callback result that may be a Promise (an async mapper /
/// predicate) by driving Perry's await pump until it settles, then
/// reading the fulfilled value or preserving the rejection reason.
pub(super) fn settle_result(value: f64) -> Result<f64, f64> {
    if crate::promise::js_value_is_promise(value) == 0 {
        return Ok(value);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    for _ in 0..10_000 {
        let current = value_handle.get_nanbox_f64();
        if crate::promise::js_value_is_promise(current) == 0 {
            return Ok(current);
        }
        let p = crate::value::js_nanbox_get_pointer(current) as *mut crate::promise::Promise;
        if p.is_null() {
            return Ok(current);
        }
        unsafe {
            match (*p).state {
                crate::promise::PromiseState::Fulfilled => return Ok((*p).value),
                crate::promise::PromiseState::Rejected => {
                    // We consumed the rejection by reading `reason` directly
                    // (no `.then`/`.catch` was attached), so clear it from the
                    // unhandled-rejection set — Node treats a helper-observed
                    // callback rejection as handled (#1545).
                    crate::promise::mark_rejection_handled(p);
                    return Err((*p).reason);
                }
                crate::promise::PromiseState::Pending => {}
            }
        }

        crate::event_pump::perry_poll();
        let _ = crate::timer::js_timer_tick();
        let _ = crate::timer::js_callback_timer_tick();
        let _ = crate::timer::js_interval_timer_tick();
        if crate::event_pump::perry_has_work() == 0 {
            break;
        }
        crate::event_pump::js_wait_for_event();
    }

    let current = value_handle.get_nanbox_f64();
    let p = crate::value::js_nanbox_get_pointer(current) as *mut crate::promise::Promise;
    if p.is_null() {
        return Ok(current);
    }
    unsafe {
        match (*p).state {
            crate::promise::PromiseState::Fulfilled => Ok((*p).value),
            crate::promise::PromiseState::Rejected => {
                crate::promise::mark_rejection_handled(p);
                Err((*p).reason)
            }
            crate::promise::PromiseState::Pending => Ok(current),
        }
    }
}

/// Invoke a single-argument stream callback and settle an async result.
#[inline]
pub(super) fn call_settled_result(cb: *const ClosureHeader, arg: f64) -> Result<f64, f64> {
    settle_result(crate::closure::js_closure_call1(cb, arg))
}

/// Coerce a `take(n)` / `drop(n)` count argument to a clamped element
/// count (negative / NaN → 0, matching Node's normalization).
#[inline]
pub(super) fn count_arg(value: f64) -> u32 {
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
pub(super) fn extend_with_array(
    mut out: *mut crate::array::ArrayHeader,
    arr: *const crate::array::ArrayHeader,
) -> *mut crate::array::ArrayHeader {
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        out = crate::array::js_array_push_f64(out, crate::array::js_array_get_f64(arr, i));
    }
    out
}

pub(super) extern "C" fn ns_iter_to_array(closure: *const ClosureHeader, opts: f64) -> f64 {
    let this = this_value(closure);
    // An already-errored stream rejects immediately (matches the error-first
    // check `settle_consuming` used).
    if let Some(err) = readable_hidden_error(this) {
        return rejected_promise(err);
    }
    // An already-aborted signal rejects before consuming anything.
    if let Some(sig) = effective_signal(this, opts) {
        if signal_is_aborted(sig) {
            return rejected_promise(abort_error());
        }
    }
    // Consume the stream TRULY ASYNCHRONOUSLY through its `[Symbol.asyncIterator]`
    // (the same pending-promise iterator `for await` uses): `js_array_from_async`
    // drives `.next()` via a then-chain that accumulates each chunk and resolves
    // when the stream ends. This replaces the previous snapshot + block-drain
    // path, which spun the event loop synchronously — re-entering unrelated
    // macrotasks (the React #327 hazard) and returning an empty array for a
    // live (socket/`setImmediate`-fed) source whose data had not buffered yet.
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let result = crate::promise::js_array_from_async(this, undefined, undefined);
    // Abort after consumption starts: a signal that fires later rejects the
    // result and cancels the stream (terminating the internal `from_async`
    // iterator), instead of leaving the promise pending forever.
    register_to_array_abort(this, opts, result);
    result
}

/// Wire a late-abort listener for `toArray`: on abort, reject `result` and
/// destroy the stream so the internal `js_array_from_async` consumer stops.
fn register_to_array_abort(stream: f64, opts: f64, result: f64) {
    let Some(sig) = effective_signal(stream, opts) else {
        return;
    };
    let Some(sig_obj) = object_ptr_from_value(sig) else {
        return;
    };
    let abort_cl = js_closure_alloc(ns_to_array_abort as *const u8, 2);
    js_closure_set_capture_ptr(abort_cl, 0, result.to_bits() as i64);
    js_closure_set_capture_f64(abort_cl, 1, stream);
    crate::url::js_abort_signal_add_listener(
        sig_obj,
        string_value(b"abort"),
        box_pointer(abort_cl as *const u8),
    );
}

/// Abort-listener body for `toArray`: cancel the live stream (terminating the
/// internal `from_async` iterator that drives it) and reject the result with an
/// AbortError. A no-op if the result already settled.
pub(super) extern "C" fn ns_to_array_abort(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let result = promise_from_capture(closure, 0);
    let stream = js_closure_get_capture_f64(closure, 1);
    destroy_stream(stream, abort_error());
    if !result.is_null() {
        crate::promise::js_promise_reject(result, abort_error());
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn ns_iter_map(closure: *const ClosureHeader, mapper: f64, opts: f64) -> f64 {
    let this = this_value(closure);
    prepare_readable_for_iteration(this);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(mapper);
    let mut out = crate::array::js_array_alloc(0);
    let mut callback_error = None;
    if readable_hidden_error(this).is_none() && !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            match call_settled_result(cb, el) {
                Ok(mapped) => out = crate::array::js_array_push_f64(out, mapped),
                Err(err) => {
                    callback_error = Some(err);
                    break;
                }
            }
        }
    }
    let result = readable_from_chunks(out);
    propagate_stream_state(this, opts, result);
    if let Some(err) = callback_error {
        set_hidden_value(result, hidden_error_key(), err);
    }
    result
}

pub(super) extern "C" fn ns_iter_filter(
    closure: *const ClosureHeader,
    predicate: f64,
    opts: f64,
) -> f64 {
    let this = this_value(closure);
    prepare_readable_for_iteration(this);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(predicate);
    let mut out = crate::array::js_array_alloc(0);
    let mut callback_error = None;
    if readable_hidden_error(this).is_none() && !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            match call_settled_result(cb, el) {
                Ok(value) if crate::value::js_is_truthy(value) != 0 => {
                    out = crate::array::js_array_push_f64(out, el);
                }
                Ok(_) => {}
                Err(err) => {
                    callback_error = Some(err);
                    break;
                }
            }
        }
    }
    let result = readable_from_chunks(out);
    propagate_stream_state(this, opts, result);
    if let Some(err) = callback_error {
        set_hidden_value(result, hidden_error_key(), err);
    }
    result
}

// ─────────────────────────────────────────────────────────────────
// Truly-async consuming helpers (forEach / reduce / find / some /
// every). Like `toArray`, these drive the stream's
// `[Symbol.asyncIterator]().next()` via a then-chain — suspending on a
// pending promise and awaiting each (possibly-async) callback through
// `.then`, instead of block-draining the event loop with
// `settle_result`/`prepare_readable_for_iteration`. Block-draining ran
// unrelated macrotasks (timers/setImmediate — the React #327 re-entrancy
// hazard) and returned wrong results for live sources whose data hadn't
// buffered yet. A synchronous source (`Readable.from([...])`) still works:
// its iterator yields the buffered chunks through resolved promises.
// ─────────────────────────────────────────────────────────────────

const CONSUME_OP_FOR_EACH: f64 = 0.0;
const CONSUME_OP_REDUCE: f64 = 1.0;
const CONSUME_OP_FIND: f64 = 2.0;
const CONSUME_OP_SOME: f64 = 3.0;
const CONSUME_OP_EVERY: f64 = 4.0;

// Capture-slot layout for the `consume_on_next` state closure.
const SC_RESULT: u32 = 0; // result promise (ptr)
const SC_ITER: u32 = 1; // async iterator (f64)
const SC_CB: u32 = 2; // user callback (f64)
const SC_OP: u32 = 3; // operation code (f64)
const SC_ACC: u32 = 4; // accumulator / found / boolean result (f64)
const SC_REJECT: u32 = 5; // reject closure (ptr)
const SC_ON_CB: u32 = 6; // on-callback-result closure (ptr)
const SC_CUR: u32 = 7; // current element (f64) — find returns it
const SC_HAS_ACC: u32 = 8; // reduce: an accumulator exists yet (f64 bool)

#[inline]
fn bool_bits(b: bool) -> f64 {
    f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE })
}

#[inline]
fn consume_op_default(op: f64) -> f64 {
    if op == CONSUME_OP_SOME {
        f64::from_bits(TAG_FALSE)
    } else if op == CONSUME_OP_EVERY {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_UNDEFINED) // forEach / find / reduce
    }
}

/// Read `{ value, done }` off an iterator-result object. A non-object
/// (malformed / undefined) result is treated as `done`.
fn consume_read_iter_result(iter_result: f64) -> (bool, f64) {
    let Some(obj) = object_ptr_from_value(iter_result) else {
        return (true, f64::from_bits(TAG_UNDEFINED));
    };
    let done = js_object_get_field_by_name_f64(obj as *const ObjectHeader, hidden_key(b"done"));
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, hidden_key(b"value"));
    (crate::value::js_is_truthy(done) != 0, value)
}

/// Pull the next element: `iterator.next()` → `.then(on_next, reject)`.
/// Always routed through a promise so each step runs on a microtask (no
/// synchronous recursion, no block-drain).
fn consume_drive_next(iter: f64, on_next: *const ClosureHeader, reject: *const ClosureHeader) {
    let next_result = unsafe {
        crate::object::js_native_call_method(
            iter,
            b"next".as_ptr() as *const i8,
            4,
            std::ptr::null(),
            0,
        )
    };
    let promise = if crate::promise::js_value_is_promise(next_result) != 0 {
        crate::value::js_nanbox_get_pointer(next_result) as *mut crate::promise::Promise
    } else {
        crate::promise::js_promise_resolved(next_result)
    };
    crate::promise::js_promise_then(promise, on_next, reject);
}

/// Close the async iterator driving a consuming helper, releasing the
/// persistent stream `data`/`end`/`error` listeners and any buffered queue. A
/// no-op once the iterator is done. Every early settle path
/// (`consume_resolve` / `consume_reject_state`) routes through this so a
/// short-circuit, callback throw, rejection, or abort never leaks a live
/// stream's listeners after the helper promise settles.
fn consume_close_iter(state: *const ClosureHeader) {
    if state.is_null() {
        return;
    }
    let iter = js_closure_get_capture_f64(state, SC_ITER);
    if object_ptr_from_value(iter).is_none() {
        return;
    }
    let _ = unsafe {
        crate::object::js_native_call_method(
            iter,
            b"return".as_ptr() as *const i8,
            6,
            std::ptr::null(),
            0,
        )
    };
}

fn consume_resolve(state: *const ClosureHeader, value: f64) {
    consume_close_iter(state);
    let result = js_closure_get_capture_ptr(state, SC_RESULT) as *mut crate::promise::Promise;
    if !result.is_null() {
        crate::promise::js_promise_resolve(result, value);
    }
}

fn consume_reject_state(state: *const ClosureHeader, reason: f64) {
    consume_close_iter(state);
    let result = js_closure_get_capture_ptr(state, SC_RESULT) as *mut crate::promise::Promise;
    if !result.is_null() {
        crate::promise::js_promise_reject(result, reason);
    }
}

fn consume_finalize(state: *const ClosureHeader) {
    let op = js_closure_get_capture_f64(state, SC_OP);
    if op == CONSUME_OP_REDUCE
        && crate::value::js_is_truthy(js_closure_get_capture_f64(state, SC_HAS_ACC)) == 0
    {
        // reduce over an empty stream with no initial value rejects.
        consume_reject_state(state, reduce_missing_initial_error());
        return;
    }
    consume_resolve(state, js_closure_get_capture_f64(state, SC_ACC));
}

/// `.then` fulfilment for `iterator.next()`: read the iter-result, then
/// either finalize (done) or invoke the user callback and await its result.
extern "C" fn consume_on_next(state: *const ClosureHeader, iter_result: f64) -> f64 {
    if state.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let (done, value) = consume_read_iter_result(iter_result);
    if done {
        consume_finalize(state);
        return f64::from_bits(TAG_UNDEFINED);
    }
    js_closure_set_capture_f64(state as *mut ClosureHeader, SC_CUR, value);

    let iter = js_closure_get_capture_f64(state, SC_ITER);
    let reject = js_closure_get_capture_ptr(state, SC_REJECT) as *const ClosureHeader;
    let cb = callback_closure(js_closure_get_capture_f64(state, SC_CB));
    if cb.is_null() {
        // No callback: still consume to the end (matches the old skip-loop,
        // which yielded the op default / accumulated nothing).
        consume_drive_next(iter, state, reject);
        return f64::from_bits(TAG_UNDEFINED);
    }

    let op = js_closure_get_capture_f64(state, SC_OP);
    let cb_result = if op == CONSUME_OP_REDUCE {
        if crate::value::js_is_truthy(js_closure_get_capture_f64(state, SC_HAS_ACC)) == 0 {
            // First element seeds the accumulator (no callback call).
            js_closure_set_capture_f64(state as *mut ClosureHeader, SC_ACC, value);
            js_closure_set_capture_f64(state as *mut ClosureHeader, SC_HAS_ACC, bool_bits(true));
            consume_drive_next(iter, state, reject);
            return f64::from_bits(TAG_UNDEFINED);
        }
        let acc = js_closure_get_capture_f64(state, SC_ACC);
        catch_pipeline_throw(|| crate::closure::js_closure_call2(cb, acc, value))
    } else {
        catch_pipeline_throw(|| crate::closure::js_closure_call1(cb, value))
    };

    match cb_result {
        Ok(result) => {
            // Await the (possibly-promise) callback result, then continue.
            let on_cb = js_closure_get_capture_ptr(state, SC_ON_CB) as *const ClosureHeader;
            let p = crate::promise::js_promise_resolved(result);
            crate::promise::js_promise_then(p, on_cb, reject);
        }
        Err(err) => {
            // Callback threw: close the iterator before rejecting so the live
            // stream's listeners are released.
            consume_reject_state(state, err);
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `.then` fulfilment for the awaited callback result: update the
/// accumulator / short-circuit, then pull the next element.
extern "C" fn consume_on_cb(closure: *const ClosureHeader, cb_result: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let state = js_closure_get_capture_ptr(closure, 0) as *const ClosureHeader;
    if state.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let op = js_closure_get_capture_f64(state, SC_OP);
    let truthy = crate::value::js_is_truthy(cb_result) != 0;
    let short_circuit = if op == CONSUME_OP_REDUCE {
        js_closure_set_capture_f64(state as *mut ClosureHeader, SC_ACC, cb_result);
        None
    } else if op == CONSUME_OP_FIND {
        truthy.then(|| js_closure_get_capture_f64(state, SC_CUR))
    } else if op == CONSUME_OP_SOME {
        truthy.then(|| f64::from_bits(TAG_TRUE))
    } else if op == CONSUME_OP_EVERY {
        (!truthy).then(|| f64::from_bits(TAG_FALSE))
    } else {
        None // forEach ignores the result
    };

    if let Some(value) = short_circuit {
        consume_resolve(state, value);
        return f64::from_bits(TAG_UNDEFINED);
    }

    let iter = js_closure_get_capture_f64(state, SC_ITER);
    let reject = js_closure_get_capture_ptr(state, SC_REJECT) as *const ClosureHeader;
    consume_drive_next(iter, state, reject);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn consume_reject(closure: *const ClosureHeader, reason: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Capture slot 0 is the consume state (not the bare result promise) so the
    // iterator is closed before the rejection settles.
    let state = js_closure_get_capture_ptr(closure, 0) as *const ClosureHeader;
    if state.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    consume_reject_state(state, reason);
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) fn register_consume_arities() {
    crate::closure::js_register_closure_arity(consume_on_next as *const u8, 1);
    crate::closure::js_register_closure_arity(consume_on_cb as *const u8, 1);
    crate::closure::js_register_closure_arity(consume_reject as *const u8, 1);
    // Abort listeners are dispatched via `js_closure_call0` (0 args).
    crate::closure::js_register_closure_arity(ns_consume_abort as *const u8, 0);
    crate::closure::js_register_closure_arity(ns_to_array_abort as *const u8, 0);
}

/// Drive a consuming helper truly-asynchronously over the stream's async
/// iterator. `initial`/`has_initial` apply to `reduce`.
fn consume_stream(stream: f64, callback: f64, op: f64, initial: f64, opts: f64) -> f64 {
    // Already-errored stream / already-aborted signal reject up front.
    if let Some(err) = readable_hidden_error(stream) {
        return rejected_promise(err);
    }
    if let Some(sig) = effective_signal(stream, opts) {
        if signal_is_aborted(sig) {
            return rejected_promise(abort_error());
        }
    }

    let has_initial = op == CONSUME_OP_REDUCE && initial.to_bits() != TAG_UNDEFINED;
    let acc = if op == CONSUME_OP_REDUCE {
        initial
    } else {
        consume_op_default(op)
    };

    let Some(iter) = crate::array::call_symbol_async_iterator_for_flat_map(stream) else {
        // Not async-iterable (shouldn't happen for a readable): empty result.
        if op == CONSUME_OP_REDUCE && !has_initial {
            return rejected_promise(reduce_missing_initial_error());
        }
        return resolved_promise(acc);
    };

    let result = crate::promise::js_promise_new();
    let state = js_closure_alloc(consume_on_next as *const u8, 9);
    let on_cb = js_closure_alloc(consume_on_cb as *const u8, 1);
    let reject = js_closure_alloc(consume_reject as *const u8, 1);

    js_closure_set_capture_ptr(reject, 0, state as i64);
    js_closure_set_capture_ptr(on_cb, 0, state as i64);

    js_closure_set_capture_ptr(state, SC_RESULT, result as i64);
    js_closure_set_capture_f64(state, SC_ITER, iter);
    js_closure_set_capture_f64(state, SC_CB, callback);
    js_closure_set_capture_f64(state, SC_OP, op);
    js_closure_set_capture_f64(state, SC_ACC, acc);
    js_closure_set_capture_ptr(state, SC_REJECT, reject as i64);
    js_closure_set_capture_ptr(state, SC_ON_CB, on_cb as i64);
    js_closure_set_capture_f64(state, SC_CUR, f64::from_bits(TAG_UNDEFINED));
    js_closure_set_capture_f64(state, SC_HAS_ACC, bool_bits(has_initial));

    // Abort after consumption starts: a per-call (or inherited) signal that
    // fires mid-stream rejects the result and closes the iterator, instead of
    // leaving the promise pending forever. An already-aborted signal was
    // rejected up front (above), so the listener only handles later aborts.
    register_consume_abort(stream, opts, state);

    consume_drive_next(iter, state, reject);
    box_pointer(result as *const u8)
}

/// Register an abort listener for a consuming helper: when the governing signal
/// fires, close the iterator (releasing the stream listeners) and reject the
/// result with an AbortError. No-op when there is no signal.
fn register_consume_abort(stream: f64, opts: f64, state: *const ClosureHeader) {
    let Some(sig) = effective_signal(stream, opts) else {
        return;
    };
    let Some(sig_obj) = object_ptr_from_value(sig) else {
        return;
    };
    let abort_cl = js_closure_alloc(ns_consume_abort as *const u8, 1);
    js_closure_set_capture_ptr(abort_cl, 0, state as i64);
    crate::url::js_abort_signal_add_listener(
        sig_obj,
        string_value(b"abort"),
        box_pointer(abort_cl as *const u8),
    );
}

/// Abort-listener body for a consuming helper (`forEach`/`reduce`/`find`/
/// `some`/`every`): close the iterator and reject the result with an
/// AbortError. A no-op if the result already settled.
pub(super) extern "C" fn ns_consume_abort(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let state = js_closure_get_capture_ptr(closure, 0) as *const ClosureHeader;
    if state.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    consume_reject_state(state, abort_error());
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn ns_iter_reduce(
    closure: *const ClosureHeader,
    reducer: f64,
    initial: f64,
    opts: f64,
) -> f64 {
    consume_stream(
        this_value(closure),
        reducer,
        CONSUME_OP_REDUCE,
        initial,
        opts,
    )
}

pub(super) extern "C" fn ns_iter_for_each(
    closure: *const ClosureHeader,
    action: f64,
    opts: f64,
) -> f64 {
    consume_stream(
        this_value(closure),
        action,
        CONSUME_OP_FOR_EACH,
        f64::from_bits(TAG_UNDEFINED),
        opts,
    )
}

pub(super) extern "C" fn ns_iter_find(
    closure: *const ClosureHeader,
    predicate: f64,
    opts: f64,
) -> f64 {
    consume_stream(
        this_value(closure),
        predicate,
        CONSUME_OP_FIND,
        f64::from_bits(TAG_UNDEFINED),
        opts,
    )
}

pub(super) extern "C" fn ns_iter_some(
    closure: *const ClosureHeader,
    predicate: f64,
    opts: f64,
) -> f64 {
    consume_stream(
        this_value(closure),
        predicate,
        CONSUME_OP_SOME,
        f64::from_bits(TAG_UNDEFINED),
        opts,
    )
}

pub(super) extern "C" fn ns_iter_every(
    closure: *const ClosureHeader,
    predicate: f64,
    opts: f64,
) -> f64 {
    consume_stream(
        this_value(closure),
        predicate,
        CONSUME_OP_EVERY,
        f64::from_bits(TAG_UNDEFINED),
        opts,
    )
}

pub(super) extern "C" fn ns_iter_flat_map(
    closure: *const ClosureHeader,
    mapper: f64,
    opts: f64,
) -> f64 {
    let this = this_value(closure);
    prepare_readable_for_iteration(this);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(mapper);
    let mut out = crate::array::js_array_alloc(0);
    let mut callback_error = None;
    if readable_hidden_error(this).is_none() && !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            let mapped = match call_settled_result(cb, el) {
                Ok(value) => value,
                Err(err) => {
                    callback_error = Some(err);
                    break;
                }
            };
            // flatMap flattens one level: an array result is spread, a
            // Readable result contributes its retained chunks, an
            // async-iterable (e.g. an `async function*` mapper return —
            // issue #1572) is driven through its `[Symbol.asyncIterator]()`
            // and its yields flattened in order, anything else is
            // appended as a single chunk.
            if is_array_like_value(mapped) {
                out = extend_with_array(out, raw_ptr_from_value(mapped) as *const _);
            } else if let Some(inner) = readable_hidden_chunks(mapped) {
                if is_array_like_value(inner) {
                    out = extend_with_array(out, raw_ptr_from_value(inner) as *const _);
                } else {
                    out = crate::array::js_array_push_f64(out, mapped);
                }
            } else if let Some(flat) = flatten_async_iterable_value(mapped) {
                out = extend_with_array(out, flat as *const _);
            } else {
                out = crate::array::js_array_push_f64(out, mapped);
            }
        }
    }
    let result = readable_from_chunks(out);
    propagate_stream_state(this, opts, result);
    if let Some(err) = callback_error {
        set_hidden_value(result, hidden_error_key(), err);
    }
    result
}

/// Issue #1572 — drive an async-iterable value (an `async function*` mapper
/// return, or any object exposing `[Symbol.asyncIterator]` /
/// `[Symbol.iterator]` / a bare `.next()` method) through its iterator
/// protocol and collect the yielded values into a flat array.
///
/// The order of probes matches what `Array.fromAsync` / `for await of`
/// already does in `array/iterator.rs`:
///   1. `[Symbol.asyncIterator]()` — the async-generator path. Each
///      `.next()` returns a `Promise<{value, done}>`; the per-step
///      promise is settled synchronously by pumping microtasks.
///   2. The value is itself an iterator (bare `.next()` method) —
///      sync-drive it. Covers caller-provided iterator objects.
///   3. Sync iterables — `[Symbol.iterator]()`. Caught earlier by
///      `is_array_like_value`/`readable_hidden_chunks` for the array
///      and Readable cases; remaining sync iterables (Map/Set/Buffer
///      iterators, custom `[Symbol.iterator]` objects) land here.
///
/// `None` signals "not iterable" so the caller can fall back to the
/// "append as a single chunk" path that pre-#1572 was the only branch.
pub(super) fn flatten_async_iterable_with_source(
    value: f64,
) -> Option<(*mut crate::array::ArrayHeader, Option<f64>)> {
    use crate::array::{
        async_iterator_to_array_for_flat_map, call_symbol_async_iterator_for_flat_map,
        has_iterator_next,
    };
    use crate::symbol::js_get_iterator;
    if let Some(async_iter) = call_symbol_async_iterator_for_flat_map(value) {
        return Some((
            async_iterator_to_array_for_flat_map(async_iter),
            Some(async_iter),
        ));
    }
    if has_iterator_next(value) {
        // Async generator step values may be already-settled promises that
        // `async_iterator_to_array_for_flat_map` unwraps; drive the same
        // helper for a bare-iterator receiver too — `js_async_iterator_to_array`
        // is a strict superset of `js_iterator_to_array` (it transparently
        // returns non-promise step results unchanged).
        return Some((async_iterator_to_array_for_flat_map(value), Some(value)));
    }
    let sync_iter = js_get_iterator(value);
    if sync_iter.to_bits() != value.to_bits() {
        return Some((
            async_iterator_to_array_for_flat_map(sync_iter),
            Some(sync_iter),
        ));
    }
    None
}

pub(super) fn flatten_async_iterable_value(value: f64) -> Option<*mut crate::array::ArrayHeader> {
    flatten_async_iterable_with_source(value).map(|(chunks, _)| chunks)
}

pub(super) extern "C" fn ns_iter_take(closure: *const ClosureHeader, count: f64) -> f64 {
    let this = this_value(closure);
    prepare_readable_for_iteration(this);
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

pub(super) extern "C" fn ns_iter_drop(closure: *const ClosureHeader, count: f64) -> f64 {
    let this = this_value(closure);
    prepare_readable_for_iteration(this);
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
