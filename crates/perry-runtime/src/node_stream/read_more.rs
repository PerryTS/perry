//! node's read-ahead for a Readable with a `_read` hook (lib/internal/streams/
//! readable.js): `reading` (`ReadableReadInvoked`) is set while a `_read` call
//! has not pushed yet and cleared by every `push()`; `sync`
//! (`ReadableSync`) holds while `_read` runs, so a chunk it pushes is buffered
//! and handed out by the `read()`/`flow()` that called it, after it; and
//! `maybeReadMore` keeps calling `_read` on the next tick while the buffer is
//! below the high-water mark.
use super::*;

/// node's `kSync`: the stream's `_read` is running.
pub(super) fn read_is_sync(stream: f64) -> bool {
    has_truthy_hidden(stream, Slot::ReadableSync)
}

/// node's `kReading`: a `_read` call has not pushed yet.
pub(super) fn read_in_flight(stream: f64) -> bool {
    get_hidden_value(stream, hidden_read_invoked_key()).is_some_and(|v| v.to_bits() == TAG_TRUE)
}

/// Every `push()` (a chunk, an empty chunk, EOF) ends the `_read` call it
/// answers: node's `state.reading = false`. True when one was in flight,
/// which proves the stream has a `_read` hook.
pub(super) fn note_push(stream: f64) -> bool {
    if read_in_flight(stream) {
        set_hidden_value(
            stream,
            hidden_read_invoked_key(),
            f64::from_bits(TAG_UNDEFINED),
        );
        return true;
    }
    false
}

fn high_water_mark(stream: f64) -> f64 {
    get_hidden_value(stream, hidden_hwm_key()).unwrap_or_else(|| default_hwm(false))
}

fn buffered(stream: f64) -> f64 {
    get_hidden_value(stream, hidden_buffered_key()).unwrap_or(0.0)
}

/// node's `read()` deciding to call `_read` before it takes `taking` units
/// from the buffer: not while reading, ended, destroyed or constructing, and
/// only while what stays buffered is below the high-water mark (or nothing
/// is buffered). Calls it when so. A stream with no `_read` hook keeps the
/// default read (`invoke_read_once`'s not-implemented error).
pub(super) fn read_ahead(stream: f64, taking: f64) {
    if has_read_hook(stream) {
        let length = buffered(stream);
        if length != 0.0 && length - taking >= high_water_mark(stream) {
            return;
        }
    }
    invoke_read_once(stream);
}

/// The units `read()` with no size takes: the head chunk (one in object
/// mode), or nothing when the buffer is empty.
pub(super) fn head_units(stream: f64) -> f64 {
    let Some(chunks) = readable_hidden_chunks(stream).filter(|v| is_array_like_value(*v)) else {
        return 0.0;
    };
    let arr = raw_ptr_from_value(chunks) as *const crate::array::ArrayHeader;
    if crate::array::js_array_length(arr) == 0 {
        return 0.0;
    }
    chunk_units(stream, crate::array::js_array_get_f64(arr, 0))
}

/// One chunk's share of the buffered length.
pub(super) fn chunk_units(stream: f64, chunk: f64) -> f64 {
    if readable_object_mode(stream) {
        1.0
    } else {
        super::chunk_byte_len(chunk) as f64
    }
}

/// node's `maybeReadMore`: after a push, keep reading on the next tick while
/// the buffer is below the high-water mark (or a flowing stream drained it).
/// `answered` is [`note_push`]'s answer for this push.
pub(super) fn maybe_read_more(stream: f64, answered: bool) {
    if has_truthy_hidden(stream, Slot::ReadableReadingMore)
        || read_in_flight(stream)
        || has_truthy_hidden(stream, Slot::ConstructPending)
        || !(answered || has_read_hook(stream))
    {
        return;
    }
    set_hidden_value(stream, Slot::ReadableReadingMore, f64::from_bits(TAG_TRUE));
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let tick = js_closure_alloc(crate::fn_info!(maybe_read_more_tick, 0), 1);
    js_closure_set_capture_f64(tick, 0, stream.get_nanbox_f64());
    crate::builtins::js_queue_next_tick(tick as i64);
}

extern "C" fn maybe_read_more_tick(c: *const ClosureHeader, _: crate::closure::JsThis) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(js_closure_get_capture_f64(c, 0));
    let s = &stream;
    // `_read` is user code: re-read every state word after it.
    while !read_in_flight(s.get_nanbox_f64())
        && !stream_hidden_ended(s.get_nanbox_f64())
        && !stream_destroyed(s.get_nanbox_f64())
    {
        let length = buffered(s.get_nanbox_f64());
        if !(length < high_water_mark(s.get_nanbox_f64())
            || (readable_is_flowing(s.get_nanbox_f64()) && length == 0.0))
        {
            break;
        }
        // `read(0)`.
        invoke_read_once(s.get_nanbox_f64());
        if buffered(s.get_nanbox_f64()) == length {
            break;
        }
    }
    set_hidden_value(
        s.get_nanbox_f64(),
        Slot::ReadableReadingMore,
        f64::from_bits(TAG_FALSE),
    );
    f64::from_bits(TAG_UNDEFINED)
}

/// node's `on('readable')` with nothing buffered: `read(0)` on the next tick.
pub(super) fn schedule_read_zero(stream: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let tick = js_closure_alloc(crate::fn_info!(read_zero_tick, 0), 1);
    js_closure_set_capture_f64(tick, 0, stream.get_nanbox_f64());
    crate::builtins::js_queue_next_tick(tick as i64);
}

extern "C" fn read_zero_tick(c: *const ClosureHeader, _: crate::closure::JsThis) -> f64 {
    let stream = js_closure_get_capture_f64(c, 0);
    if !read_in_flight(stream) {
        invoke_read_once(stream);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// A chunk a synchronous `_read` pushed into a flowing stream waits in the
/// buffer; node's `emitReadable_` then `flow()`s it on the next tick unless
/// the `read()` that called `_read` hands it out first.
pub(super) fn schedule_flow(stream: f64) {
    if has_truthy_hidden(stream, Slot::ReadableFlowScheduled) {
        return;
    }
    set_hidden_value(
        stream,
        Slot::ReadableFlowScheduled,
        f64::from_bits(TAG_TRUE),
    );
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let tick = js_closure_alloc(crate::fn_info!(flow_tick, 0), 1);
    js_closure_set_capture_f64(tick, 0, stream.get_nanbox_f64());
    crate::builtins::js_queue_next_tick(tick as i64);
}

extern "C" fn flow_tick(c: *const ClosureHeader, _: crate::closure::JsThis) -> f64 {
    let stream = js_closure_get_capture_f64(c, 0);
    set_hidden_value(
        stream,
        Slot::ReadableFlowScheduled,
        f64::from_bits(TAG_FALSE),
    );
    flush_pending_readable_chunks(stream);
    f64::from_bits(TAG_UNDEFINED)
}
