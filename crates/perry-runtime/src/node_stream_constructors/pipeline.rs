//! node:stream — `Duplex.from`, `compose`, `finished`, `pipeline` and
//! `duplexPair` (split out of node_stream_constructors.rs for the 2000-line
//! file-size gate, #1987).
use super::super::*;
use super::*;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader,
};
use crate::object::{js_object_set_field_by_name, ObjectHeader};
use crate::value::JSValue;

fn attach_duplex_readable_source(duplex: f64, source: f64) -> Result<(), f64> {
    let chunks = if let Some(chunks) = readable_hidden_chunks(source) {
        chunks
    } else {
        collect_pipeline_chunks(source)?
    };
    let values = pipeline_chunks_vec(chunks);
    let mut arr = crate::array::js_array_alloc(values.len() as u32);
    for chunk in values {
        arr = crate::array::js_array_push_f64(arr, chunk);
    }

    set_hidden_value(duplex, hidden_chunks_key(), box_pointer(arr as *const u8));
    set_hidden_value(
        duplex,
        hidden_buffered_key(),
        crate::array::js_array_length(arr) as f64,
    );
    set_hidden_value(
        duplex,
        hidden_key(b"readableLength"),
        crate::array::js_array_length(arr) as f64,
    );
    Ok(())
}

fn node_stream_duplex_from_source_chunks(source: f64) -> f64 {
    let duplex = js_node_stream_duplex_new(readable_from_options(f64::from_bits(TAG_UNDEFINED)));
    set_visible_writable(duplex, false);
    if let Err(err) = attach_duplex_readable_source(duplex, source) {
        set_hidden_value(duplex, hidden_error_key(), err);
    }
    duplex
}

pub(super) extern "C" fn duplex_from_writable_write_callback(
    closure: *const ClosureHeader,
    chunk: f64,
    encoding: f64,
    cb: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let writable = js_closure_get_capture_f64(closure, 0);
    js_node_stream_method_write(raw_ptr_from_value(writable) as i64, chunk, encoding, cb)
}

pub(super) extern "C" fn duplex_from_writable_final_callback(
    closure: *const ClosureHeader,
    cb: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let writable = js_closure_get_capture_f64(closure, 0);
    js_node_stream_method_end(
        raw_ptr_from_value(writable) as i64,
        f64::from_bits(TAG_UNDEFINED),
    );
    call_listener_args(writable, cb, &[]);
    f64::from_bits(TAG_UNDEFINED)
}

