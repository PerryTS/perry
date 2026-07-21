//! node:stream — static introspection helpers (`Readable.isDisturbed`,
//! `isReadable`, `isWritable`, `isDestroyed`, the `_isUint8Array` /
//! `_isArrayBufferView` type predicates), default-highWaterMark accessors and
//! abort-signal wiring (split out of node_stream_constructors.rs for the
//! 2000-line file-size gate, #1987).
use super::super::*;
use super::*;
use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr};
use crate::value::JSValue;

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

/// #2685: `stream.isDestroyed(s)`. Node returns `null` for non-streams and a
/// boolean for real stream instances.
#[no_mangle]
pub extern "C" fn js_node_stream_is_destroyed(stream: f64) -> f64 {
    if !is_classic_stream_instance_value(stream) {
        return f64::from_bits(TAG_NULL);
    }
    f64::from_bits(if stream_destroyed(stream) {
        TAG_TRUE
    } else {
        TAG_FALSE
    })
}

pub(crate) fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

fn stream_value_addr(value: f64) -> Option<usize> {
    let jsv = JSValue::from_bits(value.to_bits());
    if !jsv.is_pointer() {
        return None;
    }
    let addr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    if addr < 0x10000 {
        None
    } else {
        Some(addr)
    }
}

/// #2685: `stream._isArrayBufferView(value)` aliases Node's stream-local
/// helper semantics, where Buffer counts as an ArrayBuffer view.
#[no_mangle]
pub extern "C" fn js_node_stream_is_array_buffer_view(value: f64) -> f64 {
    let Some(addr) = stream_value_addr(value) else {
        return f64::from_bits(TAG_FALSE);
    };
    let registered_view = crate::buffer::is_registered_buffer(addr)
        && (!crate::buffer::is_any_array_buffer(addr)
            || crate::buffer::is_uint8array_buffer(addr)
            || crate::buffer::is_data_view(addr));
    bool_value(registered_view || crate::typedarray::lookup_typed_array_kind(addr).is_some())
}

/// #2685: `stream._isUint8Array(value)` returns true for Buffer as well as
/// Uint8Array instances, matching Node's internal type predicate.
#[no_mangle]
pub extern "C" fn js_node_stream_is_uint8_array(value: f64) -> f64 {
    let Some(addr) = stream_value_addr(value) else {
        return f64::from_bits(TAG_FALSE);
    };
    let registered_uint8 = crate::buffer::is_registered_buffer(addr)
        && (crate::buffer::is_uint8array_buffer(addr)
            || (!crate::buffer::is_any_array_buffer(addr) && !crate::buffer::is_data_view(addr)));
    bool_value(
        registered_uint8
            || crate::typedarray::lookup_typed_array_kind(addr)
                == Some(crate::typedarray::KIND_UINT8),
    )
}

fn stream_byte_view_bytes(value: f64) -> Vec<u8> {
    let Some(addr) = stream_value_addr(value) else {
        return Vec::new();
    };
    if crate::buffer::is_any_array_buffer(addr)
        && !crate::buffer::is_uint8array_buffer(addr)
        && !crate::buffer::is_data_view(addr)
    {
        return Vec::new();
    }
    if crate::buffer::is_registered_buffer(addr) {
        let data = crate::buffer::js_native_buffer_data_ptr(value);
        let len = crate::buffer::js_native_buffer_byte_len(value);
        if data.is_null() || len == 0 {
            return Vec::new();
        }
        return unsafe { std::slice::from_raw_parts(data, len).to_vec() };
    }
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        let ta = addr as *const crate::typedarray::TypedArrayHeader;
        return unsafe {
            crate::typedarray::typed_array_bytes(ta)
                .map(|bytes| bytes.to_vec())
                .unwrap_or_default()
        };
    }
    Vec::new()
}

/// #2685: `stream._uint8ArrayToBuffer(view)` returns a Buffer containing the
/// bytes visible through the passed ArrayBuffer view.
#[no_mangle]
pub extern "C" fn js_node_stream_uint8_array_to_buffer(value: f64) -> f64 {
    buffer_value_from_bytes(&stream_byte_view_bytes(value))
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

pub(crate) fn attach_abort_signal(signal: f64, stream: f64) {
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
