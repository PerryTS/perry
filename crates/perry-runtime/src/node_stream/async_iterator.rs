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

// Pending pulls are held in a FIFO queue (an iterator-local array of boxed
// promise pointers), NOT a single slot: concurrent `next()` calls made while
// the chunk queue is empty each get their own promise, settled in call order
// as `data`/`end`/`error`/`return` arrive. A single slot would overwrite the
// first promise (leaving it pending forever) on the second `next()`.
//
// Invariant: a pending pull is only ever enqueued while the chunk queue is
// empty (see `ns_readable_iterator_next`), so a non-empty pending queue implies
// nothing is buffered, and vice versa.

/// Append a pending pull to the back of the FIFO queue.
fn iterator_push_pending(iterator: f64, promise: *mut crate::promise::Promise) {
    let existing = get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_PENDING_KEY))
        .filter(|v| is_array_like_value(*v))
        .unwrap_or_else(|| box_pointer(crate::array::js_array_alloc(0) as *const u8));
    let arr = raw_ptr_from_value(existing) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, box_pointer(promise as *const u8));
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_PENDING_KEY),
        box_pointer(arr as *const u8),
    );
}

/// Take the oldest pending pull off the front of the FIFO queue, or `None`
/// when no `next()` is currently awaiting.
fn iterator_shift_pending(iterator: f64) -> Option<*mut crate::promise::Promise> {
    let value = get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_PENDING_KEY))?;
    if !is_array_like_value(value) {
        return None;
    }
    let arr = raw_ptr_from_value(value) as *mut crate::array::ArrayHeader;
    if crate::array::js_array_length(arr) == 0 {
        return None;
    }
    let boxed = crate::array::js_array_shift_f64(arr);
    let p = crate::value::js_nanbox_get_pointer(boxed) as *mut crate::promise::Promise;
    (!p.is_null()).then_some(p)
}

/// Whether any `next()` is currently awaiting a chunk.
fn iterator_has_pending(iterator: f64) -> bool {
    get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_PENDING_KEY))
        .filter(|v| is_array_like_value(*v))
        .is_some_and(|v| {
            let arr = raw_ptr_from_value(v) as *mut crate::array::ArrayHeader;
            crate::array::js_array_length(arr) != 0
        })
}

/// Resolve every outstanding pending pull with `{done:true}` (used on `end` and
/// `return()`), so a queued `next()` is never left unresolved.
fn iterator_resolve_all_pending_done(iterator: f64) {
    while let Some(p) = iterator_shift_pending(iterator) {
        crate::promise::js_promise_resolve(p, iterator_result(f64::from_bits(TAG_UNDEFINED), true));
    }
}

