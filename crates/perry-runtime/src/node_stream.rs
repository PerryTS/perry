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
//! Method semantics are intentionally pragmatic rather than a full Node
//! stream rewrite: common EventEmitter, buffering, read/write/pipe, and
//! pipeline lifecycle paths are implemented here, while deeper async
//! iterator, Web Stream, and backpressure edge cases continue to land as
//! focused compatibility work.

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::object::js_object_set_field_by_name;
#[cfg(test)]
use crate::object::{js_object_get_field_by_name_f64, ObjectHeader};
use crate::value::JSValue;

pub(crate) mod async_iterator;
mod readable_from_promises;

#[path = "node_stream_event_emitter.rs"]
mod event_emitter;
use event_emitter::{
    add_stream_listener_for_event, call_listener_args, emit_stream_event,
    emit_stream_event_from_array, is_callable_value, ns_capture_rejection, ns_event_names,
    ns_get_max_listeners, ns_listener_count, ns_listeners, ns_off2, ns_on2, ns_once2,
    ns_prepend_listener2, ns_prepend_once_listener2, ns_raw_listeners, ns_remove_all_listeners1,
    ns_remove_listener2, ns_set_max_listeners, remove_stream_listener_for_event,
    stream_listener_count_for_event,
};

/// Dispatch `event` to the listeners registered on `stream` through node:stream's
/// own emitter registry.
///
/// Exposed for `child_process`: a child's `stdout`/`stderr` is an emitter-backed
/// object with its *own* listener registry, but it carries node:stream's async
/// iterator (see `async_iterator::install_readable_async_iterator_symbol`), whose
/// `data`/`end`/`error` listeners land *here*. Without this bridge the reactor's
/// emit would never reach them and `for await (const chunk of child.stdout)` would
/// hang forever.
pub(crate) fn emit_to_stream_listeners(stream: f64, event: &[u8], args: &[f64]) {
    let _ = emit_stream_event(stream, string_value(event), args);
}
// #3049 — `process.setMaxListeners` reuses the EventEmitter setter
// validation (TypeError/RangeError + fractional/Infinity storage).
pub(crate) use event_emitter::validate_max_listeners;
pub use event_emitter::{
    js_node_stream_method_event_names, js_node_stream_method_get_max_listeners,
    js_node_stream_method_listener_count, js_node_stream_method_listeners,
    js_node_stream_method_off, js_node_stream_method_on, js_node_stream_method_once,
    js_node_stream_method_prepend_listener, js_node_stream_method_prepend_once_listener,
    js_node_stream_method_raw_listeners, js_node_stream_method_remove_all_listeners,
    js_node_stream_method_remove_listener, js_node_stream_method_set_max_listeners,
};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

// Shape ids — pick a band well clear of fs streams (`STREAM_SHAPE_ID =
// 0x7FFF_FE40` + method_count). The base ids are spaced 0x40 (64
// slots) apart so each constructor's `base + method_count` lands in
// its own band and stays a unique shape-cache key — Readable's method
// set now includes iterator and EventEmitter helpers, so the historical
// 16-slot spacing no longer left enough headroom.
const READABLE_SHAPE_ID: u32 = 0x7FFF_FE60;
const WRITABLE_SHAPE_ID: u32 = 0x7FFF_FEA0;
const DUPLEX_SHAPE_ID: u32 = 0x7FFF_FEE0;
// #1540: shape band for the WHATWG web-stream interop stubs returned by
// `Readable/Writable/Duplex.toWeb`. Placed above the Duplex band so it
// can't collide as method sets grow.
const WEB_STREAM_SHAPE_ID: u32 = 0x7FFF_FF20;
const READABLE_CHUNKS_KEY: &[u8] = b"__perryReadableChunks";
const READABLE_SOURCE_ITERATOR_KEY: &[u8] = b"__perryReadableSourceIterator";
const READABLE_ERROR_KEY: &[u8] = b"__perryReadableError";
const READABLE_SIGNAL_KEY: &[u8] = b"__perryReadableSignal";
const READABLE_READ_KEY: &[u8] = b"__perryReadableRead";
const READABLE_READ_INVOKED_KEY: &[u8] = b"__perryReadableReadInvoked";
const READABLE_DEFAULT_READ_ERROR_KEY: &[u8] = b"__perryReadableDefaultReadError";
const STREAM_DRAIN_SCHEDULED_KEY: &[u8] = b"__perryStreamDrainScheduled";
const STREAM_READABLE_SCHEDULED_KEY: &[u8] = b"__perryStreamReadableScheduled";
const STREAM_END_SCHEDULED_KEY: &[u8] = b"__perryStreamEndScheduled";
const STREAM_END_EMITTED_KEY: &[u8] = b"__perryStreamEndEmitted";
const STREAM_ENDED_KEY: &[u8] = b"__perryStreamEnded";
const STREAM_MAX_LISTENERS_KEY: &[u8] = b"__perryStreamMaxListeners";
const STREAM_CAPTURE_REJECTIONS_KEY: &[u8] = b"__perryStreamCaptureRejections";
const WRITABLE_WRITE_KEY: &[u8] = b"__perryWritableWrite";
const WRITABLE_FINISH_SCHEDULED_KEY: &[u8] = b"__perryWritableFinishScheduled";
const WRITABLE_FINISH_EMITTED_KEY: &[u8] = b"__perryWritableFinishEmitted";
const WRITABLE_CORKED_KEY: &[u8] = b"__perryWritableCorked";
const WRITABLE_BUFFERED_KEY: &[u8] = b"__perryWritableBuffered";
const WRITABLE_LENGTH_KEY: &[u8] = b"__perryWritableLength";
const WRITABLE_NEED_DRAIN_KEY: &[u8] = b"__perryWritableNeedDrain";
const WRITABLE_OBJECT_MODE_KEY: &[u8] = b"__perryWritableObjectMode";
const WRITABLE_DECODE_STRINGS_KEY: &[u8] = b"__perryWritableDecodeStrings";
const WRITABLE_DEFAULT_ENCODING_KEY: &[u8] = b"__perryWritableDefaultEncoding";
const WRITABLE_PENDING_FINISH_CALLBACK_KEY: &[u8] = b"__perryWritablePendingFinishCallback";
const WRITABLE_WRITEV_KEY: &[u8] = b"__perryWritableWritev";
const STREAM_CONSTRUCT_KEY: &[u8] = b"__perryStreamConstruct";
const STREAM_DESTROY_KEY: &[u8] = b"__perryStreamDestroy";
const WRITABLE_FINAL_KEY: &[u8] = b"__perryWritableFinal";
const WRITABLE_FINAL_INVOKED_KEY: &[u8] = b"__perryWritableFinalInvoked";
const WRITABLE_FINAL_PENDING_KEY: &[u8] = b"__perryWritableFinalPending";
const TRANSFORM_CALLBACK_KEY: &[u8] = b"__perryTransformCallback";
const TRANSFORM_FLUSH_KEY: &[u8] = b"__perryTransformFlush";
const TRANSFORM_PASSTHROUGH_KEY: &[u8] = b"__perryTransformPassThrough";
const TRANSFORM_FINISHING_KEY: &[u8] = b"__perryTransformFinishing";
// #1534: direction + disturbed bits so the static introspection helpers
// (`Readable.isReadable` / `isDisturbed` / `isErrored`) answer per-stream
// instead of with a uniform stub. Set at construction / on first read.
const READABLE_FLAG_KEY: &[u8] = b"__perryIsReadable";
const WRITABLE_FLAG_KEY: &[u8] = b"__perryIsWritable";
const STREAM_DISTURBED_KEY: &[u8] = b"__perryStreamDisturbed";
// #1539: bytes currently buffered (for `push()`'s highWaterMark return) and
// the effective readable highWaterMark.
const READABLE_BUFFERED_KEY: &[u8] = b"__perryReadableBuffered";
const READABLE_HWM_KEY: &[u8] = b"__perryReadableHwm";
const READABLE_PENDING_KEY: &[u8] = b"__perryReadablePending";
const READABLE_RESUME_SCHEDULED_KEY: &[u8] = b"__perryReadableResumeScheduled";
const STREAM_PIPES_KEY: &[u8] = b"__perryStreamPipes";
const READABLE_BASE64_REMAINDER_KEY: &[u8] = b"__perryReadableBase64Remainder";
const STREAM_PIPE_NO_END_KEY: &[u8] = b"__perryStreamPipeNoEnd";
const STREAM_PIPE_END_PENDING_KEY: &[u8] = b"__perryStreamPipeEndPending";
const STREAM_AUTO_DESTROY_KEY: &[u8] = b"__perryStreamAutoDestroy";
const STREAM_EMIT_CLOSE_KEY: &[u8] = b"__perryStreamEmitClose";
const STREAM_PIPELINE_CALLBACK_DONE_KEY: &[u8] = b"__perryStreamPipelineCallbackDone";
const STREAM_COMPOSE_LIVE_PIPE_CONSUME_KEY: &[u8] = b"__perryStreamComposeLivePipeConsume";

