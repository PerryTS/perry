//! HTTP / HTTPS / HTTP2 server + client, agents, vm/repl/worker_threads
//! stdlib FFI declarations (extracted from stdlib_ffi.rs).

use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64, VOID};

pub(crate) fn declare_net_http(module: &mut LlModule) {
    // ========== node:vm ==========
    module.declare_function("js_vm_create_context", DOUBLE, &[DOUBLE]);
    module.declare_function("js_vm_module_call", DOUBLE, &[]);
    module.declare_function("js_vm_module_constructor_error", DOUBLE, &[]);

    // ========== node:repl ==========
    module.declare_function("js_repl_start", DOUBLE, &[DOUBLE]);
    module.declare_function("js_repl_repl_server_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_repl_recoverable_new", DOUBLE, &[DOUBLE]);

    // ========== worker_threads ==========
    module.declare_function("js_worker_threads_worker_new", DOUBLE, &[I64, DOUBLE]);
    module.declare_function(
        "js_worker_threads_worker_post_message",
        DOUBLE,
        &[I64, DOUBLE],
    );
    module.declare_function("js_worker_threads_worker_on", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_worker_threads_worker_once", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_worker_threads_worker_off", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function(
        "js_worker_threads_worker_add_event_listener",
        DOUBLE,
        &[I64, DOUBLE, I64],
    );
    module.declare_function(
        "js_worker_threads_worker_remove_event_listener",
        DOUBLE,
        &[I64, DOUBLE, I64],
    );
    module.declare_function("js_worker_threads_worker_terminate", DOUBLE, &[I64]);
    module.declare_function("js_worker_threads_worker_ref", DOUBLE, &[I64]);
    module.declare_function("js_worker_threads_worker_unref", DOUBLE, &[I64]);
    module.declare_function(
        "js_worker_threads_worker_get_heap_statistics",
        DOUBLE,
        &[I64],
    );
    module.declare_function("js_worker_threads_worker_cpu_usage", DOUBLE, &[I64, DOUBLE]);
    module.declare_function(
        "js_worker_threads_worker_get_heap_snapshot",
        DOUBLE,
        &[I64, DOUBLE],
    );
    module.declare_function("js_worker_threads_worker_start_cpu_profile", DOUBLE, &[I64]);
    module.declare_function(
        "js_worker_threads_worker_start_heap_profile",
        DOUBLE,
        &[I64],
    );

    // ========== HTTP server ==========
    module.declare_function("js_http_client_request_end", I64, &[I64, DOUBLE]);
    module.declare_function("js_http_client_request_write", I64, &[I64, DOUBLE]);
    // #4909 — callback-aware client write/end/setTimeout (the `(encoding?,
    // callback?)` tail rides as raw NaN-boxed JSValues).
    module.declare_function(
        "js_http_client_request_end_full",
        I64,
        &[I64, DOUBLE, I64, I64],
    );
    module.declare_function(
        "js_http_client_request_write_full",
        DOUBLE,
        &[I64, DOUBLE, I64, I64],
    );
    module.declare_function("js_http_set_timeout_full", I64, &[I64, DOUBLE, I64]);
    module.declare_function("js_http_client_request_method", I64, &[I64]);
    module.declare_function("js_http_client_request_protocol", I64, &[I64]);
    module.declare_function("js_http_client_request_host", I64, &[I64]);
    module.declare_function("js_http_client_request_path", I64, &[I64]);
    module.declare_function("js_http_client_request_listener_count", DOUBLE, &[I64, I64]);
    module.declare_function("js_http_client_request_get_header", DOUBLE, &[I64, I64]);
    module.declare_function("js_http_client_request_has_header", DOUBLE, &[I64, I64]);
    module.declare_function("js_http_client_request_remove_header", DOUBLE, &[I64, I64]);
    module.declare_function("js_http_client_request_get_header_names", DOUBLE, &[I64]);
    module.declare_function("js_http_client_request_get_headers", DOUBLE, &[I64]);
    module.declare_function(
        "js_http_client_request_get_raw_header_names",
        DOUBLE,
        &[I64],
    );
    module.declare_function("js_http_client_request_abort", DOUBLE, &[I64]);
    module.declare_function("js_http_client_request_destroy", I64, &[I64, DOUBLE]);
    module.declare_function(
        "js_http_client_request_noop_undefined",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function("js_http_client_request_aborted", DOUBLE, &[I64]);
    module.declare_function("js_http_client_request_destroyed", DOUBLE, &[I64]);
    module.declare_function("js_http_client_request_finished", DOUBLE, &[I64]);
    module.declare_function("js_http_client_request_reused_socket", DOUBLE, &[I64]);
    module.declare_function("js_http_client_request_max_headers_count", DOUBLE, &[I64]);
    module.declare_function("js_http_client_request_writable_ended", DOUBLE, &[I64]);
    module.declare_function("js_http_client_request_writable_finished", DOUBLE, &[I64]);
    module.declare_function("js_http_client_request_socket", DOUBLE, &[I64]);
    module.declare_function("js_http_get", I64, &[DOUBLE, I64]);
    // #3226/#3227/#3228 — overload-normalizing client factories take a
    // single `NA_VARARGS` array (i64 ArrayHeader ptr) and return a
    // ClientRequest handle.
    module.declare_function("js_http_get_overload", I64, &[I64]);
    module.declare_function("js_http_request_overload", I64, &[I64]);
    module.declare_function("js_https_get_overload", I64, &[I64]);
    module.declare_function("js_https_request_overload", I64, &[I64]);
    module.declare_function("js_http_on", I64, &[I64, I64, I64]);
    module.declare_function("js_http_request", I64, &[DOUBLE, I64]);
    module.declare_function("js_http_request_body", I64, &[I64]);
    module.declare_function("js_http_request_body_length", DOUBLE, &[I64]);
    module.declare_function("js_http_request_content_type", I64, &[I64]);
    module.declare_function("js_http_request_has_header", DOUBLE, &[I64, I64]);
    module.declare_function("js_http_request_header", I64, &[I64, I64]);
    module.declare_function("js_http_request_headers_all", I64, &[I64]);
    module.declare_function("js_http_request_id", DOUBLE, &[I64]);
    module.declare_function("js_http_request_is_method", DOUBLE, &[I64, I64]);
    module.declare_function("js_http_request_method", I64, &[I64]);
    module.declare_function("js_http_request_path", I64, &[I64]);
    module.declare_function("js_http_request_query", I64, &[I64]);
    module.declare_function("js_http_request_query_all", I64, &[I64]);
    module.declare_function("js_http_request_query_param", I64, &[I64, I64]);
    module.declare_function("js_http_respond_error", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_http_respond_html", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_http_respond_json", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_http_respond_not_found", DOUBLE, &[I64]);
    module.declare_function("js_http_respond_redirect", DOUBLE, &[I64, I64, DOUBLE]);
    module.declare_function("js_http_respond_status_text", I64, &[DOUBLE]);
    module.declare_function("js_http_respond_text", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function(
        "js_http_respond_with_headers",
        DOUBLE,
        &[I64, DOUBLE, I64, I64],
    );
    module.declare_function("js_http_response_headers", DOUBLE, &[I64]);
    module.declare_function("js_http_response_trailers", DOUBLE, &[I64]);
    module.declare_function("js_http_incoming_message_set_encoding", I64, &[I64, I64]);
    module.declare_function("js_http_server_accept_v2", I64, &[I64]);
    module.declare_function("js_http_server_close", DOUBLE, &[I64]);
    module.declare_function("js_http_server_create", I64, &[DOUBLE]);
    module.declare_function("js_http_set_header", I64, &[I64, I64, I64]);
    module.declare_function("js_http_set_timeout", I64, &[I64, DOUBLE]);
    module.declare_function("js_http_status_code", DOUBLE, &[I64]);
    module.declare_function("js_http_status_message", I64, &[I64]);

    // ========== http.Agent / https.Agent (#2129 / #2154) ==========
    module.declare_function("js_http_agent_new", I64, &[DOUBLE]);
    module.declare_function("js_https_agent_new", I64, &[DOUBLE]);
    module.declare_function("js_http_agent_get_name", I64, &[I64, DOUBLE]);
    module.declare_function("js_http_agent_noop_self", I64, &[I64]);
    module.declare_function("js_http_agent_max_sockets", DOUBLE, &[I64]);
    module.declare_function("js_http_agent_max_free_sockets", DOUBLE, &[I64]);
    module.declare_function("js_http_agent_max_total_sockets", DOUBLE, &[I64]);
    module.declare_function("js_http_agent_keep_alive_msecs", DOUBLE, &[I64]);
    module.declare_function("js_http_agent_keep_alive", DOUBLE, &[I64]);
    module.declare_function("js_http_agent_protocol", I64, &[I64]);
    module.declare_function("js_http_agent_default_port", DOUBLE, &[I64]);
    module.declare_function("js_http_agent_set_protocol", VOID, &[I64, I64]);
    // #2154
    module.declare_function("js_http_agent_destroy", I64, &[I64]);
    module.declare_function("js_http_agent_destroyed", DOUBLE, &[I64]);
    module.declare_function("js_http_agent_sockets", DOUBLE, &[I64]);
    module.declare_function("js_http_agent_free_sockets", DOUBLE, &[I64]);
    module.declare_function("js_http_agent_requests", DOUBLE, &[I64]);
    module.declare_function("js_http_agent_set_max_sockets", VOID, &[I64, DOUBLE]);
    module.declare_function("js_http_agent_set_max_free_sockets", VOID, &[I64, DOUBLE]);
    module.declare_function("js_http_agent_set_max_total_sockets", VOID, &[I64, DOUBLE]);
    module.declare_function("js_http_agent_set_keep_alive", VOID, &[I64, DOUBLE]);
    module.declare_function("js_http_agent_set_keep_alive_msecs", VOID, &[I64, DOUBLE]);
    module.declare_function("js_http_agent_set_create_connection", VOID, &[I64, I64]);
    module.declare_function("js_http_agent_set_create_socket", VOID, &[I64, I64]);
    module.declare_function("js_http_agent_create_connection", I64, &[I64]);
    module.declare_function("js_http_agent_create_socket", I64, &[I64]);

    // ========== HTTPS ==========
    module.declare_function("js_https_get", I64, &[DOUBLE, I64]);
    module.declare_function("js_https_request", I64, &[DOUBLE, I64]);

    // ========== node:http / node:https / node:http2 SERVER (issue #577) ==========
    // perry-ext-http-server — handler-push HTTP/1.1 + HTTP/2 + TLS via rustls.
    // Symbols are linked through perry-ext-http (rlib dep), so the
    // existing `bindings.http` / `bindings.https` / `bindings.http2`
    // entries in well_known_bindings.toml route imports here.
    // Server / lifecycle:
    module.declare_function("js_node_http_create_server", I64, &[I64]);
    // Returns the server handle so chains like
    // `createServer(...).listen(...).on(...)` resolve correctly (#2129).
    module.declare_function("js_node_http_server_listen", I64, &[I64, I64]);
    module.declare_function("js_node_http_server_close", VOID, &[I64, I64]);
    module.declare_function("js_node_http_server_close_all_connections", VOID, &[I64]);
    module.declare_function("js_node_http_server_close_idle_connections", VOID, &[I64]);
    module.declare_function("js_node_http_server_address_json", I64, &[I64]);
    module.declare_function("js_node_http_server_listening", I32, &[I64]);
    module.declare_function("js_node_http_server_listening_value", DOUBLE, &[I64]);
    module.declare_function("js_node_http_server_on", DOUBLE, &[I64, I64, I64]);
    // #4973 http(s).Server.call(this,…) + net socket.setEncoding decls live in
    // objects.rs's declare chain to keep this file under the 2000-line gate.
    // IncomingMessage:
    module.declare_function("js_node_http_im_method", I64, &[I64]);
    module.declare_function("js_node_http_im_url", I64, &[I64]);
    module.declare_function("js_node_http_im_http_version", I64, &[I64]);
    module.declare_function("js_node_http_im_headers_json", I64, &[I64]);
    module.declare_function("js_node_http_im_raw_headers_json", I64, &[I64]);
    module.declare_function("js_node_http_im_headers_distinct_json", I64, &[I64]);
    module.declare_function("js_node_http_im_trailers_json", I64, &[I64]);
    module.declare_function("js_node_http_im_raw_trailers_json", I64, &[I64]);
    module.declare_function("js_node_http_im_trailers_distinct_json", I64, &[I64]);
    module.declare_function("js_node_http_im_complete", I32, &[I64]);
    module.declare_function("js_node_http_im_aborted", I32, &[I64]);
    module.declare_function("js_node_http_im_destroyed", I32, &[I64]);
    module.declare_function("js_node_http_im_remote_address", I64, &[I64]);
    module.declare_function("js_node_http_im_remote_port", DOUBLE, &[I64]);
    module.declare_function("js_node_http_im_pause", VOID, &[I64]);
    module.declare_function("js_node_http_im_resume", VOID, &[I64]);
    module.declare_function("js_node_http_im_destroy", VOID, &[I64]);
    module.declare_function("js_node_http_im_on", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_node_http_im_read", DOUBLE, &[I64]);
    module.declare_function("js_node_http_im_set_timeout", I64, &[I64, DOUBLE, I64]);
    // ServerResponse:
    module.declare_function("js_node_http_res_set_status", VOID, &[I64, DOUBLE]);
    module.declare_function("js_node_http_res_get_status", DOUBLE, &[I64]);
    module.declare_function("js_node_http_res_set_status_message", VOID, &[I64, I64]);
    module.declare_function("js_node_http_res_set_header", VOID, &[I64, I64, DOUBLE]);
    module.declare_function("js_node_http_res_set_header_self", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_node_http_res_get_header", DOUBLE, &[I64, I64]);
    module.declare_function("js_node_http_res_remove_header", VOID, &[I64, I64]);
    module.declare_function("js_node_http_res_has_header", I32, &[I64, I64]);
    module.declare_function("js_node_http_res_has_header_value", DOUBLE, &[I64, I64]);
    module.declare_function("js_node_http_res_get_headers_json", I64, &[I64]);
    module.declare_function("js_node_http_res_get_header_names_json", I64, &[I64]);
    module.declare_function("js_node_http_res_append_header", I64, &[I64, I64, I64]);
    module.declare_function("js_node_http_res_set_headers", I64, &[I64, DOUBLE]);
    module.declare_function("js_node_http_res_get_status_message", DOUBLE, &[I64]);
    module.declare_function("js_node_http_res_headers_sent", I32, &[I64]);
    module.declare_function("js_node_http_res_writable_ended", I32, &[I64]);
    module.declare_function("js_node_http_res_writable_finished", I32, &[I64]);
    module.declare_function("js_node_http_res_finished", I32, &[I64]);
    module.declare_function("js_node_http_res_send_date", I32, &[I64]);
    module.declare_function("js_node_http_res_set_send_date", VOID, &[I64, DOUBLE]);
    module.declare_function("js_node_http_res_strict_content_length", I32, &[I64]);
    module.declare_function(
        "js_node_http_res_set_strict_content_length",
        VOID,
        &[I64, DOUBLE],
    );
    module.declare_function("js_node_http_res_req_handle", I64, &[I64]);
    module.declare_function(
        "js_node_http_res_write_head",
        VOID,
        &[I64, DOUBLE, I64, I64],
    );
    module.declare_function("js_node_http_res_write", I32, &[I64, DOUBLE]);
    // #4909: callback-aware write/end. chunk + raw (encoding?, callback?) tail;
    // write returns a NaN-boxed bool (DOUBLE) for backpressure.
    module.declare_function(
        "js_node_http_res_write_full",
        DOUBLE,
        &[I64, DOUBLE, I64, I64],
    );
    module.declare_function("js_node_http_res_add_trailers", VOID, &[I64, DOUBLE]);
    module.declare_function("js_node_http_res_end", VOID, &[I64, DOUBLE]);
    module.declare_function("js_node_http_res_end_full", VOID, &[I64, DOUBLE, I64, I64]);
    module.declare_function("js_node_http_res_flush_headers", VOID, &[I64]);
    module.declare_function("js_node_http_res_cork", VOID, &[I64]);
    module.declare_function("js_node_http_res_uncork", VOID, &[I64]);
    module.declare_function("js_node_http_res_set_timeout", I64, &[I64, DOUBLE, I64]);
    module.declare_function(
        "js_node_http_res_write_early_hints",
        VOID,
        &[I64, DOUBLE, I64],
    );
    module.declare_function("js_node_http_res_write_continue", VOID, &[I64]);
    module.declare_function("js_node_http_res_write_processing", VOID, &[I64]);
    module.declare_function("js_node_http_res_on", DOUBLE, &[I64, I64, I64]);
    // node:https server (TLS via rustls):
    module.declare_function("js_node_https_create_server", I64, &[DOUBLE, I64]);
    module.declare_function("js_node_https_server_listen", I64, &[I64, I64]);
    module.declare_function("js_node_https_server_close", VOID, &[I64, I64]);
    module.declare_function("js_node_https_server_close_all_connections", VOID, &[I64]);
    module.declare_function("js_node_https_server_close_idle_connections", VOID, &[I64]);
    module.declare_function("js_node_https_server_address_json", I64, &[I64]);
    module.declare_function("js_node_https_server_on", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_node_https_server_listening_value", DOUBLE, &[I64]);
    module.declare_function("js_node_https_server_headers_timeout", DOUBLE, &[I64]);
    module.declare_function(
        "js_node_https_server_set_headers_timeout",
        DOUBLE,
        &[I64, DOUBLE],
    );
    module.declare_function("js_node_https_server_keep_alive_timeout", DOUBLE, &[I64]);
    module.declare_function(
        "js_node_https_server_set_keep_alive_timeout",
        DOUBLE,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_node_https_server_keep_alive_timeout_buffer",
        DOUBLE,
        &[I64],
    );
    module.declare_function(
        "js_node_https_server_set_keep_alive_timeout_buffer",
        DOUBLE,
        &[I64, DOUBLE],
    );
    module.declare_function("js_node_https_server_request_timeout", DOUBLE, &[I64]);
    module.declare_function(
        "js_node_https_server_set_request_timeout",
        DOUBLE,
        &[I64, DOUBLE],
    );
    module.declare_function("js_node_https_server_idle_timeout", DOUBLE, &[I64]);
    module.declare_function(
        "js_node_https_server_set_idle_timeout",
        DOUBLE,
        &[I64, DOUBLE],
    );
    module.declare_function("js_node_https_server_max_headers_count", DOUBLE, &[I64]);
    module.declare_function(
        "js_node_https_server_set_max_headers_count",
        DOUBLE,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_node_https_server_max_requests_per_socket",
        DOUBLE,
        &[I64],
    );
    module.declare_function(
        "js_node_https_server_set_max_requests_per_socket",
        DOUBLE,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_node_https_server_set_timeout_method",
        I64,
        &[I64, DOUBLE, I64],
    );
    // node:http2 secure server (HTTP/2 with ALPN):
    module.declare_function("js_node_http2_create_server", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_node_http2_create_secure_server", I64, &[DOUBLE, I64]);
    module.declare_function("js_node_http2_connect", I64, &[DOUBLE, DOUBLE, I64]);
    module.declare_function("js_node_http2_server_listen", I64, &[I64, I64]);
    module.declare_function("js_node_http2_server_close", VOID, &[I64, I64]);
    module.declare_function("js_node_http2_server_address_json", I64, &[I64]);
    module.declare_function("js_node_http2_server_on", DOUBLE, &[I64, I64, I64]);
    // node:http2 settings helpers (#3168) — getDefaultSettings()/
    // getUnpackedSettings() return a JSON StringHeader (reparsed via
    // NR_OBJ_FROM_JSON_STR); getPackedSettings() returns a Buffer pointer.
    module.declare_function("js_node_http2_get_default_settings", I64, &[]);
    module.declare_function("js_node_http2_get_packed_settings", I64, &[I64]);
    module.declare_function("js_node_http2_get_unpacked_settings", I64, &[I64]);
}
