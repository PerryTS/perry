//! URL / URLSearchParams + WebSocket stdlib FFI declarations
//! (extracted from stdlib_ffi.rs).

use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64, VOID};

pub(crate) fn declare_web(module: &mut LlModule) {
    // ========== URL / URLSearchParams ==========
    // Rust runtime signatures (see crates/perry-runtime/src/url.rs):
    //   js_url_new(*mut StringHeader)                         -> *mut ObjectHeader
    //   js_url_new_with_base(*mut StringHeader, *mut ...)     -> *mut ObjectHeader
    //   js_url_get_{href,pathname,protocol,host,hostname,port,search,hash,origin,search_params}
    //     (*mut ObjectHeader)                                  -> f64 (NaN-boxed string)
    //   js_url_search_params_new(*mut StringHeader)            -> *mut ObjectHeader
    //   js_url_search_params_new_empty()                       -> *mut ObjectHeader
    //   js_url_search_params_get(*mut ObjectHeader, NaN-boxed name)
    //                                                          -> *mut StringHeader (null if missing)
    //   js_url_search_params_has(*mut ObjectHeader, NaN-boxed name)
    //                                                          -> f64 (0.0 or 1.0)
    //   js_url_search_params_set/append(*mut ObjectHeader, name, value) -> void
    //   js_url_search_params_delete(*mut ObjectHeader, name)            -> void
    //   js_url_search_params_to_string(*mut ObjectHeader)     -> *mut StringHeader
    //   js_url_search_params_get_all(*mut ObjectHeader, NaN-boxed name)
    //                                                          -> f64 (NaN-boxed array)
    module.declare_function("js_url_file_url_to_path", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_url_file_url_to_path_buffer", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_url_get_hash", DOUBLE, &[I64]);
    module.declare_function("js_url_get_host", DOUBLE, &[I64]);
    module.declare_function("js_url_get_hostname", DOUBLE, &[I64]);
    module.declare_function("js_url_get_href", DOUBLE, &[I64]);
    module.declare_function("js_url_get_origin", DOUBLE, &[I64]);
    module.declare_function("js_url_get_pathname", DOUBLE, &[I64]);
    module.declare_function("js_url_get_port", DOUBLE, &[I64]);
    module.declare_function("js_url_get_protocol", DOUBLE, &[I64]);
    module.declare_function("js_url_get_search", DOUBLE, &[I64]);
    module.declare_function("js_url_get_search_params", DOUBLE, &[I64]);
    module.declare_function("js_url_new", I64, &[I64]);
    module.declare_function("js_url_new_with_base", I64, &[I64, I64]);
    module.declare_function("js_url_pattern_new", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_url_pattern_constructor_call", DOUBLE, &[DOUBLE, DOUBLE]);
    // Issue #650: URL.canParse / URL.parse static methods (Node 18+ / 22+).
    module.declare_function("js_url_can_parse", I32, &[I64]);
    module.declare_function("js_url_can_parse_with_base", I32, &[I64, I64]);
    module.declare_function("js_url_parse", I64, &[I64]);
    module.declare_function("js_url_parse_with_base", I64, &[I64, I64]);
    // Issue #650: URL setters — mutate field + re-derive href.
    module.declare_function("js_url_set_pathname", VOID, &[I64, DOUBLE]);
    module.declare_function("js_url_set_search", VOID, &[I64, DOUBLE]);
    module.declare_function("js_url_set_hash", VOID, &[I64, DOUBLE]);
    module.declare_function("js_url_set_protocol", VOID, &[I64, DOUBLE]);
    module.declare_function("js_url_set_hostname", VOID, &[I64, DOUBLE]);
    module.declare_function("js_url_set_port", VOID, &[I64, DOUBLE]);
    module.declare_function("js_url_set_username", VOID, &[I64, DOUBLE]);
    module.declare_function("js_url_set_password", VOID, &[I64, DOUBLE]);
    module.declare_function("js_url_set_href", VOID, &[I64, DOUBLE]);
    module.declare_function("js_url_search_params_has2", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_url_search_params_delete2", VOID, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_url_search_params_throw_missing_args", DOUBLE, &[I32]);
    module.declare_function("js_url_search_params_append", VOID, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_url_search_params_delete", VOID, &[I64, DOUBLE]);
    module.declare_function("js_url_search_params_get", I64, &[I64, DOUBLE]);
    module.declare_function("js_url_search_params_get_all", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_url_search_params_has", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_url_search_params_new", I64, &[I64]);
    // Generic init that handles string / record / URLSearchParams / null /
    // undefined — see `js_url_search_params_new_any` rustdoc. Refs #575.
    module.declare_function("js_url_search_params_new_any", I64, &[DOUBLE]);
    module.declare_function("js_url_search_params_new_empty", I64, &[]);
    module.declare_function("js_url_search_params_set", VOID, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_url_search_params_to_string", I64, &[I64]);
    // Issue #650: URLSearchParams.size getter — returns entries count.
    module.declare_function("js_url_search_params_size", I32, &[I64]);
    // params.entries() / iteration source — returns an already NaN-boxed
    // POINTER_TAG f64 to ArrayHeader<[k, v]> (refs #575).
    module.declare_function("js_url_search_params_entries_arr", DOUBLE, &[I64]);
    module.declare_function("js_url_search_params_keys_arr", DOUBLE, &[I64]);
    module.declare_function("js_url_search_params_values_arr", DOUBLE, &[I64]);
    module.declare_function("js_url_search_params_sort", VOID, &[I64]);
    module.declare_function(
        "js_url_search_params_for_each",
        VOID,
        &[I64, DOUBLE, DOUBLE],
    );
    // `String(value)` coercion (throws TypeError for Symbols) for WHATWG URL
    // arguments — #3054/#3055. Returns a `*mut StringHeader` (I64).
    module.declare_function("js_url_coerce_string", I64, &[DOUBLE]);
    module.declare_function("js_url_path_to_file_url", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_url_domain_to_ascii", DOUBLE, &[DOUBLE]);
    module.declare_function("js_url_domain_to_unicode", DOUBLE, &[DOUBLE]);
    module.declare_function("js_url_to_http_options", DOUBLE, &[DOUBLE]);
    module.declare_function("js_url_legacy_url_new", DOUBLE, &[]);
    module.declare_function("js_url_format", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_url_legacy_parse", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_url_legacy_resolve", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_url_legacy_resolve_object", DOUBLE, &[DOUBLE, DOUBLE]);

    // ========== WebSocket ==========
    module.declare_function("js_ws_close", VOID, &[I64]);
    module.declare_function("js_ws_connect", I64, &[I64]);
    module.declare_function("js_ws_connect_start", DOUBLE, &[DOUBLE]);
    module.declare_function("js_ws_handle_to_i64", I64, &[DOUBLE]);
    module.declare_function("js_ws_is_open", DOUBLE, &[I64]);
    module.declare_function("js_ws_message_count", DOUBLE, &[I64]);
    module.declare_function("js_ws_ready_state", DOUBLE, &[I64]);
    module.declare_function("js_ws_on", I64, &[I64, I64, I64]);
    module.declare_function("js_ws_receive", I64, &[I64]);
    module.declare_function("js_ws_send", VOID, &[I64, I64]);
    // Issue #577 Phase 4 — `js_ws_send_to_client` takes the handle
    // as f64 so a TS-side numeric ws_id (received from the
    // `Server.on('upgrade', (req, wsId, head) => ...)` callback)
    // round-trips cleanly without the i64-bits dance js_ws_send
    // requires.
    module.declare_function("js_ws_send_to_client", VOID, &[DOUBLE, I64]);
    module.declare_function("js_ws_close_client", VOID, &[DOUBLE]);
    // Issue #577 Phase 4 — receiver-method variants for Client class.
    // Take the handle as i64 (post-unbox_to_i64 from NATIVE_MODULE_TABLE
    // dispatch). Separate symbols so the dispatch table can pin
    // `class_filter: Some("Client")` entries without colliding with
    // the existing receiver-less / module-method `js_ws_send` /
    // `js_ws_on` / `js_ws_close` entries.
    module.declare_function("js_ws_send_client_i64", VOID, &[I64, I64]);
    module.declare_function("js_ws_close_client_i64", VOID, &[I64]);
    module.declare_function("js_ws_on_client_i64", I64, &[I64, I64, I64]);
    module.declare_function("js_ws_server_close", VOID, &[I64]);
    module.declare_function("js_ws_server_new", I64, &[DOUBLE]);
    // #1113 — `wss.handleUpgrade(req, socket, head, cb)`. Receiver
    // (the noServer WsServerHandle) is passed as I64 (post-unbox_to_i64
    // from NATIVE_MODULE_TABLE dispatch, same receiver convention as
    // `js_ws_on`). req/socket/head are NaN-boxed JSValues (DOUBLE);
    // cb is the unboxed closure pointer (I64).
    module.declare_function(
        "js_ws_handle_upgrade",
        I64,
        &[I64, DOUBLE, DOUBLE, DOUBLE, I64],
    );
    module.declare_function("js_ws_wait_for_message", I64, &[I64, DOUBLE]);
}