use destroy_state::{destroy_stream, ns_destroy1, ns_destroy_error_microtask};
pub use destroy_state::{js_node_stream_method_destroy, js_node_stream_method_destroyed};

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
        // A `TAG_UNDEFINED` slot-0 marks a *prototype-method value* (e.g.
        // `Stream.prototype.on`, installed by `attach_event_emitter_prototype_methods`)
        // rather than an instance-bound method: it has no fixed receiver and must
        // read the call-site `this` (set by `Function.prototype.call`/`apply` into
        // IMPLICIT_THIS), so `Stream.prototype.on.call(streamInstance, ev, fn)`
        // — readable-stream's `Readable.prototype.on` wrapper — registers the
        // listener on the instance, not on the prototype. `build_object` always
        // captures a real object pointer, so existing instance-bound closures
        // never hit this branch.
        if bits != crate::value::TAG_UNDEFINED {
            return f64::from_bits(bits);
        }
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

fn call_old_stream_on(old_stream: f64, event: &[u8], listener: *const ClosureHeader) {
    if listener.is_null() {
        return;
    }
    let event = crate::string::js_string_from_bytes(event.as_ptr(), event.len() as u32);
    let handle = raw_ptr_from_value(old_stream) as i64;
    if handle != 0 {
        if let (Some(probe), Some(on)) = (
            crate::object::event_emitter_handle_probe(),
            crate::object::event_emitter_on(),
        ) {
            if unsafe { probe(handle) } {
                let event_bits = crate::value::js_nanbox_string(event as i64).to_bits() as i64;
                let listener_value = crate::value::js_nanbox_pointer(listener as i64);
                unsafe { on(handle, event_bits, listener_value.to_bits() as i64) };
                return;
            }
        }
    }
    let event_value = f64::from_bits(JSValue::string_ptr(event).bits());
    let listener_value = f64::from_bits(JSValue::pointer(listener as *const u8).bits());
    let args = [event_value, listener_value];
    let method = b"on";
    unsafe {
        let _ = crate::object::js_native_call_method(
            old_stream,
            method.as_ptr() as *const i8,
            method.len(),
            args.as_ptr(),
            args.len(),
        );
    }
}

