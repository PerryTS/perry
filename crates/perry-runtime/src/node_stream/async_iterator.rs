use super::*;

const READABLE_ITERATOR_SHAPE_ID: u32 = 0x7FFF_FF60;
const READABLE_ITERATOR_STREAM_KEY: &[u8] = b"__perryReadableIteratorStream";
const READABLE_ITERATOR_INDEX_KEY: &[u8] = b"__perryReadableIteratorIndex";
const READABLE_ITERATOR_DONE_KEY: &[u8] = b"__perryReadableIteratorDone";
const READABLE_ITERATOR_DESTROY_ON_RETURN_KEY: &[u8] = b"__perryReadableIteratorDestroyOnReturn";
// True-async iterator state (replaces the old block-draining snapshot model).
// The iterator attaches PERSISTENT `data`/`end`/`error` listeners on the stream
// and buffers delivered chunks into this iterator-local queue; `next()` pulls
// from the queue or returns a pending promise resolved by the next event. This
// suspends on the event loop instead of synchronously spinning macrotasks, so
// it neither re-enters unrelated work nor prematurely ends a live stream.
const READABLE_ITERATOR_QUEUE_KEY: &[u8] = b"__perryReadableIteratorQueue";
const READABLE_ITERATOR_PENDING_KEY: &[u8] = b"__perryReadableIteratorPending";
const READABLE_ITERATOR_ENDED_KEY: &[u8] = b"__perryReadableIteratorEnded";
const READABLE_ITERATOR_ERROR_KEY: &[u8] = b"__perryReadableIteratorError";
const READABLE_ITERATOR_ATTACHED_KEY: &[u8] = b"__perryReadableIteratorAttached";
const READABLE_ITERATOR_DATA_CB_KEY: &[u8] = b"__perryReadableIteratorDataCb";
const READABLE_ITERATOR_END_CB_KEY: &[u8] = b"__perryReadableIteratorEndCb";
const READABLE_ITERATOR_ERROR_CB_KEY: &[u8] = b"__perryReadableIteratorErrorCb";

fn iterator_result(value: f64, done: bool) -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    js_object_set_field_by_name(obj, hidden_key(b"value"), value);
    js_object_set_field_by_name(
        obj,
        hidden_key(b"done"),
        f64::from_bits(if done { TAG_TRUE } else { TAG_FALSE }),
    );
    box_pointer(obj as *const u8)
}

fn readable_iterator_done() -> f64 {
    resolved_promise(iterator_result(f64::from_bits(TAG_UNDEFINED), true))
}

extern "C" fn ns_readable_iterator_chunk_fulfilled(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let outer = js_closure_get_capture_ptr(closure, 0) as *mut crate::promise::Promise;
    if !outer.is_null() {
        crate::promise::js_promise_resolve(outer, iterator_result(value, false));
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_readable_iterator_chunk_rejected(
    closure: *const ClosureHeader,
    reason: f64,
) -> f64 {
    let outer = js_closure_get_capture_ptr(closure, 0) as *mut crate::promise::Promise;
    if !outer.is_null() {
        crate::promise::js_promise_reject(outer, reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Wrap a yielded chunk in a resolved `{value,done:false}` promise. A chunk that
/// is itself a Promise (e.g. `Readable.from([Promise.resolve(x)])` whose element
/// reached the queue unresolved) is awaited so the consumer sees the settled
/// value — matching Node's async-iterator semantics.
fn readable_iterator_chunk_result(value: f64) -> f64 {
    if crate::promise::js_value_is_promise(value) == 0 {
        return resolved_promise(iterator_result(value, false));
    }

    let inner = crate::value::js_nanbox_get_pointer(value) as *mut crate::promise::Promise;
    let outer = crate::promise::js_promise_new();
    let fulfill = js_closure_alloc(ns_readable_iterator_chunk_fulfilled as *const u8, 1);
    let reject = js_closure_alloc(ns_readable_iterator_chunk_rejected as *const u8, 1);
    js_closure_set_capture_ptr(fulfill, 0, outer as i64);
    js_closure_set_capture_ptr(reject, 0, outer as i64);
    crate::promise::js_promise_attach_handlers(inner, fulfill, reject);
    box_pointer(outer as *const u8)
}

fn destroy_on_return_from_options(opts: f64) -> bool {
    !matches!(
        get_hidden_value(opts, hidden_key(b"destroyOnReturn")),
        Some(value) if value.to_bits() == TAG_FALSE
    )
}

fn iterator_destroys_on_return(iterator: f64) -> bool {
    get_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_DESTROY_ON_RETURN_KEY),
    )
    .is_none_or(|value| crate::value::js_is_truthy(value) != 0)
}

fn iterator_has_yielded(iterator: f64) -> bool {
    get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_INDEX_KEY))
        .and_then(jsvalue_as_f64)
        .is_some_and(|index| index > 0.0)
}

fn iterator_local_index(iterator: f64) -> u32 {
    get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_INDEX_KEY))
        .and_then(jsvalue_as_f64)
        .unwrap_or(0.0)
        .max(0.0) as u32
}

