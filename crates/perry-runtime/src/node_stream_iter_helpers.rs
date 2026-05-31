//! node:stream — async-consume + iterator-helper machinery (map/filter/reduce/compose/...) (split out of node_stream.rs for the 2000-line
//! file-size gate, #1987). Shares the parent module's constants, hidden-key
//! accessors and state primitives via `use super::*`.
#![allow(unused_imports)]
use super::*;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::object::{
    js_object_alloc, js_object_alloc_with_shape, js_object_get_field,
    js_object_get_field_by_name_f64, js_object_set_field, js_object_set_field_by_name,
    ObjectHeader,
};
use crate::value::JSValue;
use std::os::raw::c_int;

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

pub(super) fn extend_compose_output_chunks(
    mut out: *mut crate::array::ArrayHeader,
    stage: f64,
    chunks: f64,
) -> *mut crate::array::ArrayHeader {
    if !readable_object_mode(stage) {
        let mut bytes = Vec::new();
        append_chunk_bytes(chunks, &mut bytes, 0);
        if !bytes.is_empty() {
            out = crate::array::js_array_push_f64(out, buffer_value_from_bytes(&bytes));
        }
        return out;
    }

    if is_array_like_value(chunks) {
        extend_with_array(out, raw_ptr_from_value(chunks) as *const _)
    } else {
        crate::array::js_array_push_f64(out, chunks)
    }
}

pub(super) fn compose_readable_snapshot(source: f64, stage: f64) -> Option<f64> {
    prepare_readable_for_iteration(source);
    let arr = readable_chunks_array(source);
    if arr.is_null() || !is_transform_stream(stage) {
        return None;
    }

    let mut out = crate::array::js_array_alloc(0);
    if has_truthy_hidden(stage, hidden_transform_passthrough_key()) {
        out = extend_compose_output_chunks(out, stage, box_pointer(arr as *const u8));
    } else {
        transform_hidden_callback(stage)?;
        clear_readable_buffer(stage);
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let chunk = crate::array::js_array_get_f64(arr, i);
            let _ = write_writable_chunk(
                stage,
                chunk,
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
            );
            if let Some(err) = readable_hidden_error(stage) {
                let result = readable_from_chunks(out);
                propagate_stream_state(source, f64::from_bits(TAG_UNDEFINED), result);
                set_hidden_value(result, hidden_error_key(), err);
                return Some(result);
            }
        }
        if let Some(chunks) = readable_hidden_chunks(stage) {
            out = extend_compose_output_chunks(out, stage, chunks);
        }
    }

    let result = readable_from_chunks(out);
    propagate_stream_state(source, f64::from_bits(TAG_UNDEFINED), result);
    Some(result)
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
                crate::promise::PromiseState::Rejected => return Err((*p).reason),
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
            crate::promise::PromiseState::Rejected => Err((*p).reason),
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
    prepare_readable_for_iteration(this);
    let arr = readable_chunks_array(this);
    let mut out = crate::array::js_array_alloc(0);
    if !arr.is_null() {
        out = extend_with_array(out, arr);
    }
    let result = settle_consuming(this, opts, box_pointer(out as *const u8));
    if readable_hidden_error(this).is_none() {
        mark_stream_ended(this);
        clear_readable_buffer(this);
        destroy_stream(this, f64::from_bits(TAG_UNDEFINED));
    }
    result
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

pub(super) extern "C" fn ns_iter_reduce(
    closure: *const ClosureHeader,
    reducer: f64,
    initial: f64,
    opts: f64,
) -> f64 {
    let this = this_value(closure);
    prepare_readable_for_iteration(this);
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
        if readable_hidden_error(this).is_some() {
            return settle_consuming(this, opts, f64::from_bits(TAG_UNDEFINED));
        }
        if let Some(sig) = effective_signal(this, opts) {
            if signal_is_aborted(sig) {
                return rejected_promise(abort_error());
            }
        }
        return rejected_promise(reduce_missing_initial_error());
    };
    if readable_hidden_error(this).is_none() && !cb.is_null() {
        for i in start..len {
            let el = crate::array::js_array_get_f64(arr, i);
            // Node's stream reducer is (accumulator, current) — no index.
            match settle_result(crate::closure::js_closure_call2(cb, acc, el)) {
                Ok(value) => acc = value,
                Err(err) => return rejected_promise(err),
            }
        }
    }
    settle_consuming(this, opts, acc)
}

pub(super) extern "C" fn ns_iter_for_each(
    closure: *const ClosureHeader,
    action: f64,
    opts: f64,
) -> f64 {
    let this = this_value(closure);
    prepare_readable_for_iteration(this);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(action);
    if readable_hidden_error(this).is_none() && !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            if let Err(err) = call_settled_result(cb, el) {
                return rejected_promise(err);
            }
        }
    }
    settle_consuming(this, opts, f64::from_bits(TAG_UNDEFINED))
}

pub(super) extern "C" fn ns_iter_find(
    closure: *const ClosureHeader,
    predicate: f64,
    opts: f64,
) -> f64 {
    let this = this_value(closure);
    prepare_readable_for_iteration(this);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(predicate);
    let mut found = f64::from_bits(TAG_UNDEFINED);
    if readable_hidden_error(this).is_none() && !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            match call_settled_result(cb, el) {
                Ok(value) if crate::value::js_is_truthy(value) != 0 => {
                    found = el;
                    break;
                }
                Ok(_) => {}
                Err(err) => return rejected_promise(err),
            }
        }
    }
    settle_consuming(this, opts, found)
}

pub(super) extern "C" fn ns_iter_some(
    closure: *const ClosureHeader,
    predicate: f64,
    opts: f64,
) -> f64 {
    let this = this_value(closure);
    prepare_readable_for_iteration(this);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(predicate);
    let mut result = f64::from_bits(TAG_FALSE);
    if readable_hidden_error(this).is_none() && !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            match call_settled_result(cb, el) {
                Ok(value) if crate::value::js_is_truthy(value) != 0 => {
                    result = f64::from_bits(TAG_TRUE);
                    break;
                }
                Ok(_) => {}
                Err(err) => return rejected_promise(err),
            }
        }
    }
    settle_consuming(this, opts, result)
}

pub(super) extern "C" fn ns_iter_every(
    closure: *const ClosureHeader,
    predicate: f64,
    opts: f64,
) -> f64 {
    let this = this_value(closure);
    prepare_readable_for_iteration(this);
    let arr = readable_chunks_array(this);
    let cb = callback_closure(predicate);
    let mut result = f64::from_bits(TAG_TRUE);
    if readable_hidden_error(this).is_none() && !arr.is_null() && !cb.is_null() {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let el = crate::array::js_array_get_f64(arr, i);
            match call_settled_result(cb, el) {
                Ok(value) if crate::value::js_is_truthy(value) == 0 => {
                    result = f64::from_bits(TAG_FALSE);
                    break;
                }
                Ok(_) => {}
                Err(err) => return rejected_promise(err),
            }
        }
    }
    settle_consuming(this, opts, result)
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
