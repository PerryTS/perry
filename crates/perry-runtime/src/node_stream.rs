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
use crate::object::{
    js_object_alloc, js_object_alloc_with_shape, js_object_get_field,
    js_object_get_field_by_name_f64, js_object_set_field, js_object_set_field_by_name,
    ObjectHeader,
};
use crate::value::JSValue;
use std::os::raw::c_int;

mod async_iterator;

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
const READABLE_ERROR_KEY: &[u8] = b"__perryReadableError";
const READABLE_SIGNAL_KEY: &[u8] = b"__perryReadableSignal";
const READABLE_READ_KEY: &[u8] = b"__perryReadableRead";
const READABLE_READ_INVOKED_KEY: &[u8] = b"__perryReadableReadInvoked";
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
const STREAM_PIPELINE_CALLBACK_DONE_KEY: &[u8] = b"__perryStreamPipelineCallbackDone";

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
    }
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
        mark_stream_closed(stream);
        if stream_auto_destroy_enabled(stream) {
            mark_stream_destroyed(stream);
        }
        let _ = emit_stream_event(stream, string_value(b"close"), &[]);
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
    schedule_writable_finish(
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
/// the UTF-16 length for strings, the byte length for buffers, and `1`
/// (object-mode count) for anything else.
fn chunk_byte_len(chunk: f64) -> usize {
    let jsval = JSValue::from_bits(chunk.to_bits());
    if jsval.is_any_string() {
        let ptr = crate::value::js_get_string_pointer_unified(chunk) as *const crate::StringHeader;
        if !ptr.is_null() && (ptr as usize) >= 0x10000 {
            return unsafe { (*ptr).utf16_len as usize };
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
    push_chunk(this_value(closure), chunk)
}

fn unshift_chunk(stream: f64, chunk: f64) -> f64 {
    if stream_destroyed(stream) {
        return f64::from_bits(TAG_FALSE);
    }
    let jsval = JSValue::from_bits(chunk.to_bits());
    if jsval.is_null() || jsval.is_undefined() {
        return push_chunk(stream, chunk);
    }
    if has_truthy_hidden(stream, hidden_ended_key()) {
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

extern "C" fn ns_unshift1(closure: *const ClosureHeader, chunk: f64) -> f64 {
    unshift_chunk(this_value(closure), chunk)
}

/// `readable.compose(stream)` (#1539): the instance-method form of
/// `stream.compose`. Retained-chunk Readables can eagerly compose through a
/// single Transform/PassThrough so downstream iterator helpers still see a
/// readable chunk snapshot; unsupported forms keep the historical shape stub.
extern "C" fn ns_compose1(closure: *const ClosureHeader, arg: f64) -> f64 {
    let source = this_value(closure);
    if let Some(composed) = compose_readable_snapshot(source, arg) {
        return composed;
    }
    js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED))
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
    if stream_destroyed(dest) || has_truthy_hidden(dest, hidden_end_emitted_key()) {
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

#[derive(Clone, Copy)]
struct PipelineOptions {
    end_final: bool,
    signal: Option<f64>,
}

extern "C" fn pipeline_success_callback(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let state = js_closure_get_capture_f64(closure, 0);
    let callback = js_closure_get_capture_f64(closure, 1);
    if !mark_pipeline_callback_called(state) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if is_callable_value(callback) {
        call_listener_args(f64::from_bits(TAG_UNDEFINED), callback, &[]);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn pipeline_error_callback(closure: *const ClosureHeader, err: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let state = js_closure_get_capture_f64(closure, 0);
    let callback = js_closure_get_capture_f64(closure, 1);
    let stages = js_closure_get_capture_f64(closure, 2);
    if !mark_pipeline_callback_called(state) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    destroy_pipeline_stages(stages, err);
    if is_callable_value(callback) {
        call_listener_args(f64::from_bits(TAG_UNDEFINED), callback, &[err]);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn pipeline_close_callback(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stage = js_closure_get_capture_f64(closure, 3);
    if pipeline_stage_already_complete(stage) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let state = js_closure_get_capture_f64(closure, 0);
    let callback = js_closure_get_capture_f64(closure, 1);
    let stages = js_closure_get_capture_f64(closure, 2);
    if !mark_pipeline_callback_called(state) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let err = pipeline_premature_close_error();
    destroy_pipeline_stages(stages, err);
    if is_callable_value(callback) {
        call_listener_args(f64::from_bits(TAG_UNDEFINED), callback, &[err]);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn pipeline_args(args: *const crate::array::ArrayHeader) -> Vec<f64> {
    if args.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(args);
    let mut values = Vec::with_capacity(len as usize);
    for i in 0..len {
        values.push(crate::array::js_array_get_f64(args, i));
    }
    values
}

fn pipeline_array_like_values(value: f64) -> Vec<f64> {
    if !is_array_like_value(value) {
        return Vec::new();
    }
    let arr = raw_ptr_from_value(value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut values = Vec::with_capacity(len as usize);
    for i in 0..len {
        values.push(crate::array::js_array_get_f64(arr, i));
    }
    values
}

fn is_pipeline_stream(value: f64) -> bool {
    get_hidden_value(value, hidden_readable_flag_key()).is_some()
        || get_hidden_value(value, hidden_writable_flag_key()).is_some()
}

fn is_pipeline_options_arg(value: f64) -> bool {
    object_ptr_from_value(value).is_some()
        && !is_pipeline_stream(value)
        && !is_array_like_value(value)
}

fn pipeline_options_from_arg(value: f64) -> PipelineOptions {
    let end_final = get_hidden_value(value, hidden_key(b"end"))
        .map(|v| v.to_bits() != TAG_FALSE)
        .unwrap_or(true);
    PipelineOptions {
        end_final,
        signal: options_signal(value),
    }
}

fn pipe_options_end(value: f64) -> bool {
    get_hidden_value(value, hidden_key(b"end"))
        .map(|v| v.to_bits() != TAG_FALSE)
        .unwrap_or(true)
}

fn normalize_pipeline_source(value: f64, index: usize) -> f64 {
    if index == 0
        && !is_pipeline_stream(value)
        && !is_non_iterable_primitive_for_readable_from(value)
    {
        js_node_stream_readable_from(value)
    } else {
        value
    }
}

fn pipeline_stage_array(stages: &[f64]) -> f64 {
    let mut arr = crate::array::js_array_alloc(stages.len() as u32);
    for stage in stages {
        arr = crate::array::js_array_push_f64(arr, *stage);
    }
    box_pointer(arr as *const u8)
}

fn new_pipeline_callback_state() -> f64 {
    let state = js_object_alloc(0, 0);
    let value = box_pointer(state as *const u8);
    set_hidden_value(
        value,
        hidden_pipeline_callback_done_key(),
        f64::from_bits(TAG_FALSE),
    );
    value
}

fn mark_pipeline_callback_called(state: f64) -> bool {
    if has_truthy_hidden(state, hidden_pipeline_callback_done_key()) {
        return false;
    }
    set_hidden_value(
        state,
        hidden_pipeline_callback_done_key(),
        f64::from_bits(TAG_TRUE),
    );
    true
}

fn destroy_pipeline_stages(stages: f64, err: f64) {
    if !is_array_like_value(stages) {
        return;
    }
    let arr = raw_ptr_from_value(stages) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        destroy_stream(crate::array::js_array_get_f64(arr, i), err);
    }
}

fn pipeline_premature_close_error() -> f64 {
    let msg = b"Premature close";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(s, "ERR_STREAM_PREMATURE_CLOSE");
    let err = crate::error::js_error_new_with_message(s);
    crate::value::js_nanbox_pointer(err as i64)
}

fn pipeline_stage_already_complete(stage: f64) -> bool {
    stream_hidden_ended(stage)
        || has_truthy_hidden(stage, hidden_end_emitted_key())
        || has_truthy_hidden(stage, hidden_finish_emitted_key())
}

fn add_pipeline_callback_listeners(stages: &[f64], callback: f64, options: PipelineOptions) {
    let state = new_pipeline_callback_state();
    let stage_array = pipeline_stage_array(stages);
    let error_event = string_value(b"error");
    let close_event = string_value(b"close");
    for stage in stages {
        let listener = js_closure_alloc(pipeline_error_callback as *const u8, 3);
        js_closure_set_capture_f64(listener, 0, state);
        js_closure_set_capture_f64(listener, 1, callback);
        js_closure_set_capture_f64(listener, 2, stage_array);
        add_stream_listener_for_event(*stage, error_event, box_pointer(listener as *const u8));
        if !pipeline_stage_already_complete(*stage) {
            let close_listener = js_closure_alloc(pipeline_close_callback as *const u8, 4);
            js_closure_set_capture_f64(close_listener, 0, state);
            js_closure_set_capture_f64(close_listener, 1, callback);
            js_closure_set_capture_f64(close_listener, 2, stage_array);
            js_closure_set_capture_f64(close_listener, 3, *stage);
            add_stream_listener_for_event(
                *stage,
                close_event,
                box_pointer(close_listener as *const u8),
            );
        }
        if let Some(signal) = options.signal {
            attach_abort_signal(signal, *stage);
        }
    }

    let success_stage = if !options.end_final && stages.len() >= 2 {
        stages[stages.len() - 2]
    } else {
        stages[stages.len() - 1]
    };
    let success_event = if get_hidden_value(success_stage, hidden_writable_flag_key()).is_some()
        && options.end_final
    {
        string_value(b"finish")
    } else {
        string_value(b"end")
    };
    let success = js_closure_alloc(pipeline_success_callback as *const u8, 2);
    js_closure_set_capture_f64(success, 0, state);
    js_closure_set_capture_f64(success, 1, callback);
    add_stream_listener_for_event(
        success_stage,
        success_event,
        box_pointer(success as *const u8),
    );
}

fn wire_pipeline_pair(src: f64, dest: f64, end_dest: bool) {
    add_pipe_destination(src, dest);
    if !end_dest {
        add_pipe_no_end_destination(src, dest);
    }
    install_pipe_destination_listeners(src, dest);
    let _ = emit_stream_event(dest, string_value(b"pipe"), &[src]);
    set_readable_flowing(src, f64::from_bits(TAG_TRUE));
    let _ = emit_stream_event(src, string_value(b"resume"), &[]);
}

fn pipeline_stage_has_next(value: f64) -> bool {
    let Some(obj) = object_ptr_from_value(value) else {
        return false;
    };
    unsafe {
        own_field_by_key_bytes(obj as *const ObjectHeader, b"next").is_some_and(is_callable_value)
    }
}

fn pipeline_needs_collected_path(stages: &[f64]) -> bool {
    stages.iter().any(|stage| is_callable_value(*stage))
        || stages
            .first()
            .is_some_and(|stage| !is_pipeline_stream(*stage) && pipeline_stage_has_next(*stage))
}

fn pipeline_empty_chunks() -> f64 {
    box_pointer(crate::array::js_array_alloc(0) as *const u8)
}

fn pipeline_single_chunk(value: f64) -> f64 {
    let mut arr = crate::array::js_array_alloc(1);
    arr = crate::array::js_array_push_f64(arr, value);
    box_pointer(arr as *const u8)
}

fn settle_pipeline_value(value: f64) -> Result<f64, f64> {
    let value = crate::promise::adapt_foreign_promise_value(value);
    if crate::promise::js_value_is_promise(value) == 0 {
        return Ok(value);
    }
    let promise = crate::value::js_nanbox_get_pointer(value) as *mut crate::promise::Promise;
    if promise.is_null() {
        return Ok(value);
    }
    for _ in 0..100_000 {
        unsafe {
            if (*promise).state != crate::promise::PromiseState::Pending {
                break;
            }
        }
        if crate::promise::js_promise_run_microtasks() == 0 {
            break;
        }
    }
    unsafe {
        match (*promise).state {
            crate::promise::PromiseState::Fulfilled => Ok((*promise).value),
            crate::promise::PromiseState::Rejected => Err((*promise).reason),
            crate::promise::PromiseState::Pending => Ok(value),
        }
    }
}

fn catch_pipeline_throw(call: impl FnOnce() -> f64) -> Result<f64, f64> {
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut c_int) };
    if jumped == 0 {
        let value = call();
        crate::exception::js_try_end();
        Ok(value)
    } else {
        let err = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        crate::exception::js_try_end();
        Err(err)
    }
}

fn collect_pipeline_chunks(value: f64) -> Result<f64, f64> {
    let value = settle_pipeline_value(value)?;
    match value.to_bits() {
        TAG_UNDEFINED | TAG_NULL => return Ok(pipeline_empty_chunks()),
        _ => {}
    }
    if let Some(result) = js_node_stream_collect_chunks_result(value) {
        let chunks = result?;
        if pipeline_should_coalesce_chunks(value) {
            return Ok(pipeline_coalesce_chunks(chunks));
        }
        return Ok(chunks);
    }
    let raw = raw_ptr_from_value(value);
    if let Some(chunks) = collection_iterable_chunks(raw) {
        return Ok(chunks);
    }
    if let Some(chunks) = collect_pipeline_iterator_chunks(value)? {
        return Ok(chunks);
    }
    if object_ptr_from_value(value).is_some() {
        let collected = crate::promise::js_array_from_async(value);
        let settled = settle_pipeline_value(collected)?;
        if is_array_like_value(settled) {
            return Ok(settled);
        }
    }
    if is_single_chunk_value(value) {
        return Ok(pipeline_single_chunk(value));
    }
    Ok(pipeline_empty_chunks())
}

fn pipeline_should_coalesce_chunks(value: f64) -> bool {
    is_transform_stream(value)
        && !has_truthy_hidden(value, hidden_key(b"readableObjectMode"))
        && !has_truthy_hidden(value, hidden_writable_object_mode_key())
}

fn pipeline_value_to_string(value: f64) -> String {
    let ptr = crate::value::js_jsvalue_to_string(value);
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn pipeline_coalesce_chunks(chunks: f64) -> f64 {
    if !is_array_like_value(chunks) {
        return chunks;
    }
    let arr = raw_ptr_from_value(chunks) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    if len <= 1 {
        return chunks;
    }
    let mut joined = String::new();
    for i in 0..len {
        joined.push_str(&pipeline_value_to_string(crate::array::js_array_get_f64(
            arr, i,
        )));
    }
    let value = string_value(joined.as_bytes());
    pipeline_single_chunk(value)
}

fn pipeline_chunks_vec(chunks: f64) -> Vec<f64> {
    let mut values = Vec::new();
    push_chunk_values(chunks, &mut values, 0);
    values
}

fn pipeline_iterator_result(value: f64) -> Option<(bool, f64)> {
    let obj = object_ptr_from_value(value)?;
    let done = js_object_get_field_by_name_f64(obj as *const ObjectHeader, hidden_key(b"done"));
    let item = js_object_get_field_by_name_f64(obj as *const ObjectHeader, hidden_key(b"value"));
    Some((crate::value::js_is_truthy(done) != 0, item))
}

fn collect_pipeline_iterator_chunks(iterable: f64) -> Result<Option<f64>, f64> {
    if !pipeline_stage_has_next(iterable) {
        return Ok(None);
    }
    let mut out = crate::array::js_array_alloc(0);
    for _ in 0..100_000 {
        let next_result = catch_pipeline_throw(|| unsafe {
            crate::object::js_native_call_method(
                iterable,
                b"next".as_ptr() as *const i8,
                4,
                std::ptr::null(),
                0,
            )
        })?;
        let next_result = settle_pipeline_value(next_result)?;
        let Some((done, value)) = pipeline_iterator_result(next_result) else {
            return Ok(Some(box_pointer(out as *const u8)));
        };
        if done {
            return Ok(Some(box_pointer(out as *const u8)));
        }
        out = crate::array::js_array_push_f64(out, value);
    }
    Ok(Some(box_pointer(out as *const u8)))
}

fn call_pipeline_function_stage(stage: f64, source: f64) -> Result<f64, f64> {
    let args = [source];
    let result = catch_pipeline_throw(|| unsafe {
        crate::closure::js_native_call_value(stage, args.as_ptr(), args.len())
    })?;
    settle_pipeline_value(result)
}

fn write_pipeline_chunks_to_stream(stream: f64, chunks: f64, end_stream: bool) -> Result<(), f64> {
    for chunk in pipeline_chunks_vec(chunks) {
        let _ = write_writable_chunk(
            stream,
            chunk,
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
        if let Some(err) = readable_hidden_error(stream) {
            return Err(err);
        }
    }
    if end_stream {
        finish_stream_with_args(
            stream,
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    if let Some(err) = readable_hidden_error(stream) {
        Err(err)
    } else {
        Ok(())
    }
}

fn fail_collected_pipeline(stages: &[f64], callback: f64, err: f64) {
    for stage in stages {
        if is_pipeline_stream(*stage) {
            destroy_stream(*stage, err);
        }
    }
    if is_callable_value(callback) {
        call_listener_args(f64::from_bits(TAG_UNDEFINED), callback, &[err]);
    }
}

fn complete_collected_pipeline(callback: f64) {
    if is_callable_value(callback) {
        call_listener_args(f64::from_bits(TAG_UNDEFINED), callback, &[]);
    }
}

fn run_collected_pipeline(stages: &[f64], callback: f64, options: PipelineOptions) -> f64 {
    let last = *stages.last().unwrap_or(&f64::from_bits(TAG_UNDEFINED));
    let first = stages[0];
    let mut chunks = if is_callable_value(first) {
        match call_pipeline_function_stage(first, f64::from_bits(TAG_UNDEFINED))
            .and_then(collect_pipeline_chunks)
        {
            Ok(chunks) => chunks,
            Err(err) => {
                fail_collected_pipeline(stages, callback, err);
                return last;
            }
        }
    } else {
        match collect_pipeline_chunks(first) {
            Ok(chunks) => chunks,
            Err(err) => {
                fail_collected_pipeline(stages, callback, err);
                return last;
            }
        }
    };

    for idx in 1..stages.len() {
        let stage = stages[idx];
        let is_last = idx + 1 == stages.len();
        if is_callable_value(stage) {
            match call_pipeline_function_stage(stage, chunks) {
                Ok(result) if is_last => {
                    if pipeline_stage_has_next(result) {
                        if let Err(err) = collect_pipeline_chunks(result) {
                            fail_collected_pipeline(stages, callback, err);
                            return last;
                        }
                    }
                    complete_collected_pipeline(callback);
                    return last;
                }
                Ok(result) => match collect_pipeline_chunks(result) {
                    Ok(next_chunks) => chunks = next_chunks,
                    Err(err) => {
                        fail_collected_pipeline(stages, callback, err);
                        return last;
                    }
                },
                Err(err) => {
                    fail_collected_pipeline(stages, callback, err);
                    return last;
                }
            }
            continue;
        }

        if is_pipeline_stream(stage) {
            let end_stream = options.end_final || !is_last;
            if let Err(err) = write_pipeline_chunks_to_stream(stage, chunks, end_stream) {
                fail_collected_pipeline(stages, callback, err);
                return last;
            }
            if is_last {
                complete_collected_pipeline(callback);
                return last;
            }
            match collect_pipeline_chunks(stage) {
                Ok(next_chunks) => chunks = next_chunks,
                Err(err) => {
                    fail_collected_pipeline(stages, callback, err);
                    return last;
                }
            }
        } else {
            match collect_pipeline_chunks(stage) {
                Ok(next_chunks) => chunks = next_chunks,
                Err(err) => {
                    fail_collected_pipeline(stages, callback, err);
                    return last;
                }
            }
            if is_last {
                complete_collected_pipeline(callback);
                return last;
            }
        }
    }

    complete_collected_pipeline(callback);
    last
}

fn start_pipeline_readable(stream: f64) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_none() {
        return;
    }
    set_readable_flowing(stream, f64::from_bits(TAG_TRUE));
    flush_pending_readable_chunks(stream);
    invoke_read_once(stream);
    schedule_readable_from_drain(stream);
    if stream_hidden_ended(stream) || has_truthy_hidden(stream, hidden_end_emitted_key()) {
        end_pipe_destinations(stream);
    }
}

#[cold]
fn throw_pipeline_missing_streams() -> ! {
    crate::node_submodules::diagnostics::throw_type_error_no_code(
        b"The \"streams\" argument must be specified",
    )
}

#[cold]
fn throw_pipeline_callback_required() -> ! {
    crate::node_submodules::diagnostics::throw_type_error_no_code(
        b"The \"streams[stream.length - 1]\" property must be of type function",
    )
}

#[cold]
fn throw_readable_pipe_missing_destination() -> ! {
    crate::node_submodules::diagnostics::throw_type_error_no_code(
        b"Cannot read properties of undefined (reading 'on')",
    )
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
    mark_stream_ended(stream);
    refresh_readable_aborted_flag(stream);
    mark_writable_ended(stream);
    if !has_truthy_hidden(stream, hidden_end_emitted_key()) {
        set_hidden_value(stream, hidden_end_emitted_key(), f64::from_bits(TAG_TRUE));
        refresh_readable_aborted_flag(stream);
        let _ = emit_stream_event(stream, string_value(b"end"), &[]);
        end_pipe_destinations(stream);
    }
    if writable_length(stream) > 0.0 {
        set_pending_writable_finish_callback(stream, callback);
        return;
    }
    schedule_writable_finish(stream, callback);
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
    emit_stream_event_from_array(
        stream_value_from_handle(stream_handle),
        event,
        args_ptr as *const crate::array::ArrayHeader,
    )
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

extern "C" fn ns_stream_abort_listener(closure: *const ClosureHeader) -> f64 {
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

fn drain_iter_helper_microtasks() {
    for _ in 0..10_000 {
        if crate::promise::js_promise_run_microtasks() == 0 {
            break;
        }
    }
}

fn prepare_readable_for_iteration(stream: f64) {
    invoke_read_once(stream);
    drain_iter_helper_microtasks();
}

fn extend_compose_output_chunks(
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

fn compose_readable_snapshot(source: f64, stage: f64) -> Option<f64> {
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
fn settle_result(value: f64) -> Result<f64, f64> {
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
fn call_settled_result(cb: *const ClosureHeader, arg: f64) -> Result<f64, f64> {
    settle_result(crate::closure::js_closure_call1(cb, arg))
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

extern "C" fn ns_iter_map(closure: *const ClosureHeader, mapper: f64, opts: f64) -> f64 {
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

extern "C" fn ns_iter_filter(closure: *const ClosureHeader, predicate: f64, opts: f64) -> f64 {
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

extern "C" fn ns_iter_reduce(
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
        // Node throws "Reduce of empty stream with no initial value";
        // the stub resolves undefined rather than crash.
        return settle_consuming(this, opts, f64::from_bits(TAG_UNDEFINED));
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

extern "C" fn ns_iter_for_each(closure: *const ClosureHeader, action: f64, opts: f64) -> f64 {
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

extern "C" fn ns_iter_find(closure: *const ClosureHeader, predicate: f64, opts: f64) -> f64 {
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

extern "C" fn ns_iter_some(closure: *const ClosureHeader, predicate: f64, opts: f64) -> f64 {
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

extern "C" fn ns_iter_every(closure: *const ClosureHeader, predicate: f64, opts: f64) -> f64 {
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

extern "C" fn ns_iter_flat_map(closure: *const ClosureHeader, mapper: f64, opts: f64) -> f64 {
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
fn flatten_async_iterable_value(value: f64) -> Option<*mut crate::array::ArrayHeader> {
    use crate::array::{
        async_iterator_to_array_for_flat_map, call_symbol_async_iterator_for_flat_map,
        has_iterator_next,
    };
    use crate::symbol::js_get_iterator;
    if let Some(async_iter) = call_symbol_async_iterator_for_flat_map(value) {
        return Some(async_iterator_to_array_for_flat_map(async_iter));
    }
    if has_iterator_next(value) {
        // Async generator step values may be already-settled promises that
        // `async_iterator_to_array_for_flat_map` unwraps; drive the same
        // helper for a bare-iterator receiver too — `js_async_iterator_to_array`
        // is a strict superset of `js_iterator_to_array` (it transparently
        // returns non-promise step results unchanged).
        return Some(async_iterator_to_array_for_flat_map(value));
    }
    let sync_iter = js_get_iterator(value);
    if sync_iter.to_bits() != value.to_bits() {
        return Some(async_iterator_to_array_for_flat_map(sync_iter));
    }
    None
}

extern "C" fn ns_iter_take(closure: *const ClosureHeader, count: f64) -> f64 {
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

extern "C" fn ns_iter_drop(closure: *const ClosureHeader, count: f64) -> f64 {
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

    let mut on_method: Option<JSValue> = None;
    for (i, (name, func)) in methods.iter().enumerate() {
        if *name == "addListener" {
            if let Some(val) = on_method {
                js_object_set_field(obj, i as u32, val);
                continue;
            }
        }
        let closure = js_closure_alloc(*func as *const u8, 1);
        // Reuse `set_capture_ptr` (i64 payload). We only need 64 bits
        // and the NaN-boxed pattern fits cleanly when reinterpreted.
        crate::closure::js_closure_set_capture_ptr(closure, 0, this_bits as i64);
        let val = JSValue::pointer(closure as *const u8);
        if *name == "on" {
            on_method = Some(val);
        }
        js_object_set_field(obj, i as u32, val);
    }
    obj
}

fn install_methods_on_existing_object(
    obj: *mut ObjectHeader,
    this_value: f64,
    methods: &[(&str, StubFn)],
    skip_names: &[&str],
) {
    register_stub_arities();
    let this_bits = this_value.to_bits();
    let mut on_method: Option<f64> = None;
    for (name, func) in methods {
        if skip_names.iter().any(|skip| skip == name) {
            continue;
        }
        if *name == "addListener" {
            if let Some(val) = on_method {
                js_object_set_field_by_name(obj, hidden_key(name.as_bytes()), val);
                continue;
            }
        }
        let closure = js_closure_alloc(*func as *const u8, 1);
        crate::closure::js_closure_set_capture_ptr(closure, 0, this_bits as i64);
        let val = f64::from_bits(JSValue::pointer(closure as *const u8).bits());
        if *name == "on" {
            on_method = Some(val);
        }
        js_object_set_field_by_name(obj, hidden_key(name.as_bytes()), val);
    }
}

fn register_stub_arities() {
    let register = |func: *const u8, arity: u32| {
        crate::closure::js_register_closure_arity(func, arity);
    };
    register(ns_chain0 as *const u8, 0);
    register(ns_chain1 as *const u8, 1);
    register(ns_destroy_error_microtask as *const u8, 0);
    register(ns_stream_abort_listener as *const u8, 0);
    register(ns_destroy1 as *const u8, 1);
    register(ns_chain2 as *const u8, 2);
    register(ns_chain3 as *const u8, 3);
    register(ns_on2 as *const u8, 2);
    register(ns_once2 as *const u8, 2);
    register(ns_prepend_listener2 as *const u8, 2);
    register(ns_prepend_once_listener2 as *const u8, 2);
    register(ns_remove_listener2 as *const u8, 2);
    register(ns_off2 as *const u8, 2);
    register(ns_remove_all_listeners1 as *const u8, 1);
    register(ns_readable_from_drain as *const u8, 0);
    register(ns_readable_event_microtask as *const u8, 0);
    register(ns_readable_end_microtask as *const u8, 0);
    register(ns_writable_finish_microtask as *const u8, 0);
    register(ns_construct_callback_done as *const u8, 1);
    register(ns_writable_final_callback_done as *const u8, 1);
    register(ns_capture_rejection as *const u8, 1);
    register(ns_emit2 as *const u8, 2);
    crate::closure::js_register_closure_rest(ns_emit_rest as *const u8, 1);
    register(ns_resume0 as *const u8, 0);
    register(ns_async_dispose as *const u8, 0);
    register(ns_read1 as *const u8, 1);
    register(ns_pipe2 as *const u8, 2);
    register(ns_writable_write_done as *const u8, 1);
    register(pipe_unpipe_callback as *const u8, 1);
    register(pipe_error_callback as *const u8, 1);
    register(pipe_close_callback as *const u8, 0);
    register(pipe_finish_callback as *const u8, 0);
    register(pipe_drain_callback as *const u8, 0);
    register(pipe_finish_destination_callback as *const u8, 0);
    register(writable_write_callback_noop as *const u8, 0);
    register(transform_write_callback as *const u8, 2);
    register(transform_flush_callback as *const u8, 2);
    register(pipeline_success_callback as *const u8, 0);
    register(pipeline_error_callback as *const u8, 1);
    register(pipeline_close_callback as *const u8, 0);
    register(ns_write3 as *const u8, 3);
    register(ns_end3 as *const u8, 3);
    register(ns_cork0 as *const u8, 0);
    register(ns_uncork0 as *const u8, 0);
    register(ns_set_max_listeners as *const u8, 1);
    register(ns_get_max_listeners as *const u8, 0);
    register(ns_event_names as *const u8, 0);
    register(ns_listener_count as *const u8, 1);
    register(ns_listeners as *const u8, 1);
    register(ns_raw_listeners as *const u8, 1);
    register(ns_undefined0 as *const u8, 0);
    register(ns_push1 as *const u8, 1);
    register(ns_unshift1 as *const u8, 1);
    register(ns_compose1 as *const u8, 1);
    register(ns_pause0 as *const u8, 0);
    register(ns_is_paused0 as *const u8, 0);
    register(ns_unpipe1 as *const u8, 1);
    register(ns_readable_resume_microtask as *const u8, 0);
    register(ns_iter_to_array as *const u8, 1);
    register(ns_iter_map as *const u8, 2);
    register(ns_iter_filter as *const u8, 2);
    register(ns_iter_reduce as *const u8, 3);
    register(ns_iter_for_each as *const u8, 2);
    register(ns_iter_find as *const u8, 2);
    register(ns_iter_some as *const u8, 2);
    register(ns_iter_every as *const u8, 2);
    register(ns_iter_flat_map as *const u8, 2);
    register(ns_iter_take as *const u8, 1);
    register(ns_iter_drop as *const u8, 1);
    async_iterator::register_arities();
}

#[inline]
fn box_pointer(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn install_stream_async_dispose_symbol(stream: f64) {
    let async_dispose = crate::symbol::well_known_symbol("asyncDispose");
    if async_dispose.is_null() {
        return;
    }
    let closure = js_closure_alloc(ns_async_dispose as *const u8, 1);
    crate::closure::js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            stream,
            box_pointer(async_dispose as *const u8),
            box_pointer(closure as *const u8),
        );
    }
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
fn hidden_drain_scheduled_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_DRAIN_SCHEDULED_KEY)
}

#[inline]
fn hidden_readable_scheduled_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_READABLE_SCHEDULED_KEY)
}

#[inline]
fn hidden_end_scheduled_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_END_SCHEDULED_KEY)
}

#[inline]
fn hidden_end_emitted_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_END_EMITTED_KEY)
}

#[inline]
fn hidden_ended_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_ENDED_KEY)
}

#[inline]
fn hidden_max_listeners_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_MAX_LISTENERS_KEY)
}

#[inline]
fn hidden_capture_rejections_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_CAPTURE_REJECTIONS_KEY)
}

#[inline]
fn hidden_write_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_WRITE_KEY)
}

#[inline]
fn hidden_finish_scheduled_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_FINISH_SCHEDULED_KEY)
}

#[inline]
fn hidden_finish_emitted_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_FINISH_EMITTED_KEY)
}

#[inline]
fn hidden_writable_corked_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_CORKED_KEY)
}

#[inline]
fn hidden_writable_buffered_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_BUFFERED_KEY)
}

#[inline]
fn hidden_writable_length_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_LENGTH_KEY)
}

#[inline]
fn hidden_writable_need_drain_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_NEED_DRAIN_KEY)
}