/// Record that a chunk has been handed to the consumer (drives
/// `iterator_has_yielded`, which gates `return()`'s destroyOnReturn teardown).
fn note_yield(iterator: f64) {
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_INDEX_KEY),
        (iterator_local_index(iterator) + 1) as f64,
    );
}

fn iterator_is_done(iterator: f64) -> bool {
    get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_DONE_KEY))
        .is_some_and(|v| crate::value::js_is_truthy(v) != 0)
}

fn iterator_mark_done(iterator: f64) {
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_DONE_KEY),
        f64::from_bits(TAG_TRUE),
    );
}

// ── iterator-local queue / pending-slot / state accessors ──────────────────

fn iterator_enqueue(iterator: f64, chunk: f64) {
    let existing = get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_QUEUE_KEY))
        .filter(|v| is_array_like_value(*v))
        .unwrap_or_else(|| box_pointer(crate::array::js_array_alloc(0) as *const u8));
    let arr = raw_ptr_from_value(existing) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, chunk);
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_QUEUE_KEY),
        box_pointer(arr as *const u8),
    );
}

fn iterator_dequeue(iterator: f64) -> Option<f64> {
    let value = get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_QUEUE_KEY))?;
    if !is_array_like_value(value) {
        return None;
    }
    let arr = raw_ptr_from_value(value) as *mut crate::array::ArrayHeader;
    if crate::array::js_array_length(arr) == 0 {
        return None;
    }
    Some(crate::array::js_array_shift_f64(arr))
}

/// Take (and clear) the pending-slot promise, if a `next()` is currently
/// awaiting. A pending slot is only ever set while the queue is empty, so a
/// present slot implies nothing is queued.
fn iterator_take_pending(iterator: f64) -> Option<*mut crate::promise::Promise> {
    let value = get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_PENDING_KEY))?;
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_PENDING_KEY),
        f64::from_bits(TAG_UNDEFINED),
    );
    let p = crate::value::js_nanbox_get_pointer(value) as *mut crate::promise::Promise;
    (!p.is_null()).then_some(p)
}

fn iterator_set_pending(iterator: f64, promise: *mut crate::promise::Promise) {
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_PENDING_KEY),
        box_pointer(promise as *const u8),
    );
}

fn iterator_stream_ended(iterator: f64) -> bool {
    has_truthy_hidden(iterator, hidden_key(READABLE_ITERATOR_ENDED_KEY))
}

fn iterator_set_stream_ended(iterator: f64) {
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_ENDED_KEY),
        f64::from_bits(TAG_TRUE),
    );
}

fn iterator_stored_error(iterator: f64) -> Option<f64> {
    get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_ERROR_KEY))
}

fn iterator_set_error(iterator: f64, err: f64) {
    set_hidden_value(iterator, hidden_key(READABLE_ITERATOR_ERROR_KEY), err);
}

// ── persistent stream-event listeners feeding the iterator ─────────────────

fn iterator_from_listener(closure: *const ClosureHeader) -> f64 {
    js_closure_get_capture_f64(closure, 0)
}

