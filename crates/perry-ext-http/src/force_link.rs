//! Linker-retention anchors for perry-ext-http-server's `#[no_mangle]` FFI
//! symbols. Split out of lib.rs (#4975) to stay under the 2000-line CI cap;
//! see the `#[used] FORCE_LINK_HTTP_SERVER` table below for the mechanism.

// #1652: force the linker to retain perry-ext-http-server's `#[no_mangle]`
// FFI symbols. The `extern crate perry_ext_http_server as _server_link`
// at the top of this file pulls the rlib into the dependency graph, but
// the server functions are referenced only by codegen-generated callsites
// in the *user* program — never by this crate's Rust. Under LTO / staticlib
// emission they can therefore be dead-stripped, and the final link then
// fails with `Undefined symbols: _js_node_http_create_server` for any
// program that does `import { createServer } from 'node:http'` (the failure
// originally tracked at #589, reopened as #1652). Anchoring their addresses
// in a `#[used]` table makes the retention explicit so it can't silently
// regress when nobody's npm import happens to reference a given symbol.
//
// Resolution is by symbol name (C ABI): the `()` signatures below are only
// ever used to take the function's address, never to call it, so they need
// not match the real definitions — the linker keys off the `#[no_mangle]`
// symbol name alone.
//
// `cfg(not(test))`: the anchor must NOT fire in `cargo test -p perry-ext-http`.
// Forcing the server functions into the unit-test binary drags in their
// transitive `perry_ffi_spawn_blocking*` references, which only the host
// (perry-stdlib) provides at the final perry-compile link — the test binary
// has no host, so it fails with `undefined symbol: perry_ffi_spawn_blocking`.
// The staticlib (the real perry-compile artifact) is built without `test`,
// so retention there is unaffected. Nothing cargo-depends on this crate, so
// gating on `test` is sufficient.
#[cfg(not(test))]
#[allow(dead_code)]
mod force_link_http_server {
    extern "C" {
        // http server + IncomingMessage + ServerResponse entry points.
        pub fn js_node_http_create_server();
        pub fn js_node_http_create_server_with_options();
        pub fn js_node_http_server_listen();
        pub fn js_node_http_server_listening();
        pub fn js_node_http_server_close();
        pub fn js_node_http_server_on();
        pub fn js_node_http_server_address_json();
        pub fn js_node_http_server_process_pending();
        pub fn js_node_http_server_has_active();
        pub fn js_node_http_server_close_all_connections();
        pub fn js_node_http_server_close_idle_connections();
        pub fn js_node_http_server_headers_timeout();
        pub fn js_node_http_server_set_headers_timeout();
        pub fn js_node_http_server_keep_alive_timeout();
        pub fn js_node_http_server_set_keep_alive_timeout();
        pub fn js_node_http_server_keep_alive_timeout_buffer();
        pub fn js_node_http_server_set_keep_alive_timeout_buffer();
        pub fn js_node_http_server_request_timeout();
        pub fn js_node_http_server_set_request_timeout();
        pub fn js_node_http_server_idle_timeout();
        pub fn js_node_http_server_set_idle_timeout();
        pub fn js_node_http_server_max_headers_count();
        pub fn js_node_http_server_set_max_headers_count();
        pub fn js_node_http_server_max_requests_per_socket();
        pub fn js_node_http_server_set_max_requests_per_socket();
        pub fn js_node_http_server_set_timeout_method();
        pub fn js_node_http_server_ref();
        pub fn js_node_http_server_unref();
        pub fn js_node_http_res_end();
        pub fn js_node_http_res_write();
        pub fn js_node_http_res_write_head();
        pub fn js_node_http_res_set_header();
        pub fn js_node_http_res_set_header_self();
        pub fn js_node_http_res_get_header();
        pub fn js_node_http_res_get_header_names_json();
        pub fn js_node_http_res_get_headers_json();
        pub fn js_node_http_res_has_header();
        pub fn js_node_http_res_has_header_value();
        pub fn js_node_http_res_remove_header();
        pub fn js_node_http_res_append_header();
        pub fn js_node_http_res_set_headers();
        pub fn js_node_http_res_set_status();
        pub fn js_node_http_res_get_status();
        pub fn js_node_http_res_set_status_message();
        pub fn js_node_http_res_get_status_message();
        pub fn js_node_http_res_finished();
        pub fn js_node_http_res_send_date();
        pub fn js_node_http_res_set_send_date();
        pub fn js_node_http_res_strict_content_length();
        pub fn js_node_http_res_set_strict_content_length();
        pub fn js_node_http_res_req_handle();
        pub fn js_node_http_res_headers_sent();
        pub fn js_node_http_res_writable_ended();
        pub fn js_node_http_res_writable_finished();
        pub fn js_node_http_res_flush_headers();
        pub fn js_node_http_res_add_trailers();
        pub fn js_node_http_res_cork();
        pub fn js_node_http_res_uncork();
        pub fn js_node_http_res_set_timeout();
        pub fn js_node_http_res_write_early_hints();
        pub fn js_node_http_res_write_continue();
        pub fn js_node_http_res_write_processing();
        pub fn js_node_http_res_on();
        pub fn js_node_http_im_method();
        pub fn js_node_http_im_url();
        pub fn js_node_http_im_http_version();
        pub fn js_node_http_im_headers_json();
        pub fn js_node_http_im_raw_headers_json();
        pub fn js_node_http_im_headers_distinct_json();
        pub fn js_node_http_im_trailers_json();
        pub fn js_node_http_im_raw_trailers_json();
        pub fn js_node_http_im_trailers_distinct_json();
        pub fn js_node_http_im_remote_address();
        pub fn js_node_http_im_remote_port();
        pub fn js_node_http_im_on();
        pub fn js_node_http_im_read();
        pub fn js_node_http_im_pause();
        pub fn js_node_http_im_resume();
        pub fn js_node_http_im_pause_self();
        pub fn js_node_http_im_resume_self();
        pub fn js_node_http_im_aborted();
        pub fn js_node_http_im_complete();
        pub fn js_node_http_im_destroy();
        pub fn js_node_http_im_destroyed();
        pub fn js_node_http_im_set_timeout();
        // https server.
        pub fn js_node_https_create_server();
        pub fn js_node_https_server_listen();
        pub fn js_node_https_server_close();
        pub fn js_node_https_server_close_all_connections();
        pub fn js_node_https_server_close_idle_connections();
        pub fn js_node_https_server_on();
        pub fn js_node_https_server_address_json();
        pub fn js_node_https_server_headers_timeout();
        pub fn js_node_https_server_set_headers_timeout();
        pub fn js_node_https_server_keep_alive_timeout();
        pub fn js_node_https_server_set_keep_alive_timeout();
        pub fn js_node_https_server_keep_alive_timeout_buffer();
        pub fn js_node_https_server_set_keep_alive_timeout_buffer();
        pub fn js_node_https_server_request_timeout();
        pub fn js_node_https_server_set_request_timeout();
        pub fn js_node_https_server_idle_timeout();
        pub fn js_node_https_server_set_idle_timeout();
        pub fn js_node_https_server_max_headers_count();
        pub fn js_node_https_server_set_max_headers_count();
        pub fn js_node_https_server_max_requests_per_socket();
        pub fn js_node_https_server_set_max_requests_per_socket();
        pub fn js_node_https_server_set_timeout_method();
        pub fn js_node_https_server_ref();
        pub fn js_node_https_server_unref();
        // http2 secure server.
        pub fn js_node_http2_create_secure_server();
        pub fn js_node_http2_server_listen();
        pub fn js_node_http2_server_close();
        pub fn js_node_http2_server_on();
        pub fn js_node_http2_server_address_json();
        // http2 settings helpers (#3168).
        pub fn js_node_http2_get_default_settings();
        pub fn js_node_http2_get_packed_settings();
        pub fn js_node_http2_get_unpacked_settings();
    }
}

