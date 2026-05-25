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
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader,
};
use crate::object::{js_object_get_field_by_name_f64, ObjectHeader};
use crate::string::js_string_from_bytes;
use crate::value::JSValue;

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

pub(crate) extern "C" fn thunk_streamP_pipeline(
    _closure: *const ClosureHeader,
    source: f64,
    destination: f64,
    options: f64,
) -> f64 {
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

pub(crate) extern "C" fn thunk_streamP_finished(
    _closure: *const ClosureHeader,
    stream: f64,
    options: f64,
) -> f64 {
    if let Some(signal) = options_signal(options) {
        if signal_aborted(signal) {
            return promise_rejected(signal_reason(signal));
        }
        if let Some(err) = crate::node_stream::js_node_stream_hidden_error_after_read(stream) {
            return promise_rejected(err);
        }
        if crate::node_stream::js_node_stream_is_stub_ended_after_read(stream) {
            return promise_undefined();
        }
        return pending_abortable_promise(signal);
    }

    if let Some(err) = crate::node_stream::js_node_stream_hidden_error_after_read(stream) {
        promise_rejected(err)
    } else {
        promise_undefined()
    }
}