#[inline]
fn hidden_writable_object_mode_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_OBJECT_MODE_KEY)
}

#[inline]
fn hidden_writable_decode_strings_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_DECODE_STRINGS_KEY)
}

#[inline]
fn hidden_writable_default_encoding_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_DEFAULT_ENCODING_KEY)
}

#[inline]
fn hidden_writable_pending_finish_callback_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_PENDING_FINISH_CALLBACK_KEY)
}

#[inline]
fn hidden_writev_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_WRITEV_KEY)
}

#[inline]
fn hidden_writable_final_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_FINAL_KEY)
}

#[inline]
fn hidden_writable_final_invoked_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_FINAL_INVOKED_KEY)
}

#[inline]
fn hidden_writable_final_pending_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_FINAL_PENDING_KEY)
}

#[inline]
fn hidden_transform_callback_key() -> *mut crate::string::StringHeader {
    hidden_key(TRANSFORM_CALLBACK_KEY)
}

#[inline]
fn hidden_transform_flush_key() -> *mut crate::string::StringHeader {
    hidden_key(TRANSFORM_FLUSH_KEY)
}

#[inline]
fn hidden_transform_passthrough_key() -> *mut crate::string::StringHeader {
    hidden_key(TRANSFORM_PASSTHROUGH_KEY)
}

