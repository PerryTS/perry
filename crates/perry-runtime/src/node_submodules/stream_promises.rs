//! `node:stream/promises` (`pipeline`, `finished`) — concrete thunks plus the
//! AbortSignal / object-property plumbing they need (#1588).
//!
//! Extracted from `mod.rs` so the parent module stays under the file-size
//! gate. Pure code movement — no logic changes.
//!
//! NOTE: this module keeps its own copies of `raw_ptr_from_value` /
//! `gc_type_for_ptr` / `object_ptr_from_value`. The stream/consumers module
//! has near-identical helpers with slightly different signatures (`*const`
//! vs `*mut ObjectHeader`); they intentionally live in separate module scopes
//! so the names don't collide at module root.

use super::fs_promises::{promise_rejected, promise_undefined};
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader,
};
use crate::string::js_string_from_bytes;
use crate::value::JSValue;
use std::os::raw::c_int;

#[inline]
pub(crate) fn undefined_value() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[inline]
pub(crate) fn value_from_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
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

pub(crate) fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
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

fn array_ptr_from_value(value: f64) -> Option<*const crate::array::ArrayHeader> {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_ARRAY) {
            return None;
        }
    }
    Some(raw as *const crate::array::ArrayHeader)
}

fn array_values(value: f64) -> Option<Vec<f64>> {
    let arr = array_ptr_from_value(value)?;
    let len = crate::array::js_array_length(arr);
    let mut values = Vec::with_capacity(len as usize);
    for i in 0..len {
        values.push(crate::array::js_array_get_f64(arr, i));
    }
    Some(values)
}

pub(crate) fn get_object_property(value: f64, name: &[u8]) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, key);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        None
    } else {
        Some(value)
    }
}

pub(crate) fn options_signal(options: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(options.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        return None;
    }
    get_object_property(options, b"signal")
}

pub(crate) fn signal_aborted(signal: f64) -> bool {
    get_object_property(signal, b"aborted").is_some_and(|v| crate::value::js_is_truthy(v) != 0)
}

pub(crate) fn abort_error_value() -> f64 {
    let msg = b"The operation was aborted";
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_error_new_with_name_message(b"AbortError", msg_ptr);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ABORT_ERR");
    value_from_ptr(err as *const u8)
}

fn premature_close_error_value() -> f64 {
    let msg = b"Premature close";
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_error_new_with_name_message(b"Error", msg_ptr);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ERR_STREAM_PREMATURE_CLOSE");
    value_from_ptr(err as *const u8)
}

fn type_error_value_with_code(message: &str, code: &'static str) -> f64 {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    value_from_ptr(crate::error::js_typeerror_new(msg) as *const u8)
}

fn missing_streams_error_value() -> f64 {
    type_error_value_with_code(
        "The \"streams\" argument must be specified",
        "ERR_MISSING_ARGS",
    )
}