extern "C" fn ns_wrap_data(closure: *const ClosureHeader, chunk: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let _ = push_chunk(stream, chunk);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_wrap_end(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let _ = push_chunk(stream, f64::from_bits(TAG_NULL));
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_wrap_error(closure: *const ClosureHeader, err: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    destroy_stream(stream, err);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_wrap_close(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    destroy_stream(stream, f64::from_bits(TAG_UNDEFINED));
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_wrap1(closure: *const ClosureHeader, old_stream: f64) -> f64 {
    let stream = this_value(closure);
    crate::closure::js_register_closure_arity(ns_wrap_data as *const u8, 1);
    crate::closure::js_register_closure_arity(ns_wrap_end as *const u8, 0);
    crate::closure::js_register_closure_arity(ns_wrap_error as *const u8, 1);
    crate::closure::js_register_closure_arity(ns_wrap_close as *const u8, 0);
    let data = js_closure_alloc(ns_wrap_data as *const u8, 1);
    let end = js_closure_alloc(ns_wrap_end as *const u8, 1);
    let error = js_closure_alloc(ns_wrap_error as *const u8, 1);
    let close = js_closure_alloc(ns_wrap_close as *const u8, 1);
    js_closure_set_capture_f64(data, 0, stream);
    js_closure_set_capture_f64(end, 0, stream);
    js_closure_set_capture_f64(error, 0, stream);
    js_closure_set_capture_f64(close, 0, stream);

    call_old_stream_on(old_stream, b"data", data);
    call_old_stream_on(old_stream, b"end", end);
    call_old_stream_on(old_stream, b"error", error);
    call_old_stream_on(old_stream, b"close", close);
    call_old_stream_on(old_stream, b"destroy", close);
    stream
}

extern "C" fn ns_readable_from_drain(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    set_hidden_value(
        stream,
        hidden_drain_scheduled_key(),
        f64::from_bits(TAG_FALSE),
    );
    drain_readable_from_events(stream);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_readable_event_microtask(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    if !has_truthy_hidden(stream, hidden_readable_scheduled_key()) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    set_hidden_value(
        stream,
        hidden_readable_scheduled_key(),
        f64::from_bits(TAG_FALSE),
    );
    let _ = emit_stream_event(stream, string_value(b"readable"), &[]);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_readable_end_microtask(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    set_hidden_value(
        stream,
        hidden_end_scheduled_key(),
        f64::from_bits(TAG_FALSE),
    );
    if pending_readable_chunk_count(stream) == 0 && !stream_destroyed(stream) {
        emit_readable_end_once(stream);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_readable_resume_microtask(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    set_hidden_value(
        stream,
        hidden_readable_resume_scheduled_key(),
        f64::from_bits(TAG_FALSE),
    );
    if readable_is_flowing(stream) && !stream_destroyed(stream) {
        let _ = emit_stream_event(stream, string_value(b"resume"), &[]);
        flush_pending_readable_chunks(stream);
        schedule_readable_from_drain(stream);
        invoke_read_once(stream);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_finished_error_false_close(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if js_closure_get_capture_f64(closure, 2).to_bits() == TAG_TRUE {
        return f64::from_bits(TAG_UNDEFINED);
    }
    js_closure_set_capture_f64(closure as *mut ClosureHeader, 2, f64::from_bits(TAG_TRUE));
    let stream = js_closure_get_capture_f64(closure, 0);
    let callback = js_closure_get_capture_f64(closure, 1);
    if let Some(err) = readable_hidden_error(stream) {
        call_listener_args(stream, callback, &[err]);
    } else {
        call_listener_args(stream, callback, &[]);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_finished_signal_abort(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if js_closure_get_capture_f64(closure, 2).to_bits() == TAG_TRUE {
        return f64::from_bits(TAG_UNDEFINED);
    }
    js_closure_set_capture_f64(closure as *mut ClosureHeader, 2, f64::from_bits(TAG_TRUE));
    let stream = js_closure_get_capture_f64(closure, 0);
    let callback = js_closure_get_capture_f64(closure, 1);
    let signal = js_closure_get_capture_f64(closure, 3);
    if let Some(signal_obj) = object_ptr_from_value(signal) {
        crate::url::js_abort_signal_remove_listener(
            signal_obj,
            string_value(b"abort"),
            box_pointer(closure as *const u8),
        );
    }
    call_listener_args(stream, callback, &[crate::url::js_abort_error_value()]);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_writable_finish_microtask(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    let callback = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    set_hidden_value(
        stream,
        hidden_finish_scheduled_key(),
        f64::from_bits(TAG_FALSE),
    );
    if !has_truthy_hidden(stream, hidden_finish_emitted_key()) {
        set_hidden_value(
            stream,
            hidden_finish_emitted_key(),
            f64::from_bits(TAG_TRUE),
        );
        mark_writable_finished(stream);
        if is_callable_value(callback) {
            call_listener_args(stream, callback, &[]);
        }
        let _ = emit_stream_event(stream, string_value(b"finish"), &[]);
        let readable_done = get_hidden_value(stream, hidden_readable_flag_key()).is_none()
            || has_truthy_hidden(stream, hidden_end_emitted_key());
        if readable_done {
            if stream_auto_destroy_enabled(stream) {
                mark_stream_destroyed(stream);
            }
            mark_stream_closed_and_emit_close(stream);
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_construct_callback_done(closure: *const ClosureHeader, err: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    if err.to_bits() != TAG_UNDEFINED && err.to_bits() != TAG_NULL {
        destroy_stream(stream, err);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_writable_final_callback_done(closure: *const ClosureHeader, err: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let callback = js_closure_get_capture_f64(closure, 1);
    set_hidden_value(
        stream,
        hidden_writable_final_pending_key(),
        f64::from_bits(TAG_FALSE),
    );
    if err.to_bits() != TAG_UNDEFINED && err.to_bits() != TAG_NULL {
        destroy_stream(stream, err);
        if is_callable_value(callback) {
            call_listener_args(stream, callback, &[err]);
        }
        return f64::from_bits(TAG_UNDEFINED);
    }
    schedule_writable_finish_then_transform_end(
        stream,
        if is_callable_value(callback) {
            Some(callback)
        } else {
            None
        },
    );
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_emit2(closure: *const ClosureHeader, event: f64, arg: f64) -> f64 {
    let stream = this_value(closure);
    let mut args = crate::array::js_array_alloc(0);
    if arg.to_bits() != TAG_UNDEFINED {
        args = crate::array::js_array_push_f64(args, arg);
    }
    emit_stream_event_from_array(stream, event, args)
}

extern "C" fn ns_emit_rest(closure: *const ClosureHeader, event: f64, rest: f64) -> f64 {
    emit_stream_event_from_array(
        this_value(closure),
        event,
        raw_ptr_from_value(rest) as *const _,
    )
}
extern "C" fn ns_resume0(closure: *const ClosureHeader) -> f64 {
    resume_readable_stream(this_value(closure))
}

extern "C" fn ns_pause0(closure: *const ClosureHeader) -> f64 {
    pause_readable_stream(this_value(closure))
}

extern "C" fn ns_is_paused0(closure: *const ClosureHeader) -> f64 {
    f64::from_bits(if readable_is_paused(this_value(closure)) {
        TAG_TRUE
    } else {
        TAG_FALSE
    })
}

extern "C" fn ns_async_dispose(closure: *const ClosureHeader) -> f64 {
    let stream = this_value(closure);
    destroy_stream(stream, abort_error());
    resolved_promise(f64::from_bits(TAG_UNDEFINED))
}

extern "C" fn ns_read1(closure: *const ClosureHeader, n: f64) -> f64 {
    let stream = this_value(closure);
    read_stream_with_size_arg(stream, n)
}

extern "C" fn ns_set_encoding1(closure: *const ClosureHeader, encoding: f64) -> f64 {
    let stream = this_value(closure);
    set_visible_readable_encoding(stream, normalize_readable_encoding(encoding));
    stream
}

/// Shared `push(chunk)` accounting (#1539): track the buffered byte count and
/// return `true` while it stays below `highWaterMark`, `false` once it
/// reaches/exceeds it — matching Node's backpressure signal. Pushing
/// `null`/`undefined` (EOF) returns `false`.
fn push_chunk(stream: f64, chunk: f64) -> f64 {
    if stream_destroyed(stream) {
        return f64::from_bits(TAG_FALSE);
    }
    let jsval = JSValue::from_bits(chunk.to_bits());
    if jsval.is_null() || jsval.is_undefined() {
        flush_readable_decoder(stream);
        mark_stream_ended(stream);
        refresh_readable_aborted_flag(stream);
        schedule_readable_end(stream);
        return f64::from_bits(TAG_FALSE);
    }
    if has_truthy_hidden(stream, hidden_ended_key()) {
        return f64::from_bits(TAG_FALSE);
    }
    let Some(chunk) = decode_readable_chunk_for_encoding(stream, chunk) else {
        return f64::from_bits(TAG_TRUE);
    };
    push_chunk_backpressure_result(stream, append_readable_output_chunk(stream, chunk))
}

fn append_readable_output_chunk(stream: f64, chunk: f64) -> f64 {
    let added = if readable_object_mode(stream) {
        1.0
    } else {
        chunk_byte_len(chunk) as f64
    };
    let prev = get_hidden_value(stream, hidden_buffered_key()).unwrap_or(0.0);
    let total = prev + added;
    set_hidden_value(stream, hidden_buffered_key(), total);
    set_hidden_value(stream, hidden_key(b"readableLength"), total);
    if added > 0.0 {
        push_readable_buffered_chunk(stream, chunk);
        mark_disturbed(stream);
        schedule_readable_event(stream);
        if readable_is_flowing(stream) && !should_defer_initial_data_emit(stream) {
            consume_readable_buffered_front_for_live_pipe(stream, chunk);
            emit_readable_data(stream, chunk);
        } else {
            buffer_pending_readable_chunk(stream, chunk);
        }
    }
    total
}

fn decode_readable_chunk_for_encoding(stream: f64, chunk: f64) -> Option<f64> {
    let Some(encoding) = readable_encoding_tag(stream) else {
        return Some(chunk);
    };
    if JSValue::from_bits(chunk.to_bits()).is_any_string() {
        return Some(chunk);
    }
    let raw = raw_ptr_from_value(chunk);
    if raw < 0x10000 || !crate::buffer::is_registered_buffer(raw) {
        return Some(chunk);
    }
    if encoding == 2 {
        return decode_readable_base64_chunk(stream, raw);
    }
    Some(buffer_chunk_to_encoded_string(raw, encoding))
}

fn decode_readable_base64_chunk(stream: f64, raw: usize) -> Option<f64> {
    let mut bytes = readable_base64_remainder_bytes(stream);
    append_buffer_bytes(raw, &mut bytes);
    let complete_len = bytes.len() / 3 * 3;
    set_readable_base64_remainder_bytes(stream, &bytes[complete_len..]);
    if complete_len == 0 {
        return None;
    }
    Some(encoded_string_from_bytes(&bytes[..complete_len], 2))
}

fn flush_readable_decoder(stream: f64) {
    if readable_encoding_tag(stream) != Some(2) {
        return;
    }
    let bytes = readable_base64_remainder_bytes(stream);
    set_readable_base64_remainder_bytes(stream, &[]);
    if !bytes.is_empty() {
        append_readable_output_chunk(stream, encoded_string_from_bytes(&bytes, 2));
    }
}

fn readable_base64_remainder_bytes(stream: f64) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(value) = get_hidden_value(stream, hidden_readable_base64_remainder_key()) {
        append_buffer_bytes(raw_ptr_from_value(value), &mut bytes);
    }
    bytes
}

fn set_readable_base64_remainder_bytes(stream: f64, bytes: &[u8]) {
    let value = if bytes.is_empty() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        buffer_value_from_bytes(bytes)
    };
    set_hidden_value(stream, hidden_readable_base64_remainder_key(), value);
}

fn buffer_chunk_to_encoded_string(raw: usize, encoding: i32) -> f64 {
    let ptr =
        crate::buffer::js_buffer_to_string(raw as *const crate::buffer::BufferHeader, encoding);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn encoded_string_from_bytes(bytes: &[u8], encoding: i32) -> f64 {
    let value = buffer_value_from_bytes(bytes);
    buffer_chunk_to_encoded_string(raw_ptr_from_value(value), encoding)
}

fn readable_encoding_tag(stream: f64) -> Option<i32> {
    let encoding = readable_encoding_value(stream);
    if JSValue::from_bits(encoding.to_bits()).is_any_string() {
        Some(crate::buffer::js_encoding_tag_from_value(encoding))
    } else {
        None
    }
}

/// Byte length of a stream chunk for `push()`'s highWaterMark accounting:
/// the UTF-8 byte length for strings, the byte length for buffers, and `1`
/// (object-mode count) for anything else.
fn chunk_byte_len(chunk: f64) -> usize {
    let jsval = JSValue::from_bits(chunk.to_bits());
    if jsval.is_any_string() {
        let ptr = crate::value::js_get_string_pointer_unified(chunk) as *const crate::StringHeader;
        if !ptr.is_null() && (ptr as usize) >= 0x10000 {
            return unsafe { (*ptr).byte_len as usize };
        }
        return 0;
    }
    let raw = raw_ptr_from_value(chunk);
    if raw >= 0x10000 && crate::buffer::is_registered_buffer(raw) {
        return unsafe { (*(raw as *const crate::buffer::BufferHeader)).length as usize };
    }
    1
}

fn push_chunk_backpressure_result(stream: f64, total: f64) -> f64 {
    let hwm = get_hidden_value(stream, hidden_hwm_key()).unwrap_or_else(|| default_hwm(false));
    if total < hwm {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// `readable.push(chunk)` for the untyped/`as any` object-method path.
extern "C" fn ns_push1(closure: *const ClosureHeader, chunk: f64) -> f64 {
    let stream = readable_push_receiver(closure);
    if get_hidden_value(stream, hidden_readable_flag_key()).is_none() {
        throw_readable_push_invalid_receiver();
    }
    push_chunk(stream, chunk)
}

fn readable_push_receiver(closure: *const ClosureHeader) -> f64 {
    let implicit = crate::object::js_implicit_this_get();
    if implicit.to_bits() != TAG_UNDEFINED {
        return implicit;
    }
    if closure.is_null() {
        return this_value(closure);
    }
    throw_readable_push_invalid_receiver()
}

#[cold]
fn throw_readable_push_invalid_receiver() -> ! {
    crate::node_submodules::diagnostics::throw_type_error_no_code(
        b"Cannot read properties of undefined (reading '_readableState')",
    )
}

fn unshift_chunk(stream: f64, chunk: f64) -> f64 {
    if stream_destroyed(stream) {
        return f64::from_bits(TAG_FALSE);
    }
    let jsval = JSValue::from_bits(chunk.to_bits());
    if jsval.is_null() || jsval.is_undefined() {
        return push_chunk(stream, chunk);
    }
    if has_truthy_hidden(stream, hidden_end_emitted_key()) {
        destroy_stream(stream, readable_unshift_after_end_error());
        return f64::from_bits(TAG_FALSE);
    }
    let added = chunk_byte_len(chunk) as f64;
    let prev = get_hidden_value(stream, hidden_buffered_key()).unwrap_or(0.0);
    let total = prev + added;
    set_hidden_value(stream, hidden_buffered_key(), total);
    set_hidden_value(stream, hidden_key(b"readableLength"), total);
    if added > 0.0 {
        unshift_readable_buffered_chunk(stream, chunk);
        mark_disturbed(stream);
        schedule_readable_event(stream);
        if readable_is_flowing(stream) {
            consume_readable_buffered_front_for_live_pipe(stream, chunk);
            emit_readable_data(stream, chunk);
        } else {
            unshift_pending_readable_chunk(stream, chunk);
        }
    }
    let hwm = get_hidden_value(stream, hidden_hwm_key()).unwrap_or_else(|| default_hwm(false));
    if total < hwm {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

fn readable_unshift_after_end_error() -> f64 {
    let msg = b"stream.unshift() after end event";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(s, "ERR_STREAM_UNSHIFT_AFTER_END_EVENT");
    let err = crate::error::js_error_new_with_message(s);
    crate::value::js_nanbox_pointer(err as i64)
}

extern "C" fn ns_unshift1(closure: *const ClosureHeader, chunk: f64) -> f64 {
    unshift_chunk(this_value(closure), chunk)
}

/// `readable.compose(stream)` (#1539): the instance-method form of
/// `stream.compose`, routed through the shared compose builder so the same
/// data-flow and error handling paths cover module and prototype calls.
extern "C" fn ns_compose1(closure: *const ClosureHeader, arg: f64) -> f64 {
    build_node_stream_compose(vec![this_value(closure), arg])
}

extern "C" fn ns_pipe2(closure: *const ClosureHeader, dest: f64, options: f64) -> f64 {
    if pipe_destination_is_missing(dest) {
        throw_readable_pipe_missing_destination();
    }
    let stream = this_value(closure);
    pipe_stream_to_destination(stream, dest, pipe_options_end(options))
}
extern "C" fn ns_writable_write_done(closure: *const ClosureHeader, err: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let len = js_closure_get_capture_f64(closure, 1);
    let callback = js_closure_get_capture_f64(closure, 2);
    complete_writable_write(stream, len, callback, err);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_unpipe1(closure: *const ClosureHeader, dest: f64) -> f64 {
    let stream = this_value(closure);
    if dest.to_bits() == TAG_UNDEFINED {
        unpipe_all_destinations(stream);
    } else {
        unpipe_destination(stream, dest);
    }
    stream
}

fn pipe_listener_value(listener: *const ClosureHeader) -> f64 {
    box_pointer(listener as *const u8)
}

fn set_pipe_listener_captures(
    listener: *mut ClosureHeader,
    src: f64,
    dest: f64,
    unpipe: f64,
    error: f64,
    close: f64,
    finish: f64,
) {
    js_closure_set_capture_f64(listener, 0, src);
    js_closure_set_capture_f64(listener, 1, dest);
    js_closure_set_capture_f64(listener, 2, unpipe);
    js_closure_set_capture_f64(listener, 3, error);
    js_closure_set_capture_f64(listener, 4, close);
    js_closure_set_capture_f64(listener, 5, finish);
}

fn cleanup_pipe_listeners_from_closure(closure: *const ClosureHeader) {
    if closure.is_null() {
        return;
    }
    let dest = js_closure_get_capture_f64(closure, 1);
    let unpipe = js_closure_get_capture_f64(closure, 2);
    let error = js_closure_get_capture_f64(closure, 3);
    let close = js_closure_get_capture_f64(closure, 4);
    let finish = js_closure_get_capture_f64(closure, 5);
    let _ = remove_stream_listener_for_event(dest, string_value(b"unpipe"), unpipe);
    let _ = remove_stream_listener_for_event(dest, string_value(b"error"), error);
    let _ = remove_stream_listener_for_event(dest, string_value(b"close"), close);
    let _ = remove_stream_listener_for_event(dest, string_value(b"finish"), finish);
}

extern "C" fn pipe_unpipe_callback(closure: *const ClosureHeader, src: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let expected_src = js_closure_get_capture_f64(closure, 0);
    if src.to_bits() == expected_src.to_bits() {
        cleanup_pipe_listeners_from_closure(closure);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn pipe_error_callback(closure: *const ClosureHeader, _err: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let src = js_closure_get_capture_f64(closure, 0);
    let dest = js_closure_get_capture_f64(closure, 1);
    if !unpipe_destination(src, dest) {
        cleanup_pipe_listeners_from_closure(closure);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn pipe_close_callback(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let src = js_closure_get_capture_f64(closure, 0);
    let dest = js_closure_get_capture_f64(closure, 1);
    if !unpipe_destination(src, dest) {
        cleanup_pipe_listeners_from_closure(closure);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn pipe_finish_callback(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let src = js_closure_get_capture_f64(closure, 0);
    let dest = js_closure_get_capture_f64(closure, 1);
    if !unpipe_destination(src, dest) {
        cleanup_pipe_listeners_from_closure(closure);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn pipe_drain_callback(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let src = js_closure_get_capture_f64(closure, 0);
    let dest = js_closure_get_capture_f64(closure, 1);
    let listener = js_closure_get_capture_f64(closure, 2);
    let _ = remove_stream_listener_for_event(dest, string_value(b"drain"), listener);
    if pipe_destination_contains(src, dest) && !stream_destroyed(src) {
        if stream_hidden_ended(src) && pending_readable_chunk_count(src) == 0 {
            set_readable_flowing(src, f64::from_bits(TAG_TRUE));
            schedule_readable_end(src);
            return f64::from_bits(TAG_UNDEFINED);
        }
        let _ = resume_readable_stream_from_pipe(src);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn pipe_finish_destination_callback(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let dest = js_closure_get_capture_f64(closure, 0);
    if stream_destroyed(dest) || has_truthy_hidden(dest, hidden_finish_emitted_key()) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if writable_length(dest) > 0.0 {
        set_hidden_value(
            dest,
            hidden_stream_pipe_end_pending_key(),
            f64::from_bits(TAG_TRUE),
        );
    } else {
        set_hidden_value(
            dest,
            hidden_stream_pipe_end_pending_key(),
            f64::from_bits(TAG_FALSE),
        );
        finish_stream(dest, None);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn install_pipe_destination_listeners(src: f64, dest: f64) {
    let unpipe = js_closure_alloc(pipe_unpipe_callback as *const u8, 6);
    let error = js_closure_alloc(pipe_error_callback as *const u8, 6);
    let close = js_closure_alloc(pipe_close_callback as *const u8, 6);
    let finish = js_closure_alloc(pipe_finish_callback as *const u8, 6);
    let unpipe_value = pipe_listener_value(unpipe);
    let error_value = pipe_listener_value(error);
    let close_value = pipe_listener_value(close);
    let finish_value = pipe_listener_value(finish);
    set_pipe_listener_captures(
        unpipe,
        src,
        dest,
        unpipe_value,
        error_value,
        close_value,
        finish_value,
    );
    set_pipe_listener_captures(
        error,
        src,
        dest,
        unpipe_value,
        error_value,
        close_value,
        finish_value,
    );
    set_pipe_listener_captures(
        close,
        src,
        dest,
        unpipe_value,
        error_value,
        close_value,
        finish_value,
    );
    set_pipe_listener_captures(
        finish,
        src,
        dest,
        unpipe_value,
        error_value,
        close_value,
        finish_value,
    );
    add_stream_listener_for_event(dest, string_value(b"unpipe"), unpipe_value);
    add_stream_listener_for_event(dest, string_value(b"error"), error_value);
    add_stream_listener_for_event(dest, string_value(b"close"), close_value);
    add_stream_listener_for_event(dest, string_value(b"finish"), finish_value);
}

fn add_pipe_drain_listener(src: f64, dest: f64) {
    let listener = js_closure_alloc(pipe_drain_callback as *const u8, 3);
    let value = pipe_listener_value(listener);
    js_closure_set_capture_f64(listener, 0, src);
    js_closure_set_capture_f64(listener, 1, dest);
    js_closure_set_capture_f64(listener, 2, value);
    add_stream_listener_for_event(dest, string_value(b"drain"), value);
}

fn schedule_pipe_destination_finish(dest: f64) {
    let closure = js_closure_alloc(pipe_finish_destination_callback as *const u8, 1);
    js_closure_set_capture_f64(closure, 0, dest);
    crate::builtins::js_queue_microtask(closure as i64);
}

fn schedule_pipe_destination_finish_check(dest: f64) {
    let closure = js_closure_alloc(pipe_finish_destination_callback as *const u8, 1);
    js_closure_set_capture_f64(closure, 0, dest);
    crate::timer::js_set_immediate_callback(closure as i64);
}

fn request_pipe_destination_finish(dest: f64) {
    if writable_length(dest) > 0.0 {
        set_hidden_value(
            dest,
            hidden_stream_pipe_end_pending_key(),
            f64::from_bits(TAG_TRUE),
        );
        schedule_pipe_destination_finish_check(dest);
    } else {
        schedule_pipe_destination_finish(dest);
    }
}

fn finish_pending_pipe_destination_if_ready(dest: f64) {
    if !has_truthy_hidden(dest, hidden_stream_pipe_end_pending_key()) || writable_length(dest) > 0.0
    {
        return;
    }
    set_hidden_value(
        dest,
        hidden_stream_pipe_end_pending_key(),
        f64::from_bits(TAG_FALSE),
    );
    schedule_pipe_destination_finish(dest);
}

fn pipe_destination_is_missing(dest: f64) -> bool {
    let value = JSValue::from_bits(dest.to_bits());
    value.is_undefined() || value.is_null()
}

extern "C" fn transform_write_callback(closure: *const ClosureHeader, err: f64, value: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let len = js_closure_get_capture_f64(closure, 1);
    let callback = js_closure_get_capture_f64(closure, 2);
    if err.to_bits() != TAG_UNDEFINED && err.to_bits() != TAG_NULL {
        complete_writable_write(stream, len, callback, err);
        destroy_stream(stream, err);
        return f64::from_bits(TAG_UNDEFINED);
    }
    push_callback_value(stream, value);
    complete_writable_write(stream, len, callback, f64::from_bits(TAG_UNDEFINED));
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn transform_flush_callback(closure: *const ClosureHeader, err: f64, value: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let callback = js_closure_get_capture_f64(closure, 1);
    set_hidden_value(
        stream,
        hidden_transform_finishing_key(),
        f64::from_bits(TAG_FALSE),
    );
    if err.to_bits() != TAG_UNDEFINED && err.to_bits() != TAG_NULL {
        destroy_stream(stream, err);
        if is_callable_value(callback) {
            call_listener_args(stream, callback, &[err]);
        }
        return f64::from_bits(TAG_UNDEFINED);
    }
    push_callback_value(stream, value);
    finish_stream(
        stream,
        if is_callable_value(callback) {
            Some(callback)
        } else {
            None
        },
    );
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_write3(closure: *const ClosureHeader, chunk: f64, enc: f64, cb: f64) -> f64 {
    let stream = this_value(closure);
    write_writable_chunk(stream, chunk, enc, cb)
}

extern "C" fn ns_end3(closure: *const ClosureHeader, chunk: f64, encoding: f64, cb: f64) -> f64 {
    let stream = this_value(closure);
    finish_stream_with_args(stream, chunk, encoding, cb);
    stream
}

extern "C" fn ns_cork0(closure: *const ClosureHeader) -> f64 {
    cork_stream(this_value(closure))
}

extern "C" fn ns_uncork0(closure: *const ClosureHeader) -> f64 {
    uncork_stream(this_value(closure))
}

extern "C" fn writable_write_callback_noop(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn invoke_writable_write(stream: f64, chunk: f64, enc: f64, len: f64, callback: f64) {
    if let Some(write) = writable_hidden_write(stream) {
        let cb = js_closure_alloc(ns_writable_write_done as *const u8, 3);
        js_closure_set_capture_f64(cb, 0, stream);
        js_closure_set_capture_f64(cb, 1, len);
        js_closure_set_capture_f64(cb, 2, callback);
        let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
        let args = [chunk, enc, cb_value];
        let prev_this = crate::object::js_implicit_this_set(stream);
        unsafe {
            let _ = crate::closure::js_native_call_value(write, args.as_ptr(), args.len());
        }
        crate::object::js_implicit_this_set(prev_this);
    } else {
        complete_writable_write(stream, len, callback, f64::from_bits(TAG_UNDEFINED));
    }
}

fn invoke_writable_writev(stream: f64, chunks: f64) {
    if let Some(writev) = writable_hidden_writev(stream) {
        let cb = js_closure_alloc(writable_write_callback_noop as *const u8, 0);
        let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
        let args = [chunks, cb_value];
        let prev_this = crate::object::js_implicit_this_set(stream);
        unsafe {
            let _ = crate::closure::js_native_call_value(writev, args.as_ptr(), args.len());
        }
        crate::object::js_implicit_this_set(prev_this);
    }
}

fn writable_write_after_end_error() -> f64 {
    let msg = b"write after end";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(s, "ERR_STREAM_WRITE_AFTER_END");
    let err = crate::error::js_error_new_with_message(s);
    crate::value::js_nanbox_pointer(err as i64)
}

fn readable_default_read_error() -> f64 {
    let msg = b"The _read() method is not implemented";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(s, "ERR_METHOD_NOT_IMPLEMENTED");
    let err = crate::error::js_error_new_with_message(s);
    crate::value::js_nanbox_pointer(err as i64)
}

fn push_callback_value(stream: f64, value: f64) {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_null() && !jsval.is_undefined() {
        let _ = push_chunk(stream, value);
    }
}

fn invoke_transform_write(stream: f64, chunk: f64, enc: f64, len: f64, callback: f64) {
    if has_truthy_hidden(stream, hidden_transform_passthrough_key()) {
        let _ = push_chunk(stream, chunk);
        complete_writable_write(stream, len, callback, f64::from_bits(TAG_UNDEFINED));
        return;
    }
    if let Some(transform) = transform_hidden_callback(stream) {
        let cb = js_closure_alloc(transform_write_callback as *const u8, 3);
        js_closure_set_capture_f64(cb, 0, stream);
        js_closure_set_capture_f64(cb, 1, len);
        js_closure_set_capture_f64(cb, 2, callback);
        let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
        let args = [chunk, enc, cb_value];
        let prev_this = crate::object::js_implicit_this_set(stream);
        unsafe {
            let _ = crate::closure::js_native_call_value(transform, args.as_ptr(), args.len());
        }
        crate::object::js_implicit_this_set(prev_this);
        return;
    }
    emit_writable_chunk(stream, chunk);
    complete_writable_write(stream, len, callback, f64::from_bits(TAG_UNDEFINED));
}

#[cold]
fn throw_writable_null_chunk() -> ! {
    let msg = b"May not write null values to stream";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(s, "ERR_STREAM_NULL_VALUES");
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cold]
fn throw_readable_from_invalid_iterable() -> ! {
    let msg = b"The \"iterable\" argument must be an instance of Iterable.";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(s, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn normalize_write_args(stream: f64, chunk: f64, enc: f64, cb: f64) -> (f64, f64, f64) {
    let (encoding, callback) = if is_callable_value(enc) {
        (f64::from_bits(TAG_UNDEFINED), enc)
    } else {
        (enc, cb)
    };
    let (chunk, encoding) = normalize_writable_write_chunk(stream, chunk, encoding);
    (chunk, encoding, callback)
}

fn normalize_writable_write_chunk(stream: f64, chunk: f64, encoding: f64) -> (f64, f64) {
    let value = JSValue::from_bits(chunk.to_bits());
    if value.is_any_string() {
        let encoding = normalize_writable_string_encoding(stream, encoding);
        if !writable_should_decode_string(stream) {
            return (chunk, encoding);
        }
        let enc_tag = crate::buffer::js_encoding_tag_from_value(encoding);
        let buf = crate::buffer::js_buffer_from_value(chunk.to_bits() as i64, enc_tag);
        return (box_pointer(buf as *const u8), string_value(b"buffer"));
    }
    let raw = raw_ptr_from_value(chunk);
    if raw >= 0x10000 && crate::buffer::is_registered_buffer(raw) {
        return (chunk, string_value(b"buffer"));
    }
    (chunk, encoding)
}

fn normalize_writable_string_encoding(stream: f64, encoding: f64) -> f64 {
    if JSValue::from_bits(encoding.to_bits()).is_any_string() {
        encoding
    } else {
        writable_default_encoding(stream)
    }
}

fn writable_should_decode_string(stream: f64) -> bool {
    !has_truthy_hidden(stream, hidden_writable_object_mode_key())
        && has_truthy_hidden(stream, hidden_writable_decode_strings_key())
}

fn writable_default_encoding(stream: f64) -> f64 {
    get_hidden_value(stream, hidden_writable_default_encoding_key())
        .unwrap_or_else(|| string_value(b"utf8"))
}

fn write_writable_chunk(stream: f64, chunk: f64, enc: f64, cb: f64) -> f64 {
    if stream_hidden_ended(stream) {
        let err = writable_write_after_end_error();
        let _ = emit_stream_event(stream, string_value(b"error"), &[err]);
        return f64::from_bits(TAG_FALSE);
    }
    if JSValue::from_bits(chunk.to_bits()).is_null() {
        throw_writable_null_chunk();
    }
    let (chunk, enc, callback) = normalize_write_args(stream, chunk, enc, cb);
    let len = writable_chunk_len(stream, chunk);
    add_writable_length(stream, len);
    let ret = writable_backpressure_return(stream);
    if writable_corked_count(stream) > 0.0 {
        buffer_writable_write(stream, chunk, enc, len, callback);
    } else if is_transform_stream(stream) {
        invoke_transform_write(stream, chunk, enc, len, callback);
    } else {
        invoke_writable_write(stream, chunk, enc, len, callback);
        emit_writable_chunk(stream, chunk);
    }
    ret
}

fn writable_backpressure_return(stream: f64) -> f64 {
    let len = writable_length(stream);
    let hwm = get_hidden_value(stream, hidden_key(b"writableHighWaterMark")).unwrap_or(16384.0);
    let ok = len < hwm || len == 0.0;
    set_writable_need_drain(stream, !ok);
    f64::from_bits(if ok { TAG_TRUE } else { TAG_FALSE })
}

fn writable_chunk_len(stream: f64, chunk: f64) -> f64 {
    if has_truthy_hidden(stream, hidden_writable_object_mode_key()) {
        1.0
    } else {
        chunk_byte_len(chunk) as f64
    }
}

fn complete_writable_write(stream: f64, len: f64, callback: f64, err: f64) {
    subtract_writable_length(stream, len);
    let has_error = err.to_bits() != TAG_UNDEFINED && err.to_bits() != TAG_NULL;
    if is_callable_value(callback) {
        let arg = if err.to_bits() == TAG_UNDEFINED {
            f64::from_bits(TAG_NULL)
        } else {
            err
        };
        let args = [arg];
        unsafe {
            let _ = crate::closure::js_native_call_value(callback, args.as_ptr(), args.len());
        }
    }
    if has_error {
        destroy_stream(stream, err);
        return;
    }
    if writable_length(stream) == 0.0 {
        let should_emit_drain = writable_need_drain_raw(stream)
            && !stream_hidden_ended(stream)
            && !has_truthy_hidden(stream, hidden_key(b"destroyed"));
        set_writable_need_drain(stream, false);
        if should_emit_drain {
            let _ = emit_stream_event(stream, string_value(b"drain"), &[]);
        }
        finish_pending_pipe_destination_if_ready(stream);
        schedule_pending_writable_finish_if_ready(stream);
    }
}

fn emit_writable_chunk(stream: f64, chunk: f64) {
    // Custom Duplex sinks own readable output by calling push(); the generic
    // Perry fallback auto-echoes only when there is no user write sink.
    if has_truthy_hidden(stream, hidden_key(b"writableCustomSink")) {
        return;
    }
    if has_truthy_hidden(stream, hidden_readable_flag_key()) {
        mark_disturbed(stream);
        if readable_is_flowing(stream) {
            emit_readable_data(stream, chunk);
        } else {
            buffer_pending_readable_chunk(stream, chunk);
        }
    }
}

fn finish_stream(stream: f64, callback: Option<f64>) {
    let pair_peer = get_hidden_value(stream, hidden_key(b"duplexPairPeer"));
    if pair_peer.is_none() {
        mark_stream_ended(stream);
        refresh_readable_aborted_flag(stream);
    }
    mark_writable_ended(stream);
    if pair_peer.is_none()
        && get_hidden_value(stream, hidden_readable_flag_key()).is_none()
        && !has_truthy_hidden(stream, hidden_end_emitted_key())
    {
        set_hidden_value(stream, hidden_end_emitted_key(), f64::from_bits(TAG_TRUE));
        refresh_readable_aborted_flag(stream);
        let _ = emit_stream_event(stream, string_value(b"end"), &[]);
        end_pipe_destinations(stream);
    }
    if writable_length(stream) > 0.0 {
        set_pending_writable_finish_callback(stream, callback);
        return;
    }
    schedule_writable_finish_then_transform_end(stream, callback);
}

fn finish_stream_with_args(stream: f64, chunk: f64, encoding: f64, cb: f64) {
    let (chunk, encoding, callback) = normalize_end_args(chunk, encoding, cb);
    if has_end_chunk(chunk) {
        let _ = write_writable_chunk(stream, chunk, encoding, f64::from_bits(TAG_UNDEFINED));
    }
    flush_writable_buffered(stream);
    if finish_transform_stream(stream, callback) {
        return;
    }
    finish_stream(stream, callback);
}

fn normalize_end_args(chunk: f64, encoding: f64, cb: f64) -> (f64, f64, Option<f64>) {
    if is_callable_value(chunk) {
        return (
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
            Some(chunk),
        );
    }
    if is_callable_value(encoding) {
        return (chunk, f64::from_bits(TAG_UNDEFINED), Some(encoding));
    }
    let callback = if is_callable_value(cb) {
        Some(cb)
    } else {
        None
    };
    (chunk, encoding, callback)
}

fn has_end_chunk(chunk: f64) -> bool {
    let value = JSValue::from_bits(chunk.to_bits());
    !value.is_null() && !value.is_undefined()
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
    if let Some(result) = crate::fs::utf8_stream_emit_value(stream, event, arg) {
        return result;
    }
    let mut args = crate::array::js_array_alloc(0);
    if arg.to_bits() != TAG_UNDEFINED {
        args = crate::array::js_array_push_f64(args, arg);
    }
    emit_stream_event_from_array(stream, event, args)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_emit_args(
    stream_handle: i64,
    event: f64,
    args_ptr: i64,
) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    let first_arg = if args_ptr == 0 {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        let args = args_ptr as *mut crate::array::ArrayHeader;
        if crate::array::js_array_length(args) > 0 {
            f64::from_bits(crate::array::js_array_get(args, 0).bits())
        } else {
            f64::from_bits(TAG_UNDEFINED)
        }
    };
    if let Some(result) = crate::fs::utf8_stream_emit_value(stream, event, first_arg) {
        return result;
    }
    emit_stream_event_from_array(stream, event, args_ptr as *const crate::array::ArrayHeader)
}

/// `readable.push(chunk)` on a typed stream instance (#1539). Tracks the
/// buffered byte count and returns `true` while it stays below the stream's
/// highWaterMark, `false` once it reaches/exceeds it — Node's backpressure
/// signal. The hidden buffered/hwm fields are seeded by `init_readable_state`
/// at construction. Pushing `null`/`undefined` (EOF) returns `false`.
#[no_mangle]
pub extern "C" fn js_node_stream_method_push(stream_handle: i64, chunk: f64) -> f64 {
    push_chunk(stream_value_from_handle(stream_handle), chunk)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_unshift(stream_handle: i64, chunk: f64) -> f64 {
    unshift_chunk(stream_value_from_handle(stream_handle), chunk)
}

/// `stream.readableHighWaterMark` property getter on a typed instance
/// (#1539). Returns the effective readable highWaterMark stored at
/// construction (default 16384).
#[no_mangle]
pub extern "C" fn js_node_stream_method_readable_hwm(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    get_hidden_value(stream, hidden_key(b"readableHighWaterMark")).unwrap_or(16384.0)
}

/// `stream.readableLength` property getter on a typed instance.
#[no_mangle]
pub extern "C" fn js_node_stream_method_readable_length(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    get_hidden_value(stream, hidden_buffered_key()).unwrap_or(0.0)
}

/// `stream.readableObjectMode` property getter on a typed instance.
#[no_mangle]
pub extern "C" fn js_node_stream_method_readable_object_mode(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    get_hidden_value(stream, hidden_key(b"readableObjectMode"))
        .unwrap_or_else(|| f64::from_bits(TAG_FALSE))
}

/// `stream.readable` property getter on a typed readable-side instance.
/// Mirrors `Readable.isReadable(stream)` for the current stub state.
#[no_mangle]
pub extern "C" fn js_node_stream_method_readable(stream_handle: i64) -> f64 {
    js_node_stream_is_readable(stream_value_from_handle(stream_handle))
}

/// `stream.readableEnded` property getter on a typed readable-side instance.
#[no_mangle]
pub extern "C" fn js_node_stream_method_readable_ended(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    if stream_hidden_ended(stream) {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// `stream.readableEncoding` property getter on typed readable-side instances.
#[no_mangle]
pub extern "C" fn js_node_stream_method_readable_encoding(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    if get_hidden_value(stream, hidden_readable_flag_key()).is_none() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    readable_encoding_value(stream)
}

/// `stream.writableHighWaterMark` property getter on a typed instance
/// (#1539).
#[no_mangle]
pub extern "C" fn js_node_stream_method_writable_hwm(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    get_hidden_value(stream, hidden_key(b"writableHighWaterMark")).unwrap_or(16384.0)
}

/// `stream.writableLength` property getter on a typed instance.
#[no_mangle]
pub extern "C" fn js_node_stream_method_writable_length(stream_handle: i64) -> f64 {
    writable_length(stream_value_from_handle(stream_handle))
}

/// `stream.writableNeedDrain` property getter on a typed instance.
#[no_mangle]
pub extern "C" fn js_node_stream_method_writable_need_drain(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    f64::from_bits(if writable_need_drain(stream) {
        TAG_TRUE
    } else {
        TAG_FALSE
    })
}

/// `stream.writableObjectMode` property getter on a typed instance.
#[no_mangle]
pub extern "C" fn js_node_stream_method_writable_object_mode(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    get_hidden_value(stream, hidden_key(b"writableObjectMode"))
        .unwrap_or_else(|| f64::from_bits(TAG_FALSE))
}

/// `stream.readableAborted` property getter on a typed readable-side instance.
#[no_mangle]
pub extern "C" fn js_node_stream_method_readable_aborted(stream_handle: i64) -> f64 {
    readable_aborted_value(stream_value_from_handle(stream_handle))
}

/// `stream.closed` property getter on typed stream instances.
#[no_mangle]
pub extern "C" fn js_node_stream_method_closed(stream_handle: i64) -> f64 {
    get_hidden_value(
        stream_value_from_handle(stream_handle),
        hidden_key(b"closed"),
    )
    .unwrap_or(f64::from_bits(TAG_FALSE))
}

/// `stream.errored` property getter on typed stream instances.
#[no_mangle]
pub extern "C" fn js_node_stream_method_errored(stream_handle: i64) -> f64 {
    readable_hidden_error(stream_value_from_handle(stream_handle))
        .unwrap_or(f64::from_bits(TAG_NULL))
}

/// `stream.readableDidRead` property getter on typed readable-side instances.
#[no_mangle]
pub extern "C" fn js_node_stream_method_readable_did_read(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    f64::from_bits(if has_truthy_hidden(stream, hidden_disturbed_key()) {
        TAG_TRUE
    } else {
        TAG_FALSE
    })
}

/// `stream.writableCorked` property getter on a typed writable-side instance.
#[no_mangle]
pub extern "C" fn js_node_stream_method_writable_corked(stream_handle: i64) -> f64 {
    writable_corked_count(stream_value_from_handle(stream_handle))
}

/// `stream.writable` property getter on typed writable-side instances.
#[no_mangle]
pub extern "C" fn js_node_stream_method_writable(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    if get_hidden_value(stream, hidden_writable_flag_key()).is_none() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let unavailable = stream_hidden_ended(stream) || readable_hidden_error(stream).is_some();
    f64::from_bits(if unavailable { TAG_FALSE } else { TAG_TRUE })
}

/// `stream.writableEnded` property getter on typed writable-side instances.
#[no_mangle]
pub extern "C" fn js_node_stream_method_writable_ended(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    if get_hidden_value(stream, hidden_writable_flag_key()).is_none() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    f64::from_bits(if stream_hidden_ended(stream) {
        TAG_TRUE
    } else {
        TAG_FALSE
    })
}

/// `stream.writableFinished` property getter on typed writable-side instances.
#[no_mangle]
pub extern "C" fn js_node_stream_method_writable_finished(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    if get_hidden_value(stream, hidden_writable_flag_key()).is_none() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    f64::from_bits(if has_truthy_hidden(stream, hidden_finish_emitted_key()) {
        TAG_TRUE
    } else {
        TAG_FALSE
    })
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_allow_half_open(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    get_hidden_value(stream, hidden_key(b"allowHalfOpen"))
        .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_read(stream_handle: i64, n: f64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    mark_disturbed(stream);
    read_stream_with_size_arg(stream, n)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_set_encoding(stream_handle: i64, encoding: f64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    set_visible_readable_encoding(stream, normalize_readable_encoding(encoding));
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_resume(stream_handle: i64) -> f64 {
    resume_readable_stream(stream_value_from_handle(stream_handle))
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_pause(stream_handle: i64) -> f64 {
    pause_readable_stream(stream_value_from_handle(stream_handle))
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_is_paused(stream_handle: i64) -> f64 {
    f64::from_bits(
        if readable_is_paused(stream_value_from_handle(stream_handle)) {
            TAG_TRUE
        } else {
            TAG_FALSE
        },
    )
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_readable_flowing(stream_handle: i64) -> f64 {
    readable_flowing_value(stream_value_from_handle(stream_handle))
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_pipe(stream_handle: i64, dest: f64, options: f64) -> f64 {
    if pipe_destination_is_missing(dest) {
        throw_readable_pipe_missing_destination();
    }
    let stream = stream_value_from_handle(stream_handle);
    pipe_stream_to_destination(stream, dest, pipe_options_end(options))
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_unpipe(stream_handle: i64, dest: f64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    if dest.to_bits() == TAG_UNDEFINED {
        unpipe_all_destinations(stream);
    } else {
        unpipe_destination(stream, dest);
    }
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_write(
    stream_handle: i64,
    chunk: f64,
    enc: f64,
    cb: f64,
) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_write_value(stream, chunk) {
        return result;
    }
    write_writable_chunk(stream, chunk, enc, cb)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_write3(
    stream_handle: i64,
    chunk: f64,
    enc: f64,
    cb: f64,
) -> f64 {
    js_node_stream_method_write(stream_handle, chunk, enc, cb)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_end(stream_handle: i64, chunk: f64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_end_value(stream, chunk) {
        return result;
    }
    finish_stream_with_args(
        stream,
        chunk,
        f64::from_bits(TAG_UNDEFINED),
        f64::from_bits(TAG_UNDEFINED),
    );
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_end3(
    stream_handle: i64,
    chunk: f64,
    encoding: f64,
    cb: f64,
) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_end_value(stream, chunk) {
        return result;
    }
    finish_stream_with_args(stream, chunk, encoding, cb);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_cork(stream_handle: i64) -> f64 {
    cork_stream(stream_value_from_handle(stream_handle))
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_uncork(stream_handle: i64) -> f64 {
    uncork_stream(stream_value_from_handle(stream_handle))
}

// Topical sub-modules split out of this file for the 2000-line file-size
// gate (#1987). Each child does `use super::*` for the shared constants and
// helpers; this file re-globs them with `pub use` so existing call sites keep
// resolving by bare name AND the `pub extern "C"` FFI entry points stay
// reachable at `perry_runtime::node_stream::*` for the other crates (e.g.
// perry-stdlib). Glob re-export caps each item at its own visibility, so the
// `pub(super)` helpers remain crate-internal.
#[path = "node_stream_keys.rs"]
mod keys;
pub use keys::*;

#[path = "node_stream_dispatch.rs"]
mod dispatch;
pub use dispatch::*;

#[path = "node_stream_iter_helpers.rs"]
mod iter_helpers;
pub use iter_helpers::*;

#[path = "node_stream_pipeline.rs"]
mod pipeline;
pub use pipeline::*;

#[path = "node_stream_readwrite.rs"]
mod readwrite;
pub use readwrite::*;

#[path = "node_stream_readable_read.rs"]
mod readable_read;
pub use readable_read::*;

#[path = "node_stream_duplex_methods.rs"]
mod duplex_method_table;
pub use duplex_method_table::*;

#[path = "node_stream_compose_live.rs"]
mod compose_live;
pub use compose_live::*;

#[path = "node_stream_json.rs"]
mod json_stream;
pub use json_stream::*;

#[path = "node_stream_constructors.rs"]
mod constructors;
pub use constructors::*;

#[path = "node_stream_keepalive.rs"]
mod keepalive;

#[path = "node_stream_destroy_state.rs"]
mod destroy_state;

#[cfg(test)]
#[path = "node_stream_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "node_stream_tests_extra.rs"]
mod tests_extra;

#[cfg(test)]
#[path = "node_stream_state_tests.rs"]
mod state_tests;