/// On `error`, reject the oldest outstanding pull with the error, then resolve
/// the rest with `{done:true}` — mirrors an async generator, whose in-flight
/// pull observes the throw while later-queued pulls see the now-finished
/// iterator. Every pull is settled; none is dropped.
fn iterator_reject_all_pending(iterator: f64, reason: f64) {
    if let Some(first) = iterator_shift_pending(iterator) {
        crate::promise::js_promise_reject(first, reason);
    }
    iterator_resolve_all_pending_done(iterator);
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
    if let Some(pending) = iterator_shift_pending(iterator) {
        note_yield(iterator);
        // Resolve the waiting `next()` with the same awaited-chunk handling the
        // queued path (`readable_iterator_chunk_result`) uses, so a
        // promise-valued chunk settles to its value instead of leaking as
        // `{ value: Promise, done:false }`. `js_promise_resolve` does not
        // assimilate thenables, so a promise chunk is adopted via
        // `js_promise_resolve_with_promise`.
        if crate::promise::js_value_is_promise(chunk) == 0 {
            crate::promise::js_promise_resolve(pending, iterator_result(chunk, false));
        } else {
            let result = readable_iterator_chunk_result(chunk);
            let inner = crate::value::js_nanbox_get_pointer(result) as *mut crate::promise::Promise;
            crate::promise::js_promise_resolve_with_promise(pending, inner);
        }
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
    // Outstanding pulls imply an empty chunk queue (invariant): finish the
    // iterator and resolve every waiter with `{done:true}`. With no waiters,
    // buffered chunks are still delivered by later `next()` calls, which then
    // observe the ended flag.
    if iterator_has_pending(iterator) {
        iterator_mark_done(iterator);
        iterator_remove_listeners(iterator);
        iterator_resolve_all_pending_done(iterator);
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
    if iterator_has_pending(iterator) {
        iterator_mark_done(iterator);
        iterator_remove_listeners(iterator);
        iterator_reject_all_pending(iterator, reason);
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
    // `data`/`end`/`error` event settles. Concurrent `next()` calls each enqueue
    // their own promise (FIFO) — none is overwritten or dropped.
    let promise = crate::promise::js_promise_new();
    iterator_push_pending(iterator, promise);
    box_pointer(promise as *const u8)
}

extern "C" fn ns_readable_iterator_return(closure: *const ClosureHeader) -> f64 {
    let iterator = this_value(closure);
    let already_done = iterator_is_done(iterator);
    iterator_mark_done(iterator);
    iterator_remove_listeners(iterator);
    // Settle every outstanding pull with `{done:true}` — `return()` must never
    // drop a pending pull without resolving it.
    iterator_resolve_all_pending_done(iterator);
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

#[cfg(test)]
mod fifo_pending_tests {
    use super::*;

    fn new_iterator() -> f64 {
        // A dummy stream value is fine: these tests drive the pending-queue
        // helpers/handler directly and never flow the stream.
        build_readable_async_iterator(f64::from_bits(TAG_UNDEFINED), true)
    }

    fn result_field(promise: *mut crate::promise::Promise, field: &[u8]) -> f64 {
        let boxed = unsafe { (*promise).value };
        let obj = crate::value::js_nanbox_get_pointer(boxed) as *const crate::object::ObjectHeader;
        crate::object::js_object_get_field_by_name_f64(obj, hidden_key(field))
    }

    #[test]
    fn pending_pulls_settle_in_fifo_order_on_data() {
        let iterator = new_iterator();
        let p1 = crate::promise::js_promise_new();
        let p2 = crate::promise::js_promise_new();
        // Two `next()` pulls made while the queue is empty: both must be
        // retained. Pre-fix the single slot dropped p1 on the second push,
        // leaving its promise pending forever.
        iterator_push_pending(iterator, p1);
        iterator_push_pending(iterator, p2);

        let data_cb = js_closure_alloc(ns_readable_iter_on_data as *const u8, 1);
        js_closure_set_capture_f64(data_cb, 0, iterator);

        // Two data events resolve p1 then p2 in FIFO order.
        ns_readable_iter_on_data(data_cb, 1.0);
        ns_readable_iter_on_data(data_cb, 2.0);

        assert_eq!(
            unsafe { (*p1).state },
            crate::promise::PromiseState::Fulfilled
        );
        assert_eq!(
            unsafe { (*p2).state },
            crate::promise::PromiseState::Fulfilled
        );
        assert_eq!(result_field(p1, b"value"), 1.0);
        assert_eq!(result_field(p2, b"value"), 2.0);
        // Queue fully drained.
        assert!(iterator_shift_pending(iterator).is_none());
    }

    #[test]
    fn return_settles_every_outstanding_pull_done() {
        let iterator = new_iterator();
        let p1 = crate::promise::js_promise_new();
        let p2 = crate::promise::js_promise_new();
        iterator_push_pending(iterator, p1);
        iterator_push_pending(iterator, p2);

        // `return()`/`end` must resolve BOTH waiters with {done:true}, never
        // drop one unsettled.
        iterator_resolve_all_pending_done(iterator);

        assert_eq!(
            unsafe { (*p1).state },
            crate::promise::PromiseState::Fulfilled
        );
        assert_eq!(
            unsafe { (*p2).state },
            crate::promise::PromiseState::Fulfilled
        );
        assert!(crate::value::js_is_truthy(result_field(p1, b"done")) != 0);
        assert!(crate::value::js_is_truthy(result_field(p2, b"done")) != 0);
        assert!(iterator_shift_pending(iterator).is_none());
    }

    #[test]
    fn error_rejects_oldest_pull_and_finishes_rest() {
        let iterator = new_iterator();
        let p1 = crate::promise::js_promise_new();
        let p2 = crate::promise::js_promise_new();
        iterator_push_pending(iterator, p1);
        iterator_push_pending(iterator, p2);

        // Oldest pull observes the error; later pulls see the finished iterator.
        iterator_reject_all_pending(iterator, 7.0);

        assert_eq!(
            unsafe { (*p1).state },
            crate::promise::PromiseState::Rejected
        );
        assert_eq!(unsafe { (*p1).reason }, 7.0);
        assert_eq!(
            unsafe { (*p2).state },
            crate::promise::PromiseState::Fulfilled
        );
        assert!(crate::value::js_is_truthy(result_field(p2, b"done")) != 0);
    }
}