#[inline]
fn hidden_transform_finishing_key() -> *mut crate::string::StringHeader {
    hidden_key(TRANSFORM_FINISHING_KEY)
}

#[inline]
fn hidden_readable_flag_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_FLAG_KEY)
}

#[inline]
fn hidden_writable_flag_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_FLAG_KEY)
}

#[inline]
fn hidden_disturbed_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_DISTURBED_KEY)
}

#[inline]
fn hidden_buffered_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_BUFFERED_KEY)
}

#[inline]
fn hidden_hwm_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_HWM_KEY)
}

#[inline]
fn hidden_readable_pending_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_PENDING_KEY)
}

#[inline]
fn hidden_readable_resume_scheduled_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_RESUME_SCHEDULED_KEY)
}

#[inline]
fn hidden_stream_pipes_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_PIPES_KEY)
}

#[inline]
fn hidden_readable_base64_remainder_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_BASE64_REMAINDER_KEY)
}

#[inline]
fn hidden_stream_pipe_no_end_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_PIPE_NO_END_KEY)
}

#[inline]
fn hidden_stream_pipe_end_pending_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_PIPE_END_PENDING_KEY)
}

#[inline]
fn hidden_stream_auto_destroy_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_AUTO_DESTROY_KEY)
}

#[inline]
fn hidden_pipeline_callback_done_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_PIPELINE_CALLBACK_DONE_KEY)
}

#[inline]
fn readable_flowing_key() -> *mut crate::string::StringHeader {
    hidden_key(b"readableFlowing")
}

/// Mark a stream as disturbed (it has been read from / resumed). Backs
/// `Readable.isDisturbed(s)` (#1534).
fn mark_disturbed(stream: f64) {
    set_hidden_value(stream, hidden_disturbed_key(), f64::from_bits(TAG_TRUE));
    set_visible_readable_did_read(stream, true);
}

fn push_json_number(buf: &mut String, value: f64) {
    if value.is_nan() || value.is_infinite() {
        buf.push_str("null");
    } else if value.fract() == 0.0 && value.abs() < (i64::MAX as f64) {
        let mut itoa_buf = itoa::Buffer::new();
        buf.push_str(itoa_buf.format(value as i64));
    } else {
        let mut ryu_buf = ryu::Buffer::new();
        buf.push_str(ryu_buf.format(value));
    }
}

