//! Stdlib / FFI runtime function declarations (extracted from runtime_decls.rs).

use super::*;

/// Stdlib / FFI runtime functions. Without these declarations, user code
/// that touches any of the third-party stdlib modules (http, mysql2, pg,
/// redis, mongodb, bcrypt, jsonwebtoken, axios, sharp, cron, WebSocket,
/// zlib, etc.) emits `use of undefined value '@js_*'` at clang -c time
/// because the IR references the name without a preceding `declare`.
///
/// Signatures cross-checked against `crates/perry-runtime/src/` and
/// `crates/perry-stdlib/src/`.
pub fn declare_stdlib_ffi(module: &mut LlModule) {
    // ========== HTTP server ==========
    module.declare_function("js_http_client_request_end", I64, &[I64, DOUBLE]);
    module.declare_function("js_http_client_request_write", I64, &[I64, DOUBLE]);
    module.declare_function("js_http_get", I64, &[DOUBLE, I64]);
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
    module.declare_function("js_http_server_accept_v2", I64, &[I64]);
    module.declare_function("js_http_server_close", DOUBLE, &[I64]);
    module.declare_function("js_http_server_create", I64, &[DOUBLE]);
    module.declare_function("js_http_set_header", I64, &[I64, I64, I64]);
    module.declare_function("js_http_set_timeout", I64, &[I64, DOUBLE]);
    module.declare_function("js_http_status_code", DOUBLE, &[I64]);
    module.declare_function("js_http_status_message", I64, &[I64]);

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
    module.declare_function("js_node_http_server_listen", VOID, &[I64, DOUBLE, I64]);
    module.declare_function("js_node_http_server_close", VOID, &[I64, I64]);
    module.declare_function("js_node_http_server_close_all_connections", VOID, &[I64]);
    module.declare_function("js_node_http_server_close_idle_connections", VOID, &[I64]);
    module.declare_function("js_node_http_server_address_json", I64, &[I64]);
    module.declare_function("js_node_http_server_listening", I32, &[I64]);
    module.declare_function("js_node_http_server_on", DOUBLE, &[I64, I64, I64]);
    // IncomingMessage:
    module.declare_function("js_node_http_im_method", I64, &[I64]);
    module.declare_function("js_node_http_im_url", I64, &[I64]);
    module.declare_function("js_node_http_im_http_version", I64, &[I64]);
    module.declare_function("js_node_http_im_headers_json", I64, &[I64]);
    module.declare_function("js_node_http_im_raw_headers_json", I64, &[I64]);
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
    // ServerResponse:
    module.declare_function("js_node_http_res_set_status", VOID, &[I64, DOUBLE]);
    module.declare_function("js_node_http_res_get_status", DOUBLE, &[I64]);
    module.declare_function("js_node_http_res_set_status_message", VOID, &[I64, I64]);
    module.declare_function("js_node_http_res_set_header", VOID, &[I64, I64, I64]);
    module.declare_function("js_node_http_res_get_header", DOUBLE, &[I64, I64]);
    module.declare_function("js_node_http_res_remove_header", VOID, &[I64, I64]);
    module.declare_function("js_node_http_res_has_header", I32, &[I64, I64]);
    module.declare_function("js_node_http_res_get_headers_json", I64, &[I64]);
    module.declare_function("js_node_http_res_get_header_names_json", I64, &[I64]);
    module.declare_function("js_node_http_res_headers_sent", I32, &[I64]);
    module.declare_function("js_node_http_res_writable_ended", I32, &[I64]);
    module.declare_function("js_node_http_res_writable_finished", I32, &[I64]);
    module.declare_function(
        "js_node_http_res_write_head",
        VOID,
        &[I64, DOUBLE, I64, I64],
    );
    module.declare_function("js_node_http_res_write", I32, &[I64, DOUBLE]);
    module.declare_function("js_node_http_res_add_trailers", VOID, &[I64, DOUBLE]);
    module.declare_function("js_node_http_res_end", VOID, &[I64, DOUBLE]);
    module.declare_function("js_node_http_res_flush_headers", VOID, &[I64]);
    module.declare_function("js_node_http_res_write_continue", VOID, &[I64]);
    module.declare_function("js_node_http_res_write_processing", VOID, &[I64]);
    module.declare_function("js_node_http_res_on", DOUBLE, &[I64, I64, I64]);
    // node:https server (TLS via rustls):
    module.declare_function("js_node_https_create_server", I64, &[DOUBLE, I64]);
    module.declare_function("js_node_https_server_listen", VOID, &[I64, DOUBLE, I64]);
    module.declare_function("js_node_https_server_close", VOID, &[I64, I64]);
    module.declare_function("js_node_https_server_address_json", I64, &[I64]);
    module.declare_function("js_node_https_server_on", DOUBLE, &[I64, I64, I64]);
    // node:http2 secure server (HTTP/2 with ALPN):
    module.declare_function("js_node_http2_create_secure_server", I64, &[DOUBLE, I64]);
    module.declare_function("js_node_http2_server_listen", VOID, &[I64, DOUBLE, I64]);
    module.declare_function("js_node_http2_server_close", VOID, &[I64, I64]);
    module.declare_function("js_node_http2_server_address_json", I64, &[I64]);
    module.declare_function("js_node_http2_server_on", DOUBLE, &[I64, I64, I64]);

    // ========== PostgreSQL (pg) ==========
    module.declare_function("js_pg_client_connect", I64, &[I64]);
    module.declare_function("js_pg_client_end", I64, &[I64]);
    module.declare_function("js_pg_client_new", I64, &[I64]);
    module.declare_function("js_pg_client_query", I64, &[I64, I64]);
    module.declare_function("js_pg_client_query_params", I64, &[I64, I64, I64]);
    module.declare_function("js_pg_connect", I64, &[I64]);
    module.declare_function("js_pg_create_pool", I64, &[I64]);
    module.declare_function("js_pg_pool_end", I64, &[I64]);
    module.declare_function("js_pg_pool_new", I64, &[I64]);
    module.declare_function("js_pg_pool_query", I64, &[I64, I64]);

    // ========== Redis / ioredis ==========
    module.declare_function("js_ioredis_connect", I64, &[I64]);
    module.declare_function("js_ioredis_decr", I64, &[I64, I64]);
    module.declare_function("js_ioredis_del", I64, &[I64, I64]);
    module.declare_function("js_ioredis_disconnect", VOID, &[I64]);
    module.declare_function("js_ioredis_exists", I64, &[I64, I64]);
    module.declare_function("js_ioredis_expire", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_ioredis_get", I64, &[I64, I64]);
    module.declare_function("js_ioredis_hdel", I64, &[I64, I64, I64]);
    module.declare_function("js_ioredis_hget", I64, &[I64, I64, I64]);
    module.declare_function("js_ioredis_hgetall", I64, &[I64, I64]);
    module.declare_function("js_ioredis_hlen", I64, &[I64, I64]);
    module.declare_function("js_ioredis_hset", I64, &[I64, I64, I64, I64]);
    module.declare_function("js_ioredis_incr", I64, &[I64, I64]);
    module.declare_function("js_ioredis_new", I64, &[I64]);
    module.declare_function("js_ioredis_ping", I64, &[I64]);
    module.declare_function("js_ioredis_quit", I64, &[I64]);
    module.declare_function("js_ioredis_set", I64, &[I64, I64, I64]);
    module.declare_function("js_ioredis_setex", I64, &[I64, I64, DOUBLE, I64]);

    // ========== MongoDB ==========
    module.declare_function("js_mongodb_client_close", I64, &[I64]);
    module.declare_function("js_mongodb_client_connect", I64, &[I64]);
    module.declare_function("js_mongodb_client_db", I64, &[I64, I64]);
    module.declare_function("js_mongodb_client_list_databases", I64, &[I64]);
    module.declare_function("js_mongodb_client_new", I64, &[I64]);
    // _value wrappers (JSON-stringify f64 JSValue arg, forward to existing fns)
    module.declare_function("js_mongodb_collection_count_value", I64, &[I64, DOUBLE]);
    module.declare_function(
        "js_mongodb_collection_delete_many_value",
        I64,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_mongodb_collection_delete_one_value",
        I64,
        &[I64, DOUBLE],
    );
    module.declare_function("js_mongodb_collection_find_one_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_mongodb_collection_find_value", I64, &[I64, DOUBLE]);
    module.declare_function(
        "js_mongodb_collection_insert_many_value",
        I64,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_mongodb_collection_insert_one_value",
        I64,
        &[I64, DOUBLE],
    );
    module.declare_function(
        "js_mongodb_collection_update_many_value",
        I64,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_mongodb_collection_update_one_value",
        I64,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function("js_mongodb_collection_count", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_delete_many", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_delete_one", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_find", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_find_one", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_insert_many", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_insert_one", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_update_many", I64, &[I64, I64, I64]);
    module.declare_function("js_mongodb_collection_update_one", I64, &[I64, I64, I64]);
    module.declare_function("js_mongodb_connect", I64, &[I64]);
    module.declare_function("js_mongodb_db_collection", I64, &[I64, I64]);
    module.declare_function("js_mongodb_db_list_collections", I64, &[I64]);

    // ========== bcrypt / argon2 ==========
    module.declare_function("js_argon2_hash", I64, &[I64]);
    module.declare_function("js_argon2_hash_options", I64, &[I64, I64]);
    module.declare_function("js_argon2_verify", I64, &[I64, I64]);
    module.declare_function("js_bcrypt_compare", I64, &[I64, I64]);
    module.declare_function("js_bcrypt_compare_sync", DOUBLE, &[I64, I64]);
    module.declare_function("js_bcrypt_gen_salt", I64, &[DOUBLE]);
    module.declare_function("js_bcrypt_hash", I64, &[I64, DOUBLE]);
    module.declare_function("js_bcrypt_hash_sync", I64, &[I64, DOUBLE]);

    // `@perryts/google-auth` is no longer declared centrally — the
    // signatures come from the installed npm package's
    // `perry.nativeLibrary.functions` block (see
    // https://github.com/PerryTS/google-auth) and are added to
    // `ffi_signatures` on demand by the external-nativeLibrary path.

    // ========== perry/ads (issue #867) ==========
    // Four promise-returning entry points (NR_PTR — i64 return,
    // NaN-boxed as POINTER) plus two synchronous banner FFI
    // functions (NR_F64 / NR_VOID). String args lower to
    // `*const StringHeader` (i64) per the codegen NA_STR
    // convention; the f64 handle is the NaN-boxable numeric
    // return for banner_create.
    module.declare_function("js_ads_interstitial_load", I64, &[I64]);
    module.declare_function("js_ads_interstitial_show", I64, &[]);
    module.declare_function("js_ads_rewarded_load", I64, &[I64]);
    module.declare_function("js_ads_rewarded_show", I64, &[]);
    module.declare_function("js_ads_banner_create", DOUBLE, &[I64, I64]);
    module.declare_function("js_ads_banner_destroy", VOID, &[DOUBLE]);

    // ========== perry/thread (parallelMap, parallelFilter, spawn) ==========
    module.declare_function("js_thread_parallel_map", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_thread_parallel_filter", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_thread_spawn", DOUBLE, &[DOUBLE]);

    // ========== jsonwebtoken / JWT ==========
    module.declare_function("js_jwt_decode", I64, &[I64]);
    module.declare_function("js_jwt_sign", I64, &[I64, I64, DOUBLE, I64]);
    module.declare_function("js_jwt_sign_es256", I64, &[I64, I64, DOUBLE, I64]);
    module.declare_function("js_jwt_sign_rs256", I64, &[I64, I64, DOUBLE, I64]);
    module.declare_function("js_jwt_verify", I64, &[I64, I64]);
    module.declare_function("js_jwt_verify_es256", I64, &[I64, I64]);
    module.declare_function("js_jwt_verify_rs256", I64, &[I64, I64]);
    // #1074: runtime-algorithm dispatchers. The codegen `lower_jsonwebtoken_*`
    // fast paths still hard-route literal `algorithm: "ES256"` to the typed
    // helpers above; non-literal shapes (const-bound ident, spread, ternary)
    // are routed here with the alg name lowered as a string at runtime.
    module.declare_function("js_jwt_sign_dyn", I64, &[I64, I64, I64, DOUBLE, I64]);
    module.declare_function("js_jwt_verify_dyn", I64, &[I64, I64, I64]);
    // #1074 case C: options is a whole non-extractable expression
    // (`const opts = { algorithm: "ES256" }; jwt.sign(p, k, opts)`). We
    // pass `opts` as a NaN-boxed JSValue and the runtime helper extracts
    // `algorithm` / `expiresIn` / `keyid` via `js_object_get_field_by_name`.
    module.declare_function("js_jwt_sign_dyn_opts", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_jwt_verify_dyn_opts", I64, &[I64, I64, DOUBLE]);

    // ========== axios / node-fetch ==========
    module.declare_function("js_axios_create", DOUBLE, &[I64]);
    module.declare_function("js_axios_delete", I64, &[I64]);
    module.declare_function("js_axios_get", I64, &[I64]);
    // #598: body arg is a NaN-boxed f64 (DOUBLE) so the runtime can
    // distinguish strings from objects via the tag and JSON.stringify
    // non-string bodies. Pre-fix this was I64 (raw unboxed pointer)
    // which had no way to tell `axios.post(url, "raw json")` from
    // `axios.post(url, {a: 1})`.
    module.declare_function("js_axios_post", I64, &[I64, DOUBLE]);
    module.declare_function("js_axios_put", I64, &[I64, DOUBLE]);
    module.declare_function("js_axios_patch", I64, &[I64, DOUBLE]);
    module.declare_function("js_axios_request", I64, &[I64]);
    module.declare_function("js_axios_response_status", DOUBLE, &[I64]);
    module.declare_function("js_axios_response_status_text", I64, &[I64]);
    module.declare_function("js_axios_response_data", I64, &[I64]);
    // Issue #604 followup — JSON-auto-parsing variant of `.data`. Returns
    // a NaN-boxed JSValue (parsed object/array/number/bool/null when the
    // response body is JSON, raw string otherwise) so `r.data.ok` works
    // the same way as npm `axios` does for `application/json` responses.
    module.declare_function("js_axios_response_data_parsed", DOUBLE, &[I64]);

    // ========== sharp / image ==========
    module.declare_function("js_sharp_blur", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_flip", I64, &[I64]);
    module.declare_function("js_sharp_flop", I64, &[I64]);
    module.declare_function("js_sharp_from_buffer", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_from_file", I64, &[I64]);
    module.declare_function("js_sharp_grayscale", I64, &[I64]);
    module.declare_function("js_sharp_metadata", I64, &[I64]);
    module.declare_function("js_sharp_negate", I64, &[I64]);
    module.declare_function("js_sharp_quality", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_resize", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_sharp_rotate", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_to_buffer", I64, &[I64]);
    module.declare_function("js_sharp_to_file", I64, &[I64, I64]);
    module.declare_function("js_sharp_to_format", I64, &[I64, I64]);

    // ========== cron / scheduler ==========
    module.declare_function("js_cron_clear_interval", VOID, &[I64]);
    module.declare_function("js_cron_clear_timeout", VOID, &[I64]);
    module.declare_function("js_cron_describe", I64, &[I64]);
    module.declare_function("js_cron_job_is_running", DOUBLE, &[I64]);
    module.declare_function("js_cron_job_start", VOID, &[I64]);
    module.declare_function("js_cron_job_stop", VOID, &[I64]);
    module.declare_function("js_cron_next_date", I64, &[I64]);
    module.declare_function("js_cron_next_dates", I64, &[I64, DOUBLE]);
    module.declare_function("js_cron_schedule", I64, &[I64, I64]);
    module.declare_function("js_cron_set_interval", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_cron_set_timeout", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_cron_timer_has_pending", I32, &[]);
    module.declare_function("js_cron_timer_tick", I32, &[]);
    module.declare_function("js_cron_validate", DOUBLE, &[I64]);

    // ========== async_hooks / AsyncLocalStorage ==========
    module.declare_function("js_async_hooks_create_hook", I64, &[DOUBLE]);
    module.declare_function("js_async_hooks_execution_async_id", DOUBLE, &[]);
    module.declare_function("js_async_hooks_trigger_async_id", DOUBLE, &[]);
    module.declare_function("js_async_hook_enable", I64, &[I64]);
    module.declare_function("js_async_hook_disable", I64, &[I64]);
    module.declare_function("js_async_resource_new", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_async_resource_async_id", DOUBLE, &[I64]);
    module.declare_function("js_async_resource_trigger_async_id", DOUBLE, &[I64]);
    module.declare_function("js_async_resource_emit_destroy", I64, &[I64]);
    module.declare_function(
        "js_async_resource_run_in_async_scope",
        DOUBLE,
        &[I64, I64, DOUBLE, I64],
    );
    module.declare_function("js_async_resource_bind", I64, &[I64, I64]);
    module.declare_function("js_async_resource_static_bind", I64, &[I64, DOUBLE]);
    module.declare_function("js_async_local_storage_disable", VOID, &[I64]);
    module.declare_function("js_async_local_storage_enter_with", VOID, &[I64, DOUBLE]);
    module.declare_function("js_async_local_storage_exit", DOUBLE, &[I64, I64]);
    module.declare_function("js_async_local_storage_get_store", DOUBLE, &[I64]);
    module.declare_function("js_async_local_storage_new", I64, &[]);
    module.declare_function("js_async_local_storage_run", DOUBLE, &[I64, DOUBLE, I64]);

    // ========== zlib ==========
    module.declare_function("js_zlib_deflate_sync", I64, &[I64]);
    module.declare_function("js_zlib_gunzip", I64, &[I64]);
    module.declare_function("js_zlib_gunzip_sync", I64, &[I64]);
    module.declare_function("js_zlib_gzip", I64, &[I64]);
    module.declare_function("js_zlib_gzip_sync", I64, &[I64]);
    module.declare_function("js_zlib_inflate_sync", I64, &[I64]);
    module.declare_function("js_zlib_deflate_raw_sync", I64, &[I64]);
    module.declare_function("js_zlib_inflate_raw_sync", I64, &[I64]);
    module.declare_function("js_zlib_unzip_sync", I64, &[I64]);
    module.declare_function("js_zlib_crc32", DOUBLE, &[I64, DOUBLE]);
    // #1843 — Brotli one-shots (StringHeader ptr in/out; async returns a Promise ptr).
    module.declare_function("js_zlib_brotli_compress_sync", I64, &[I64]);
    module.declare_function("js_zlib_brotli_decompress_sync", I64, &[I64]);
    module.declare_function("js_zlib_brotli_compress", I64, &[I64]);
    module.declare_function("js_zlib_brotli_decompress", I64, &[I64]);
    // #1843 — Transform-stream factories: `_opts` (DOUBLE) in, i64 handle out.
    // (`js_zlib_create_brotli_decompress` is declared alongside the other
    // crypto/zlib helpers in runtime_decls/strings.rs.)
    module.declare_function("js_zlib_create_gzip", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_gunzip", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_deflate", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_inflate", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_deflate_raw", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_inflate_raw", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_unzip", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_brotli_compress", I64, &[DOUBLE]);

    // ========== Buffer ==========
    module.declare_function("js_buffer_alloc_unsafe", I64, &[I32]);
    module.declare_function("js_buffer_byte_length", I32, &[I64]);
    module.declare_function("js_buffer_byte_length_value", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_buffer_concat", I64, &[I64]);
    module.declare_function("js_buffer_copy", I32, &[I64, I64, I32, I32, I32]);
    module.declare_function("js_buffer_equals", I32, &[I64, I64]);
    module.declare_function("js_buffer_fill", I64, &[I64, I32]);
    module.declare_function("js_buffer_from_value", I64, &[I64, I32]);
    module.declare_function("js_buffer_is_ascii", DOUBLE, &[DOUBLE]);
    module.declare_function("js_buffer_is_buffer", I32, &[I64]);
    module.declare_function("js_buffer_is_encoding", I32, &[DOUBLE]);
    module.declare_function("js_buffer_is_utf8", DOUBLE, &[DOUBLE]);
    module.declare_function("js_buffer_print", VOID, &[I64]);
    module.declare_function("js_buffer_set", VOID, &[I64, I32, I32]);
    module.declare_function("js_buffer_set_from", VOID, &[I64, I64, I32]);
    module.declare_function("js_buffer_slice", I64, &[I64, I32, I32]);
    module.declare_function("js_buffer_to_string", I64, &[I64, I32]);
    // Issue #1210: `buffer.transcode(source, fromEnc, toEnc)`. Source is a
    // NaN-boxed Buffer pointer (DOUBLE), encodings are NaN-boxed strings
    // (DOUBLE). Returns a raw *mut BufferHeader (I64) — NR_PTR in the
    // native dispatch table NaN-boxes the result with POINTER_TAG.
    module.declare_function("js_buffer_transcode", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_buffer_write", I32, &[I64, I64, I32, I32]);

    // ========== child_process ==========
    // execSync → NaN-boxed stdout (Buffer by default / string with `encoding`);
    // throws on non-zero exit. Returns DOUBLE. #1937/#1938.
    module.declare_function("js_child_process_exec_sync", DOUBLE, &[I64, I64]);
    // exec(cmd, options?, callback?): cmd string ptr (I64), options + callback
    // as NaN-boxed f64 in either slot; returns undefined (callback form) or the
    // stdout string (no-callback form). See `js_child_process_exec`.
    module.declare_function("js_child_process_exec", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_child_process_get_process_status", I64, &[DOUBLE]);
    module.declare_function("js_child_process_kill_process", I32, &[DOUBLE]);
    module.declare_function(
        "js_child_process_spawn_background",
        I64,
        &[DOUBLE, I64, DOUBLE, DOUBLE],
    );
    module.declare_function("js_child_process_spawn_sync", I64, &[I64, I64, I64]);
    // #1780: streaming spawn → NaN-boxed ChildProcess pointer (returns DOUBLE).
    module.declare_function("js_child_process_spawn_streams", DOUBLE, &[I64, I64, I64]);
    // #1933: fork(modulePath, args, options) → NaN-boxed ChildProcess with an
    // IPC channel (send/disconnect/'message'/connected/channel).
    module.declare_function("js_child_process_fork", DOUBLE, &[I64, I64, I64]);
    // #1780: execFile (file, args, options, callback) + execFileSync (file, args, options).
    module.declare_function(
        "js_child_process_exec_file",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE, DOUBLE],
    );
    // execFileSync → NaN-boxed stdout (Buffer by default / string with
    // `encoding`); throws on non-zero exit. Returns DOUBLE. #1937/#1938.
    module.declare_function(
        "js_child_process_exec_file_sync",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE],
    );

    // ========== cheerio ==========
    module.declare_function("js_cheerio_load", I64, &[I64]);
    module.declare_function("js_cheerio_load_fragment", I64, &[I64]);
    module.declare_function("js_cheerio_select", I64, &[I64, I64]);
    module.declare_function("js_cheerio_selection_attr", I64, &[I64, I64]);
    module.declare_function("js_cheerio_selection_attrs", I64, &[I64, I64]);
    module.declare_function("js_cheerio_selection_children", I64, &[I64, I64]);
    module.declare_function("js_cheerio_selection_eq", I64, &[I64, DOUBLE]);
    module.declare_function("js_cheerio_selection_find", I64, &[I64, I64]);
    module.declare_function("js_cheerio_selection_first", I64, &[I64]);
    module.declare_function("js_cheerio_selection_has_class", DOUBLE, &[I64, I64]);
    module.declare_function("js_cheerio_selection_html", I64, &[I64]);
    module.declare_function("js_cheerio_selection_is", DOUBLE, &[I64, I64]);
    module.declare_function("js_cheerio_selection_last", I64, &[I64]);
    module.declare_function("js_cheerio_selection_length", DOUBLE, &[I64]);
    module.declare_function("js_cheerio_selection_parent", I64, &[I64]);
    module.declare_function("js_cheerio_selection_text", I64, &[I64]);
    module.declare_function("js_cheerio_selection_texts", I64, &[I64]);
    module.declare_function("js_cheerio_selection_to_array", I64, &[I64]);

    // ========== URL / URLSearchParams ==========
    // Rust runtime signatures (see crates/perry-runtime/src/url.rs):
    //   js_url_new(*mut StringHeader)                         -> *mut ObjectHeader
    //   js_url_new_with_base(*mut StringHeader, *mut ...)     -> *mut ObjectHeader
    //   js_url_get_{href,pathname,protocol,host,hostname,port,search,hash,origin,search_params}
    //     (*mut ObjectHeader)                                  -> f64 (NaN-boxed string)
    //   js_url_search_params_new(*mut StringHeader)            -> *mut ObjectHeader
    //   js_url_search_params_new_empty()                       -> *mut ObjectHeader
    //   js_url_search_params_get(*mut ObjectHeader, *mut StringHeader)
    //                                                          -> *mut StringHeader (null if missing)
    //   js_url_search_params_has(*mut ObjectHeader, *mut StringHeader)
    //                                                          -> f64 (0.0 or 1.0)
    //   js_url_search_params_set/append(*mut ObjectHeader, *mut ..., *mut ...) -> void
    //   js_url_search_params_delete(*mut ObjectHeader, *mut StringHeader)      -> void
    //   js_url_search_params_to_string(*mut ObjectHeader)     -> *mut StringHeader
    //   js_url_search_params_get_all(*mut ObjectHeader, *mut StringHeader)
    //                                                          -> f64 (NaN-boxed array)
    module.declare_function("js_url_file_url_to_path", DOUBLE, &[DOUBLE]);
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
    // Issue #650: URL.canParse / URL.parse static methods (Node 18+ / 22+).
    module.declare_function("js_url_can_parse", I32, &[I64]);
    module.declare_function("js_url_can_parse_with_base", I32, &[I64, I64]);
    module.declare_function("js_url_parse", I64, &[I64]);
    // Issue #650: URL setters — mutate field + re-derive href.
    module.declare_function("js_url_set_pathname", VOID, &[I64, I64]);
    module.declare_function("js_url_set_search", VOID, &[I64, I64]);
    module.declare_function("js_url_set_hash", VOID, &[I64, I64]);
    module.declare_function("js_url_set_protocol", VOID, &[I64, I64]);
    module.declare_function("js_url_set_hostname", VOID, &[I64, I64]);
    module.declare_function("js_url_set_port", VOID, &[I64, I64]);
    module.declare_function("js_url_set_username", VOID, &[I64, I64]);
    module.declare_function("js_url_set_password", VOID, &[I64, I64]);
    module.declare_function("js_url_search_params_has2", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_url_search_params_delete2", VOID, &[I64, I64, I64]);
    module.declare_function("js_url_search_params_append", VOID, &[I64, I64, I64]);
    module.declare_function("js_url_search_params_delete", VOID, &[I64, I64]);
    module.declare_function("js_url_search_params_get", I64, &[I64, I64]);
    module.declare_function("js_url_search_params_get_all", DOUBLE, &[I64, I64]);
    module.declare_function("js_url_search_params_has", DOUBLE, &[I64, I64]);
    module.declare_function("js_url_search_params_new", I64, &[I64]);
    // Generic init that handles string / record / URLSearchParams / null /
    // undefined — see `js_url_search_params_new_any` rustdoc. Refs #575.
    module.declare_function("js_url_search_params_new_any", I64, &[DOUBLE]);
    module.declare_function("js_url_search_params_new_empty", I64, &[]);
    module.declare_function("js_url_search_params_set", VOID, &[I64, I64, I64]);
    module.declare_function("js_url_search_params_to_string", I64, &[I64]);
    // Issue #650: URLSearchParams.size getter — returns entries count.
    module.declare_function("js_url_search_params_size", I32, &[I64]);
    // params.entries() / iteration source — returns an already NaN-boxed
    // POINTER_TAG f64 to ArrayHeader<[k, v]> (refs #575).
    module.declare_function("js_url_search_params_entries_arr", DOUBLE, &[I64]);
    module.declare_function("js_url_search_params_keys_arr", DOUBLE, &[I64]);
    module.declare_function("js_url_search_params_values_arr", DOUBLE, &[I64]);
    module.declare_function("js_url_search_params_sort", VOID, &[I64]);
    module.declare_function("js_url_search_params_for_each", VOID, &[I64, DOUBLE]);
    module.declare_function("js_url_path_to_file_url", DOUBLE, &[DOUBLE]);
    module.declare_function("js_url_domain_to_ascii", DOUBLE, &[DOUBLE]);
    module.declare_function("js_url_domain_to_unicode", DOUBLE, &[DOUBLE]);
    module.declare_function("js_url_to_http_options", DOUBLE, &[DOUBLE]);
    module.declare_function("js_url_format", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_url_legacy_parse", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_url_legacy_resolve", DOUBLE, &[DOUBLE, DOUBLE]);

    // ========== WebSocket ==========
    module.declare_function("js_ws_close", VOID, &[I64]);
    module.declare_function("js_ws_connect", I64, &[I64]);
    module.declare_function("js_ws_connect_start", DOUBLE, &[DOUBLE]);
    module.declare_function("js_ws_handle_to_i64", I64, &[DOUBLE]);
    module.declare_function("js_ws_is_open", DOUBLE, &[I64]);
    module.declare_function("js_ws_message_count", DOUBLE, &[I64]);
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

    // ========== SQLite ==========
    module.declare_function("js_sqlite_close", VOID, &[I64]);
    module.declare_function("js_sqlite_exec", VOID, &[I64, I64]);
    module.declare_function("js_sqlite_open", I64, &[I64]);
    module.declare_function("js_sqlite_pragma", I64, &[I64, I64, I64]);
    module.declare_function("js_sqlite_prepare", I64, &[I64, I64]);
    module.declare_function("js_sqlite_stmt_all", I64, &[I64, I64]);
    module.declare_function("js_sqlite_stmt_get", I64, &[I64, I64]);
    module.declare_function("js_sqlite_stmt_run", I64, &[I64, I64]);
    module.declare_function("js_sqlite_transaction", I64, &[I64, I64]);
    module.declare_function("js_sqlite_transaction_commit", VOID, &[I64]);
    module.declare_function("js_sqlite_transaction_rollback", VOID, &[I64]);

    // ========== OS ==========
    module.declare_function("js_os_cpus", I64, &[]);
    module.declare_function("js_os_freemem", DOUBLE, &[]);
    module.declare_function("js_os_homedir", I64, &[]);
    module.declare_function("js_os_network_interfaces", I64, &[]);
    module.declare_function("js_os_tmpdir", I64, &[]);
    module.declare_function("js_os_totalmem", DOUBLE, &[]);
    module.declare_function("js_os_uptime", DOUBLE, &[]);
    module.declare_function("js_os_user_info", I64, &[]);
    module.declare_function("js_os_user_info_buffer", I64, &[]);

    // ========== Crypto ==========
    module.declare_function("js_crypto_aes256_decrypt", I64, &[I64, I64, I64]);
    module.declare_function("js_crypto_aes256_encrypt", I64, &[I64, I64, I64]);
    module.declare_function("js_crypto_aes256_gcm_decrypt", I64, &[I64, I64, I64]);
    module.declare_function("js_crypto_aes256_gcm_encrypt", I64, &[I64, I64, I64]);
    // Handle-based createCipheriv / createDecipheriv (#1075) — return a
    // pre-NaN-boxed f64 carrying POINTER_TAG + handle id. Dispatched
    // through HANDLE_METHOD_DISPATCH → `dispatch_cipher` for .update() /
    // .final() / .getAuthTag() / .setAuthTag().
    module.declare_function(
        "js_crypto_create_cipheriv",
        DOUBLE,
        &[I64, I64, I64, DOUBLE],
    );
    module.declare_function(
        "js_crypto_create_decipheriv",
        DOUBLE,
        &[I64, I64, I64, DOUBLE],
    );
    // crypto.createSign(alg) / createVerify(alg) -> SignHandle (NaN-boxed).
    module.declare_function("js_crypto_create_sign", DOUBLE, &[I64]);
    module.declare_function("js_crypto_create_verify", DOUBLE, &[I64]);
    module.declare_function("js_crypto_hkdf_sha256", I64, &[I64, I64, I64, DOUBLE]);
    // crypto.hkdfSync(digest, ikm, salt, info, keylen) -> ArrayBuffer.
    module.declare_function("js_crypto_hkdf_sync", I64, &[I64, I64, I64, I64, DOUBLE]);
    module.declare_function("js_crypto_pbkdf2", I64, &[I64, I64, DOUBLE, DOUBLE]);
    module.declare_function("js_crypto_random_bytes_hex", I64, &[DOUBLE]);
    module.declare_function("js_crypto_random_nonce", I64, &[]);
    module.declare_function("js_crypto_scrypt", I64, &[I64, I64, DOUBLE]);
    // crypto.scryptSync(password, salt, keylen, options?) -> Buffer. The 4th
    // arg is the NaN-unboxed options-object pointer (0 = none).
    module.declare_function("js_crypto_scrypt_bytes", I64, &[I64, I64, DOUBLE, I64]);
    // crypto.generateKeyPairSync(type, options) -> { publicKey, privateKey }.
    module.declare_function("js_crypto_generate_key_pair_sync", DOUBLE, &[I64, I64]);
    module.declare_function(
        "js_crypto_scrypt_custom",
        I64,
        &[I64, I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_crypto_x25519_keypair", I64, &[]);
    module.declare_function("js_crypto_x25519_shared_secret", I64, &[I64, I64]);
    module.declare_function("js_keccak256_native", I64, &[I64]);
    module.declare_function("js_keccak256_native_bytes", I64, &[I64]);

    // ========== Nanoid ==========
    module.declare_function("js_nanoid", I64, &[DOUBLE]);
    module.declare_function("js_nanoid_custom", I64, &[I64, DOUBLE]);

    // ========== @perryts/pdf (issue #516) ==========
    // createPdf returns an i64 handle (NaN-boxed POINTER_TAG by
    // codegen via NR_PTR). The mutator ops are Rust `-> ()` and
    // therefore VOID at the LLVM ABI level.
    module.declare_function("js_pdf_create_pdf", I64, &[DOUBLE]);
    module.declare_function("js_pdf_add_text", VOID, &[I64, I64, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_pdf_add_line",
        VOID,
        &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_pdf_new_page", VOID, &[I64]);
    module.declare_function("js_pdf_save", VOID, &[I64]);

    // ========== Commander CLI ==========
    module.declare_function("js_commander_action", I64, &[I64, I64]);
    module.declare_function("js_commander_command", I64, &[I64, I64]);
    module.declare_function("js_commander_description", I64, &[I64, I64]);
    module.declare_function("js_commander_get_option", I64, &[I64, I64]);
    module.declare_function("js_commander_get_option_bool", DOUBLE, &[I64, I64]);
    module.declare_function("js_commander_get_option_number", DOUBLE, &[I64, I64]);
    module.declare_function("js_commander_name", I64, &[I64, I64]);
    module.declare_function("js_commander_new", I64, &[]);
    module.declare_function("js_commander_option", I64, &[I64, I64, I64, I64]);
    module.declare_function("js_commander_opts", I64, &[I64]);
    module.declare_function("js_commander_parse", I64, &[I64, DOUBLE]);
    module.declare_function("js_commander_required_option", I64, &[I64, I64, I64, I64]);
    module.declare_function("js_commander_version", I64, &[I64, I64]);

    // ========== Dotenv ==========
    module.declare_function("js_dotenv_config", DOUBLE, &[]);
    module.declare_function("js_dotenv_config_path", DOUBLE, &[I64]);
    module.declare_function("js_dotenv_parse", I64, &[I64]);

    // ========== Date libs (dayjs/datefns/moment) ==========
    module.declare_function("js_datefns_add_days", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_add_months", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_add_years", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_difference_in_days", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_difference_in_hours", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_datefns_difference_in_minutes",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_datefns_end_of_day", DOUBLE, &[DOUBLE]);
    module.declare_function("js_datefns_format", I64, &[DOUBLE, I64]);
    module.declare_function("js_datefns_is_after", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_is_before", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_parse_iso", DOUBLE, &[I64]);
    module.declare_function("js_datefns_start_of_day", DOUBLE, &[DOUBLE]);
    module.declare_function("js_dayjs_add", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_dayjs_date", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_day", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_diff", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_dayjs_end_of", DOUBLE, &[I64, I64]);
    module.declare_function("js_dayjs_format", I64, &[I64, I64]);
    module.declare_function("js_dayjs_from_timestamp", DOUBLE, &[DOUBLE]);
    module.declare_function("js_dayjs_hour", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_is_after", DOUBLE, &[I64, I64]);
    module.declare_function("js_dayjs_is_before", DOUBLE, &[I64, I64]);
    module.declare_function("js_dayjs_is_same", DOUBLE, &[I64, I64]);
    module.declare_function("js_dayjs_is_valid", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_millisecond", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_minute", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_month", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_now", DOUBLE, &[]);
    module.declare_function("js_dayjs_parse", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_second", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_start_of", DOUBLE, &[I64, I64]);
    module.declare_function("js_dayjs_subtract", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_dayjs_to_iso_string", I64, &[I64]);
    module.declare_function("js_dayjs_unix", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_value_of", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_year", DOUBLE, &[I64]);
    module.declare_function("js_moment_add", I64, &[I64, DOUBLE, I64]);
    module.declare_function("js_moment_date", DOUBLE, &[I64]);
    module.declare_function("js_moment_day", DOUBLE, &[I64]);
    module.declare_function("js_moment_diff", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_moment_end_of", I64, &[I64, I64]);
    module.declare_function("js_moment_format", I64, &[I64, I64]);
    module.declare_function("js_moment_from_timestamp", I64, &[DOUBLE]);
    module.declare_function("js_moment_hour", DOUBLE, &[I64]);
    module.declare_function("js_moment_is_valid", DOUBLE, &[I64]);
    module.declare_function("js_moment_millisecond", DOUBLE, &[I64]);
    module.declare_function("js_moment_minute", DOUBLE, &[I64]);
    module.declare_function("js_moment_month", DOUBLE, &[I64]);
    module.declare_function("js_moment_now", I64, &[]);
    module.declare_function("js_moment_parse", I64, &[I64]);
    module.declare_function("js_moment_second", DOUBLE, &[I64]);
    module.declare_function("js_moment_start_of", I64, &[I64, I64]);
    module.declare_function("js_moment_subtract", I64, &[I64, DOUBLE, I64]);
    module.declare_function("js_moment_unix", DOUBLE, &[I64]);
    module.declare_function("js_moment_value_of", DOUBLE, &[I64]);
    module.declare_function("js_moment_year", DOUBLE, &[I64]);

    // ========== Decimal.js ==========
    module.declare_function("js_decimal_abs", I64, &[I64]);
    module.declare_function("js_decimal_ceil", I64, &[I64]);
    module.declare_function("js_decimal_cmp", DOUBLE, &[I64, I64]);
    module.declare_function("js_decimal_cmp_value", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_decimal_coerce_to_handle", I64, &[DOUBLE]);
    module.declare_function("js_decimal_div", I64, &[I64, I64]);
    module.declare_function("js_decimal_div_number", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_div_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_eq", DOUBLE, &[I64, I64]);
    module.declare_function("js_decimal_eq_value", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_decimal_floor", I64, &[I64]);
    module.declare_function("js_decimal_from_number", I64, &[DOUBLE]);
    module.declare_function("js_decimal_from_string", I64, &[I64]);
    module.declare_function("js_decimal_gt", DOUBLE, &[I64, I64]);
    module.declare_function("js_decimal_gt_value", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_decimal_gte", DOUBLE, &[I64, I64]);
    module.declare_function("js_decimal_gte_value", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_decimal_is_negative", DOUBLE, &[I64]);
    module.declare_function("js_decimal_is_positive", DOUBLE, &[I64]);
    module.declare_function("js_decimal_is_zero", DOUBLE, &[I64]);
    module.declare_function("js_decimal_lt", DOUBLE, &[I64, I64]);
    module.declare_function("js_decimal_lt_value", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_decimal_lte", DOUBLE, &[I64, I64]);
    module.declare_function("js_decimal_lte_value", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_decimal_minus", I64, &[I64, I64]);
    module.declare_function("js_decimal_minus_number", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_minus_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_mod", I64, &[I64, I64]);
    module.declare_function("js_decimal_mod_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_neg", I64, &[I64]);
    module.declare_function("js_decimal_plus", I64, &[I64, I64]);
    module.declare_function("js_decimal_plus_number", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_plus_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_pow", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_round", I64, &[I64]);
    module.declare_function("js_decimal_sqrt", I64, &[I64]);
    module.declare_function("js_decimal_times", I64, &[I64, I64]);
    module.declare_function("js_decimal_times_number", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_times_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_to_fixed", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_to_number", DOUBLE, &[I64]);
    module.declare_function("js_decimal_to_string", I64, &[I64]);

    // ========== Ethers / blockchain ==========
    module.declare_function("js_ethers_format_ether", I64, &[I64]);
    module.declare_function("js_ethers_format_units", I64, &[I64, DOUBLE]);
    module.declare_function("js_ethers_get_address", I64, &[I64]);
    module.declare_function("js_ethers_parse_ether", I64, &[I64]);
    module.declare_function("js_ethers_parse_units", I64, &[I64, DOUBLE]);

    // ========== Lodash ==========
    module.declare_function("js_lodash_camel_case", I64, &[I64]);
    module.declare_function("js_lodash_capitalize", I64, &[I64]);
    module.declare_function("js_lodash_chunk", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_clamp", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_lodash_compact", I64, &[I64]);
    module.declare_function("js_lodash_concat", I64, &[I64, I64]);
    module.declare_function("js_lodash_difference", I64, &[I64, I64]);
    module.declare_function("js_lodash_drop", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_drop_right", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_ends_with", DOUBLE, &[I64, I64]);
    module.declare_function("js_lodash_escape", I64, &[I64]);
    module.declare_function("js_lodash_first", DOUBLE, &[I64]);
    module.declare_function("js_lodash_flatten", I64, &[I64]);
    module.declare_function("js_lodash_in_range", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_lodash_includes", DOUBLE, &[I64, I64]);
    module.declare_function("js_lodash_initial", I64, &[I64]);
    module.declare_function("js_lodash_kebab_case", I64, &[I64]);
    module.declare_function("js_lodash_last", DOUBLE, &[I64]);
    module.declare_function("js_lodash_lower_case", I64, &[I64]);
    module.declare_function("js_lodash_lower_first", I64, &[I64]);
    module.declare_function("js_lodash_max", DOUBLE, &[I64]);
    module.declare_function("js_lodash_max_by", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lodash_mean", DOUBLE, &[I64]);
    module.declare_function("js_lodash_mean_by", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lodash_min", DOUBLE, &[I64]);
    module.declare_function("js_lodash_min_by", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lodash_pad", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_pad_end", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_pad_start", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_random", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_lodash_repeat", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_replace", I64, &[I64, I64, I64]);
    module.declare_function("js_lodash_reverse", I64, &[I64]);
    module.declare_function("js_lodash_size", DOUBLE, &[I64]);
    module.declare_function("js_lodash_snake_case", I64, &[I64]);
    module.declare_function("js_lodash_split", I64, &[I64, I64]);
    module.declare_function("js_lodash_start_case", I64, &[I64]);
    module.declare_function("js_lodash_starts_with", DOUBLE, &[I64, I64]);
    module.declare_function("js_lodash_sum", DOUBLE, &[I64]);
    module.declare_function("js_lodash_sum_by", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lodash_tail", I64, &[I64]);
    module.declare_function("js_lodash_take", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_take_right", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_trim", I64, &[I64]);
    module.declare_function("js_lodash_trim_end", I64, &[I64]);
    module.declare_function("js_lodash_trim_start", I64, &[I64]);
    module.declare_function("js_lodash_truncate", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_unescape", I64, &[I64]);
    module.declare_function("js_lodash_uniq", I64, &[I64]);
    module.declare_function("js_lodash_upper_case", I64, &[I64]);
    module.declare_function("js_lodash_upper_first", I64, &[I64]);

    // ========== LRU Cache ==========
    module.declare_function("js_lru_cache_clear", VOID, &[I64]);
    module.declare_function("js_lru_cache_delete", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lru_cache_get", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lru_cache_has", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lru_cache_new", I64, &[DOUBLE]);
    module.declare_function("js_lru_cache_peek", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lru_cache_set", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_lru_cache_size", DOUBLE, &[I64]);

    // ========== node:stream stubs (issue #631) ==========
    module.declare_function("js_node_stream_readable_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_writable_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_duplex_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_transform_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_passthrough_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_readable_from", DOUBLE, &[DOUBLE]);
    // #1534: static introspection helpers reflecting tracked stream state.
    module.declare_function("js_node_stream_is_disturbed", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_is_errored", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_is_readable", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_is_writable", DOUBLE, &[DOUBLE]);
    // #1537: getDefaultHighWaterMark(objectMode) / setDefaultHighWaterMark(objectMode, value).
    module.declare_function("js_node_stream_get_default_hwm", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_set_default_hwm", DOUBLE, &[DOUBLE, DOUBLE]);
    // #1541: addAbortSignal(signal, stream) — identity-returns the stream.
    module.declare_function("js_node_stream_add_abort_signal", DOUBLE, &[DOUBLE, DOUBLE]);
    // #1539: compose(...streams) -> new Duplex; duplexPair(opts) -> [Duplex, Duplex].
    module.declare_function("js_node_stream_compose", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_duplex_pair", DOUBLE, &[DOUBLE]);
    // #1540: Readable/Writable .toWeb / .fromWeb — return fresh Duplex stubs.
    module.declare_function("js_node_stream_to_web", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_from_web", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_method_readable_aborted", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_destroyed", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_destroy", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_node_stream_method_readable", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_readable_ended", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_cork", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_uncork", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_writable_corked", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_writable", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_writable_ended", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_writable_finished", DOUBLE, &[I64]);

    // ========== Event emitter ==========
    module.declare_function("js_event_emitter_emit", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_event_emitter_emit0", DOUBLE, &[I64, I64]);
    module.declare_function("js_event_emitter_listener_count", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_event_emitter_new", I64, &[]);
    module.declare_function("js_event_emitter_new_with_options", I64, &[DOUBLE]);
    module.declare_function("js_event_emitter_on", I64, &[I64, I64, I64]);
    module.declare_function("js_event_emitter_once", I64, &[I64, I64, I64]);
    module.declare_function("js_event_emitter_prepend_listener", I64, &[I64, I64, I64]);
    module.declare_function(
        "js_event_emitter_prepend_once_listener",
        I64,
        &[I64, I64, I64],
    );
    module.declare_function("js_event_emitter_remove_all_listeners", I64, &[I64, I64]);
    module.declare_function("js_event_emitter_remove_listener", I64, &[I64, I64, I64]);
    module.declare_function("js_event_emitter_set_max_listeners", I64, &[I64, DOUBLE]);
    module.declare_function("js_event_emitter_get_max_listeners", DOUBLE, &[I64]);
    module.declare_function("js_event_emitter_event_names", I64, &[I64]);
    module.declare_function("js_event_emitter_listeners", I64, &[I64, I64]);
    module.declare_function("js_event_emitter_raw_listeners", I64, &[I64, I64]);
    // Module-level helpers
    module.declare_function("js_events_once", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_events_on", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_events_add_abort_listener", I64, &[I64, I64]);
    module.declare_function("js_events_get_event_listeners", I64, &[I64, I64]);
    module.declare_function("js_events_listener_count", DOUBLE, &[I64, I64]);
    module.declare_function("js_events_get_max_listeners", DOUBLE, &[I64]);
    module.declare_function("js_events_set_max_listeners", DOUBLE, &[DOUBLE, I64]);

    // ========== StringDecoder (issue #848) ==========
    // `js_string_decoder_new` allocates a real handle; `write` / `end`
    // are reachable both through the static NATIVE_MODULE_TABLE dispatch
    // (typed-receiver path: `const d = new StringDecoder("utf8");
    // d.write(buf)`) AND through HANDLE_METHOD_DISPATCH in
    // perry-stdlib's common/dispatch.rs (any-typed receiver fallback —
    // `(d as any).write(buf)`, `Map.get(...).write(...)`). Both routes
    // converge on `dispatch_string_decoder` in the stdlib. Property
    // getters `lastNeed` / `lastTotal` / `lastChar` only go through
    // HANDLE_PROPERTY_DISPATCH and need no static-call entry.
    module.declare_function("js_string_decoder_new", I64, &[I64]);
    module.declare_function("js_string_decoder_write", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_string_decoder_end", DOUBLE, &[I64, DOUBLE]);

    // ========== node:querystring ==========
    // Module-level functions (no receiver). `escape` / `unescape` take
    // a single NaN-boxed string and return one. `parse` returns a raw
    // ObjectHeader pointer (NaN-boxed at the call site via the
    // dispatcher's NR_PTR shape). `stringify` returns a NaN-boxed
    // STRING_TAG value directly.
    module.declare_function("js_querystring_escape", DOUBLE, &[DOUBLE]);
    module.declare_function("js_querystring_unescape", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_querystring_parse",
        I64,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_querystring_stringify",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );

    // ========== Fastify ==========
    module.declare_function("js_fastify_add_hook", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_all", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_create", I64, &[]);
    module.declare_function("js_fastify_create_with_opts", I64, &[DOUBLE]);
    module.declare_function("js_fastify_ctx_html", DOUBLE, &[I64, I64, DOUBLE]);
    module.declare_function("js_fastify_ctx_json", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_fastify_ctx_redirect", DOUBLE, &[I64, I64, DOUBLE]);
    module.declare_function("js_fastify_ctx_text", DOUBLE, &[I64, I64, DOUBLE]);
    module.declare_function("js_fastify_delete", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_get", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_head", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_listen", VOID, &[I64, DOUBLE, I64]);
    // `app.close()` — shuts every server bound to this FastifyApp.
    // Declared so the dispatch-table arm in lower_call.rs can emit a
    // call site. Returns void (Rust signature returns bool, but the
    // codegen-side caller discards the result).
    module.declare_function("js_fastify_app_close", VOID, &[I64]);
    // #1113: `app.server` getter — returns the same FastifyApp handle
    // id (raw i64). The `NATIVE_MODULE_TABLE` arm at
    // `module: "fastify", method: "server"` declares the return as
    // NR_PTR so the codegen NaN-boxes it with POINTER_TAG before it
    // reaches the JS world, making `typeof app.server === "object"`
    // and routing `.on(…)` back into the FastifyApp method dispatch.
    module.declare_function("js_fastify_app_server", I64, &[I64]);
    // #1113: `app.server.on(event, cb)` — registers an event handler.
    // `event` arrives as a NaN-boxed string pointer (i64); `cb` as a
    // raw ClosureHeader pointer (i64). Returns void at the C ABI
    // (the FastifyApp dispatch wraps it to return the handle for
    // chaining, matching Node's `EventEmitter.on` contract).
    module.declare_function("js_fastify_app_on", VOID, &[I64, I64, I64]);
    module.declare_function("js_fastify_options", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_patch", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_post", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_put", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_register", I32, &[I64, I64, DOUBLE]);
    module.declare_function("js_fastify_reply_header", I64, &[I64, I64, I64]);
    module.declare_function("js_fastify_reply_send", I32, &[I64, DOUBLE]);
    module.declare_function("js_fastify_reply_status", I64, &[I64, DOUBLE]);
    module.declare_function("js_fastify_reply_type", I64, &[I64, I64]);
    module.declare_function("js_fastify_req_body", I64, &[I64]);
    module.declare_function("js_fastify_req_get_user_data", DOUBLE, &[I64]);
    module.declare_function("js_fastify_req_header", I64, &[I64, I64]);
    module.declare_function("js_fastify_req_headers", I64, &[I64]);
    module.declare_function("js_fastify_req_json", DOUBLE, &[I64]);
    module.declare_function("js_fastify_req_method", I64, &[I64]);
    module.declare_function("js_fastify_req_param", I64, &[I64, I64]);
    module.declare_function("js_fastify_req_params", I64, &[I64]);
    module.declare_function("js_fastify_req_query", I64, &[I64]);
    module.declare_function("js_fastify_req_query_object", DOUBLE, &[I64]);
    module.declare_function("js_fastify_req_set_user_data", VOID, &[I64, DOUBLE]);
    module.declare_function("js_fastify_req_url", I64, &[I64]);
    module.declare_function("js_fastify_route", I32, &[I64, I64, I64, I64]);
    module.declare_function("js_fastify_set_error_handler", I32, &[I64, I64]);

    // ========== Nodemailer ==========
    module.declare_function("js_nodemailer_create_transport", DOUBLE, &[I64]);
    module.declare_function("js_nodemailer_send_mail", I64, &[I64, I64]);
    module.declare_function("js_nodemailer_verify", I64, &[I64]);

    // ========== Rate limit ==========
    module.declare_function("js_ratelimit_block", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_ratelimit_consume", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_ratelimit_create", I64, &[I64]);
    module.declare_function("js_ratelimit_delete", I64, &[I64, I64]);
    module.declare_function("js_ratelimit_get", I64, &[I64, I64]);
    module.declare_function("js_ratelimit_penalty", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_ratelimit_reward", I64, &[I64, I64, DOUBLE]);

    // ========== Validator ==========
    module.declare_function("js_validator_contains", DOUBLE, &[I64, I64]);
    module.declare_function("js_validator_equals", DOUBLE, &[I64, I64]);
    module.declare_function("js_validator_is_alpha", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_alphanumeric", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_email", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_empty", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_float", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_hexadecimal", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_int", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_json", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_length", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_validator_is_lowercase", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_numeric", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_uppercase", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_url", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_uuid", DOUBLE, &[I64]);

    // ========== Date ==========
    module.declare_function("js_date_to_locale_string", I64, &[DOUBLE]);
    // #600: number-form `(n).toLocaleString()` — formats with
    // thousands separators (en-US default). Routed by the
    // `Expr::DateToLocaleString` LLVM arm when the receiver's static
    // type narrows to `HirType::Number` / `HirType::Int32`.
    module.declare_function("js_number_to_locale_string", I64, &[DOUBLE]);

    // ========== String ==========
    module.declare_function("js_string_split_regex", I64, &[I64, I64]);

    // ========== Object ==========
    module.declare_function("js_object_delete_dynamic", I32, &[I64, DOUBLE]);
    module.declare_function("js_object_get_prototype_of", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_set_prototype_of", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_object_define_properties", DOUBLE, &[DOUBLE, DOUBLE]);

    // ========== Math ==========
    module.declare_function("js_math_acos", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_asin", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_atan", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_atan2", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_math_cos", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_expm1", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_log", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_log10", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_log1p", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_log2", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_sin", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_tan", DOUBLE, &[DOUBLE]);

    // ========== Number ==========
    module.declare_function("js_number_is_finite", DOUBLE, &[DOUBLE]);

    // ========== JSON ==========
    module.declare_function("js_json_get_bool", DOUBLE, &[I64, I64]);
    module.declare_function("js_json_get_number", DOUBLE, &[I64, I64]);
    module.declare_function("js_json_get_string", I64, &[I64, I64]);
    module.declare_function("js_json_is_valid", DOUBLE, &[I64]);
    module.declare_function("js_json_stringify_bool", I64, &[DOUBLE]);
    module.declare_function("js_json_stringify_null", I64, &[]);
    module.declare_function("js_json_stringify_number", I64, &[DOUBLE]);
    module.declare_function("js_json_stringify_string", I64, &[I64]);

    // ========== Map / Set / WeakMap ==========
    module.declare_function("js_set_property", VOID, &[DOUBLE, I64, I64, DOUBLE]);

    // ========== Error ==========
    module.declare_function("js_error_get_message", I64, &[I64]);

    // ========== Promise ==========
    module.declare_function("js_await_js_promise", DOUBLE, &[DOUBLE]);

    // ========== Text encoding ==========
    module.declare_function("js_text_decoder_decode", I64, &[I64]);
    module.declare_function("js_text_encoder_encode", I64, &[DOUBLE]);

    // ========== Closures / functions ==========
    module.declare_function("js_call_function", DOUBLE, &[I64, I64, I64, I64, I64]);
    module.declare_function("js_call_method", DOUBLE, &[DOUBLE, I64, I64, I64, I64]);
    module.declare_function("js_call_value", DOUBLE, &[DOUBLE, I64, I64]);
    module.declare_function("js_closure_call_array", DOUBLE, &[I64, I64, I64]);
    module.declare_function(
        "js_closure_call_apply_with_spread",
        DOUBLE,
        &[DOUBLE, PTR, I64, I64],
    );
    module.declare_function("js_create_callback", DOUBLE, &[I64, I64, I64]);

    // ========== NaN-boxing / typeof / is_* ==========
    module.declare_function("js_dynamic_neg", DOUBLE, &[DOUBLE]);
    module.declare_function("js_dynamic_string_equals", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_is_nan", DOUBLE, &[DOUBLE]);
    module.declare_function("js_jsvalue_compare", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_jsvalue_equals", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_jsvalue_loose_equals", I32, &[DOUBLE, DOUBLE]);

    // ========== GC ==========
    module.declare_function("js_gc_collect", VOID, &[]);

    // ========== Console ==========
    module.declare_function("js_console_assert", VOID, &[DOUBLE, I64]);
    module.declare_function("js_console_assert_spread", VOID, &[DOUBLE, I64]);
    module.declare_function("js_console_group", VOID, &[I64]);

    // ========== Fetch ==========
    module.declare_function("js_fetch_get", I64, &[I64]);
    module.declare_function("js_fetch_get_with_auth", I64, &[I64, I64]);
    module.declare_function("js_fetch_post", I64, &[I64, I64, I64]);
    module.declare_function("js_fetch_post_with_auth", I64, &[I64, I64, I64]);
    module.declare_function("js_fetch_stream_close", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fetch_stream_poll", I64, &[DOUBLE]);
    module.declare_function("js_fetch_stream_start", DOUBLE, &[I64, I64, I64, I64]);
    module.declare_function("js_fetch_stream_status", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fetch_text", I64, &[I64]);
    module.declare_function("js_fetch_with_options", I64, &[I64, I64, I64, I64]);

    // ========== Net ==========
    module.declare_function("js_net_create_connection", DOUBLE, &[I32, I64, I64]);
    // Issue #1123 followup — switched from `DOUBLE` to `I64` return.
    // Previous shape returned `id as f64` which arrived in user code
    // as a bare number; the receiver-unboxing path on `server.listen`
    // masked the lower 48 bits of `1.0` and got 0, so the listen FFI
    // ran with `handle=0` and silently bailed. Now we return the raw
    // handle as i64 and let codegen NaN-box with POINTER_TAG in
    // `expr.rs::Expr::NetCreateServer`, matching the
    // `js_node_http_create_server` (`I64, &[I64]`) convention.
    module.declare_function("js_net_create_server", I64, &[I64, I64]);
    // Issue #1123 followup — `net.Server` instance method FFIs. The
    // NA_PTR slot for callbacks is `I64` here (closures arrive as raw
    // pointer-bits after the codegen's `unbox_to_i64` lowering); ports
    // are `DOUBLE` because the codegen passes NA_F64 args as JS
    // numbers without unboxing. address() returns a `*mut StringHeader`
    // — `I64` at the FFI level.
    module.declare_function("js_net_server_listen", VOID, &[I64, DOUBLE, I64]);
    module.declare_function("js_net_server_close", VOID, &[I64, I64]);
    module.declare_function("js_net_server_address", I64, &[I64]);
    module.declare_function("js_net_server_on", VOID, &[I64, I64, I64]);

    // ========== Performance ==========
    module.declare_function("js_performance_now", DOUBLE, &[]);
    // node:perf_hooks User Timing + ELU (perf_hooks.rs). All NaN-boxed f64.
    module.declare_function("js_perf_mark", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_perf_measure", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_perf_get_entries", DOUBLE, &[]);
    module.declare_function("js_perf_get_entries_by_type", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_get_entries_by_name", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_perf_clear_marks", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_clear_measures", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_event_loop_utilization", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_to_json", DOUBLE, &[]);
    module.declare_function("js_perf_clear_resource_timings", DOUBLE, &[]);
    module.declare_function("js_perf_set_resource_timing_buffer_size", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_observer_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_observer_observe", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_perf_observer_disconnect", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_observer_take_records", DOUBLE, &[DOUBLE]);
    // #1336: histogram stubs for perf_hooks.monitorEventLoopDelay() /
    // .createHistogram(). Histogram methods route via the perf_histogram
    // namespace through native_module_dispatch.
    module.declare_function("js_perf_monitor_event_loop_delay", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_create_histogram", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_histogram_noop", DOUBLE, &[]);
    module.declare_function("js_perf_histogram_percentile", DOUBLE, &[DOUBLE]);

    // ========== Async-step iter-result scratch (perf hot path) ==========
    // See promise.rs::ITER_RESULT_VALUE / ITER_RESULT_DONE — eliminates
    // the per-await {value, done} object alloc by stowing both fields
    // in a thread-local cell that the async-step driver consumes
    // immediately.
    module.declare_function("js_iter_result_set", DOUBLE, &[DOUBLE, I32]);
    module.declare_function("js_iter_result_get_value", DOUBLE, &[]);
    module.declare_function("js_iter_result_get_done", DOUBLE, &[]);
    // Optimized async-step chain: replaces
    // `Promise.resolve(value).then(then_v_arrow, then_e_arrow)` in
    // the async-step driver by carrying `step_closure` directly
    // through the task queue.
    module.declare_function("js_async_step_chain", I64, &[DOUBLE, I64]);
    // Optimized async-step done: replaces `Promise.resolve(value)` in
    // the state-machine terminal branch by reusing the in-flight `next`
    // Promise (INLINE_TRAP_NEXT) when called from inside the microtask
    // runner dispatching this same step closure.
    module.declare_function("js_async_step_done", I64, &[DOUBLE, I64]);
    // #691 Phase 2: returns the live step closure pointer from
    // INLINE_TRAP.current_step TLS. Codegen NaN-boxes the result.
    module.declare_function("js_get_current_step_closure", I64, &[]);
    // #691 Phase 2: wrap the wrapper's initial step invocation with
    // TLS setup so `js_get_current_step_closure` inside the body sees
    // the right pointer on the very first state. Saves/restores
    // INLINE_TRAP across the call for nested-async composition.
    module.declare_function("js_async_first_call", DOUBLE, &[DOUBLE]);

    // ========== Slugify ==========
    module.declare_function("js_slugify", I64, &[I64]);
    module.declare_function("js_slugify_strict", I64, &[I64]);

    // ========== Class registration ==========
    module.declare_function("js_register_class_getter", VOID, &[I64, I64, I64, I64]);
    // Refs #486: per-class setter dispatch — see object.rs::js_register_class_setter.
    module.declare_function("js_register_class_setter", VOID, &[I64, I64, I64, I64]);
    module.declare_function("js_register_class_method", VOID, &[I64, I64, I64, I64, I64]);
    // #1788: register a class STATIC method + dispatch an inherited static
    // method on a class value (subclass extends a class-expression value).
    module.declare_function(
        "js_register_class_static_method",
        VOID,
        &[I64, I64, I64, I64, I64, I64],
    );
    module.declare_function(
        "js_class_static_method_call",
        DOUBLE,
        &[DOUBLE, I64, I64, PTR, I64],
    );
    // #446: bound-method closure for `obj.method` PropertyGet on a known class.
    // Lets `typeof obj.method === "function"` and `let f = obj.method; f(args)`
    // dispatch through CLASS_VTABLE_REGISTRY instead of returning undefined.
    module.declare_function("js_class_method_bind", DOUBLE, &[DOUBLE, I64, I64]);
    module.declare_function("js_class_prototype_method_value", DOUBLE, &[DOUBLE, DOUBLE]);
    // #519: read the implicit `this` thread-local set by
    // `js_native_call_method`'s field-scan dispatch when invoking a
    // closure-typed class field method-style. `Expr::This` codegen reads
    // this when the lexical this_stack is empty.
    module.declare_function("js_implicit_this_get", DOUBLE, &[]);
    module.declare_function("js_implicit_this_set", DOUBLE, &[DOUBLE]);

    // ========== Runtime init / module loader ==========
    module.declare_function("js_get_export", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_get_property", DOUBLE, &[DOUBLE, I64, I64]);
    module.declare_function("js_load_module", I64, &[I64, I64]);
    module.declare_function(
        "js_native_call_method",
        DOUBLE,
        &[DOUBLE, I64, I64, I64, I64],
    );
    module.declare_function("js_native_call_value", DOUBLE, &[DOUBLE, I64, I64]);
    module.declare_function("js_new_from_handle", DOUBLE, &[DOUBLE, I64, I64]);
    module.declare_function("js_new_instance", DOUBLE, &[I64, I64, I64, I64, I64]);
    module.declare_function("js_runtime_init", VOID, &[]);

    // ========== Well-known Symbol conversion hooks ==========
    // Triggered by:
    //   - `js_object_set_symbol_method`: HIR IIFE wrapper for object-literal
    //     computed-key methods whose closure captures `this`
    //     (e.g. `{ [Symbol.toPrimitive](hint) { return this.value; } }`).
    //     Stores the closure AND patches its reserved `this` slot with obj.
    //   - `js_to_primitive`: consulted by `js_number_coerce` and
    //     `js_jsvalue_to_string` to route through a user-defined
    //     `[Symbol.toPrimitive]` method when the value is an object. Called
    //     indirectly from within the runtime; declared here so HIR
    //     `Call(ExternFuncRef("js_to_primitive"), ...)` can also call it.
    //   - `js_register_class_has_instance` / `js_register_class_to_string_tag`:
    //     called from `init_static_fields` for each class whose HIR lowering
    //     lifted a `static [Symbol.hasInstance]()` method or a
    //     `get [Symbol.toStringTag]()` getter to a top-level function with
    //     a `__perry_wk_<hook>_<class>` prefix. The runtime stores the
    //     function pointer against the class_id and consults it from
    //     `js_instanceof` / `js_object_to_string`.
    //   - `js_object_to_string`: implements `Object.prototype.toString.call(x)`
    //     by reading the class's registered `Symbol.toStringTag` getter.
    //     Called directly from HIR via `Call(ExternFuncRef, [obj])`.
    module.declare_function(
        "js_object_set_symbol_method",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    // #809: string-key analog of `js_object_set_symbol_method`. Used by the
    // ordered-IIFE lowering of object literals that mix a spread with
    // `this`-binding methods (Effect `HashRing.ts` `Proto`). Sets the field
    // by name AND patches the closure's reserved (last) `this` capture slot
    // with the object, so a method written after a `...spread` still sees
    // the right receiver.
    module.declare_function(
        "js_object_set_method_by_name",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_to_primitive", DOUBLE, &[DOUBLE, I32]);
    module.declare_function("js_register_class_has_instance", VOID, &[I32, I64]);
    module.declare_function("js_register_class_to_string_tag", VOID, &[I32, I64]);
    module.declare_function("js_object_to_string", DOUBLE, &[DOUBLE]);

    // ---- Object.groupBy (Node 22+) ----
    // Triggered by HIR variant `Expr::ObjectGroupBy { items, key_fn }`
    // (perry-hir/src/lower.rs catches the AST `Object.groupBy(items, fn)`
    // call site). The runtime implementation walks `items`, invokes
    // `key_fn(item, index)` per element, and materializes a result
    // object grouping items by their string key. See
    // `crates/perry-runtime/src/object.rs::js_object_group_by`.
    //
    // `Array.fromAsync(input)` — Node 22+. Dispatched at the LLVM
    // codegen level in `lower_call.rs` when the receiver is a global
    // and the property is `fromAsync`. The runtime function returns a
    // NaN-boxed Promise pointer; for arrays it forwards to
    // `js_promise_all`, for async iterators it chains `.next()` calls
    // through `array_from_async_step`.
    module.declare_function("js_object_group_by", DOUBLE, &[DOUBLE, I64]);
    module.declare_function("js_array_from_async", DOUBLE, &[DOUBLE]);

    // ========== JSX runtime stubs (issue #277) ==========
    // `js_jsx(type, props)` and `js_jsxs(type, props)` are no-op stubs that
    // let TSX/JSX files compile and link without a real JSX runtime package.
    // The codegen intercepts ExternFuncRef { name: "jsx" } / "jsxs" in
    // `lower_call.rs` and routes them here with both args as DOUBLE
    // (NaN-boxed), bypassing the string→PTR conversion the generic path
    // would apply to string literals.  When a real JSX runtime is imported
    // via `perry.compilePackages` the imported symbol takes precedence and
    // these stubs are never called.
    module.declare_function("js_jsx", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_jsxs", DOUBLE, &[DOUBLE, DOUBLE]);
}
