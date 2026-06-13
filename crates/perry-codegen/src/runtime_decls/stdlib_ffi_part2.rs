//! Stdlib / FFI runtime function declarations, continued — split from
//! stdlib_ffi.rs to stay under the 2000-line CI cap (same recipe as
//! strings.rs / strings_part2.rs). New stdlib FFI declarations go here.

use super::*;

/// Continuation of `declare_stdlib_ffi` — must be called wherever that is.
pub(crate) fn declare_stdlib_ffi_part2(module: &mut LlModule) {
    // ========== node:http / node:https (continued) ==========
    // #5011 — ref()/unref() return the receiver handle (Node returns `this`).
    module.declare_function("js_node_http_server_ref", I64, &[I64]);
    module.declare_function("js_node_http_server_unref", I64, &[I64]);
    module.declare_function("js_node_https_server_ref", I64, &[I64]);
    module.declare_function("js_node_https_server_unref", I64, &[I64]);
    // Streaming-bodies PR — client flushHeaders early dispatch + numeric
    // httpVersion halves.
    module.declare_function(
        "js_http_client_request_flush_headers",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function("js_node_http_im_http_version_major", DOUBLE, &[I64]);
    module.declare_function("js_node_http_im_http_version_minor", DOUBLE, &[I64]);
}