/// `data` listener: resolve a waiting `next()` or buffer the chunk.
extern "C" fn ns_readable_iter_on_data(closure: *const ClosureHeader, chunk: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let iterator = iterator_from_listener(closure);
    if iterator_is_done(iterator) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if let Some(pending) = iterator_take_pending(iterator) {
        note_yield(iterator);
        crate::promise::js_promise_resolve(pending, iterator_result(chunk, false));
    } else {
        iterator_enqueue(iterator, chunk);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `end` listener: a waiting `next()` resolves to `{done:true}`; otherwise the
/// end is recorded so a later `next()` (after the queue drains) reports done.
extern "C" fn ns_readable_iter_on_end(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let iterator = iterator_from_listener(closure);
    iterator_set_stream_ended(iterator);
    if iterator_is_done(iterator) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if let Some(pending) = iterator_take_pending(iterator) {
        iterator_mark_done(iterator);
        iterator_remove_listeners(iterator);
        crate::promise::js_promise_resolve(
            pending,
            iterator_result(f64::from_bits(TAG_UNDEFINED), true),
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `error` listener: reject a waiting `next()` or store the error for the next
/// pull.
extern "C" fn ns_readable_iter_on_error(closure: *const ClosureHeader, reason: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let iterator = iterator_from_listener(closure);
    if iterator_is_done(iterator) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    iterator_set_error(iterator, reason);
    if let Some(pending) = iterator_take_pending(iterator) {
        iterator_mark_done(iterator);
        iterator_remove_listeners(iterator);
        crate::promise::js_promise_reject(pending, reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn attach_iterator_listener(
    iterator: f64,
    stream: f64,
    event: &[u8],
    func: *const u8,
    store_key: &[u8],
) {
    let cb = js_closure_alloc(func, 1);
    js_closure_set_capture_f64(cb, 0, iterator);
    let cb_value = box_pointer(cb as *const u8);
    set_hidden_value(iterator, hidden_key(store_key), cb_value);
    add_stream_listener_for_event(stream, string_value(event), cb_value);
}

/// Attach the persistent `data`/`end`/`error` listeners on the first `next()`
/// and start the stream flowing. Idempotent (guarded by the ATTACHED flag).
/// Nothing is delivered synchronously here — `resume` only schedules the
/// resume/drain microtasks, so this never re-enters the event loop.
fn iterator_ensure_attached(iterator: f64, stream: f64) {
    if has_truthy_hidden(iterator, hidden_key(READABLE_ITERATOR_ATTACHED_KEY)) {
        return;
    }
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_ATTACHED_KEY),
        f64::from_bits(TAG_TRUE),
    );

    attach_iterator_listener(
        iterator,
        stream,
        b"data",
        ns_readable_iter_on_data as *const u8,
        READABLE_ITERATOR_DATA_CB_KEY,
    );
    attach_iterator_listener(
        iterator,
        stream,
        b"end",
        ns_readable_iter_on_end as *const u8,
        READABLE_ITERATOR_END_CB_KEY,
    );
    attach_iterator_listener(
        iterator,
        stream,
        b"error",
        ns_readable_iter_on_error as *const u8,
        READABLE_ITERATOR_ERROR_CB_KEY,
    );

    mark_disturbed(stream);

    // Already-terminal-before-attach: no future event will reach our listeners,
    // so seed the terminal state directly.
    if let Some(err) = readable_hidden_error(stream) {
        iterator_set_error(iterator, err);
        return;
    }
    if has_truthy_hidden(stream, hidden_end_emitted_key()) || stream_destroyed(stream) {
        iterator_set_stream_ended(iterator);
        return;
    }

    // Start flow: delivers any already-buffered chunks via `data` (on the
    // resume/drain microtask) and schedules `end` if the source is finite.
    resume_readable_stream(stream);
}

fn remove_iterator_listener(iterator: f64, stream: f64, event: &[u8], store_key: &[u8]) {
    if let Some(cb_value) = get_hidden_value(iterator, hidden_key(store_key)) {
        remove_stream_listener_for_event(stream, string_value(event), cb_value);
        set_hidden_value(
            iterator,
            hidden_key(store_key),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
}

fn iterator_remove_listeners(iterator: f64) {
    let Some(stream) = get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_STREAM_KEY)) else {
        return;
    };
    remove_iterator_listener(iterator, stream, b"data", READABLE_ITERATOR_DATA_CB_KEY);
    remove_iterator_listener(iterator, stream, b"end", READABLE_ITERATOR_END_CB_KEY);
    remove_iterator_listener(iterator, stream, b"error", READABLE_ITERATOR_ERROR_CB_KEY);
}

fn settle_iterator_return_value(value: f64) {
    if crate::promise::js_value_is_promise(value) == 0 {
        return;
    }
    let promise = crate::value::js_nanbox_get_pointer(value) as *mut crate::promise::Promise;
    if promise.is_null() {
        return;
    }
    for _ in 0..10_000 {
        if unsafe { (*promise).state } != crate::promise::PromiseState::Pending {
            return;
        }
        if crate::promise::js_promise_run_microtasks() == 0 {
            return;
        }
    }
}

fn call_source_iterator_return(stream: f64) {
    let Some(source_iterator) = get_hidden_value(stream, hidden_key(READABLE_SOURCE_ITERATOR_KEY))
    else {
        return;
    };
    let returned = unsafe {
        crate::object::js_native_call_method(
            source_iterator,
            b"return".as_ptr() as *const i8,
            6,
            std::ptr::null(),
            0,
        )
    };
    settle_iterator_return_value(returned);
}

extern "C" fn ns_readable_iterator_next(closure: *const ClosureHeader) -> f64 {
    let iterator = this_value(closure);
    if iterator_is_done(iterator) {
        return readable_iterator_done();
    }
    let Some(stream) = get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_STREAM_KEY)) else {
        return readable_iterator_done();
    };

    // First pull: attach persistent listeners + start flow. Listeners deliver
    // asynchronously (resume schedules microtasks), so nothing arrives
    // synchronously here — no event-loop re-entrancy.
    iterator_ensure_attached(iterator, stream);

    // A chunk is already buffered → resolve immediately.
    if let Some(chunk) = iterator_dequeue(iterator) {
        note_yield(iterator);
        return readable_iterator_chunk_result(chunk);
    }

    // A stored error surfaces (once) as a rejection, then the iterator is done.
    if let Some(err) = iterator_stored_error(iterator) {
        iterator_mark_done(iterator);
        iterator_remove_listeners(iterator);
        return rejected_promise(err);
    }

    // The stream has ended and the queue is drained → done.
    if iterator_stream_ended(iterator) {
        iterator_mark_done(iterator);
        iterator_remove_listeners(iterator);
        return readable_iterator_done();
    }

    // Nothing available yet: hand back a pending promise that the next
    // `data`/`end`/`error` event resolves.
    let promise = crate::promise::js_promise_new();
    iterator_set_pending(iterator, promise);
    box_pointer(promise as *const u8)
}

extern "C" fn ns_readable_iterator_return(closure: *const ClosureHeader) -> f64 {
    let iterator = this_value(closure);
    let already_done = iterator_is_done(iterator);
    iterator_mark_done(iterator);
    // Drop a never-resolved pending pull and detach from the stream.
    let _ = iterator_take_pending(iterator);
    iterator_remove_listeners(iterator);
    if !already_done && iterator_has_yielded(iterator) && iterator_destroys_on_return(iterator) {
        if let Some(stream) = get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_STREAM_KEY)) {
            call_source_iterator_return(stream);
            destroy_stream(stream, f64::from_bits(TAG_UNDEFINED));
        }
    }
    readable_iterator_done()
}

extern "C" fn ns_readable_iterator_self(closure: *const ClosureHeader) -> f64 {
    this_value(closure)
}

pub(super) extern "C" fn ns_async_iterator(closure: *const ClosureHeader) -> f64 {
    build_readable_async_iterator(this_value(closure), true)
}

pub(super) extern "C" fn ns_iterator1(closure: *const ClosureHeader, opts: f64) -> f64 {
    build_readable_async_iterator(this_value(closure), destroy_on_return_from_options(opts))
}

fn install_async_iterator_symbol(target: f64, func: extern "C" fn(*const ClosureHeader) -> f64) {
    let async_iterator = crate::symbol::well_known_symbol("asyncIterator");
    if async_iterator.is_null() {
        return;
    }
    let closure = js_closure_alloc(func as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, target.to_bits() as i64);
    let closure_value = box_pointer(closure as *const u8);
    let symbol_value = box_pointer(async_iterator as *const u8);
    unsafe {
        crate::symbol::js_object_set_symbol_property(target, symbol_value, closure_value);
    }
}

fn build_readable_async_iterator(stream: f64, destroy_on_return: bool) -> f64 {
    let methods = [
        ("next", cast0(ns_readable_iterator_next)),
        ("return", cast0(ns_readable_iterator_return)),
    ];
    let obj = build_object(&methods, READABLE_ITERATOR_SHAPE_ID + methods.len() as u32);
    let iterator = box_pointer(obj as *const u8);
    set_hidden_value(iterator, hidden_key(READABLE_ITERATOR_STREAM_KEY), stream);
    set_hidden_value(iterator, hidden_key(READABLE_ITERATOR_INDEX_KEY), 0.0);
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_DONE_KEY),
        f64::from_bits(TAG_FALSE),
    );
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_DESTROY_ON_RETURN_KEY),
        f64::from_bits(if destroy_on_return {
            TAG_TRUE
        } else {
            TAG_FALSE
        }),
    );
    install_async_iterator_symbol(iterator, ns_readable_iterator_self);
    iterator
}

pub(super) fn install_readable_async_iterator_symbol(stream: f64) {
    install_async_iterator_symbol(stream, ns_async_iterator);
}

pub(super) fn register_arities() {
    crate::closure::js_register_closure_arity(ns_async_iterator as *const u8, 0);
    crate::closure::js_register_closure_arity(ns_iterator1 as *const u8, 1);
    crate::closure::js_register_closure_arity(ns_readable_iterator_next as *const u8, 0);
    crate::closure::js_register_closure_arity(ns_readable_iterator_return as *const u8, 0);
    crate::closure::js_register_closure_arity(ns_readable_iterator_self as *const u8, 0);
    crate::closure::js_register_closure_arity(ns_readable_iterator_chunk_fulfilled as *const u8, 1);
    crate::closure::js_register_closure_arity(ns_readable_iterator_chunk_rejected as *const u8, 1);
    crate::closure::js_register_closure_arity(ns_readable_iter_on_data as *const u8, 1);
    crate::closure::js_register_closure_arity(ns_readable_iter_on_end as *const u8, 0);
    crate::closure::js_register_closure_arity(ns_readable_iter_on_error as *const u8, 1);
}