fn install_duplex_from_writable(duplex: f64, writable: f64) {
    let raw = raw_ptr_from_value(duplex);
    if raw < 0x10000 {
        return;
    }
    let obj = raw as *mut ObjectHeader;
    let write = js_closure_alloc(duplex_from_writable_write_callback as *const u8, 1);
    js_closure_set_capture_f64(write, 0, writable);
    js_object_set_field_by_name(
        obj,
        hidden_write_key(),
        f64::from_bits(JSValue::pointer(write as *const u8).bits()),
    );

    let final_cb = js_closure_alloc(duplex_from_writable_final_callback as *const u8, 1);
    js_closure_set_capture_f64(final_cb, 0, writable);
    js_object_set_field_by_name(
        obj,
        hidden_writable_final_key(),
        f64::from_bits(JSValue::pointer(final_cb as *const u8).bits()),
    );

    set_hidden_value(duplex, hidden_key(b"duplexWrappedWritable"), writable);
    set_hidden_value(
        duplex,
        hidden_key(b"writableCustomSink"),
        f64::from_bits(TAG_TRUE),
    );
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_from_options(body: f64, _opts: f64) -> f64 {
    if object_ptr_from_value(body).is_some() && !is_classic_stream_instance_value(body) {
        let readable = get_hidden_value(body, hidden_key(b"readable"));
        let writable = get_hidden_value(body, hidden_key(b"writable"));
        if readable.is_some() || writable.is_some() {
            let duplex =
                js_node_stream_duplex_new(readable_from_options(f64::from_bits(TAG_UNDEFINED)));
            if let Some(readable) = readable {
                if let Err(err) = attach_duplex_readable_source(duplex, readable) {
                    set_hidden_value(duplex, hidden_error_key(), err);
                }
            } else {
                set_visible_readable(duplex, false);
            }
            if let Some(writable) = writable {
                install_duplex_from_writable(duplex, writable);
            } else {
                set_visible_writable(duplex, false);
            }
            return duplex;
        }
    }

    node_stream_duplex_from_source_chunks(body)
}

/// #1539: `stream.compose(...streams)` chains a sequence of streams or
/// callable stages into one composite Duplex.
#[no_mangle]
pub extern "C" fn js_node_stream_compose(args: *const crate::array::ArrayHeader) -> f64 {
    js_node_stream_compose_args(args)
}

/// Variadic `stream.compose(...)` entry used by bound native-module property
/// reads and by direct named imports through codegen's packed varargs ABI.
pub extern "C" fn js_node_stream_compose_args(args: *const crate::array::ArrayHeader) -> f64 {
    build_node_stream_compose(pipeline_args(args))
}

pub(super) fn add_finished_once_listeners(
    stream: f64,
    callback: f64,
    watch_finish: bool,
    watch_close: bool,
) {
    let listener = js_closure_alloc(ns_finished_error_false_close as *const u8, 3);
    js_closure_set_capture_f64(listener, 0, stream);
    js_closure_set_capture_f64(listener, 1, callback);
    js_closure_set_capture_f64(listener, 2, f64::from_bits(TAG_FALSE));
    let listener_value = box_pointer(listener as *const u8);
    if watch_finish {
        add_stream_listener_for_event(stream, string_value(b"finish"), listener_value);
    }
    if watch_close {
        add_stream_listener_for_event(stream, string_value(b"close"), listener_value);
    }
}

pub(super) fn add_finished_signal_abort_listener(stream: f64, signal: f64, callback: f64) {
    let listener = js_closure_alloc(ns_finished_signal_abort as *const u8, 4);
    js_closure_set_capture_f64(listener, 0, stream);
    js_closure_set_capture_f64(listener, 1, callback);
    js_closure_set_capture_f64(listener, 2, f64::from_bits(TAG_FALSE));
    js_closure_set_capture_f64(listener, 3, signal);
    if signal_is_aborted(signal) {
        crate::builtins::js_queue_microtask(listener as i64);
        return;
    }
    let Some(signal_obj) = object_ptr_from_value(signal) else {
        return;
    };
    crate::url::js_abort_signal_add_listener(
        signal_obj,
        string_value(b"abort"),
        box_pointer(listener as *const u8),
    );
}

pub(super) fn add_finished_cleanup_completion_listener(stream: f64, callback: f64) {
    let listener = js_closure_alloc(ns_finished_error_false_close as *const u8, 3);
    js_closure_set_capture_f64(listener, 0, stream);
    js_closure_set_capture_f64(listener, 1, callback);
    js_closure_set_capture_f64(listener, 2, f64::from_bits(TAG_FALSE));
    let listener_value = box_pointer(listener as *const u8);
    add_stream_listener_for_event(stream, string_value(b"end"), listener_value);
    add_stream_listener_for_event(stream, string_value(b"finish"), listener_value);
    add_stream_listener_for_event(stream, string_value(b"close"), listener_value);
}

/// `stream.finished(stream, [options], cb)` callback form. This slice covers
/// focused option paths:
///
/// - `{ error: false }`: do not install an error listener, but `close` still
///   observes the stream's stored error and calls the callback.
/// - `{ readable: false }`: ignore the readable side and call back when the
///   writable side emits `finish`.
#[no_mangle]
pub extern "C" fn js_node_stream_finished(args: *const crate::array::ArrayHeader) -> f64 {
    let args = pipeline_args(args);
    if args.len() < 2 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = args[0];
    let mut options = f64::from_bits(TAG_UNDEFINED);
    let mut callback = args[1];
    if args.len() >= 3 && is_pipeline_options_arg(args[1]) {
        options = args[1];
        callback = args[2];
    }
    if !is_callable_value(callback) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let watch_close =
        get_hidden_value(options, hidden_key(b"error")).is_some_and(|v| v.to_bits() == TAG_FALSE);
    let watch_finish = get_hidden_value(options, hidden_key(b"readable"))
        .is_some_and(|v| v.to_bits() == TAG_FALSE);
    if watch_close || watch_finish {
        add_finished_once_listeners(stream, callback, watch_finish, watch_close);
    }
    if let Some(signal) = options_signal(options) {
        add_finished_signal_abort_listener(stream, signal, callback);
    }
    if get_hidden_value(options, hidden_key(b"cleanup"))
        .is_some_and(|v| crate::value::js_is_truthy(v) != 0)
    {
        add_finished_cleanup_completion_listener(stream, callback);
    }
    f64::from_bits(TAG_UNDEFINED)
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

pub(crate) extern "C" fn duplex_pair_write_callback(
    closure: *const ClosureHeader,
    chunk: f64,
    _encoding: f64,
    cb: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let peer = js_closure_get_capture_f64(closure, 0);
    if get_hidden_value(peer, hidden_readable_flag_key()).is_some() && !stream_destroyed(peer) {
        mark_disturbed(peer);
        if readable_is_flowing(peer) {
            emit_readable_data(peer, chunk);
        } else {
            buffer_pending_readable_chunk(peer, chunk);
        }
    }
    call_listener_args(peer, cb, &[]);
    f64::from_bits(TAG_UNDEFINED)
}

pub(crate) extern "C" fn duplex_pair_final_callback(closure: *const ClosureHeader, cb: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let peer = js_closure_get_capture_f64(closure, 0);
    schedule_readable_end(peer);
    call_listener_args(peer, cb, &[]);
    f64::from_bits(TAG_UNDEFINED)
}

fn install_duplex_pair_endpoint(endpoint: f64, peer: f64) {
    let raw = raw_ptr_from_value(endpoint);
    if raw < 0x10000 {
        return;
    }
    let obj = raw as *mut ObjectHeader;
    let write = js_closure_alloc(duplex_pair_write_callback as *const u8, 1);
    js_closure_set_capture_f64(write, 0, peer);
    js_object_set_field_by_name(
        obj,
        hidden_write_key(),
        f64::from_bits(JSValue::pointer(write as *const u8).bits()),
    );

    let final_cb = js_closure_alloc(duplex_pair_final_callback as *const u8, 1);
    js_closure_set_capture_f64(final_cb, 0, peer);
    js_object_set_field_by_name(
        obj,
        hidden_writable_final_key(),
        f64::from_bits(JSValue::pointer(final_cb as *const u8).bits()),
    );

    set_hidden_value(endpoint, hidden_key(b"duplexPairPeer"), peer);
    set_hidden_value(
        endpoint,
        hidden_key(b"writableCustomSink"),
        f64::from_bits(TAG_TRUE),
    );
}

/// #1539: `stream.duplexPair([options])` returns a two-element array
/// `[Duplex, Duplex]` where writes to one show up as reads on the
/// other and vice versa.
#[no_mangle]
pub extern "C" fn js_node_stream_duplex_pair(_opts: f64) -> f64 {
    let a = js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED));
    let b = js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED));
    install_duplex_pair_endpoint(a, b);
    install_duplex_pair_endpoint(b, a);
    let arr = crate::array::js_array_alloc(2);
    crate::array::js_array_push(arr, JSValue::from_bits(a.to_bits()));
    crate::array::js_array_push(arr, JSValue::from_bits(b.to_bits()));
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}