pub(crate) unsafe fn try_stringify_node_stream_json(ptr: *const u8, buf: &mut String) -> bool {
    if ptr.is_null() {
        return false;
    }
    let obj = ptr as *const ObjectHeader;
    let readable = own_field_by_key_bytes(obj, READABLE_FLAG_KEY).is_some();
    let writable = own_field_by_key_bytes(obj, WRITABLE_FLAG_KEY).is_some();
    if readable == writable {
        return false;
    }

    buf.push_str(r#"{"_events":{},"#);
    if readable {
        let hwm =
            own_field_by_key_bytes(obj, READABLE_HWM_KEY).unwrap_or_else(|| default_hwm(false));
        let length = own_field_by_key_bytes(obj, READABLE_BUFFERED_KEY).unwrap_or(0.0);
        buf.push_str(r#""_readableState":{"highWaterMark":"#);
        push_json_number(buf, hwm);
        buf.push_str(r#","buffer":[],"bufferIndex":0,"length":"#);
        push_json_number(buf, length);
        buf.push_str(r#","pipes":[],"awaitDrainWriters":null}}"#);
    } else {
        let hwm = own_field_by_key_bytes(obj, b"writableHighWaterMark")
            .unwrap_or_else(|| default_hwm(false));
        let length = 0.0;
        let corked = own_field_by_key_bytes(obj, WRITABLE_CORKED_KEY).unwrap_or(0.0);
        buf.push_str(r#""_writableState":{"highWaterMark":"#);
        push_json_number(buf, hwm);
        buf.push_str(r#","length":"#);
        push_json_number(buf, length);
        buf.push_str(r#","corked":"#);
        push_json_number(buf, corked);
        buf.push_str(r#","writelen":0,"bufferedIndex":0,"pendingcb":0}}"#);
    }
    true
}

unsafe fn own_field_by_key_bytes(obj: *const ObjectHeader, key: &[u8]) -> Option<f64> {
    if obj.is_null() {
        return None;
    }
    let keys = (*obj).keys_array;
    let keys_ptr = keys as usize;
    if keys.is_null() || keys_ptr < 0x10000 {
        return None;
    }
    if gc_type_for_ptr(keys_ptr) != Some(crate::gc::GC_TYPE_ARRAY) {
        return None;
    }

    let key_count = crate::array::js_array_length(keys) as usize;
    if key_count > 65_536 {
        return None;
    }
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if string_value_eq(f64::from_bits(key_val.bits()), key) {
            let value = js_object_get_field(obj, i as u32);
            return if value.bits() == TAG_UNDEFINED {
                None
            } else {
                Some(f64::from_bits(value.bits()))
            };
        }
    }
    None
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

pub(crate) fn is_classic_stream_instance_value(value: f64) -> bool {
    let Some(obj) = object_ptr_from_value(value) else {
        return false;
    };
    unsafe {
        own_field_by_key_bytes(obj, READABLE_FLAG_KEY).is_some()
            || own_field_by_key_bytes(obj, WRITABLE_FLAG_KEY).is_some()
    }
}

pub(crate) fn is_classic_stream_instance_of(value: f64, constructor_name: &str) -> bool {
    if constructor_name == "Stream" {
        return is_classic_stream_instance_value(value);
    }

    let Some(obj) = object_ptr_from_value(value) else {
        return false;
    };
    let Some(constructor) = (unsafe { own_field_by_key_bytes(obj, b"constructor") }) else {
        return false;
    };
    let Some((module, actual)) =
        (unsafe { crate::object::bound_native_callable_module_and_method(constructor) })
    else {
        return false;
    };
    if module != "stream" {
        return false;
    }
    let actual = actual.as_str();

    match constructor_name {
        "Readable" => matches!(actual, "Readable" | "Duplex" | "Transform" | "PassThrough"),
        "Writable" => matches!(actual, "Writable" | "Duplex" | "Transform" | "PassThrough"),
        "Duplex" => matches!(actual, "Duplex" | "Transform" | "PassThrough"),
        "Transform" => matches!(actual, "Transform" | "PassThrough"),
        "PassThrough" => actual == "PassThrough",
        _ => false,
    }
}

fn set_hidden_value(value: f64, key: *mut crate::string::StringHeader, field_value: f64) {
    if let Some(obj) = object_ptr_from_value(value) {
        js_object_set_field_by_name(obj, key, field_value);
    }
}

fn has_truthy_hidden(stream: f64, key: *mut crate::string::StringHeader) -> bool {
    get_hidden_value(stream, key).is_some_and(|v| crate::value::js_is_truthy(v) != 0)
}

fn stream_destroyed(stream: f64) -> bool {
    has_truthy_hidden(stream, hidden_key(b"destroyed"))
}

fn set_stream_auto_destroy(stream: f64, opts: f64) {
    let enabled = get_hidden_value(opts, hidden_key(b"autoDestroy"))
        .map(|v| v.to_bits() != TAG_FALSE)
        .unwrap_or(true);
    set_hidden_value(
        stream,
        hidden_stream_auto_destroy_key(),
        f64::from_bits(if enabled { TAG_TRUE } else { TAG_FALSE }),
    );
}

fn stream_auto_destroy_enabled(stream: f64) -> bool {
    get_hidden_value(stream, hidden_stream_auto_destroy_key())
        .map(|v| v.to_bits() != TAG_FALSE)
        .unwrap_or(true)
}

fn mark_stream_destroyed(stream: f64) {
    set_hidden_value(stream, hidden_key(b"destroyed"), f64::from_bits(TAG_TRUE));
    refresh_readable_aborted_flag(stream);
}

fn readable_flowing_value(stream: f64) -> f64 {
    get_hidden_value(stream, readable_flowing_key()).unwrap_or(f64::from_bits(TAG_NULL))
}

fn readable_is_flowing(stream: f64) -> bool {
    readable_flowing_value(stream).to_bits() == TAG_TRUE
}

fn readable_is_paused(stream: f64) -> bool {
    readable_flowing_value(stream).to_bits() == TAG_FALSE
}

fn has_writable_side(stream: f64) -> bool {
    get_hidden_value(stream, hidden_writable_flag_key()).is_some()
}

fn should_defer_initial_data_emit(stream: f64) -> bool {
    has_truthy_hidden(stream, hidden_readable_resume_scheduled_key()) && !has_writable_side(stream)
}

fn set_readable_flowing(stream: f64, value: f64) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        set_hidden_value(stream, readable_flowing_key(), value);
    }
}

fn ensure_hidden_array(stream: f64, key: *mut crate::string::StringHeader) -> f64 {
    if let Some(value) = get_hidden_value(stream, key) {
        return value;
    }
    let arr = box_pointer(crate::array::js_array_alloc(0) as *const u8);
    set_hidden_value(stream, key, arr);
    arr
}

fn buffer_pending_readable_chunk(stream: f64, chunk: f64) {
    let pending = ensure_hidden_array(stream, hidden_readable_pending_key());
    let arr = raw_ptr_from_value(pending) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, chunk);
    set_hidden_value(
        stream,
        hidden_readable_pending_key(),
        box_pointer(arr as *const u8),
    );
}

fn pending_readable_chunk_count(stream: f64) -> u32 {
    let pending = ensure_hidden_array(stream, hidden_readable_pending_key());
    let arr = raw_ptr_from_value(pending) as *const crate::array::ArrayHeader;
    crate::array::js_array_length(arr)
}

fn emit_readable_data(stream: f64, chunk: f64) {
    if stream_destroyed(stream) {
        return;
    }
    emit_readable_data_unchecked(stream, chunk);
}

fn emit_readable_data_unchecked(stream: f64, chunk: f64) {
    let _ = emit_stream_event(stream, string_value(b"data"), &[chunk]);
    write_chunk_to_pipe_destinations(stream, chunk);
}

fn flush_pending_readable_chunks(stream: f64) {
    if !readable_is_flowing(stream) || stream_destroyed(stream) {
        return;
    }
    let pending = ensure_hidden_array(stream, hidden_readable_pending_key());
    let arr = raw_ptr_from_value(pending) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    if len == 0 {
        return;
    }
    let mut chunks = Vec::with_capacity(len as usize);
    for i in 0..len {
        chunks.push(crate::array::js_array_get_f64(arr, i));
    }
    set_hidden_value(
        stream,
        hidden_readable_pending_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    for chunk in chunks {
        if !readable_is_flowing(stream) {
            buffer_pending_readable_chunk(stream, chunk);
            continue;
        }
        emit_readable_data_unchecked(stream, chunk);
    }
    if stream_hidden_ended(stream)
        && pending_readable_chunk_count(stream) == 0
        && !readable_is_paused(stream)
        && !stream_destroyed(stream)
    {
        schedule_readable_end(stream);
    }
}

pub(super) fn readable_data_listener_added(stream: f64) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_none() || readable_is_paused(stream)
    {
        return;
    }
    set_readable_flowing(stream, f64::from_bits(TAG_TRUE));
    schedule_readable_resume(stream);
    invoke_read_once(stream);
}

fn schedule_readable_resume(stream: f64) {
    if has_truthy_hidden(stream, hidden_readable_resume_scheduled_key()) {
        return;
    }
    set_hidden_value(
        stream,
        hidden_readable_resume_scheduled_key(),
        f64::from_bits(TAG_TRUE),
    );
    let closure = js_closure_alloc(ns_readable_resume_microtask as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    crate::builtins::js_queue_microtask(closure as i64);
}

fn pause_readable_stream(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() && !readable_is_paused(stream)
    {
        set_readable_flowing(stream, f64::from_bits(TAG_FALSE));
        let _ = emit_stream_event(stream, string_value(b"pause"), &[]);
    }
    stream
}

fn pause_readable_stream_after_unpipe(stream: f64) -> f64 {
    if !stream_hidden_ended(stream) && !has_truthy_hidden(stream, hidden_end_emitted_key()) {
        let _ = pause_readable_stream(stream);
    }
    stream
}

fn resume_readable_stream(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        set_readable_flowing(stream, f64::from_bits(TAG_TRUE));
        mark_disturbed(stream);
        flush_pending_readable_chunks(stream);
        schedule_readable_from_drain(stream);
        if stream_hidden_ended(stream)
            && pending_readable_chunk_count(stream) == 0
            && !readable_is_paused(stream)
        {
            schedule_readable_end(stream);
        }
        schedule_readable_resume(stream);
    }
    stream
}

fn resume_readable_stream_from_pipe(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() && !stream_destroyed(stream) {
        let was_paused = readable_is_paused(stream);
        set_readable_flowing(stream, f64::from_bits(TAG_TRUE));
        mark_disturbed(stream);
        if was_paused {
            let _ = emit_stream_event(stream, string_value(b"resume"), &[]);
        }
        flush_pending_readable_chunks(stream);
        schedule_readable_from_drain(stream);
        if stream_hidden_ended(stream)
            && pending_readable_chunk_count(stream) == 0
            && !readable_is_paused(stream)
        {
            schedule_readable_end(stream);
        }
    }
    stream
}

fn pipe_destinations(stream: f64) -> f64 {
    ensure_hidden_array(stream, hidden_stream_pipes_key())
}

fn pipe_no_end_destinations(stream: f64) -> f64 {
    ensure_hidden_array(stream, hidden_stream_pipe_no_end_key())
}

fn pipe_destination_contains(stream: f64, dest: f64) -> bool {
    let arr_value = pipe_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        if crate::array::js_array_get_f64(arr, i).to_bits() == dest.to_bits() {
            return true;
        }
    }
    false
}

fn pipe_no_end_destination_contains(stream: f64, dest: f64) -> bool {
    let arr_value = pipe_no_end_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        if crate::array::js_array_get_f64(arr, i).to_bits() == dest.to_bits() {
            return true;
        }
    }
    false
}

fn add_pipe_destination(stream: f64, dest: f64) {
    if dest.to_bits() == TAG_UNDEFINED {
        return;
    }
    let arr_value = pipe_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, dest);
    set_hidden_value(
        stream,
        hidden_stream_pipes_key(),
        box_pointer(arr as *const u8),
    );
}

fn add_pipe_no_end_destination(stream: f64, dest: f64) {
    if dest.to_bits() == TAG_UNDEFINED || pipe_no_end_destination_contains(stream, dest) {
        return;
    }
    let arr_value = pipe_no_end_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, dest);
    set_hidden_value(
        stream,
        hidden_stream_pipe_no_end_key(),
        box_pointer(arr as *const u8),
    );
}

fn pipe_stream_to_destination(stream: f64, dest: f64, end_dest: bool) -> f64 {
    add_pipe_destination(stream, dest);
    if !end_dest {
        add_pipe_no_end_destination(stream, dest);
    }
    install_pipe_destination_listeners(stream, dest);
    let _ = emit_stream_event(dest, string_value(b"pipe"), &[stream]);
    set_readable_flowing(stream, f64::from_bits(TAG_TRUE));
    let _ = emit_stream_event(stream, string_value(b"resume"), &[]);
    flush_pending_readable_chunks(stream);
    schedule_readable_from_drain(stream);
    dest
}

fn remove_pipe_no_end_destination_once(stream: f64, dest: f64) -> bool {
    let arr_value = pipe_no_end_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut out = crate::array::js_array_alloc(len.saturating_sub(1));
    let mut found = false;
    for i in 0..len {
        let current = crate::array::js_array_get_f64(arr, i);
        if !found && current.to_bits() == dest.to_bits() {
            found = true;
        } else {
            out = crate::array::js_array_push_f64(out, current);
        }
    }
    if found {
        set_hidden_value(
            stream,
            hidden_stream_pipe_no_end_key(),
            box_pointer(out as *const u8),
        );
    }
    found
}

fn unpipe_destination(stream: f64, dest: f64) -> bool {
    let arr_value = pipe_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut out = crate::array::js_array_alloc(len.saturating_sub(1));
    let mut found = false;
    for i in 0..len {
        let current = crate::array::js_array_get_f64(arr, i);
        if !found && current.to_bits() == dest.to_bits() {
            found = true;
        } else {
            out = crate::array::js_array_push_f64(out, current);
        }
    }
    if found {
        set_hidden_value(
            stream,
            hidden_stream_pipes_key(),
            box_pointer(out as *const u8),
        );
        remove_pipe_no_end_destination_once(stream, dest);
        let _ = emit_stream_event(dest, string_value(b"unpipe"), &[stream]);
        if crate::array::js_array_length(out) == 0 {
            let _ = pause_readable_stream_after_unpipe(stream);
        }
    }
    found
}

fn unpipe_all_destinations(stream: f64) {
    let arr_value = pipe_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut dests = Vec::with_capacity(len as usize);
    for i in 0..len {
        dests.push(crate::array::js_array_get_f64(arr, i));
    }
    set_hidden_value(
        stream,
        hidden_stream_pipes_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    set_hidden_value(
        stream,
        hidden_stream_pipe_no_end_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    let _ = pause_readable_stream_after_unpipe(stream);
    for dest in dests {
        let _ = emit_stream_event(dest, string_value(b"unpipe"), &[stream]);
    }
}

fn write_chunk_to_pipe_destinations(stream: f64, chunk: f64) {
    let arr_value = pipe_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut dests = Vec::with_capacity(len as usize);
    for i in 0..len {
        dests.push(crate::array::js_array_get_f64(arr, i));
    }
    for dest in dests {
        let ret = write_writable_chunk(
            dest,
            chunk,
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
        if ret.to_bits() == TAG_FALSE {
            let _ = pause_readable_stream(stream);
            if writable_length(dest) == 0.0 {
                let _ = resume_readable_stream(stream);
            } else {
                add_pipe_drain_listener(stream, dest);
            }
        }
    }
}

fn end_pipe_destinations(stream: f64) {
    let arr_value = pipe_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut dests = Vec::with_capacity(len as usize);
    for i in 0..len {
        dests.push(crate::array::js_array_get_f64(arr, i));
    }
    for dest in dests {
        if stream_destroyed(dest) || has_truthy_hidden(dest, hidden_end_emitted_key()) {
            continue;
        }
        if pipe_no_end_destination_contains(stream, dest) {
            continue;
        }
        request_pipe_destination_finish(dest);
    }
}

fn schedule_readable_from_drain(stream: f64) {
    if readable_hidden_chunks(stream).is_none()
        || has_truthy_hidden(stream, hidden_drain_scheduled_key())
        || readable_is_paused(stream)
        || stream_destroyed(stream)
    {
        return;
    }
    set_hidden_value(
        stream,
        hidden_drain_scheduled_key(),
        f64::from_bits(TAG_TRUE),
    );
    let closure = js_closure_alloc(ns_readable_from_drain as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    crate::builtins::js_queue_microtask(closure as i64);
}

fn schedule_readable_event(stream: f64) {
    if get_hidden_value(stream, hidden_buffered_key()).unwrap_or(0.0) <= 0.0
        || !readable_chunks_nonempty(stream)
    {
        return;
    }
    queue_readable_event(stream);
}

fn queue_readable_event(stream: f64) {
    if has_truthy_hidden(stream, hidden_readable_scheduled_key())
        || stream_listener_count_for_event(stream, string_value(b"readable")) == 0
    {
        return;
    }
    set_hidden_value(
        stream,
        hidden_readable_scheduled_key(),
        f64::from_bits(TAG_TRUE),
    );
    let closure = js_closure_alloc(ns_readable_event_microtask as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    crate::builtins::js_queue_microtask(closure as i64);
}

fn schedule_readable_end(stream: f64) {
    if has_truthy_hidden(stream, hidden_end_emitted_key())
        || has_truthy_hidden(stream, hidden_end_scheduled_key())
    {
        return;
    }
    set_hidden_value(stream, hidden_end_scheduled_key(), f64::from_bits(TAG_TRUE));
    let closure = js_closure_alloc(ns_readable_end_microtask as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    crate::builtins::js_queue_microtask(closure as i64);
}

fn schedule_writable_finish(stream: f64, callback: Option<f64>) {
    if has_truthy_hidden(stream, hidden_finish_emitted_key())
        || has_truthy_hidden(stream, hidden_finish_scheduled_key())
        || has_truthy_hidden(stream, hidden_writable_final_pending_key())
    {
        return;
    }
    if let Some(final_callback) = writable_hidden_final(stream) {
        if !has_truthy_hidden(stream, hidden_writable_final_invoked_key()) {
            set_hidden_value(
                stream,
                hidden_writable_final_invoked_key(),
                f64::from_bits(TAG_TRUE),
            );
            set_hidden_value(
                stream,
                hidden_writable_final_pending_key(),
                f64::from_bits(TAG_TRUE),
            );
            let cb = js_closure_alloc(ns_writable_final_callback_done as *const u8, 2);
            js_closure_set_capture_f64(cb, 0, stream);
            js_closure_set_capture_f64(
                cb,
                1,
                callback.unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED)),
            );
            let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
            let prev_this = crate::object::js_implicit_this_set(stream);
            unsafe {
                let _ =
                    crate::closure::js_native_call_value(final_callback, [cb_value].as_ptr(), 1);
            }
            crate::object::js_implicit_this_set(prev_this);
            return;
        }
    }
    set_hidden_value(
        stream,
        hidden_finish_scheduled_key(),
        f64::from_bits(TAG_TRUE),
    );
    let closure = js_closure_alloc(ns_writable_finish_microtask as *const u8, 2);
    js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    js_closure_set_capture_ptr(
        closure,
        1,
        callback
            .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
            .to_bits() as i64,
    );
    crate::builtins::js_queue_microtask(closure as i64);
}

fn set_pending_writable_finish_callback(stream: f64, callback: Option<f64>) {
    let value = callback.unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED));
    set_hidden_value(stream, hidden_writable_pending_finish_callback_key(), value);
}

fn take_pending_writable_finish_callback(stream: f64) -> Option<f64> {
    let value = get_hidden_value(stream, hidden_writable_pending_finish_callback_key());
    set_hidden_value(
        stream,
        hidden_writable_pending_finish_callback_key(),
        f64::from_bits(TAG_UNDEFINED),
    );
    value.filter(|v| is_callable_value(*v))
}

fn schedule_pending_writable_finish_if_ready(stream: f64) {
    if !stream_hidden_ended(stream)
        || writable_length(stream) > 0.0
        || has_truthy_hidden(stream, hidden_finish_emitted_key())
        || has_truthy_hidden(stream, hidden_finish_scheduled_key())
    {
        return;
    }
    let callback = take_pending_writable_finish_callback(stream);
    schedule_writable_finish(stream, callback);
}

fn emit_readable_end_once(stream: f64) {
    if !has_truthy_hidden(stream, hidden_end_emitted_key()) {
        if pending_readable_chunk_count(stream) > 0 {
            if !readable_is_paused(stream) {
                flush_pending_readable_chunks(stream);
            }
            if pending_readable_chunk_count(stream) > 0 || readable_is_paused(stream) {
                return;
            }
        } else if readable_is_paused(stream) {
            return;
        }
        set_hidden_value(stream, hidden_end_emitted_key(), f64::from_bits(TAG_TRUE));
        mark_stream_ended(stream);
        refresh_readable_aborted_flag(stream);
        let _ = emit_stream_event(stream, string_value(b"end"), &[]);
        end_pipe_destinations(stream);
        // autoDestroy (default) tears the stream down after 'end'; the
        // destroy microtask marks it closed and emits 'close'. Only when
        // autoDestroy is off do we fall back to the readable-only direct
        // close path (#2302): a Readable-only stream (no writable side)
        // emits 'close' after 'end' so `readable.closed` flips to true once
        // the data is fully consumed. A Duplex defers `close` until BOTH
        // 'end' and 'finish' have fired (handled in the writable-side
        // `ns_end1`). Routing both through one branch avoids a double
        // 'close' emission. Refs node-suite/stream/readable/closed-flag.
        if stream_auto_destroy_enabled(stream) {
            destroy_stream(stream, f64::from_bits(TAG_UNDEFINED));
        } else if get_hidden_value(stream, hidden_writable_flag_key()).is_none() {
            mark_stream_closed(stream);
            let _ = emit_stream_event(stream, string_value(b"close"), &[]);
        }
    }
}

fn push_readable_buffered_chunk(stream: f64, chunk: f64) {
    let existing = readable_hidden_chunks(stream).unwrap_or_else(|| {
        let arr = crate::array::js_array_alloc(0);
        box_pointer(arr as *const u8)
    });
    if !is_array_like_value(existing) {
        return;
    }
    let arr = raw_ptr_from_value(existing) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, chunk);
    set_hidden_value(stream, hidden_chunks_key(), box_pointer(arr as *const u8));
}

fn unshift_readable_buffered_chunk(stream: f64, chunk: f64) {
    let existing = readable_hidden_chunks(stream).unwrap_or_else(|| {
        let arr = crate::array::js_array_alloc(0);
        box_pointer(arr as *const u8)
    });
    if !is_array_like_value(existing) {
        return;
    }
    let arr = raw_ptr_from_value(existing) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_unshift_f64(arr, chunk);
    set_hidden_value(stream, hidden_chunks_key(), box_pointer(arr as *const u8));
}

fn unshift_pending_readable_chunk(stream: f64, chunk: f64) {
    let pending = ensure_hidden_array(stream, hidden_readable_pending_key());
    let arr = raw_ptr_from_value(pending) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_unshift_f64(arr, chunk);
    set_hidden_value(
        stream,
        hidden_readable_pending_key(),
        box_pointer(arr as *const u8),
    );
}

fn clear_readable_buffer(stream: f64) {
    set_hidden_value(
        stream,
        hidden_chunks_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    set_hidden_value(stream, hidden_buffered_key(), 0.0);
    set_hidden_value(stream, hidden_key(b"readableLength"), 0.0);
}

fn read_stream_with_size_arg(stream: f64, size: f64) -> f64 {
    let size_value = JSValue::from_bits(size.to_bits());
    if size_value.is_undefined() || !size_value.is_number() {
        return read_stream_default_size(stream);
    }
    let size = size_value.as_number();
    if size.is_nan() {
        return read_stream_default_size(stream);
    }
    read_stream_exact_size(stream, size.trunc())
}

fn read_stream_default_size(stream: f64) -> f64 {
    invoke_read_once(stream);
    read_stream_available_default(stream)
}

fn read_stream_available_default(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_buffered_key()).unwrap_or(0.0) <= 0.0 {
        if stream_hidden_ended(stream) {
            refresh_readable_aborted_flag(stream);
        }
        return f64::from_bits(TAG_NULL);
    }
    if readable_object_mode(stream) {
        return read_stream_object_mode_chunk(stream);
    }
    let mut values = Vec::new();
    if let Some(chunks) = readable_hidden_chunks(stream) {
        push_chunk_values(chunks, &mut values, 0);
    }
    if values.is_empty() {
        if stream_hidden_ended(stream) {
            refresh_readable_aborted_flag(stream);
        }
        return f64::from_bits(TAG_NULL);
    }
    clear_readable_buffer(stream);
    mark_disturbed(stream);
    if stream_hidden_ended(stream) {
        queue_readable_event(stream);
    }
    let encoded = readable_encoding_tag(stream).is_some();
    if values.len() == 1 {
        if encoded {
            return values[0];
        }
        return string_chunk_to_buffer(values[0]).unwrap_or(values[0]);
    }
    let result = crate::string::js_string_concat_chain(values.as_ptr(), values.len() as i32);
    if encoded {
        return f64::from_bits(JSValue::string_ptr(result).bits());
    }
    box_pointer(crate::buffer::js_buffer_from_string(result, 0) as *const u8)
}

fn read_stream_exact_size(stream: f64, size: f64) -> f64 {
    invoke_read_once(stream);
    if size <= 0.0 {
        return f64::from_bits(TAG_NULL);
    }
    let requested = size as usize;
    let available = get_hidden_value(stream, hidden_buffered_key())
        .unwrap_or(0.0)
        .max(0.0) as usize;
    if available == 0 {
        if stream_hidden_ended(stream) {
            refresh_readable_aborted_flag(stream);
        }
        return f64::from_bits(TAG_NULL);
    }
    if readable_encoding_tag(stream).is_some() {
        return read_stream_available_default(stream);
    }
    if requested > available && !stream_hidden_ended(stream) {
        return f64::from_bits(TAG_NULL);
    }
    if requested >= available {
        return read_stream_available_default(stream);
    }

    let mut bytes = Vec::new();
    if let Some(chunks) = readable_hidden_chunks(stream) {
        append_chunk_bytes(chunks, &mut bytes, 0);
    }
    if bytes.len() <= requested {
        return read_stream_available_default(stream);
    }
    let result = buffer_value_from_bytes(&bytes[..requested]);
    set_readable_buffer_bytes(stream, &bytes[requested..]);
    mark_disturbed(stream);
    result
}

fn set_readable_buffer_bytes(stream: f64, bytes: &[u8]) {
    if bytes.is_empty() {
        clear_readable_buffer(stream);
        return;
    }
    let chunk = buffer_value_from_bytes(bytes);
    let mut arr = crate::array::js_array_alloc(0);
    arr = crate::array::js_array_push_f64(arr, chunk);
    set_hidden_value(stream, hidden_chunks_key(), box_pointer(arr as *const u8));
    let remaining = bytes.len() as f64;
    set_hidden_value(stream, hidden_buffered_key(), remaining);
    set_hidden_value(stream, hidden_key(b"readableLength"), remaining);
}

fn buffer_value_from_bytes(bytes: &[u8]) -> f64 {
    let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
    if !bytes.is_empty() {
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                crate::buffer::buffer_data_mut(buf),
                bytes.len(),
            );
        }
    }
    box_pointer(buf as *const u8)
}

