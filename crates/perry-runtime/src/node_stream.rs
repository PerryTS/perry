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
mod readable_from_iterator;
mod readable_from_promises;

#[path = "node_stream_event_emitter.rs"]
mod event_emitter;
use event_emitter::{
    add_stream_listener_for_event, call_listener_args, emit_stream_event,
    emit_stream_event_from_array, is_callable_value, ns_event_names, ns_get_max_listeners,
    ns_listener_count, ns_listeners, ns_off2, ns_on2, ns_once2, ns_prepend_listener2,
    ns_prepend_once_listener2, ns_raw_listeners, ns_remove_all_listeners1, ns_remove_listener2,
    ns_set_max_listeners, remove_stream_listener_for_event, stream_listener_count_for_event,
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

/// Whether node:stream's registry holds a listener for `event` on `stream`.
pub(crate) fn has_stream_listeners(stream: f64, event: &[u8]) -> bool {
    event_emitter::has_stream_listeners(stream, string_value(event))
}
// #3049 — `process.setMaxListeners` reuses the EventEmitter setter
// validation (TypeError/RangeError + fractional/Infinity storage).
pub(crate) use event_emitter::validate_max_listeners;
// node EventEmitter instance state (EventEmitter.init) and the prototype defaults.
pub(crate) use event_emitter::{
    init_event_emitter_capture, init_event_emitter_state, install_event_emitter_prototype_state,
    new_events_object,
};
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

// #1540: shape band for the WHATWG web-stream interop stubs returned by
// `Readable/Writable/Duplex.toWeb`. Placed above the Duplex band so it
// can't collide as method sets grow.
const WEB_STREAM_SHAPE_ID: u32 = 0x7FFF_FF20;
const READABLE_CHUNKS_KEY: Slot = Slot::ReadableChunks;
const READABLE_SOURCE_ITERATOR_KEY: Slot = Slot::ReadableSourceIterator;
const READABLE_ERROR_KEY: Slot = Slot::ReadableError;
const READABLE_SIGNAL_KEY: Slot = Slot::ReadableSignal;
const READABLE_READ_KEY: Slot = Slot::ReadableRead;
const READABLE_READ_INVOKED_KEY: Slot = Slot::ReadableReadInvoked;
const READABLE_DEFAULT_READ_ERROR_KEY: Slot = Slot::ReadableDefaultReadError;
const STREAM_DRAIN_SCHEDULED_KEY: Slot = Slot::DrainScheduled;
const STREAM_READABLE_SCHEDULED_KEY: Slot = Slot::ReadableScheduled;
const STREAM_END_SCHEDULED_KEY: Slot = Slot::EndScheduled;
const STREAM_END_EMITTED_KEY: Slot = Slot::EndEmitted;
const STREAM_ENDED_KEY: Slot = Slot::Ended;
/// An emitter's `captureRejections` flag (node's `this[kCapture]`), in its
/// state record, so no own key exists to hide.
const STREAM_CAPTURE_REJECTIONS_KEY: Slot = Slot::CaptureRejections;
const EVENT_EMITTER_ASYNC_RESOURCE_KEY: &[u8] = b"__perryEventEmitterAsyncResource";
const WRITABLE_WRITE_KEY: Slot = Slot::WritableWrite;
const WRITABLE_FINISH_SCHEDULED_KEY: Slot = Slot::FinishScheduled;
const WRITABLE_FINISH_EMITTED_KEY: Slot = Slot::FinishEmitted;
const WRITABLE_CORKED_KEY: Slot = Slot::WritableCorked;
const WRITABLE_BUFFERED_KEY: Slot = Slot::WritableBuffered;
const WRITABLE_LENGTH_KEY: Slot = Slot::WritableLength;
const WRITABLE_NEED_DRAIN_KEY: Slot = Slot::WritableNeedDrain;
const WRITABLE_OBJECT_MODE_KEY: Slot = Slot::WritableObjectMode;
const WRITABLE_DECODE_STRINGS_KEY: Slot = Slot::WritableDecodeStrings;
const WRITABLE_DEFAULT_ENCODING_KEY: Slot = Slot::WritableDefaultEncoding;
const WRITABLE_PENDING_FINISH_CALLBACK_KEY: Slot = Slot::WritablePendingFinishCallback;
const WRITABLE_WRITEV_KEY: Slot = Slot::WritableWritev;
const STREAM_CONSTRUCT_KEY: Slot = Slot::Construct;
const STREAM_DESTROY_KEY: Slot = Slot::Destroy;
const WRITABLE_FINAL_KEY: Slot = Slot::WritableFinal;
const WRITABLE_FINAL_INVOKED_KEY: Slot = Slot::WritableFinalInvoked;
const WRITABLE_FINAL_PENDING_KEY: Slot = Slot::WritableFinalPending;
const TRANSFORM_CALLBACK_KEY: Slot = Slot::TransformCallback;
const TRANSFORM_FLUSH_KEY: Slot = Slot::TransformFlush;
const TRANSFORM_PASSTHROUGH_KEY: Slot = Slot::TransformPassThrough;
const TRANSFORM_FINISHING_KEY: Slot = Slot::TransformFinishing;
/// `end()` ran on a Transform with writes still in flight (node's `ending`).
const TRANSFORM_END_PENDING_KEY: Slot = Slot::TransformEndPending;
/// A Transform (direct, subclass, PassThrough, or a native-payload family).
const TRANSFORM_FLAG_KEY: Slot = Slot::TransformFlag;
// #1534: direction + disturbed bits so the static introspection helpers
// (`Readable.isReadable` / `isDisturbed` / `isErrored`) answer per-stream
// instead of with a uniform stub. Set at construction / on first read.
const READABLE_FLAG_KEY: Slot = Slot::ReadableFlag;
const WRITABLE_FLAG_KEY: Slot = Slot::WritableFlag;
const STREAM_DISTURBED_KEY: Slot = Slot::Disturbed;
// #1539: bytes currently buffered (for `push()`'s highWaterMark return) and
// the effective readable highWaterMark.
const READABLE_BUFFERED_KEY: Slot = Slot::ReadableBuffered;
const READABLE_HWM_KEY: Slot = Slot::ReadableHwm;
const READABLE_PENDING_KEY: Slot = Slot::ReadablePending;
const READABLE_RESUME_SCHEDULED_KEY: Slot = Slot::ReadableResumeScheduled;
const STREAM_PIPES_KEY: Slot = Slot::Pipes;
const READABLE_BASE64_REMAINDER_KEY: Slot = Slot::ReadableBase64Remainder;
/// #9490: the incomplete trailing UTF-8 sequence held between `push()` calls
/// on a `setEncoding("utf8")` readable, mirroring the base64 remainder above.
const READABLE_UTF8_REMAINDER_KEY: Slot = Slot::ReadableUtf8Remainder;
const STREAM_PIPE_NO_END_KEY: Slot = Slot::PipeNoEnd;
const STREAM_PIPE_END_PENDING_KEY: Slot = Slot::PipeEndPending;
const STREAM_AUTO_DESTROY_KEY: Slot = Slot::AutoDestroy;
const STREAM_EMIT_CLOSE_KEY: Slot = Slot::EmitClose;
const STREAM_PIPELINE_CALLBACK_DONE_KEY: Slot = Slot::PipelineCallbackDone;
const STREAM_READABLE_LIVE_PUSH_KEY: Slot = Slot::ReadableLivePush;

use destroy_state::{destroy_stream, ns_destroy1};
pub use destroy_state::{js_node_stream_method_destroy, js_node_stream_method_destroyed};

// ─────────────────────────────────────────────────────────────────
// Stub method bodies. Each receives the closure pointer (slot 0
// holds the host object's NaN-boxed bits cast to i64) plus its
// argument list. Bodies return either `this`, `null`, `true`, or
// `false`, matching the most useful subset of Node's contract for
// chained no-ops.
// ─────────────────────────────────────────────────────────────────

#[inline]
fn this_value(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    // Slot 0 was set by `build_object` to the NaN-boxed bits of the
    // host object value cast to i64; reverse the cast.
    if !closure.is_null() {
        let bits = js_closure_get_capture_ptr(closure, 0) as u64;
        // A `TAG_UNDEFINED` slot-0 marks a *prototype-method value* (e.g.
        // `Stream.prototype.on`, installed by `attach_event_emitter_prototype_methods`)
        // rather than an instance-bound method: it has no fixed receiver and must
        // read the call-site `this` (the method's receiver parameter, which
        // `Function.prototype.call`/`apply` supply), so `Stream.prototype.on.call(streamInstance, ev, fn)`
        // — readable-stream's `Readable.prototype.on` wrapper — registers the
        // listener on the instance, not on the prototype. `build_object` always
        // captures a real object pointer, so existing instance-bound closures
        // never hit this branch.
        if bits != crate::value::TAG_UNDEFINED {
            return constructors::ensure_lazy_stream(f64::from_bits(bits));
        }
    }
    constructors::ensure_lazy_stream(this.as_f64())
}

extern "C" fn ns_chain1(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    _a: f64,
) -> f64 {
    this_value(closure, this)
}
extern "C" fn ns_chain3(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    _a: f64,
    _b: f64,
    _c: f64,
) -> f64 {
    this_value(closure, this)
}

fn call_old_stream_on(old_stream: f64, event: &[u8], listener: *const ClosureHeader) {
    if listener.is_null() {
        return;
    }
    let event = crate::string::js_string_from_bytes(event.as_ptr(), event.len() as u32);
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

extern "C" fn ns_wrap_data(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    chunk: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let _ = push_chunk(stream, chunk);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_wrap_end(closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let _ = push_chunk(stream, f64::from_bits(TAG_NULL));
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_wrap_error(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    destroy_stream(stream, err);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_wrap_close(closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    destroy_stream(stream, f64::from_bits(TAG_UNDEFINED));
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_wrap1(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    old_stream: f64,
) -> f64 {
    let stream = this_value(closure, this);
    let data = js_closure_alloc(crate::fn_info!(ns_wrap_data, 1; with_declared(1)), 1);
    let end = js_closure_alloc(crate::fn_info!(ns_wrap_end, 0; with_declared(0)), 1);
    let error = js_closure_alloc(crate::fn_info!(ns_wrap_error, 1; with_declared(1)), 1);
    let close = js_closure_alloc(crate::fn_info!(ns_wrap_close, 0; with_declared(0)), 1);
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

extern "C" fn ns_readable_from_drain(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream =
        scope.root_nanbox_f64(f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64));
    set_hidden_value(
        stream.get_nanbox_f64(),
        hidden_drain_scheduled_key(),
        f64::from_bits(TAG_FALSE),
    );
    drain_readable_from_events(stream.get_nanbox_f64());
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_readable_event_microtask(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
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
    let _ = emit_stream_event(stream, literal_string_value(b"readable"), &[]);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_readable_end_microtask(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
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

extern "C" fn ns_readable_resume_microtask(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // `resume` listeners can collect before the flush (#11828).
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream =
        scope.root_nanbox_f64(f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64));
    let s = || stream.get_nanbox_f64();
    set_hidden_value(
        s(),
        hidden_readable_resume_scheduled_key(),
        f64::from_bits(TAG_FALSE),
    );
    if readable_is_flowing(s()) && !stream_destroyed(s()) {
        let _ = emit_stream_event(s(), literal_string_value(b"resume"), &[]);
        flush_pending_readable_chunks(s());
        schedule_readable_from_drain(s());
        invoke_read_once(s());
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_finished_error_false_close(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
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

extern "C" fn ns_finished_default_completion(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    if closure.is_null() || js_closure_get_capture_f64(closure, 2).to_bits() == TAG_TRUE {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let readable_done = !js_node_stream_has_readable_side(stream)
        || has_truthy_hidden(stream, hidden_end_emitted_key());
    let writable_done = !js_node_stream_has_writable_side(stream)
        || has_truthy_hidden(stream, hidden_finish_emitted_key());
    let closed = has_truthy_hidden(stream, hidden_key(b"closed"));
    let error = readable_hidden_error(stream);
    if error.is_none() && !closed && !(readable_done && writable_done) {
        return f64::from_bits(TAG_UNDEFINED);
    }

    js_closure_set_capture_f64(closure as *mut ClosureHeader, 2, f64::from_bits(TAG_TRUE));
    let callback = js_closure_get_capture_f64(closure, 1);
    if let Some(error) = error {
        call_listener_args(stream, callback, &[error]);
    } else {
        call_listener_args(stream, callback, &[]);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_finished_signal_abort(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
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
            literal_string_value(b"abort"),
            box_pointer(closure as *const u8),
        );
    }
    call_listener_args(stream, callback, &[crate::url::js_abort_error_value()]);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_writable_finish_microtask(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
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
        let _ = emit_stream_event(stream, literal_string_value(b"finish"), &[]);
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

extern "C" fn ns_construct_callback_done(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    if err.to_bits() != TAG_UNDEFINED && err.to_bits() != TAG_NULL {
        destroy_stream(stream, err);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_writable_final_callback_done(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
) -> f64 {
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

extern "C" fn ns_emit_rest(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    rest: f64,
) -> f64 {
    emit_stream_event_from_array(
        this_value(closure, this),
        event,
        raw_ptr_from_value(rest) as *const _,
    )
}
/// A method-call site's miss (`NmEeOps::emit_call`): when `recv`'s shapes
/// resolve the requested name to an ordinary data property holding
/// the emitter `emit` body (every emitter prototype and stream table installs
/// the one body), run it with the call's arguments as they arrived -- the
/// body's rest array would only be unpacked again. `None`, having done
/// nothing, for anything else (an override, an accessor, a receiver the
/// shapes cannot answer for): the caller's ordinary dispatch handles it.
///
/// # Safety
/// `args_ptr` holds `argc` values (or is null with `argc == 0`).
pub(crate) unsafe fn emitter_emit_call(
    recv: f64,
    key: i64,
    name: &[u8],
    args_ptr: *const f64,
    argc: usize,
) -> Option<f64> {
    let value = match event_emitter::shape_method(recv, key, name) {
        Some(value) => JSValue::from_bits(value.to_bits()),
        None => {
            crate::object::native_get::try_data_get_bytes(JSValue::from_bits(recv.to_bits()), name)?
        }
    };
    let bits = value.bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(addr) {
        return None;
    }
    let closure = addr as *const ClosureHeader;
    let info = (*closure).info.as_ref()?;
    if info.code != ns_emit_rest as *const u8 {
        return None;
    }
    let args: &[f64] = if args_ptr.is_null() || argc == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, argc)
    };
    // No allocation precedes the read of the closure's receiver capture; the
    // emit roots everything it holds before it allocates.
    let target = this_value(closure, crate::closure::JsThis::from_f64(recv));
    Some(match args.split_first() {
        Some((event, rest)) => event_emitter::emit_stream_event(target, *event, rest),
        None => event_emitter::emit_stream_event(target, f64::from_bits(TAG_UNDEFINED), &[]),
    })
}

extern "C" fn ns_resume0(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    resume_readable_stream(this_value(closure, this))
}

extern "C" fn ns_pause0(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    pause_readable_stream(this_value(closure, this))
}

extern "C" fn ns_is_paused0(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    f64::from_bits(if readable_is_paused(this_value(closure, this)) {
        TAG_TRUE
    } else {
        TAG_FALSE
    })
}

extern "C" fn ns_async_dispose(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let stream = this_value(closure, this);
    destroy_stream(stream, abort_error());
    resolved_promise(f64::from_bits(TAG_UNDEFINED))
}

extern "C" fn ns_read1(closure: *const ClosureHeader, this: crate::closure::JsThis, n: f64) -> f64 {
    let stream = this_value(closure, this);
    read_stream_with_size_arg(stream, n)
}

extern "C" fn ns_set_encoding1(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    encoding: f64,
) -> f64 {
    let stream = this_value(closure, this);
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
    let chunk = if !readable_object_mode(stream)
        && readable_encoding_tag(stream).is_none()
        && JSValue::from_bits(chunk.to_bits()).is_any_string()
    {
        let mut bytes = Vec::new();
        append_chunk_bytes(chunk, &mut bytes, 0);
        buffer_value_from_bytes(&bytes)
    } else {
        chunk
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
        mark_readable_live_push(stream);
        schedule_readable_event(stream);
        if readable_is_flowing(stream) && !should_defer_initial_data_emit(stream) {
            consume_readable_buffered_front_on_live_emit(stream, chunk);
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
    if encoding == 0 {
        return decode_readable_utf8_chunk(stream, raw);
    }
    Some(buffer_chunk_to_encoded_string(raw, encoding))
}

/// UTF-8 with carry-over (#9490). `buffer_chunk_to_encoded_string` decodes
/// each chunk on its own, so a code point straddling two `push()` calls came
/// out as replacement characters. Hold the incomplete tail in a per-stream
/// hidden field — the same shape the base64 arm above already uses — and
/// re-prefix it onto the next chunk.
fn decode_readable_utf8_chunk(stream: f64, raw: usize) -> Option<f64> {
    let mut bytes = readable_utf8_remainder_bytes(stream);
    append_buffer_bytes(raw, &mut bytes);
    let mut decoder = crate::utf8_stream_decoder::Utf8StreamDecoder::new();
    // Feeding remainder+chunk to a fresh decoder is equivalent to feeding the
    // chunk to a decoder that already held the remainder: the held bytes are
    // by construction the prefix of an incomplete sequence.
    let text = decoder.write(&bytes);
    set_readable_utf8_remainder_bytes(stream, decoder.pending_bytes());
    if text.is_empty() {
        // Whole chunk absorbed into the partial — Node emits no chunk here.
        return None;
    }
    Some(string_value_from_str(&text))
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
    if readable_encoding_tag(stream) == Some(0) {
        // #9490: an incomplete UTF-8 sequence at end-of-stream flushes as a
        // single U+FFFD, in its own final chunk.
        let held = readable_utf8_remainder_bytes(stream);
        set_readable_utf8_remainder_bytes(stream, &[]);
        if !held.is_empty() {
            let mut decoder = crate::utf8_stream_decoder::Utf8StreamDecoder::new();
            let mut flushed = decoder.write(&held);
            flushed.push_str(&decoder.end(None));
            if !flushed.is_empty() {
                append_readable_output_chunk(stream, string_value_from_str(&flushed));
            }
        }
        return;
    }
    if readable_encoding_tag(stream) != Some(2) {
        return;
    }
    let bytes = readable_base64_remainder_bytes(stream);
    set_readable_base64_remainder_bytes(stream, &[]);
    if !bytes.is_empty() {
        append_readable_output_chunk(stream, encoded_string_from_bytes(&bytes, 2));
    }
}

fn string_value_from_str(text: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn readable_utf8_remainder_bytes(stream: f64) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(value) = get_hidden_value(stream, hidden_readable_utf8_remainder_key()) {
        append_buffer_bytes(raw_ptr_from_value(value), &mut bytes);
    }
    bytes
}

fn set_readable_utf8_remainder_bytes(stream: f64, bytes: &[u8]) {
    let value = if bytes.is_empty() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        buffer_value_from_bytes(bytes)
    };
    set_hidden_value(stream, hidden_readable_utf8_remainder_key(), value);
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
        return crate::buffer::bytes::no_gc(|scope| {
            crate::buffer::bytes::bytes(chunk, scope).map_or(0, |bytes| bytes.len())
        });
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
extern "C" fn ns_push1(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    chunk: f64,
) -> f64 {
    let stream = readable_push_receiver(this, closure);
    if get_hidden_value(stream, hidden_readable_flag_key()).is_none() {
        throw_readable_push_invalid_receiver();
    }
    push_chunk(stream, chunk)
}

fn readable_push_receiver(this: crate::closure::JsThis, closure: *const ClosureHeader) -> f64 {
    let implicit = this.as_f64();
    if implicit.to_bits() != TAG_UNDEFINED {
        return implicit;
    }
    if closure.is_null() {
        return this_value(closure, this);
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
        mark_readable_live_push(stream);
        schedule_readable_event(stream);
        if readable_is_flowing(stream) {
            consume_readable_buffered_front_on_live_emit(stream, chunk);
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

extern "C" fn ns_unshift1(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    chunk: f64,
) -> f64 {
    unshift_chunk(this_value(closure, this), chunk)
}

/// `readable.compose(stream)` (#1539): the instance-method form of
/// `stream.compose`, routed through the shared compose builder so the same
/// data-flow and error handling paths cover module and prototype calls.
extern "C" fn ns_compose1(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    arg: f64,
) -> f64 {
    build_node_stream_compose(vec![this_value(closure, this), arg])
}

extern "C" fn ns_pipe2(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    dest: f64,
    options: f64,
) -> f64 {
    if pipe_destination_is_missing(dest) {
        throw_readable_pipe_missing_destination();
    }
    let stream = this_value(closure, this);
    pipe_stream_to_destination(stream, dest, pipe_options_end(options))
}
extern "C" fn ns_writable_write_done(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let len = js_closure_get_capture_f64(closure, 1);
    let callback = js_closure_get_capture_f64(closure, 2);
    complete_writable_write(stream, len, callback, err);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_unpipe1(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    dest: f64,
) -> f64 {
    let stream = this_value(closure, this);
    if dest.to_bits() == TAG_UNDEFINED {
        unpipe_all_destinations(stream);
    } else {
        unpipe_destination(stream, dest);
    }
    stream
}

mod pipe_listeners;
use pipe_listeners::{
    destination_listener, finish_pipe_destination, pipe_close_callback, pipe_drain_callback,
    pipe_error_callback, pipe_finish_callback, pipe_finish_destination_callback,
    pipe_listener_value, pipe_unpipe_callback, set_pipe_listener_captures,
};

fn install_pipe_destination_listeners(src: f64, dest: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let src = scope.root_nanbox_f64(src);
    let dest = scope.root_nanbox_f64(dest);
    let unpipe = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(pipe_unpipe_callback, 1; with_declared(1)),
        6,
    ));
    let error = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(pipe_error_callback, 1; with_declared(1)),
        6,
    ));
    let close = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(pipe_close_callback, 0; with_declared(0)),
        6,
    ));
    let finish = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(pipe_finish_callback, 0; with_declared(0)),
        6,
    ));
    for listener in [&unpipe, &error, &close, &finish] {
        set_pipe_listener_captures(
            listener.get_raw_mut_ptr(),
            src.get_nanbox_f64(),
            dest.get_nanbox_f64(),
            pipe_listener_value(unpipe.get_raw_const_ptr()),
            pipe_listener_value(error.get_raw_const_ptr()),
            pipe_listener_value(close.get_raw_const_ptr()),
            pipe_listener_value(finish.get_raw_const_ptr()),
        );
    }
    for (event, listener) in [
        (b"unpipe".as_slice(), &unpipe),
        (b"error".as_slice(), &error),
        (b"close".as_slice(), &close),
        (b"finish".as_slice(), &finish),
    ] {
        destination_listener(
            dest.get_nanbox_f64(),
            event,
            pipe_listener_value(listener.get_raw_const_ptr()),
            false,
        );
    }
}

fn add_pipe_drain_listener(src: f64, dest: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let src = scope.root_nanbox_f64(src);
    let dest = scope.root_nanbox_f64(dest);
    let listener = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(pipe_drain_callback, 0; with_declared(0)),
        3,
    ));
    let value = pipe_listener_value(listener.get_raw_const_ptr());
    js_closure_set_capture_f64(listener.get_raw_mut_ptr(), 0, src.get_nanbox_f64());
    js_closure_set_capture_f64(listener.get_raw_mut_ptr(), 1, dest.get_nanbox_f64());
    js_closure_set_capture_f64(listener.get_raw_mut_ptr(), 2, value);
    destination_listener(dest.get_nanbox_f64(), b"drain", value, false);
}

fn schedule_pipe_destination_finish(dest: f64) {
    let closure = js_closure_alloc(
        crate::fn_info!(pipe_finish_destination_callback, 0; with_declared(0)),
        1,
    );
    js_closure_set_capture_f64(closure, 0, dest);
    crate::builtins::js_queue_microtask(closure as i64);
}

fn schedule_pipe_destination_finish_check(dest: f64) {
    let closure = js_closure_alloc(
        crate::fn_info!(pipe_finish_destination_callback, 0; with_declared(0)),
        1,
    );
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
        finish_pipe_destination(dest);
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

extern "C" fn transform_flush_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
    value: f64,
) -> f64 {
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

extern "C" fn ns_write3(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    chunk: f64,
    enc: f64,
    cb: f64,
) -> f64 {
    let stream = this_value(closure, this);
    write_writable_chunk(stream, chunk, enc, cb)
}

extern "C" fn ns_end3(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    chunk: f64,
    encoding: f64,
    cb: f64,
) -> f64 {
    let stream = this_value(closure, this);
    finish_stream_with_args(stream, chunk, encoding, cb);
    stream
}

extern "C" fn ns_cork0(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    cork_stream(this_value(closure, this))
}

extern "C" fn ns_uncork0(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    uncork_stream(this_value(closure, this))
}

extern "C" fn writable_write_callback_noop(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn invoke_writable_write(stream: f64, chunk: f64, enc: f64, len: f64, callback: f64) {
    if let Some(write) = writable_hidden_write(stream) {
        let cb = js_closure_alloc(
            crate::fn_info!(ns_writable_write_done, 1; with_declared(1)),
            3,
        );
        js_closure_set_capture_f64(cb, 0, stream);
        js_closure_set_capture_f64(cb, 1, len);
        js_closure_set_capture_f64(cb, 2, callback);
        let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
        let args = [chunk, enc, cb_value];
        unsafe {
            let _ = crate::closure::native_call_value_this(
                write,
                crate::closure::JsThis::from_f64(stream),
                args.as_ptr(),
                args.len(),
            );
        }
    } else {
        throw_missing_stream_method("The _write() method is not implemented");
    }
}

fn invoke_writable_writev(stream: f64, chunks: f64) {
    if let Some(writev) = writable_hidden_writev(stream) {
        let cb = js_closure_alloc(
            crate::fn_info!(writable_write_callback_noop, 0; with_declared(0)),
            0,
        );
        let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
        let args = [chunks, cb_value];
        unsafe {
            let _ = crate::closure::native_call_value_this(
                writev,
                crate::closure::JsThis::from_f64(stream),
                args.as_ptr(),
                args.len(),
            );
        }
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

#[cold]
fn throw_missing_stream_method(message: &str) -> ! {
    let s = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(s, "ERR_METHOD_NOT_IMPLEMENTED");
    let err = crate::error::js_error_new_with_message(s);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
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
fn throw_readable_from_invalid_iterable(value: f64) -> ! {
    let message = format!(
        "The \"iterable\" argument must be an instance of Iterable. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
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
        return (
            box_pointer(buf as *const u8),
            literal_string_value(b"buffer"),
        );
    }
    let raw = raw_ptr_from_value(chunk);
    if raw >= 0x10000 && crate::buffer::is_registered_buffer(raw) {
        return (chunk, literal_string_value(b"buffer"));
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
        .unwrap_or_else(|| literal_string_value(b"utf8"))
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

fn emit_writable_chunk(stream: f64, chunk: f64) {
    // Custom Duplex sinks own readable output by calling push(); the generic
    // Perry fallback auto-echoes only when there is no user write sink.
    if has_truthy_hidden(stream, Slot::WritableCustomSink) {
        return;
    }
    if has_truthy_hidden(stream, hidden_readable_flag_key()) {
        if readable_is_flowing(stream) {
            emit_readable_data(stream, chunk);
        } else {
            buffer_pending_readable_chunk(stream, chunk);
        }
    }
}

fn finish_stream(stream: f64, callback: Option<f64>) {
    let pair_peer = get_hidden_value(stream, Slot::DuplexPairPeer);
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
        let _ = emit_stream_event(stream, literal_string_value(b"end"), &[]);
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
    // G2: a transform's flush (or a native stream's Final step) runs only once
    // every write has completed, as node's prefinish does.
    if is_transform_stream(stream) && (writable_length(stream) > 0.0 || writable_writing(stream)) {
        // node's `ending`: no more writes, `writableEnded` reads true, but the
        // readable side stays open for the outputs still to come.
        set_visible_writable_ended(stream, true);
        set_visible_writable(stream, false);
        set_pending_writable_finish_callback(stream, callback);
        set_hidden_value(
            stream,
            hidden_transform_end_pending_key(),
            f64::from_bits(TAG_TRUE),
        );
        return;
    }
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
use keys::*;

#[path = "node_stream_dispatch.rs"]
pub(crate) mod dispatch;
pub(crate) use dispatch::*;

#[path = "node_stream_iter_helpers.rs"]
mod iter_helpers;
use iter_helpers::*;

#[path = "node_stream_rooted_values.rs"]
mod rooted_values;
use rooted_values::*;

#[path = "node_stream_pipeline.rs"]
mod pipeline;
use pipeline::*;

#[path = "node_stream_readwrite.rs"]
mod readwrite;
pub use readwrite::*;

#[path = "node_stream_readable_read.rs"]
mod readable_read;
use readable_read::*;

#[path = "node_stream_compose_live.rs"]
mod compose_live;
use compose_live::*;

#[path = "node_stream_state_view.rs"]
mod state_view;
use state_view::*;

#[path = "node_stream_json.rs"]
mod json_stream;
pub(crate) use json_stream::*;

#[path = "node_stream_constructors.rs"]
mod constructors;
pub use constructors::*;

#[path = "node_stream_keepalive.rs"]
mod keepalive;

#[path = "node_stream_destroy_state.rs"]
mod destroy_state;

pub(crate) mod native_hooks;
mod proto_methods;
mod state_record;
#[cfg(test)]
pub(crate) use state_record::{test_read_inert_slot, test_record_slot_bits, test_write_inert_slot};
pub(crate) use state_record::{
    is_stream_record_word, record_alias_word, record_payload_cell, store_record_alias_word,
    store_record_payload_cell,
};
use state_record::*;
pub(crate) use proto_methods::{install_stream_prototype_methods, StreamProto};
mod write_state;
pub use constructors::{init_transform_in_place, init_writable_payload_in_place};
use write_state::*;

#[cfg(test)]
#[path = "node_stream_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "node_stream_tests_extra.rs"]
mod tests_extra;

#[cfg(test)]
#[path = "node_stream_state_tests.rs"]
mod state_tests;

/// `stream.<name>.bind(stream)`: a stream method as a value that keeps its
/// receiver. G1 puts the methods on the prototypes, so a method read off an
/// instance is node's unbound function; tests that call it detached bind it,
/// as JS code must.
#[cfg(test)]
pub(super) fn bound_method(stream: f64, name: &'static [u8]) -> f64 {
    let method = crate::object::js_object_get_field_by_name_f64(
        raw_ptr_from_value(stream) as *const crate::object::ObjectHeader,
        hidden_key(name),
    );
    unsafe { crate::closure::js_function_bind(method, [stream].as_ptr(), 1) }
}

/// Test seams for `gc::tests::native_payload_streams`.
#[cfg(test)]
pub(crate) fn test_append_chunk_bytes(value: f64, out: &mut Vec<u8>) {
    append_chunk_bytes(value, out, 0);
}

#[cfg(test)]
pub(crate) fn test_buffer_value_from_bytes(bytes: &[u8]) -> f64 {
    buffer_value_from_bytes(bytes)
}
