//! `ReadableStream.prototype.pipeThrough`: argument validation and transform-pair resolution.

use super::{
    is_object_value, js_nanbox_get_pointer, js_object_get_field_by_name,
    js_promise_mark_internally_handled, js_readable_stream_pipe_to, js_stream_unwrap_handle,
    js_string_from_bytes, js_transform_stream_readable, js_transform_stream_writable, rs_exists,
    stream_id, subclass, throw_type_error, with_rs, ws_rec, ObjectHeader, RsRef, WsRef,
    TAG_UNDEFINED,
};

pub(super) fn pipe_through_validation_error(
    readable_id: usize,
    writable_id: usize,
    output_id: usize,
) -> Option<&'static str> {
    match with_rs(RsRef(readable_id), |s| s.reader_handle.is_some()) {
        Some(true) => return Some("ReadableStream is locked"),
        Some(false) => {}
        None => return Some("Invalid ReadableStream"),
    }
    if !rs_exists(RsRef(output_id)) {
        return Some("Invalid transform readable");
    }
    match ws_rec(WsRef(writable_id)).as_deref() {
        Some(stream) if stream.writer_handle.is_some() => Some("WritableStream is locked"),
        Some(_) => None,
        None => Some("Invalid transform writable"),
    }
}

pub(super) unsafe fn pipe_through_options_error(options: f64) -> Option<&'static str> {
    let signal = perry_runtime::value::js_get_property(options, b"signal".as_ptr() as i64, 6);
    pipe_through_signal_error(signal)
}

pub(super) fn pipe_through_signal_error(signal: f64) -> Option<&'static str> {
    (signal.to_bits() != TAG_UNDEFINED
        && perry_runtime::event_target_hook_slots::js_abort_signal_resolve_ptr(signal).is_null())
    .then_some("The options.signal property must be an AbortSignal")
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_pipe_through_validate(
    readable_handle: f64,
    transform_writable_handle: f64,
    transform_readable_handle: f64,
    options: f64,
) -> f64 {
    if let Some(message) = pipe_through_validation_error(
        stream_id(readable_handle),
        stream_id(transform_writable_handle),
        stream_id(transform_readable_handle),
    ) {
        throw_type_error(message);
    }
    if let Some(message) = pipe_through_options_error(options) {
        throw_type_error(message);
    }
    transform_readable_handle
}

/// Legacy three-argument ABI shared with the external streams library.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_pipe_through(
    readable_handle: f64,
    transform_writable_handle: f64,
    transform_readable_handle: f64,
) -> f64 {
    let output = js_readable_stream_pipe_through_validate(
        readable_handle,
        transform_writable_handle,
        transform_readable_handle,
        f64::from_bits(TAG_UNDEFINED),
    );
    let pipe = js_readable_stream_pipe_to(
        readable_handle,
        transform_writable_handle,
        f64::from_bits(TAG_UNDEFINED),
    );
    js_promise_mark_internally_handled(pipe);
    output
}

/// Resolve a WHATWG transform pair and start piping, keeping object-backed
/// transforms and options live across arbitrary getter callbacks. Numeric
/// TransformStream handles without options take no runtime handle slots.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_pipe_through_pair(
    readable_handle: f64,
    pair: f64,
    options: f64,
) -> f64 {
    if is_object_value(pair) || is_object_value(options) {
        return pipe_through_rooted_pair(readable_handle, pair, options);
    }
    let transform = js_stream_unwrap_handle(pair);
    let readable = js_transform_stream_readable(transform);
    let writable = js_transform_stream_writable(transform);
    let output =
        js_readable_stream_pipe_through_validate(readable_handle, writable, readable, options);
    let pipe = js_readable_stream_pipe_to(readable_handle, writable, options);
    js_promise_mark_internally_handled(pipe);
    output
}

// Keep the scoped-root frame out of the stream-handle path. The object path
// needs those roots across arbitrary getter callbacks; stream handles do not.
#[cold]
#[inline(never)]
pub(super) unsafe fn pipe_through_rooted_pair(
    readable_handle: f64,
    pair: f64,
    options: f64,
) -> f64 {
    let pair_is_object = is_object_value(pair);
    let options_is_object = is_object_value(options);
    // The caller enters this cold helper only for object-backed pairs or
    // options. The scope always exists here; individual numeric values need
    // no handle slots.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let pair_root = pair_is_object.then(|| scope.root_nanbox_f64(pair));
    let options_root = options_is_object.then(|| scope.root_nanbox_f64(options));
    let transform = match pair_root {
        Some(root) => subclass::unwrap_rooted_pair_stream_handle(root),
        None => pair,
    };
    let transform_root = is_object_value(transform).then(|| {
        // Unwrapping can move the receiver. Compare with the root only
        // after it returns, and reuse its slot when the pair is itself
        // the transform. A distinct object still needs its own root.
        pair_root
            .filter(|root| root.get_nanbox_f64().to_bits() == transform.to_bits())
            .unwrap_or_else(|| scope.root_nanbox_f64(transform))
    });
    let transform_value = || {
        transform_root
            .as_ref()
            .map(|root| root.get_nanbox_f64())
            .unwrap_or(transform)
    };
    let options_value = || {
        options_root
            .as_ref()
            .map(|root| root.get_nanbox_f64())
            .unwrap_or(options)
    };
    let transform_field = |name: &[u8]| {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        // The key allocation can move the pair. Read the already-rooted
        // receiver only after it, and again for each getter.
        let object = js_nanbox_get_pointer(transform_value()) as *const ObjectHeader;
        f64::from_bits(js_object_get_field_by_name(object, key).bits())
    };
    let (readable, writable) = if is_object_value(transform) {
        // Keep object-specific property dispatch out of the numeric endpoint
        // getters, so merely reading TransformStream endpoints does not link
        // the generic object-property machinery into an otherwise lean binary.
        let readable = subclass::unwrap_pair_stream_handle(transform_field(b"readable"));
        let writable = subclass::unwrap_pair_stream_handle(transform_field(b"writable"));
        (readable, writable)
    } else {
        (
            js_transform_stream_readable(transform),
            js_transform_stream_writable(transform),
        )
    };
    let output = js_readable_stream_pipe_through_validate(
        readable_handle,
        writable,
        readable,
        options_value(),
    );
    let pipe = js_readable_stream_pipe_to(readable_handle, writable, options_value());
    js_promise_mark_internally_handled(pipe);
    output
}