fn read_stream_object_mode_chunk(stream: f64) -> f64 {
    let Some(chunks) = readable_hidden_chunks(stream) else {
        return f64::from_bits(TAG_NULL);
    };
    if !is_array_like_value(chunks) {
        clear_readable_buffer(stream);
        return chunks;
    }
    let arr = raw_ptr_from_value(chunks) as *mut crate::array::ArrayHeader;
    if crate::array::js_array_length(arr) == 0 {
        clear_readable_buffer(stream);
        return f64::from_bits(TAG_NULL);
    }
    let chunk = crate::array::js_array_shift_f64(arr);
    let remaining = crate::array::js_array_length(arr) as f64;
    set_hidden_value(stream, hidden_buffered_key(), remaining);
    set_hidden_value(stream, hidden_key(b"readableLength"), remaining);
    mark_disturbed(stream);
    if stream_hidden_ended(stream) && remaining == 0.0 {
        queue_readable_event(stream);
    }
    chunk
}

fn string_chunk_to_buffer(value: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return None;
    }
    Some(box_pointer(
        crate::buffer::js_buffer_from_string(ptr, 0) as *const u8
    ))
}

fn drain_readable_from_events(stream: f64) {
    if !readable_is_flowing(stream) || stream_destroyed(stream) {
        return;
    }
    let data_event = string_value(b"data");
    if stream_listener_count_for_event(stream, data_event) == 0
        && crate::array::js_array_length(
            raw_ptr_from_value(pipe_destinations(stream)) as *const crate::array::ArrayHeader
        ) == 0
    {
        return;
    }
    if let Some(chunks) = readable_hidden_chunks(stream) {
        let mut values = Vec::new();
        push_chunk_values(chunks, &mut values, 0);
        if !values.is_empty() {
            mark_disturbed(stream);
        }
        let mut emit_destroyed_tail = false;
        for chunk in values {
            if !readable_is_flowing(stream) {
                return;
            }
            if stream_destroyed(stream) {
                if !emit_destroyed_tail {
                    return;
                }
                emit_readable_data_unchecked(stream, chunk);
                return;
            }
            emit_readable_data_unchecked(stream, chunk);
            if stream_destroyed(stream) {
                emit_destroyed_tail = true;
            }
        }
    }
    if !stream_destroyed(stream) {
        emit_readable_end_once(stream);
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

fn readable_object_mode(value: f64) -> bool {
    has_truthy_hidden(value, hidden_key(b"readableObjectMode"))
}

fn readable_chunks_nonempty(stream: f64) -> bool {
    let Some(chunks) = readable_hidden_chunks(stream) else {
        return false;
    };
    if is_array_like_value(chunks) {
        let raw = raw_ptr_from_value(chunks);
        return raw >= 0x10000
            && crate::array::js_array_length(raw as *const crate::array::ArrayHeader) > 0;
    }
    is_single_chunk_value(chunks)
}

fn readable_hidden_error(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_error_key())
}

fn stream_hidden_ended(value: f64) -> bool {
    get_hidden_value(value, hidden_ended_key()).is_some_and(|v| crate::value::js_is_truthy(v) != 0)
}

fn readable_aborted_value(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_none() {
        return f64::from_bits(TAG_FALSE);
    }
    let destroyed = has_truthy_hidden(stream, hidden_key(b"destroyed"));
    let errored = readable_hidden_error(stream).is_some();
    let ended = stream_hidden_ended(stream) || has_truthy_hidden(stream, hidden_end_emitted_key());
    if (destroyed || errored) && !ended {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

fn refresh_readable_aborted_flag(stream: f64) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        set_hidden_value(
            stream,
            hidden_key(b"readableAborted"),
            readable_aborted_value(stream),
        );
    }
}

fn writable_hidden_write(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_write_key())
}

fn writable_hidden_writev(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_writev_key())
}

fn transform_hidden_callback(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_transform_callback_key())
}

