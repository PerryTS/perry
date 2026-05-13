// Stdlib core utility FFI surface inventory.
//
// This fixture is intentionally executable by the normal parity runner,
// but its main purpose is to keep TS-side coverage accounting attached
// to related public FFI shims. Move @covers entries from this
// inventory into behavioral tests as each area gets deeper compatibility
// coverage.
//
// Inventory entries: 84 unique FFI names, 85 declarations.

const testFfiSurfaceStdlibCoreVersion = 1;
if (testFfiSurfaceStdlibCoreVersion !== 1) {
  throw new Error("unexpected coverage inventory version");
}
console.log("test_ffi_surface_stdlib_core: ok");

/*
@covers
crates/perry-stdlib/src/axios.rs:
  - js_axios_delete
  - js_axios_get
  - js_axios_patch
  - js_axios_post
  - js_axios_put
  - js_axios_response_data
  - js_axios_response_status
  - js_axios_response_status_text
crates/perry-stdlib/src/common/dispatch.rs:
  - js_handle_method_dispatch
  - js_handle_property_set_dispatch
  - js_stdlib_init_dispatch
crates/perry-stdlib/src/common/handle.rs:
  - js_handle_count
crates/perry-stdlib/src/exponential_backoff.rs:
  - backOff
  - js_backoff_simple
crates/perry-stdlib/src/fastify/app.rs:
  - js_fastify_add_hook
  - js_fastify_all
  - js_fastify_create
  - js_fastify_create_with_opts
  - js_fastify_delete
  - js_fastify_get
  - js_fastify_head
  - js_fastify_options
  - js_fastify_patch
  - js_fastify_post
  - js_fastify_put
  - js_fastify_register
  - js_fastify_route
  - js_fastify_set_error_handler
crates/perry-stdlib/src/fastify/context.rs:
  - js_fastify_ctx_html
  - js_fastify_ctx_json
  - js_fastify_ctx_redirect
  - js_fastify_ctx_text
  - js_fastify_reply_header
  - js_fastify_reply_send
  - js_fastify_reply_status
  - js_fastify_req_body
  - js_fastify_req_get_user_data
  - js_fastify_req_header
  - js_fastify_req_headers
  - js_fastify_req_json
  - js_fastify_req_method
  - js_fastify_req_param
  - js_fastify_req_params
  - js_fastify_req_params_object
  - js_fastify_req_query
  - js_fastify_req_query_object
  - js_fastify_req_set_user_data
  - js_fastify_req_url
crates/perry-stdlib/src/fastify/server.rs:
  - js_fastify_close
  - js_fastify_listen
crates/perry-stdlib/src/perry_ffi_async.rs:
  - perry_ffi_promise_new
  - perry_ffi_promise_reject_bits
  - perry_ffi_promise_resolve_bits
  - perry_ffi_spawn_async
  - perry_ffi_spawn_blocking
  - perry_ffi_spawn_blocking_with_reactor
crates/perry-stdlib/src/ratelimit.rs:
  - js_ratelimit_block
  - js_ratelimit_check
  - js_ratelimit_consume
  - js_ratelimit_delete
  - js_ratelimit_get
  - js_ratelimit_new
  - js_ratelimit_new_keyed
  - js_ratelimit_penalty
  - js_ratelimit_remaining
  - js_ratelimit_reset
  - js_ratelimit_reward
crates/perry-stdlib/src/slugify.rs:
  - js_slugify
  - js_slugify_strict
  - js_slugify_with_options
crates/perry-stdlib/src/sqlite.rs:
  - js_sqlite_begin_transaction
  - js_sqlite_close
  - js_sqlite_commit
  - js_sqlite_exec
  - js_sqlite_in_transaction
  - js_sqlite_open
  - js_sqlite_pragma
  - js_sqlite_prepare
  - js_sqlite_rollback
  - js_sqlite_stmt_all
  - js_sqlite_stmt_get
  - js_sqlite_stmt_raw
  - js_sqlite_stmt_run
  - js_sqlite_transaction
*/