/// `#[used]` anchor table referencing every server FFI entry point so the
/// linker keeps them in `libperry_ext_http.a`. See the module above (#1652).
/// Gated with the module on `not(test)` so the unit-test binary doesn't drag
/// in the server's host-provided `perry_ffi_*` symbols.
#[cfg(not(test))]
#[used]
static FORCE_LINK_HTTP_SERVER: &[unsafe extern "C" fn()] = {
    use force_link_http_server::*;
    &[
        js_node_http_create_server,
        js_node_http_create_server_with_options,
        js_node_http_server_listen,
        js_node_http_server_listening,
        js_node_http_server_close,
        js_node_http_server_on,
        js_node_http_server_address_json,
        js_node_http_server_process_pending,
        js_node_http_server_has_active,
        js_node_http_server_close_all_connections,
        js_node_http_server_close_idle_connections,
        js_node_http_server_headers_timeout,
        js_node_http_server_set_headers_timeout,
        js_node_http_server_keep_alive_timeout,
        js_node_http_server_set_keep_alive_timeout,
        js_node_http_server_keep_alive_timeout_buffer,
        js_node_http_server_set_keep_alive_timeout_buffer,
        js_node_http_server_request_timeout,
        js_node_http_server_set_request_timeout,
        js_node_http_server_idle_timeout,
        js_node_http_server_set_idle_timeout,
        js_node_http_server_max_headers_count,
        js_node_http_server_set_max_headers_count,
        js_node_http_server_max_requests_per_socket,
        js_node_http_server_set_max_requests_per_socket,
        js_node_http_server_set_timeout_method,
        js_node_http_server_ref,
        js_node_http_server_unref,
        js_node_http_res_end,
        js_node_http_res_write,
        js_node_http_res_write_head,
        js_node_http_res_set_header,
        js_node_http_res_set_header_self,
        js_node_http_res_get_header,
        js_node_http_res_get_header_names_json,
        js_node_http_res_get_headers_json,
        js_node_http_res_has_header,
        js_node_http_res_has_header_value,
        js_node_http_res_remove_header,
        js_node_http_res_append_header,
        js_node_http_res_set_headers,
        js_node_http_res_set_status,
        js_node_http_res_get_status,
        js_node_http_res_set_status_message,
        js_node_http_res_get_status_message,
        js_node_http_res_finished,
        js_node_http_res_send_date,
        js_node_http_res_set_send_date,
        js_node_http_res_strict_content_length,
        js_node_http_res_set_strict_content_length,
        js_node_http_res_req_handle,
        js_node_http_res_headers_sent,
        js_node_http_res_writable_ended,
        js_node_http_res_writable_finished,
        js_node_http_res_flush_headers,
        js_node_http_res_add_trailers,
        js_node_http_res_cork,
        js_node_http_res_uncork,
        js_node_http_res_set_timeout,
        js_node_http_res_write_early_hints,
        js_node_http_res_write_continue,
        js_node_http_res_write_processing,
        js_node_http_res_on,
        js_node_http_im_method,
        js_node_http_im_url,
        js_node_http_im_http_version,
        js_node_http_im_headers_json,
        js_node_http_im_raw_headers_json,
        js_node_http_im_headers_distinct_json,
        js_node_http_im_trailers_json,
        js_node_http_im_raw_trailers_json,
        js_node_http_im_trailers_distinct_json,
        js_node_http_im_remote_address,
        js_node_http_im_remote_port,
        js_node_http_im_on,
        js_node_http_im_read,
        js_node_http_im_pause,
        js_node_http_im_resume,
        js_node_http_im_pause_self,
        js_node_http_im_resume_self,
        js_node_http_im_aborted,
        js_node_http_im_complete,
        js_node_http_im_destroy,
        js_node_http_im_destroyed,
        js_node_http_im_set_timeout,
        js_node_https_create_server,
        js_node_https_server_listen,
        js_node_https_server_close,
        js_node_https_server_close_all_connections,
        js_node_https_server_close_idle_connections,
        js_node_https_server_on,
        js_node_https_server_address_json,
        js_node_https_server_headers_timeout,
        js_node_https_server_set_headers_timeout,
        js_node_https_server_keep_alive_timeout,
        js_node_https_server_set_keep_alive_timeout,
        js_node_https_server_keep_alive_timeout_buffer,
        js_node_https_server_set_keep_alive_timeout_buffer,
        js_node_https_server_request_timeout,
        js_node_https_server_set_request_timeout,
        js_node_https_server_idle_timeout,
        js_node_https_server_set_idle_timeout,
        js_node_https_server_max_headers_count,
        js_node_https_server_set_max_headers_count,
        js_node_https_server_max_requests_per_socket,
        js_node_https_server_set_max_requests_per_socket,
        js_node_https_server_set_timeout_method,
        js_node_https_server_ref,
        js_node_https_server_unref,
        js_node_http2_create_secure_server,
        js_node_http2_server_listen,
        js_node_http2_server_close,
        js_node_http2_server_on,
        js_node_http2_server_address_json,
        js_node_http2_get_default_settings,
        js_node_http2_get_packed_settings,
        js_node_http2_get_unpacked_settings,
    ]
};