fn transform_hidden_flush(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_transform_flush_key())
}

fn writable_hidden_final(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_writable_final_key())
}

fn is_transform_stream(stream: f64) -> bool {
    transform_hidden_callback(stream).is_some()
        || transform_hidden_flush(stream).is_some()
        || has_truthy_hidden(stream, hidden_transform_passthrough_key())
}

fn finish_transform_stream(stream: f64, callback: Option<f64>) -> bool {
    let Some(flush) = transform_hidden_flush(stream) else {
        return false;
    };
    if has_truthy_hidden(stream, hidden_transform_finishing_key()) {
        return true;
    }
    set_hidden_value(
        stream,
        hidden_transform_finishing_key(),
        f64::from_bits(TAG_TRUE),
    );
    let cb = js_closure_alloc(transform_flush_callback as *const u8, 2);
    js_closure_set_capture_f64(cb, 0, stream);
    js_closure_set_capture_f64(
        cb,
        1,
        callback.unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED)),
    );
    let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
    let prev_this = crate::object::js_implicit_this_set(stream);
    unsafe {
        let _ = crate::closure::js_native_call_value(flush, [cb_value].as_ptr(), 1);
    }
    crate::object::js_implicit_this_set(prev_this);
    true
}

fn writable_corked_count(value: f64) -> f64 {
    get_hidden_value(value, hidden_writable_corked_key()).unwrap_or(0.0)
}

fn writable_length(value: f64) -> f64 {
    get_hidden_value(value, hidden_writable_length_key()).unwrap_or(0.0)
}

fn set_writable_length(stream: f64, len: f64) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let len = len.max(0.0);
        set_hidden_value(stream, hidden_writable_length_key(), len);
        set_hidden_value(stream, hidden_key(b"writableLength"), len);
    }
}

fn add_writable_length(stream: f64, len: f64) {
    if len > 0.0 {
        set_writable_length(stream, writable_length(stream) + len);
    }
}

fn subtract_writable_length(stream: f64, len: f64) {
    if len > 0.0 {
        set_writable_length(stream, writable_length(stream) - len);
    }
}

fn writable_need_drain_raw(stream: f64) -> bool {
    has_truthy_hidden(stream, hidden_writable_need_drain_key())
}

fn writable_need_drain(stream: f64) -> bool {
    writable_need_drain_raw(stream)
        && !stream_hidden_ended(stream)
        && !has_truthy_hidden(stream, hidden_key(b"destroyed"))
}

fn set_writable_need_drain(stream: f64, need_drain: bool) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let value = if need_drain { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(
            stream,
            hidden_writable_need_drain_key(),
            f64::from_bits(value),
        );
        set_hidden_value(
            stream,
            hidden_key(b"writableNeedDrain"),
            f64::from_bits(value),
        );
    }
}

fn set_writable_corked_count(stream: f64, count: f64) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let count = count.max(0.0);
        set_hidden_value(stream, hidden_writable_corked_key(), count);
        set_hidden_value(stream, hidden_key(b"writableCorked"), count);
    }
}

fn cork_stream(stream: f64) -> f64 {
    set_writable_corked_count(stream, writable_corked_count(stream) + 1.0);
    f64::from_bits(TAG_UNDEFINED)
}

fn uncork_stream(stream: f64) -> f64 {
    let corked = writable_corked_count(stream);
    if corked > 0.0 {
        set_writable_corked_count(stream, corked - 1.0);
        if corked <= 1.0 {
            flush_writable_buffered(stream);
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn buffered_writable_writes(stream: f64) -> Option<f64> {
    get_hidden_value(stream, hidden_writable_buffered_key())
}

fn buffer_writable_write(stream: f64, chunk: f64, enc: f64, len: f64, callback: f64) {
    let mut buffered = buffered_writable_writes(stream).unwrap_or_else(|| {
        let arr = crate::array::js_array_alloc(0);
        box_pointer(arr as *const u8)
    });
    let arr = raw_ptr_from_value(buffered) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, chunk);
    let arr = crate::array::js_array_push_f64(arr, enc);
    let arr = crate::array::js_array_push_f64(arr, len);
    let arr = crate::array::js_array_push_f64(arr, callback);
    buffered = box_pointer(arr as *const u8);
    set_hidden_value(stream, hidden_writable_buffered_key(), buffered);
}

fn writev_record_chunk(chunk: f64, enc: f64) -> (f64, f64) {
    if JSValue::from_bits(chunk.to_bits()).is_any_string() {
        (chunk, enc)
    } else {
        let raw = raw_ptr_from_value(chunk);
        if raw >= 0x10000 && crate::buffer::is_registered_buffer(raw) {
            (chunk, string_value(b"buffer"))
        } else {
            (chunk, enc)
        }
    }
}

fn build_writev_chunks(buffered: *const crate::array::ArrayHeader, len: u32) -> f64 {
    let mut chunks = crate::array::js_array_alloc(0);
    let mut i = 0;
    while i < len {
        let chunk = crate::array::js_array_get_f64(buffered, i);
        let enc = if i + 1 < len {
            crate::array::js_array_get_f64(buffered, i + 1)
        } else {
            f64::from_bits(TAG_UNDEFINED)
        };
        let (chunk, encoding) = writev_record_chunk(chunk, enc);
        let record = crate::object::js_object_alloc(0, 2);
        js_object_set_field_by_name(record, hidden_key(b"chunk"), chunk);
        js_object_set_field_by_name(record, hidden_key(b"encoding"), encoding);
        chunks = crate::array::js_array_push_f64(chunks, box_pointer(record as *const u8));
        i += 4;
    }
    box_pointer(chunks as *const u8)
}

fn flush_writable_buffered(stream: f64) {
    let Some(buffered) = buffered_writable_writes(stream) else {
        return;
    };
    let raw = raw_ptr_from_value(buffered);
    if raw < 0x10000 {
        return;
    }
    let arr = raw as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    set_hidden_value(
        stream,
        hidden_writable_buffered_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    if len > 4 && writable_hidden_writev(stream).is_some() {
        let chunks = build_writev_chunks(arr, len);
        invoke_writable_writev(stream, chunks);
        let mut i = 0;
        while i < len {
            let chunk = crate::array::js_array_get_f64(arr, i);
            let write_len = if i + 2 < len {
                crate::array::js_array_get_f64(arr, i + 2)
            } else {
                writable_chunk_len(stream, chunk)
            };
            let callback = if i + 3 < len {
                crate::array::js_array_get_f64(arr, i + 3)
            } else {
                f64::from_bits(TAG_UNDEFINED)
            };
            emit_writable_chunk(stream, chunk);
            complete_writable_write(stream, write_len, callback, f64::from_bits(TAG_UNDEFINED));
            i += 4;
        }
        return;
    }
    let mut i = 0;
    while i < len {
        let chunk = crate::array::js_array_get_f64(arr, i);
        let enc = if i + 1 < len {
            crate::array::js_array_get_f64(arr, i + 1)
        } else {
            f64::from_bits(TAG_UNDEFINED)
        };
        let write_len = if i + 2 < len {
            crate::array::js_array_get_f64(arr, i + 2)
        } else {
            writable_chunk_len(stream, chunk)
        };
        let callback = if i + 3 < len {
            crate::array::js_array_get_f64(arr, i + 3)
        } else {
            f64::from_bits(TAG_UNDEFINED)
        };
        invoke_writable_write(stream, chunk, enc, write_len, callback);
        emit_writable_chunk(stream, chunk);
        i += 4;
    }
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

fn writev_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"writev"))
}

fn transform_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"transform"))
}

fn transform_flush_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"flush"))
}

fn construct_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"construct")).filter(|v| is_callable_value(*v))
}

fn destroy_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"destroy")).filter(|v| is_callable_value(*v))
}

fn final_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"final")).filter(|v| is_callable_value(*v))
}

fn install_common_lifecycle_callbacks(stream: f64, opts: f64) {
    if let Some(destroy) = destroy_callback_from_options(opts) {
        set_hidden_value(
            stream,
            hidden_key(STREAM_DESTROY_KEY),
            rebind_callback_this(destroy, stream),
        );
    }
}

fn install_writable_lifecycle_callbacks(stream: f64, opts: f64) {
    if let Some(final_callback) = final_callback_from_options(opts) {
        set_hidden_value(
            stream,
            hidden_writable_final_key(),
            rebind_callback_this(final_callback, stream),
        );
        set_hidden_value(
            stream,
            hidden_writable_final_invoked_key(),
            f64::from_bits(TAG_FALSE),
        );
        set_hidden_value(
            stream,
            hidden_writable_final_pending_key(),
            f64::from_bits(TAG_FALSE),
        );
    }
}

