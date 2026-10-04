//! node:stream — `Duplex.from`, `compose`, `finished`, `pipeline` and
//! `duplexPair` (split out of node_stream_constructors.rs for the 2000-line
//! file-size gate, #1987).
use super::*;
use crate::closure::{js_closure_get_capture_f64, ClosureHeader};
use crate::object::{js_object_set_field_by_name, ObjectHeader};

fn attach_duplex_readable_source(duplex: f64, source: f64) -> Result<(), f64> {
    // Collecting the source runs its iterator or `_read`, which can collect.
    let scope = crate::gc::RuntimeHandleScope::new();
    let duplex = scope.root_nanbox_f64(duplex);
    let source = scope.root_nanbox_f64(source);
    let chunks = if let Some(chunks) = readable_hidden_chunks(source.get_nanbox_f64()) {
        chunks
    } else {
        collect_pipeline_chunks(source.get_nanbox_f64())?
    };
    let arr = chunk_values_snapshot(&scope, chunks);
    let len = rooted_array_len(&arr) as f64;
    set_hidden_value(
        duplex.get_nanbox_f64(),
        hidden_chunks_key(),
        arr.get_nanbox_f64(),
    );
    set_hidden_value(duplex.get_nanbox_f64(), hidden_buffered_key(), len);
    set_hidden_value(duplex.get_nanbox_f64(), hidden_key(b"readableLength"), len);
    Ok(())
}

fn node_stream_duplex_from_source_chunks(source: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(source);
    let options = readable_from_options(f64::from_bits(TAG_UNDEFINED));
    let duplex = scope.root_nanbox_f64(js_node_stream_duplex_new(options));
    set_visible_writable(duplex.get_nanbox_f64(), false);
    if let Err(err) =
        attach_duplex_readable_source(duplex.get_nanbox_f64(), source.get_nanbox_f64())
    {
        set_hidden_value(duplex.get_nanbox_f64(), hidden_error_key(), err);
    }
    duplex.get_nanbox_f64()
}

pub(super) extern "C" fn duplex_from_writable_write_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
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
    _this: crate::closure::JsThis,
    cb: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // `end` runs the writable's `_final` and listeners, which can collect.
    let scope = crate::gc::RuntimeHandleScope::new();
    let writable = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let cb = scope.root_nanbox_f64(cb);
    js_node_stream_method_end(
        raw_ptr_from_value(writable.get_nanbox_f64()) as i64,
        f64::from_bits(TAG_UNDEFINED),
    );
    call_listener_args(writable.get_nanbox_f64(), cb.get_nanbox_f64(), &[]);
    f64::from_bits(TAG_UNDEFINED)
}

/// Store the rooted closure `closure` in `target`'s hidden field `key`.
fn set_hidden_closure(
    target: &crate::gc::RuntimeHandle<'_>,
    key: *mut crate::string::StringHeader,
    closure: &crate::gc::RuntimeHandle<'_>,
) {
    let obj = raw_ptr_from_value(target.get_nanbox_f64()) as *mut ObjectHeader;
    js_object_set_field_by_name(obj, key, closure.get_nanbox_f64());
}

