use perry_ffi::get_handle;

use crate::http2_server::{
    bind_handle_method, bool_value, empty_object_value, pairs_to_js_object, Http2StreamHandle,
};
use crate::request::handle_to_pointer_f64;
use crate::types::TAG_UNDEFINED;

#[no_mangle]
pub unsafe extern "C" fn js_ext_http2_stream_dispatch_property(
    handle: i64,
    property_ptr: *const u8,
    property_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    let property = String::from_utf8_lossy(std::slice::from_raw_parts(property_ptr, property_len))
        .into_owned();
    match property.as_str() {
        "on" => bind_handle_method(handle, b"on"),
        "addListener" => bind_handle_method(handle, b"addListener"),
        "setEncoding" => bind_handle_method(handle, b"setEncoding"),
        "respond" => bind_handle_method(handle, b"respond"),
        "end" => bind_handle_method(handle, b"end"),
        "close" => bind_handle_method(handle, b"close"),
        "setTimeout" => bind_handle_method(handle, b"setTimeout"),
        "priority" => bind_handle_method(handle, b"priority"),
        "additionalHeaders" => bind_handle_method(handle, b"additionalHeaders"),
        "pushStream" => bind_handle_method(handle, b"pushStream"),
        "respondWithFD" => bind_handle_method(handle, b"respondWithFD"),
        "respondWithFile" => bind_handle_method(handle, b"respondWithFile"),
        "sendTrailers" => bind_handle_method(handle, b"sendTrailers"),
        "id" => get_handle::<Http2StreamHandle>(handle)
            .map(|s| s.id as f64)
            .unwrap_or(0.0),
        "pending" => bool_value(
            get_handle::<Http2StreamHandle>(handle)
                .map(|s| s.pending)
                .unwrap_or(false),
        ),
        "closed" => bool_value(
            get_handle::<Http2StreamHandle>(handle)
                .map(|s| s.closed)
                .unwrap_or(false),
        ),
        "destroyed" => bool_value(
            get_handle::<Http2StreamHandle>(handle)
                .map(|s| s.destroyed)
                .unwrap_or(false),
        ),
        "aborted" => bool_value(
            get_handle::<Http2StreamHandle>(handle)
                .map(|s| s.aborted)
                .unwrap_or(false),
        ),
        "rstCode" => get_handle::<Http2StreamHandle>(handle)
            .map(|s| s.rst_code as f64)
            .unwrap_or(0.0),
        "headersSent" => bool_value(
            get_handle::<Http2StreamHandle>(handle)
                .map(|s| s.headers_sent)
                .unwrap_or(false),
        ),
        "sentHeaders" => get_handle::<Http2StreamHandle>(handle)
            .map(|s| pairs_to_js_object(&s.sent_headers))
            .unwrap_or(undef),
        "session" => get_handle::<Http2StreamHandle>(handle)
            .map(|s| handle_to_pointer_f64(s.session_handle))
            .unwrap_or(undef),
        "state" => empty_object_value(),
        "bufferSize" => 0.0,
        "endAfterHeaders" => bool_value(false),
        _ => undef,
    }
}