fn invoke_construct_callback(stream: f64, opts: f64) {
    let Some(construct) = construct_callback_from_options(opts) else {
        return;
    };
    let construct = rebind_callback_this(construct, stream);
    set_hidden_value(stream, hidden_key(STREAM_CONSTRUCT_KEY), construct);
    let cb = js_closure_alloc(ns_construct_callback_done as *const u8, 1);
    js_closure_set_capture_f64(cb, 0, stream);
    let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
    let prev_this = crate::object::js_implicit_this_set(stream);
    unsafe {
        let _ = crate::closure::js_native_call_value(construct, [cb_value].as_ptr(), 1);
    }
    crate::object::js_implicit_this_set(prev_this);
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

fn is_non_iterable_primitive_for_readable_from(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    (jsval.is_number() || jsval.is_int32() || jsval.is_bool()) && !jsval.is_any_string()
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

fn typed_uint8array_byte_chunks(raw: usize) -> Option<f64> {
    if crate::typedarray::lookup_typed_array_kind(raw) != Some(crate::typedarray::KIND_UINT8) {
        return None;
    }
    let ta = raw as *const crate::typedarray::TypedArrayHeader;
    let len = crate::typedarray::js_typed_array_length(ta).max(0) as u32;
    let mut out = crate::array::js_array_alloc(len);
    for i in 0..len {
        out = crate::array::js_array_push_f64(
            out,
            crate::typedarray::js_typed_array_get(ta, i as i32),
        );
    }
    Some(box_pointer(out as *const u8))
}

fn collection_iterable_chunks(raw: usize) -> Option<f64> {
    if raw < 0x10000 {
        return None;
    }
    if crate::set::is_registered_set(raw) {
        let chunks = crate::set::js_set_to_array(raw as *const crate::set::SetHeader);
        return Some(box_pointer(chunks as *const u8));
    }
    if crate::map::is_registered_map(raw) {
        let chunks = crate::map::js_map_entries(raw as *const crate::map::MapHeader);
        return Some(box_pointer(chunks as *const u8));
    }
    None
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
    if let Some(chunks) = typed_uint8array_byte_chunks(raw) {
        return chunks;
    }
    if let Some(chunks) = collection_iterable_chunks(raw) {
        return chunks;
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

fn readable_from_options(opts: f64) -> f64 {
    let merged = crate::object::js_object_alloc(0, 2);
    let object_mode = !get_hidden_value(opts, hidden_key(b"objectMode"))
        .is_some_and(|v| v.to_bits() == TAG_FALSE);
    set_hidden_value(
        box_pointer(merged as *const u8),
        hidden_key(b"objectMode"),
        f64::from_bits(if object_mode { TAG_TRUE } else { TAG_FALSE }),
    );
    let hwm = opt_number(opts, b"highWaterMark").unwrap_or(1.0);
    set_hidden_value(
        box_pointer(merged as *const u8),
        hidden_key(b"highWaterMark"),
        hwm,
    );
    box_pointer(merged as *const u8)
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
// shape-cache key when added to its base shape id, so the Readable,
// Writable, and Duplex method tables stay in distinct shape bands.
// ─────────────────────────────────────────────────────────────────

fn readable_methods() -> [(&'static str, StubFn); 39] {
    [
        ("on", cast2(ns_on2)),
        ("once", cast2(ns_once2)),
        ("prependListener", cast2(ns_prepend_listener2)),
        ("prependOnceListener", cast2(ns_prepend_once_listener2)),
        ("off", cast2(ns_off2)),
        ("addListener", cast2(ns_on2)),
        ("removeListener", cast2(ns_remove_listener2)),
        ("removeAllListeners", cast1(ns_remove_all_listeners1)),
        ("emit", cast2(ns_emit_rest)),
        ("setMaxListeners", cast1(ns_set_max_listeners)),
        ("getMaxListeners", cast0(ns_get_max_listeners)),
        ("eventNames", cast0(ns_event_names)),
        ("listenerCount", cast1(ns_listener_count)),
        ("listeners", cast1(ns_listeners)),
        ("rawListeners", cast1(ns_raw_listeners)),
        ("read", cast1(ns_read1)),
        ("pipe", cast2(ns_pipe2)),
        ("unpipe", cast1(ns_unpipe1)),
        ("wrap", cast1(ns_chain1)),
        ("pause", cast0(ns_pause0)),
        ("resume", cast0(ns_resume0)),
        ("destroy", cast1(ns_destroy1)),
        ("setEncoding", cast1(ns_set_encoding1)),
        ("isPaused", cast0(ns_is_paused0)),
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
        ("iterator", cast1(async_iterator::ns_iterator1)),
        // #1539 — push() backpressure return + readable.compose() instance form.
        ("push", cast1(ns_push1)),
        ("unshift", cast1(ns_unshift1)),
        ("compose", cast1(ns_compose1)),
    ]
}

fn writable_methods() -> [(&'static str, StubFn); 22] {
    [
        ("on", cast2(ns_on2)),
        ("once", cast2(ns_once2)),
        ("prependListener", cast2(ns_prepend_listener2)),
        ("prependOnceListener", cast2(ns_prepend_once_listener2)),
        ("off", cast2(ns_off2)),
        ("addListener", cast2(ns_on2)),
        ("removeListener", cast2(ns_remove_listener2)),
        ("removeAllListeners", cast1(ns_remove_all_listeners1)),
        ("emit", cast2(ns_emit_rest)),
        ("setMaxListeners", cast1(ns_set_max_listeners)),
        ("getMaxListeners", cast0(ns_get_max_listeners)),
        ("eventNames", cast0(ns_event_names)),
        ("listenerCount", cast1(ns_listener_count)),
        ("listeners", cast1(ns_listeners)),
        ("rawListeners", cast1(ns_raw_listeners)),
        ("write", cast3(ns_write3)),
        ("end", cast3(ns_end3)),
        ("cork", cast0(ns_cork0)),
        ("uncork", cast0(ns_uncork0)),
        ("destroy", cast1(ns_destroy1)),
        ("setDefaultEncoding", cast1(ns_chain1)),
        ("_write", cast3(ns_chain3)),
    ]
}

fn duplex_methods() -> [(&'static str, StubFn); 32] {
    // Union of readable + writable, deduped (`on/once/off/addListener/
    // removeListener/removeAllListeners/emit/listenerCount/listeners/
    // destroy` appear once each).
    [
        ("on", cast2(ns_on2)),
        ("once", cast2(ns_once2)),
        ("prependListener", cast2(ns_prepend_listener2)),
        ("prependOnceListener", cast2(ns_prepend_once_listener2)),
        ("off", cast2(ns_off2)),
        ("addListener", cast2(ns_on2)),
        ("removeListener", cast2(ns_remove_listener2)),
        ("removeAllListeners", cast1(ns_remove_all_listeners1)),
        ("emit", cast2(ns_emit_rest)),
        ("setMaxListeners", cast1(ns_set_max_listeners)),
        ("getMaxListeners", cast0(ns_get_max_listeners)),
        ("eventNames", cast0(ns_event_names)),
        ("listenerCount", cast1(ns_listener_count)),
        ("listeners", cast1(ns_listeners)),
        ("rawListeners", cast1(ns_raw_listeners)),
        ("read", cast1(ns_read1)),
        ("pipe", cast2(ns_pipe2)),
        ("unpipe", cast1(ns_unpipe1)),
        ("wrap", cast1(ns_chain1)),
        ("pause", cast0(ns_pause0)),
        ("resume", cast0(ns_resume0)),
        ("setEncoding", cast1(ns_set_encoding1)),
        ("isPaused", cast0(ns_is_paused0)),
        ("push", cast1(ns_push1)),
        ("unshift", cast1(ns_unshift1)),
        ("compose", cast1(ns_compose1)),
        ("write", cast3(ns_write3)),
        ("end", cast3(ns_end3)),
        ("cork", cast0(ns_cork0)),
        ("uncork", cast0(ns_uncork0)),
        ("destroy", cast1(ns_destroy1)),
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

/// Coerce a NaN-boxed value to an `f64` if it is numeric (handling both the
/// int32-boxed and double representations). Returns `None` for non-numbers.
fn jsvalue_as_f64(v: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(v.to_bits());
    if jsval.is_int32() {
        Some(jsval.as_int32() as f64)
    } else if jsval.is_number() {
        Some(jsval.as_number())
    } else {
        None
    }
}

/// Read a numeric constructor option (e.g. `highWaterMark`) off the opts
/// object, returning `None` when absent or non-numeric.
fn opt_number(opts: f64, key: &[u8]) -> Option<f64> {
    jsvalue_as_f64(get_hidden_value(opts, hidden_key(key))?)
}

/// Read a string constructor option and preserve the existing JS string value.
fn opt_string_value(opts: f64, key: &[u8]) -> Option<f64> {
    let value = get_hidden_value(opts, hidden_key(key))?;
    if JSValue::from_bits(value.to_bits()).is_any_string() {
        Some(value)
    } else {
        None
    }
}

/// Read a boolean constructor option, returning `true` only when the option
/// is present and truthy.
fn opt_bool(opts: f64, key: &[u8]) -> bool {
    get_hidden_value(opts, hidden_key(key)).is_some_and(|v| crate::value::js_is_truthy(v) != 0)
}

fn resolve_object_mode(opts: f64, specific_object_mode: &[u8]) -> bool {
    opt_bool(opts, specific_object_mode) || opt_bool(opts, b"objectMode")
}

// #1537: the platform-default highWaterMark, settable at runtime via
// `stream.setDefaultHighWaterMark(objectMode, value)`. Node's defaults are
// 65536 bytes for byte streams and 16 for objectMode; both are mutable for
// the lifetime of the process (Perry tracks them per-thread, matching its
// per-thread runtime model). Streams constructed without an explicit
// `highWaterMark` inherit the current default for their mode.
thread_local! {
    static DEFAULT_HWM_BYTE: std::cell::Cell<f64> = const { std::cell::Cell::new(65536.0) };
    static DEFAULT_HWM_OBJECT: std::cell::Cell<f64> = const { std::cell::Cell::new(16.0) };
}

fn default_hwm(object_mode: bool) -> f64 {
    if object_mode {
        DEFAULT_HWM_OBJECT.with(|c| c.get())
    } else {
        DEFAULT_HWM_BYTE.with(|c| c.get())
    }
}

/// Resolve an effective highWaterMark: the direction-specific option
/// (`readableHighWaterMark` / `writableHighWaterMark`) falls back to the
/// generic `highWaterMark`, then to the platform default for the stream's
/// mode (#1537: 65536 for byte streams, 16 for objectMode).
fn resolve_hwm(opts: f64, specific: &[u8], specific_object_mode: &[u8]) -> f64 {
    if let Some(v) = opt_number(opts, specific).or_else(|| opt_number(opts, b"highWaterMark")) {
        return v;
    }
    let object_mode = resolve_object_mode(opts, specific_object_mode);
    default_hwm(object_mode)
}

/// Initialize visible lifecycle flags shared by all stream sides.
fn init_lifecycle_state(stream: f64, opts: f64) {
    set_hidden_value(stream, hidden_key(b"destroyed"), f64::from_bits(TAG_FALSE));
    set_hidden_value(
        stream,
        hidden_capture_rejections_key(),
        f64::from_bits(if opt_bool(opts, b"captureRejections") {
            TAG_TRUE
        } else {
            TAG_FALSE
        }),
    );
    set_visible_closed(stream, false);
}

fn init_constructor(stream: f64, name: &str) {
    let constructor = crate::object::bound_native_callable_export_value("stream", name);
    set_hidden_value(stream, hidden_key(b"constructor"), constructor);
}

fn set_visible_readable(stream: f64, readable: bool) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        let value = if readable { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(stream, hidden_key(b"readable"), f64::from_bits(value));
    }
}

fn set_visible_readable_ended(stream: f64, ended: bool) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        let value = if ended { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(stream, hidden_key(b"readableEnded"), f64::from_bits(value));
    }
}

fn set_visible_readable_did_read(stream: f64, did_read: bool) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        let value = if did_read { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(
            stream,
            hidden_key(b"readableDidRead"),
            f64::from_bits(value),
        );
    }
}

fn readable_encoding_value(stream: f64) -> f64 {
    get_hidden_value(stream, hidden_key(b"readableEncoding")).unwrap_or(f64::from_bits(TAG_NULL))
}

fn normalize_readable_encoding(encoding: f64) -> f64 {
    if JSValue::from_bits(encoding.to_bits()).is_any_string() {
        encoding
    } else {
        f64::from_bits(TAG_NULL)
    }
}

fn set_visible_readable_encoding(stream: f64, encoding: f64) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        set_hidden_value(stream, hidden_key(b"readableEncoding"), encoding);
    }
}

fn mark_stream_ended(stream: f64) {
    set_hidden_value(stream, hidden_ended_key(), f64::from_bits(TAG_TRUE));
    set_visible_readable(stream, false);
    set_visible_readable_ended(stream, true);
}

fn set_visible_writable(stream: f64, writable: bool) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let value = if writable { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(stream, hidden_key(b"writable"), f64::from_bits(value));
    }
}

fn set_visible_writable_ended(stream: f64, ended: bool) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let value = if ended { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(stream, hidden_key(b"writableEnded"), f64::from_bits(value));
    }
}

fn set_visible_writable_finished(stream: f64, finished: bool) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let value = if finished { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(
            stream,
            hidden_key(b"writableFinished"),
            f64::from_bits(value),
        );
    }
}

fn mark_writable_ended(stream: f64) {
    set_hidden_value(stream, hidden_ended_key(), f64::from_bits(TAG_TRUE));
    set_visible_writable(stream, false);
    set_visible_writable_ended(stream, true);
}

fn mark_writable_finished(stream: f64) {
    set_visible_writable(stream, false);
    set_visible_writable_finished(stream, true);
}

fn set_visible_closed(stream: f64, closed: bool) {
    let value = if closed { TAG_TRUE } else { TAG_FALSE };
    set_hidden_value(stream, hidden_key(b"closed"), f64::from_bits(value));
}

pub(super) fn mark_stream_closed(stream: f64) {
    set_visible_closed(stream, true);
}

/// Initialize the readable side of a stream: direction flag, buffered byte
/// counter, effective readable highWaterMark, and the visible
/// `readableHighWaterMark` / `destroyed` properties (#1534/#1539).
fn init_readable_state(stream: f64, opts: f64) {
    set_stream_auto_destroy(stream, opts);
    set_hidden_value(stream, hidden_readable_flag_key(), f64::from_bits(TAG_TRUE));
    set_hidden_value(stream, hidden_key(b"destroyed"), f64::from_bits(TAG_FALSE));
    set_hidden_value(
        stream,
        hidden_key(b"readableAborted"),
        f64::from_bits(TAG_FALSE),
    );
    set_hidden_value(stream, hidden_buffered_key(), 0.0);
    set_hidden_value(stream, hidden_key(b"readableLength"), 0.0);
    let readable_object_mode = resolve_object_mode(opts, b"readableObjectMode");
    set_hidden_value(
        stream,
        hidden_key(b"readableObjectMode"),
        f64::from_bits(if readable_object_mode {
            TAG_TRUE
        } else {
            TAG_FALSE
        }),
    );
    let r_hwm = resolve_hwm(opts, b"readableHighWaterMark", b"readableObjectMode");
    set_hidden_value(stream, hidden_hwm_key(), r_hwm);
    set_hidden_value(stream, hidden_key(b"readableHighWaterMark"), r_hwm);
    set_hidden_value(stream, readable_flowing_key(), f64::from_bits(TAG_NULL));
    set_hidden_value(
        stream,
        hidden_readable_pending_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    set_hidden_value(
        stream,
        hidden_stream_pipes_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    set_visible_readable(stream, true);
    set_visible_readable_ended(stream, false);
    set_visible_readable_did_read(stream, false);
    let encoding = opt_string_value(opts, b"encoding").unwrap_or(f64::from_bits(TAG_NULL));
    set_visible_readable_encoding(stream, encoding);
}

/// Initialize the writable side: direction flag and visible stream flags.
fn init_writable_state(stream: f64, opts: f64) {
    set_stream_auto_destroy(stream, opts);
    set_hidden_value(stream, hidden_writable_flag_key(), f64::from_bits(TAG_TRUE));
    set_hidden_value(stream, hidden_key(b"destroyed"), f64::from_bits(TAG_FALSE));
    let writable_object_mode = resolve_object_mode(opts, b"writableObjectMode");
    set_hidden_value(
        stream,
        hidden_key(b"writableObjectMode"),
        f64::from_bits(if writable_object_mode {
            TAG_TRUE
        } else {
            TAG_FALSE
        }),
    );
    let w_hwm = resolve_hwm(opts, b"writableHighWaterMark", b"writableObjectMode");
    set_hidden_value(stream, hidden_key(b"writableHighWaterMark"), w_hwm);
    set_hidden_value(
        stream,
        hidden_writable_object_mode_key(),
        f64::from_bits(if writable_object_mode {
            TAG_TRUE
        } else {
            TAG_FALSE
        }),
    );
    let decode_strings = !get_hidden_value(opts, hidden_key(b"decodeStrings"))
        .is_some_and(|v| v.to_bits() == TAG_FALSE);
    set_hidden_value(
        stream,
        hidden_writable_decode_strings_key(),
        f64::from_bits(if decode_strings { TAG_TRUE } else { TAG_FALSE }),
    );
    let default_encoding =
        opt_string_value(opts, b"defaultEncoding").unwrap_or_else(|| string_value(b"utf8"));
    set_hidden_value(
        stream,
        hidden_writable_default_encoding_key(),
        default_encoding,
    );
    set_writable_length(stream, 0.0);
    set_writable_need_drain(stream, false);
    set_pending_writable_finish_callback(stream, None);
    set_writable_corked_count(stream, 0.0);
    set_hidden_value(
        stream,
        hidden_writable_buffered_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    set_visible_writable(stream, true);
    set_visible_writable_ended(stream, false);
    set_visible_writable_finished(stream, false);
}

fn init_duplex_state(stream: f64, opts: f64) {
    let allow_half_open = if get_hidden_value(opts, hidden_key(b"allowHalfOpen"))
        .is_some_and(|v| v.to_bits() == TAG_FALSE)
    {
        TAG_FALSE
    } else {
        TAG_TRUE
    };
    set_hidden_value(
        stream,
        hidden_key(b"allowHalfOpen"),
        f64::from_bits(allow_half_open),
    );
}

fn init_abort_signal_state(stream: f64, opts: f64) {
    if let Some(signal) = options_signal(opts) {
        attach_abort_signal(signal, stream);
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
    init_lifecycle_state(readable, opts);
    init_constructor(readable, "Readable");
    init_readable_state(readable, opts);
    install_common_lifecycle_callbacks(readable, opts);
    init_abort_signal_state(readable, opts);
    async_iterator::install_readable_async_iterator_symbol(readable);
    install_stream_async_dispose_symbol(readable);
    invoke_construct_callback(readable, opts);
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
    if let Some(writev) = writev_callback_from_options(opts) {
        js_object_set_field_by_name(
            obj,
            hidden_writev_key(),
            rebind_callback_this(writev, writable),
        );
    }
    init_lifecycle_state(writable, opts);
    init_constructor(writable, "Writable");
    init_writable_state(writable, opts);
    install_common_lifecycle_callbacks(writable, opts);
    install_writable_lifecycle_callbacks(writable, opts);
    init_abort_signal_state(writable, opts);
    install_stream_async_dispose_symbol(writable);
    invoke_construct_callback(writable, opts);
    writable
}

#[no_mangle]
pub extern "C" fn js_node_stream_writable_subclass_init(this: f64, opts: f64) -> f64 {
    let obj = {
        let bits = this.to_bits();
        let top16 = bits >> 48;
        let raw = if top16 >= 0x7FF8 {
            if top16 == 0x7FFC {
                return f64::from_bits(TAG_UNDEFINED);
            }
            (bits & crate::value::POINTER_MASK) as usize
        } else {
            bits as usize
        };
        if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return f64::from_bits(TAG_UNDEFINED);
        }
        raw as *mut ObjectHeader
    };
    let this = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    unsafe {
        if gc_type_for_ptr(obj as usize) != Some(crate::gc::GC_TYPE_OBJECT) {
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    if obj.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }

    let subclass_write = js_object_get_field_by_name_f64(obj, hidden_key(b"_write"));
    let subclass_writev = js_object_get_field_by_name_f64(obj, hidden_key(b"_writev"));
    let methods = writable_methods();
    install_methods_on_existing_object(obj, this, &methods, &["_write"]);

    if let Some(write) = write_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_write_key(), rebind_callback_this(write, this));
    } else if is_callable_value(subclass_write) {
        js_object_set_field_by_name(obj, hidden_write_key(), subclass_write);
    }
    if let Some(writev) = writev_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_writev_key(), rebind_callback_this(writev, this));
    } else if is_callable_value(subclass_writev) {
        js_object_set_field_by_name(obj, hidden_writev_key(), subclass_writev);
    }

    init_lifecycle_state(this, opts);
    init_constructor(this, "Writable");
    init_writable_state(this, opts);
    install_common_lifecycle_callbacks(this, opts);
    install_writable_lifecycle_callbacks(this, opts);
    init_abort_signal_state(this, opts);
    install_stream_async_dispose_symbol(this);
    invoke_construct_callback(this, opts);
    this
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_new(opts: f64) -> f64 {
    let methods = duplex_methods();
    let obj = build_object(&methods, DUPLEX_SHAPE_ID + methods.len() as u32);
    let duplex = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    if let Some(write) = write_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_write_key(), rebind_callback_this(write, duplex));
    }
    if let Some(writev) = writev_callback_from_options(opts) {
        js_object_set_field_by_name(
            obj,
            hidden_writev_key(),
            rebind_callback_this(writev, duplex),
        );
    }
    init_lifecycle_state(duplex, opts);
    init_constructor(duplex, "Duplex");
    init_readable_state(duplex, opts);
    init_writable_state(duplex, opts);
    init_duplex_state(duplex, opts);
    install_common_lifecycle_callbacks(duplex, opts);
    install_writable_lifecycle_callbacks(duplex, opts);
    init_abort_signal_state(duplex, opts);
    async_iterator::install_readable_async_iterator_symbol(duplex);
    install_stream_async_dispose_symbol(duplex);
    invoke_construct_callback(duplex, opts);
    duplex
}

#[no_mangle]
pub extern "C" fn js_node_stream_transform_new(opts: f64) -> f64 {
    let transform = js_node_stream_duplex_new(opts);
    if let Some(callback) = transform_callback_from_options(opts) {
        set_hidden_value(
            transform,
            hidden_transform_callback_key(),
            rebind_callback_this(callback, transform),
        );
    }
    if let Some(flush) = transform_flush_from_options(opts) {
        set_hidden_value(
            transform,
            hidden_transform_flush_key(),
            rebind_callback_this(flush, transform),
        );
    }
    init_constructor(transform, "Transform");
    transform
}

#[no_mangle]
pub extern "C" fn js_node_stream_passthrough_new(opts: f64) -> f64 {
    let passthrough = js_node_stream_duplex_new(opts);
    set_hidden_value(
        passthrough,
        hidden_transform_passthrough_key(),
        f64::from_bits(TAG_TRUE),
    );
    init_constructor(passthrough, "PassThrough");
    passthrough
}

/// `Readable.from(iterable)` — Node's static factory. Returns a
/// Readable object and retains simple iterable chunks so
/// `node:stream/consumers` can drain the current stub stream surface.
#[no_mangle]
pub extern "C" fn js_node_stream_readable_from(iterable: f64) -> f64 {
    js_node_stream_readable_from_options(iterable, f64::from_bits(TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_node_stream_readable_from_options(iterable: f64, opts: f64) -> f64 {
    if matches!(iterable.to_bits(), TAG_NULL | TAG_UNDEFINED)
        || is_non_iterable_primitive_for_readable_from(iterable)
    {
        throw_readable_from_invalid_iterable();
    }
    let readable = js_node_stream_readable_new(readable_from_options(opts));
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
pub extern "C" fn js_node_stream_is_disturbed(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_disturbed_key())
        .is_some_and(|v| crate::value::js_is_truthy(v) != 0)
    {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_is_errored(stream: f64) -> f64 {
    if readable_hidden_error(stream).is_some() {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// #1534/#1746: `Readable.isReadable(s)` / module-level `isReadable(s)`.
/// Node returns `null` for a stream with no readable side (e.g. a bare
/// Writable), `false` once the readable side has ended or errored, and
/// `true` while it's still readable. Perry tracks the readable-direction
/// flag at construction and the ended/errored bits as methods run.
#[no_mangle]
pub extern "C" fn js_node_stream_is_readable(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_none() {
        return f64::from_bits(TAG_NULL);
    }
    let ended = stream_hidden_ended(stream);
    let errored = readable_hidden_error(stream).is_some();
    if ended || errored {
        f64::from_bits(TAG_FALSE)
    } else {
        f64::from_bits(TAG_TRUE)
    }
}

/// #1746: `stream.isWritable(s)` / `Writable.isWritable(s)`. Mirror of
/// `isReadable` for the writable side: `null` for a stream with no
/// writable side (a bare Readable), `false` once it has ended (`.end()`)
/// or errored, `true` otherwise. A Duplex answers for its writable side.
#[no_mangle]
pub extern "C" fn js_node_stream_is_writable(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_none() {
        return f64::from_bits(TAG_NULL);
    }
    let ended = stream_hidden_ended(stream);
    let errored = readable_hidden_error(stream).is_some();
    if ended || errored {
        f64::from_bits(TAG_FALSE)
    } else {
        f64::from_bits(TAG_TRUE)
    }
}

/// #1537: `stream.getDefaultHighWaterMark(objectMode)` returns the current
/// platform-default highWaterMark — 65536 for byte streams, 16 for
/// objectMode (both settable via `setDefaultHighWaterMark`).
#[no_mangle]
pub extern "C" fn js_node_stream_get_default_hwm(object_mode: f64) -> f64 {
    default_hwm(crate::value::js_is_truthy(object_mode) != 0)
}

/// #1537: `stream.setDefaultHighWaterMark(objectMode, value)` updates the
/// per-mode default returned by `getDefaultHighWaterMark` and inherited by
/// streams constructed without an explicit `highWaterMark`. Returns
/// `undefined`, matching Node.
#[no_mangle]
pub extern "C" fn js_node_stream_set_default_hwm(object_mode: f64, value: f64) -> f64 {
    let n = jsvalue_as_f64(value).unwrap_or(0.0);
    if crate::value::js_is_truthy(object_mode) != 0 {
        DEFAULT_HWM_OBJECT.with(|c| c.set(n));
    } else {
        DEFAULT_HWM_BYTE.with(|c| c.set(n));
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn attach_abort_signal(signal: f64, stream: f64) {
    if signal_is_aborted(signal) {
        destroy_stream(stream, abort_error());
        return;
    }
    let Some(signal_obj) = object_ptr_from_value(signal) else {
        return;
    };
    let listener = js_closure_alloc(ns_stream_abort_listener as *const u8, 1);
    js_closure_set_capture_ptr(listener, 0, stream.to_bits() as i64);
    crate::url::js_abort_signal_add_listener(
        signal_obj,
        string_value(b"abort"),
        box_pointer(listener as *const u8),
    );
}

/// #1541: `stream.addAbortSignal(signal, stream)` — wire an AbortSignal so
/// aborting it destroys the stream with an AbortError, then return the same
/// stream for chaining.
#[no_mangle]
pub extern "C" fn js_node_stream_add_abort_signal(signal: f64, stream: f64) -> f64 {
    attach_abort_signal(signal, stream);
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

/// `stream.pipeline(...streams, cb)` wires classic streams end-to-end and
/// invokes the callback once on success or on the first observed error.
#[no_mangle]
pub extern "C" fn js_node_stream_pipeline(args: *const crate::array::ArrayHeader) -> f64 {
    let mut args = pipeline_args(args);
    if args.is_empty() {
        throw_pipeline_missing_streams();
    }

    let callback = *args.last().unwrap_or(&f64::from_bits(TAG_UNDEFINED));
    if !is_callable_value(callback) {
        throw_pipeline_callback_required();
    }
    args.pop();

    let mut options = PipelineOptions {
        end_final: true,
        signal: None,
    };
    if args.last().copied().is_some_and(is_pipeline_options_arg) {
        let option_arg = args.pop().unwrap_or(f64::from_bits(TAG_UNDEFINED));
        options = pipeline_options_from_arg(option_arg);
    }

    if args.len() == 1 && is_array_like_value(args[0]) {
        args = pipeline_array_like_values(args[0]);
    }
    if args.len() < 2 {
        throw_pipeline_missing_streams();
    }

    if pipeline_needs_collected_path(&args) {
        return run_collected_pipeline(&args, callback, options);
    }

    let stages: Vec<f64> = args
        .into_iter()
        .enumerate()
        .map(|(idx, stage)| normalize_pipeline_source(stage, idx))
        .collect();
    add_pipeline_callback_listeners(&stages, callback, options);

    for i in 0..stages.len() - 1 {
        let is_final_pair = i + 1 == stages.len() - 1;
        wire_pipeline_pair(
            stages[i],
            stages[i + 1],
            options.end_final || !is_final_pair,
        );
    }
    for stage in stages.iter().take(stages.len() - 1) {
        start_pipeline_readable(*stage);
    }

    *stages.last().unwrap_or(&f64::from_bits(TAG_UNDEFINED))
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

/// A WHATWG-stream-shaped stub: an object carrying both `getReader` and
/// `getWriter` method stubs. A real `ReadableStream` only has `getReader`
/// and a `WritableStream` only `getWriter`, but the single `js_node_stream_to_web`
/// entry can't tell which class `.toWeb` was called on (the NativeMethodCall
/// drops the class), so the union shape lets `Readable.toWeb`,
/// `Writable.toWeb`, and the `{ readable, writable }` pair from
/// `Duplex.toWeb` all satisfy their `typeof x.getReader/getWriter === "function"`
/// existence checks. Data isn't forwarded between the Node and WHATWG
/// universes — that's the remaining #1540 gap.
fn build_web_stream_stub() -> f64 {
    let methods: [(&str, StubFn); 2] = [
        ("getReader", cast0(ns_undefined0)),
        ("getWriter", cast0(ns_undefined0)),
    ];
    let obj = build_object(&methods, WEB_STREAM_SHAPE_ID + methods.len() as u32);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// `Readable.toWeb` / `Writable.toWeb` / `Duplex.toWeb` — returns a
/// web-stream-shaped stub (#1540). For Duplex the result also exposes
/// `readable` / `writable` web-stream stubs so `pair.readable.getReader`
/// / `pair.writable.getWriter` resolve.
#[no_mangle]
pub extern "C" fn js_node_stream_to_web(_node_stream: f64) -> f64 {
    let top = build_web_stream_stub();
    set_hidden_value(top, hidden_key(b"readable"), build_web_stream_stub());
    set_hidden_value(top, hidden_key(b"writable"), build_web_stream_stub());
    top
}

/// `Readable.fromWeb` / `Writable.fromWeb` — Perry returns a fresh
/// Duplex stub for either direction. Real bidirectional adapters
/// are tracked separately.
#[no_mangle]
pub extern "C" fn js_node_stream_from_web(_web_stream: f64) -> f64 {
    js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED))
}

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