fn install_duplex_from_writable(duplex: f64, writable: f64) {
    if raw_ptr_from_value(duplex) < 0x10000 {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let duplex = scope.root_nanbox_f64(duplex);
    let writable = scope.root_nanbox_f64(writable);
    let write = rooted_closure(
        &scope,
        crate::fn_info!(duplex_from_writable_write_callback, 3),
        &[&writable],
    );
    set_hidden_closure(&duplex, hidden_write_key(), &write);

    let final_cb = rooted_closure(
        &scope,
        crate::fn_info!(duplex_from_writable_final_callback, 1),
        &[&writable],
    );
    set_hidden_closure(&duplex, hidden_writable_final_key(), &final_cb);

    set_hidden_value(
        duplex.get_nanbox_f64(),
        hidden_key(b"duplexWrappedWritable"),
        writable.get_nanbox_f64(),
    );
    set_hidden_value(
        duplex.get_nanbox_f64(),
        hidden_key(b"writableCustomSink"),
        f64::from_bits(TAG_TRUE),
    );
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_from_options(body: f64, _opts: f64) -> f64 {
    if object_ptr_from_value(body).is_some() && !is_classic_stream_instance_value(body) {
        // Collecting the readable side runs user code that can collect.
        let scope = crate::gc::RuntimeHandleScope::new();
        let body = scope.root_nanbox_f64(body);
        let readable_key = hidden_key(b"readable");
        let readable = get_hidden_value(body.get_nanbox_f64(), readable_key)
            .map(|readable| scope.root_nanbox_f64(readable));
        let writable_key = hidden_key(b"writable");
        let writable = get_hidden_value(body.get_nanbox_f64(), writable_key)
            .map(|writable| scope.root_nanbox_f64(writable));
        if readable.is_some() || writable.is_some() {
            let options = readable_from_options(f64::from_bits(TAG_UNDEFINED));
            let duplex = scope.root_nanbox_f64(js_node_stream_duplex_new(options));
            if let Some(readable) = &readable {
                if let Err(err) = attach_duplex_readable_source(
                    duplex.get_nanbox_f64(),
                    readable.get_nanbox_f64(),
                ) {
                    set_hidden_value(duplex.get_nanbox_f64(), hidden_error_key(), err);
                }
            } else {
                set_visible_readable(duplex.get_nanbox_f64(), false);
            }
            if let Some(writable) = &writable {
                install_duplex_from_writable(duplex.get_nanbox_f64(), writable.get_nanbox_f64());
            } else {
                set_visible_writable(duplex.get_nanbox_f64(), false);
            }
            return duplex.get_nanbox_f64();
        }
        return node_stream_duplex_from_source_chunks(body.get_nanbox_f64());
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

/// Add the rooted `listener` to `stream`'s `event` listeners.
fn add_rooted_listener(
    stream: &crate::gc::RuntimeHandle<'_>,
    event: &'static [u8],
    listener: &crate::gc::RuntimeHandle<'_>,
) {
    let event = literal_string_value(event);
    add_stream_listener_for_event(stream.get_nanbox_f64(), event, listener.get_nanbox_f64());
}

pub(super) fn add_finished_once_listeners(
    stream: f64,
    callback: f64,
    watch_finish: bool,
    watch_close: bool,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let callback = scope.root_nanbox_f64(callback);
    let not_called = scope.root_nanbox_f64(f64::from_bits(TAG_FALSE));
    let listener = rooted_closure(
        &scope,
        crate::fn_info!(ns_finished_error_false_close, 0; with_declared(0)),
        &[&stream, &callback, &not_called],
    );
    if watch_finish {
        add_rooted_listener(&stream, b"finish", &listener);
    }
    if watch_close {
        add_rooted_listener(&stream, b"close", &listener);
    }
}

pub(super) fn add_finished_signal_abort_listener(stream: f64, signal: f64, callback: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let signal = scope.root_nanbox_f64(signal);
    let callback = scope.root_nanbox_f64(callback);
    let not_called = scope.root_nanbox_f64(f64::from_bits(TAG_FALSE));
    let listener = rooted_closure(
        &scope,
        crate::fn_info!(ns_finished_signal_abort, 0; with_declared(0)),
        &[&stream, &callback, &not_called, &signal],
    );
    if signal_is_aborted(signal.get_nanbox_f64()) {
        crate::builtins::js_queue_microtask(raw_ptr_from_value(listener.get_nanbox_f64()) as i64);
        return;
    }
    let abort = literal_string_value(b"abort");
    let Some(signal_obj) = object_ptr_from_value(signal.get_nanbox_f64()) else {
        return;
    };
    crate::url::js_abort_signal_add_listener(signal_obj, abort, listener.get_nanbox_f64());
}

pub(super) fn add_finished_cleanup_completion_listener(stream: f64, callback: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let callback = scope.root_nanbox_f64(callback);
    let not_called = scope.root_nanbox_f64(f64::from_bits(TAG_FALSE));
    let listener = rooted_closure(
        &scope,
        crate::fn_info!(ns_finished_default_completion, 0; with_declared(0)),
        &[&stream, &callback, &not_called],
    );
    add_rooted_listener(&stream, b"end", &listener);
    add_rooted_listener(&stream, b"finish", &listener);
    add_rooted_listener(&stream, b"close", &listener);
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let args = rooted_args_copy(&scope, args);
    let len = rooted_array_len(&args);
    if len < 2 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = scope.root_nanbox_f64(rooted_array_at(&args, 0));
    let options = scope.root_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
    let callback = scope.root_nanbox_f64(rooted_array_at(&args, 1));
    if len >= 3 && is_pipeline_options_arg(rooted_array_at(&args, 1)) {
        options.set_nanbox_f64(rooted_array_at(&args, 1));
        callback.set_nanbox_f64(rooted_array_at(&args, 2));
    }
    if !is_callable_value(callback.get_nanbox_f64()) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let error_key = hidden_key(b"error");
    let watch_close = get_hidden_value(options.get_nanbox_f64(), error_key)
        .is_some_and(|v| v.to_bits() == TAG_FALSE);
    let readable_key = hidden_key(b"readable");
    let watch_finish = get_hidden_value(options.get_nanbox_f64(), readable_key)
        .is_some_and(|v| v.to_bits() == TAG_FALSE);
    if watch_close || watch_finish {
        add_finished_once_listeners(
            stream.get_nanbox_f64(),
            callback.get_nanbox_f64(),
            watch_finish,
            watch_close,
        );
    } else {
        // The default callback form observes normal stream completion too.
        // A shared once-guard makes the first terminal event sufficient for
        // this lightweight stream model and prevents the duplex sequence
        // (`finish`, `end`, `close`) from invoking the callback repeatedly.
        add_finished_cleanup_completion_listener(
            stream.get_nanbox_f64(),
            callback.get_nanbox_f64(),
        );
    }
    if let Some(signal) = options_signal(options.get_nanbox_f64()) {
        add_finished_signal_abort_listener(
            stream.get_nanbox_f64(),
            signal,
            callback.get_nanbox_f64(),
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `stream.pipeline(...streams, cb)` wires classic streams end-to-end and
/// invokes the callback once on success or on the first observed error.
#[no_mangle]
pub extern "C" fn js_node_stream_pipeline(args: *const crate::array::ArrayHeader) -> f64 {
    // `Readable.from`, the listener allocations, the `'pipe'`/`'resume'`
    // listeners and each stage's `_read` can all collect, so the stage list is
    // a rooted GC array and every stage is read from it just before use.
    let scope = crate::gc::RuntimeHandleScope::new();
    let args = rooted_args_copy(&scope, args);
    let mut len = rooted_array_len(&args);
    if len == 0 {
        throw_pipeline_missing_streams();
    }

    let callback = scope.root_nanbox_f64(rooted_array_at(&args, len - 1));
    if !is_callable_value(callback.get_nanbox_f64()) {
        throw_pipeline_callback_required(callback.get_nanbox_f64());
    }
    len -= 1;

    let options = PipelineOptions {
        end_final: true,
        signal: None,
    };
    if len > 0 && is_pipeline_options_arg(rooted_array_at(&args, len - 1)) {
        throw_pipeline_invalid_body(rooted_array_at(&args, len - 1));
    }

    let stages = if len == 1 && is_array_like_value(rooted_array_at(&args, 0)) {
        let list = scope.root_nanbox_f64(rooted_array_at(&args, 0));
        rooted_array_copy(&scope, &list, u32::MAX)
    } else {
        rooted_array_copy(&scope, &args, len)
    };
    let len = rooted_array_len(&stages);
    if len < 2 {
        throw_pipeline_missing_streams();
    }

    if pipeline_needs_collected_path(&stages) {
        return run_collected_pipeline(&stages, &callback, options);
    }

    let source = normalize_pipeline_source(rooted_array_at(&stages, 0), 0);
    rooted_array_set(&stages, 0, source);
    add_pipeline_callback_listeners(&stages, &callback, options);

    for i in 0..len - 1 {
        let is_final_pair = i + 1 == len - 1;
        wire_pipeline_pair(
            rooted_array_at(&stages, i),
            rooted_array_at(&stages, i + 1),
            options.end_final || !is_final_pair,
        );
    }
    for i in 0..len - 1 {
        start_pipeline_readable(rooted_array_at(&stages, i));
    }

    rooted_array_at(&stages, len - 1)
}

pub(crate) extern "C" fn duplex_pair_write_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    chunk: f64,
    _encoding: f64,
    cb: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // The peer's `'data'` listeners can collect.
    let scope = crate::gc::RuntimeHandleScope::new();
    let peer = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let cb = scope.root_nanbox_f64(cb);
    if get_hidden_value(peer.get_nanbox_f64(), hidden_readable_flag_key()).is_some()
        && !stream_destroyed(peer.get_nanbox_f64())
    {
        if readable_is_flowing(peer.get_nanbox_f64()) {
            emit_readable_data(peer.get_nanbox_f64(), chunk);
        } else {
            buffer_pending_readable_chunk(peer.get_nanbox_f64(), chunk);
        }
    }
    call_listener_args(peer.get_nanbox_f64(), cb.get_nanbox_f64(), &[]);
    f64::from_bits(TAG_UNDEFINED)
}

pub(crate) extern "C" fn duplex_pair_final_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    cb: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let peer = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let cb = scope.root_nanbox_f64(cb);
    schedule_readable_end(peer.get_nanbox_f64());
    call_listener_args(peer.get_nanbox_f64(), cb.get_nanbox_f64(), &[]);
    f64::from_bits(TAG_UNDEFINED)
}

fn install_duplex_pair_endpoint(endpoint: f64, peer: f64) {
    if raw_ptr_from_value(endpoint) < 0x10000 {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let endpoint = scope.root_nanbox_f64(endpoint);
    let peer = scope.root_nanbox_f64(peer);
    let write = rooted_closure(
        &scope,
        crate::fn_info!(duplex_pair_write_callback, 3; with_declared(3)),
        &[&peer],
    );
    set_hidden_closure(&endpoint, hidden_write_key(), &write);

    let final_cb = rooted_closure(
        &scope,
        crate::fn_info!(duplex_pair_final_callback, 1; with_declared(1)),
        &[&peer],
    );
    set_hidden_closure(&endpoint, hidden_writable_final_key(), &final_cb);

    set_hidden_value(
        endpoint.get_nanbox_f64(),
        hidden_key(b"duplexPairPeer"),
        peer.get_nanbox_f64(),
    );
    set_hidden_value(
        endpoint.get_nanbox_f64(),
        hidden_key(b"writableCustomSink"),
        f64::from_bits(TAG_TRUE),
    );
}

/// #1539: `stream.duplexPair([options])` returns a two-element array
/// `[Duplex, Duplex]` where writes to one show up as reads on the
/// other and vice versa.
#[no_mangle]
pub extern "C" fn js_node_stream_duplex_pair(_opts: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let a = scope.root_nanbox_f64(js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED)));
    let b = scope.root_nanbox_f64(js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED)));
    install_duplex_pair_endpoint(a.get_nanbox_f64(), b.get_nanbox_f64());
    install_duplex_pair_endpoint(b.get_nanbox_f64(), a.get_nanbox_f64());
    let pair = rooted_empty_array(&scope);
    rooted_array_push(&pair, a.get_nanbox_f64());
    rooted_array_push(&pair, b.get_nanbox_f64());
    pair.get_nanbox_f64()
}