fn invalid_finished_stream_error_value(stream: f64) -> f64 {
    let message = format!(
        "The \"stream\" argument must be an instance of ReadableStream, WritableStream, or Stream. Received {}",
        crate::fs::validate::describe_received(stream)
    );
    type_error_value_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn invalid_pipeline_body_error_value(body: f64) -> f64 {
    let message = format!(
        "The \"body\" argument must be of type function or an instance of Blob, ReadableStream, WritableStream, Stream, Iterable, AsyncIterable, or Promise or {{ readable, writable }} pair. Received {}",
        crate::fs::validate::describe_received(body)
    );
    type_error_value_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

pub(crate) fn signal_reason(signal: f64) -> f64 {
    match get_object_property(signal, b"reason") {
        Some(reason) if !JSValue::from_bits(reason.to_bits()).is_undefined() => reason,
        _ => abort_error_value(),
    }
}

extern "C" fn stream_promises_abort_listener(closure: *const ClosureHeader) -> f64 {
    let promise_value = js_closure_get_capture_f64(closure, 0);
    let signal = js_closure_get_capture_f64(closure, 1);
    let promise =
        crate::value::js_nanbox_get_pointer(promise_value) as *mut crate::promise::Promise;
    crate::promise::js_promise_reject(promise, signal_reason(signal));
    undefined_value()
}

fn promise_value_from_ptr(promise: *mut crate::promise::Promise) -> f64 {
    value_from_ptr(promise as *const u8)
}

pub(crate) fn register_abort_listener(signal: f64, promise: *mut crate::promise::Promise) {
    let Some(signal_obj) = object_ptr_from_value(signal) else {
        return;
    };
    let closure = js_closure_alloc(stream_promises_abort_listener as *const u8, 2);
    js_closure_set_capture_f64(closure, 0, promise_value_from_ptr(promise));
    js_closure_set_capture_f64(closure, 1, signal);
    let event = b"abort";
    let event_str = js_string_from_bytes(event.as_ptr(), event.len() as u32);
    let event_value = f64::from_bits(JSValue::string_ptr(event_str).bits());
    let listener_value = value_from_ptr(closure as *const u8);
    crate::url::js_abort_signal_add_listener(signal_obj, event_value, listener_value);
}

fn pending_abortable_promise(signal: f64) -> f64 {
    let promise = crate::promise::js_promise_new();
    register_abort_listener(signal, promise);
    promise_value_from_ptr(promise)
}

fn event_value(name: &[u8]) -> f64 {
    let ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

extern "C" fn stream_promises_finished_error_listener(
    closure: *const ClosureHeader,
    err: f64,
) -> f64 {
    let state = js_closure_get_capture_ptr(closure, 0) as *mut ObjectHeader;
    finished_state_reject(state, err);
    undefined_value()
}

extern "C" fn stream_promises_finished_side_listener(closure: *const ClosureHeader) -> f64 {
    let state = js_closure_get_capture_ptr(closure, 0) as *mut ObjectHeader;
    let readable_side = js_closure_get_capture_f64(closure, 1).to_bits() == crate::value::TAG_TRUE;
    if readable_side {
        finished_state_set_bool(state, b"readableDone", true);
    } else {
        finished_state_set_bool(state, b"writableDone", true);
    }
    finished_state_try_resolve(state);
    undefined_value()
}

extern "C" fn stream_promises_finished_close_listener(closure: *const ClosureHeader) -> f64 {
    let state = js_closure_get_capture_ptr(closure, 0) as *mut ObjectHeader;
    if finished_state_bool(state, b"settled") {
        return undefined_value();
    }
    let stream = finished_state_value(state, b"stream");
    if let Some(err) = crate::node_stream::js_node_stream_hidden_error(stream) {
        finished_state_reject(state, err);
        return undefined_value();
    }
    finished_state_refresh_done(state, stream);
    if !finished_state_try_resolve(state) {
        finished_state_reject(state, premature_close_error_value());
    }
    undefined_value()
}

fn listener_value(listener: *mut ClosureHeader) -> f64 {
    value_from_ptr(listener as *const u8)
}

fn add_once_listener(stream_handle: i64, event: &[u8], listener: f64) {
    let event = event_value(event);
    let _ = crate::node_stream::js_node_stream_method_once(stream_handle, event, listener);
}

fn register_finished_listener_arities() {
    crate::closure::js_register_closure_arity(
        stream_promises_finished_error_listener as *const u8,
        1,
    );
    crate::closure::js_register_closure_arity(
        stream_promises_finished_side_listener as *const u8,
        0,
    );
    crate::closure::js_register_closure_arity(
        stream_promises_finished_close_listener as *const u8,
        0,
    );
}

fn finished_state_key(name: &[u8]) -> *mut crate::string::StringHeader {
    js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn finished_state_value(state: *mut ObjectHeader, name: &[u8]) -> f64 {
    if state.is_null() {
        return undefined_value();
    }
    js_object_get_field_by_name_f64(state as *const ObjectHeader, finished_state_key(name))
}

fn finished_state_set(state: *mut ObjectHeader, name: &[u8], value: f64) {
    if state.is_null() {
        return;
    }
    js_object_set_field_by_name(state, finished_state_key(name), value);
}

fn finished_state_bool(state: *mut ObjectHeader, name: &[u8]) -> bool {
    finished_state_value(state, name).to_bits() == crate::value::TAG_TRUE
}

fn finished_state_set_bool(state: *mut ObjectHeader, name: &[u8], value: bool) {
    finished_state_set(
        state,
        name,
        f64::from_bits(if value {
            crate::value::TAG_TRUE
        } else {
            crate::value::TAG_FALSE
        }),
    );
}

fn finished_state_promise(state: *mut ObjectHeader) -> *mut crate::promise::Promise {
    crate::value::js_nanbox_get_pointer(finished_state_value(state, b"promise"))
        as *mut crate::promise::Promise
}

fn finished_state_resolve(state: *mut ObjectHeader) {
    if finished_state_bool(state, b"settled") {
        return;
    }
    finished_state_set_bool(state, b"settled", true);
    crate::promise::js_promise_resolve(finished_state_promise(state), undefined_value());
}

fn finished_state_reject(state: *mut ObjectHeader, err: f64) {
    if finished_state_bool(state, b"settled") {
        return;
    }
    finished_state_set_bool(state, b"settled", true);
    crate::promise::js_promise_reject(finished_state_promise(state), err);
}

fn finished_state_try_resolve(state: *mut ObjectHeader) -> bool {
    if finished_state_bool(state, b"settled") {
        return true;
    }
    let readable_done =
        !finished_state_bool(state, b"needReadable") || finished_state_bool(state, b"readableDone");
    let writable_done =
        !finished_state_bool(state, b"needWritable") || finished_state_bool(state, b"writableDone");
    if readable_done && writable_done {
        finished_state_resolve(state);
        true
    } else {
        false
    }
}

fn finished_state_refresh_done(state: *mut ObjectHeader, stream: f64) {
    if crate::node_stream::js_node_stream_readable_side_done(stream) {
        finished_state_set_bool(state, b"readableDone", true);
    }
    if crate::node_stream::js_node_stream_writable_side_done(stream) {
        finished_state_set_bool(state, b"writableDone", true);
    }
}

fn finished_option_enabled(options: f64, name: &[u8]) -> bool {
    get_object_property(options, name)
        .map(|value| value.to_bits() != crate::value::TAG_FALSE)
        .unwrap_or(true)
}

fn finished_required_sides(stream: f64, options: f64) -> (bool, bool) {
    let need_readable = finished_option_enabled(options, b"readable")
        && crate::node_stream::js_node_stream_has_readable_side(stream);
    let need_writable = finished_option_enabled(options, b"writable")
        && crate::node_stream::js_node_stream_has_writable_side(stream);
    (need_readable, need_writable)
}

fn finished_sides_done(stream: f64, need_readable: bool, need_writable: bool) -> bool {
    (!need_readable || crate::node_stream::js_node_stream_readable_side_done(stream))
        && (!need_writable || crate::node_stream::js_node_stream_writable_side_done(stream))
}

fn pending_finished_promise(
    stream: f64,
    signal: Option<f64>,
    need_readable: bool,
    need_writable: bool,
) -> f64 {
    register_finished_listener_arities();
    let promise = crate::promise::js_promise_new();
    let handle = raw_ptr_from_value(stream) as i64;
    if handle == 0 {
        crate::promise::js_promise_resolve(promise, undefined_value());
        return promise_value_from_ptr(promise);
    }

    let state = js_object_alloc(0, 7);
    finished_state_set(state, b"promise", promise_value_from_ptr(promise));
    finished_state_set(state, b"stream", stream);
    finished_state_set_bool(state, b"settled", false);
    finished_state_set_bool(state, b"needReadable", need_readable);
    finished_state_set_bool(state, b"needWritable", need_writable);
    finished_state_set_bool(
        state,
        b"readableDone",
        crate::node_stream::js_node_stream_readable_side_done(stream),
    );
    finished_state_set_bool(
        state,
        b"writableDone",
        crate::node_stream::js_node_stream_writable_side_done(stream),
    );
    if finished_state_try_resolve(state) {
        return promise_value_from_ptr(promise);
    }

    let error_listener = js_closure_alloc(stream_promises_finished_error_listener as *const u8, 1);
    js_closure_set_capture_ptr(error_listener, 0, state as i64);
    add_once_listener(handle, b"error", listener_value(error_listener));

    if need_readable {
        let end_listener = js_closure_alloc(stream_promises_finished_side_listener as *const u8, 2);
        js_closure_set_capture_ptr(end_listener, 0, state as i64);
        js_closure_set_capture_f64(end_listener, 1, f64::from_bits(crate::value::TAG_TRUE));
        add_once_listener(handle, b"end", listener_value(end_listener));
    }
    if need_writable {
        let finish_listener =
            js_closure_alloc(stream_promises_finished_side_listener as *const u8, 2);
        js_closure_set_capture_ptr(finish_listener, 0, state as i64);
        js_closure_set_capture_f64(finish_listener, 1, f64::from_bits(crate::value::TAG_FALSE));
        add_once_listener(handle, b"finish", listener_value(finish_listener));
    }

    let close_listener = js_closure_alloc(stream_promises_finished_close_listener as *const u8, 1);
    js_closure_set_capture_ptr(close_listener, 0, state as i64);
    add_once_listener(handle, b"close", listener_value(close_listener));

    if let Some(signal) = signal {
        register_abort_listener(signal, promise);
    }

    promise_value_from_ptr(promise)
}

fn invoke_destination_method(destination: f64, method: &[u8], args: &[f64]) -> f64 {
    let Some(func) = get_object_property(destination, method) else {
        return undefined_value();
    };
    let prev_this = crate::object::js_implicit_this_set(destination);
    let result = unsafe { crate::closure::js_native_call_value(func, args.as_ptr(), args.len()) };
    crate::object::js_implicit_this_set(prev_this);
    result
}

fn write_chunks_to_destination(destination: f64, chunks: &[f64]) {
    let undef = undefined_value();
    for chunk in chunks {
        let args = [*chunk, undef];
        let _ = invoke_destination_method(destination, b"write", &args);
    }
    let end_args = [undef];
    let _ = invoke_destination_method(destination, b"end", &end_args);
}

fn is_missing_pipeline_arg(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_undefined()
}

fn is_invalid_pipeline_body(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    jsval.is_undefined()
        || jsval.is_null()
        || jsval.is_bool()
        || jsval.is_number()
        || jsval.is_int32()
        || jsval.is_bigint()
        || unsafe { crate::symbol::js_is_symbol(value) != 0 }
}

fn validate_stream_promises_pipeline_args(
    source: f64,
    destination: f64,
    rest: &[f64],
) -> Result<(), f64> {
    if is_missing_pipeline_arg(source) || is_missing_pipeline_arg(destination) {
        return Err(missing_streams_error_value());
    }
    for body in std::iter::once(source)
        .chain(std::iter::once(destination))
        .chain(rest.iter().copied())
    {
        if is_invalid_pipeline_body(body) {
            return Err(invalid_pipeline_body_error_value(body));
        }
    }
    Ok(())
}

fn direct_stream_promises_pipeline(source: f64, destination: f64, options: f64) -> f64 {
    if let Err(err) = validate_stream_promises_pipeline_args(source, destination, &[]) {
        return promise_rejected(err);
    }
    let signal = options_signal(options);
    if let Some(signal) = signal {
        if signal_aborted(signal) {
            return promise_rejected(signal_reason(signal));
        }
    }

    match crate::node_stream::js_node_stream_readable_chunks_result(source) {
        Err(err) => promise_rejected(err),
        Ok(Some(chunks)) => {
            write_chunks_to_destination(destination, &chunks);
            if let Some(signal) = signal {
                if signal_aborted(signal) {
                    return promise_rejected(signal_reason(signal));
                }
            }
            promise_undefined()
        }
        Ok(None) => {
            if let Some(signal) = signal {
                pending_abortable_promise(signal)
            } else if let Some(err) =
                crate::node_stream::js_node_stream_hidden_error_after_read(source)
            {
                promise_rejected(err)
            } else {
                promise_undefined()
            }
        }
    }
}

extern "C" fn stream_promises_pipeline_callback(
    closure: *const ClosureHeader,
    err: f64,
    value: f64,
) -> f64 {
    if closure.is_null() {
        return undefined_value();
    }
    let promise_value = js_closure_get_capture_f64(closure, 0);
    let promise =
        crate::value::js_nanbox_get_pointer(promise_value) as *mut crate::promise::Promise;
    let err_value = JSValue::from_bits(err.to_bits());
    if err_value.is_undefined() || err_value.is_null() {
        crate::promise::js_promise_resolve(promise, value);
    } else {
        crate::promise::js_promise_reject(promise, err);
    }
    undefined_value()
}

fn catch_stream_promises_throw(call: impl FnOnce()) -> Result<(), f64> {
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut c_int) };
    if jumped == 0 {
        call();
        crate::exception::js_try_end();
        Ok(())
    } else {
        let err = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        crate::exception::js_try_end();
        Err(err)
    }
}

pub(crate) extern "C" fn thunk_streamP_pipeline(
    _closure: *const ClosureHeader,
    source: f64,
    destination: f64,
    options_or_rest: f64,
) -> f64 {
    let rest_values = match array_values(options_or_rest) {
        Some(values) => values,
        None => return direct_stream_promises_pipeline(source, destination, options_or_rest),
    };

    if let Err(err) = validate_stream_promises_pipeline_args(source, destination, &rest_values) {
        return promise_rejected(err);
    }

    let promise = crate::promise::js_promise_new();
    let promise_value = promise_value_from_ptr(promise);

    crate::closure::js_register_closure_arity(stream_promises_pipeline_callback as *const u8, 2);
    let callback = js_closure_alloc(stream_promises_pipeline_callback as *const u8, 1);
    js_closure_set_capture_f64(callback, 0, promise_value);

    let mut args = crate::array::js_array_alloc(4);
    args = crate::array::js_array_push_f64(args, source);
    args = crate::array::js_array_push_f64(args, destination);

    for value in rest_values {
        args = crate::array::js_array_push_f64(args, value);
    }
    args = crate::array::js_array_push_f64(args, value_from_ptr(callback as *const u8));

    if let Err(err) = catch_stream_promises_throw(|| {
        crate::node_stream::js_node_stream_pipeline(args as *const crate::array::ArrayHeader);
    }) {
        crate::promise::js_promise_reject(promise, err);
    }

    promise_value
}

pub(crate) extern "C" fn thunk_streamP_finished(
    _closure: *const ClosureHeader,
    stream: f64,
    options: f64,
) -> f64 {
    if !crate::node_stream::js_node_stream_is_classic_stream(stream) {
        return promise_rejected(invalid_finished_stream_error_value(stream));
    }

    let signal = options_signal(options);
    if let Some(signal) = signal {
        if signal_aborted(signal) {
            return promise_rejected(signal_reason(signal));
        }
    }

    if let Some(err) = crate::node_stream::js_node_stream_hidden_error(stream) {
        promise_rejected(err)
    } else {
        let (need_readable, need_writable) = finished_required_sides(stream, options);
        if finished_sides_done(stream, need_readable, need_writable) {
            return promise_undefined();
        }
        pending_finished_promise(stream, signal, need_readable, need_writable)
    }
}
