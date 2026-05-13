// Stdlib IO, network, stream, and framework FFI surface inventory.
//
// This fixture is intentionally executable by the normal parity runner,
// but its main purpose is to keep TS-side coverage accounting attached
// to related public FFI shims. Move @covers entries from this
// inventory into behavioral tests as each area gets deeper compatibility
// coverage.
//
// Inventory entries: 155 unique FFI names, 173 declarations.

const testFfiSurfaceStdlibIoVersion = 1;
if (testFfiSurfaceStdlibIoVersion !== 1) {
  throw new Error("unexpected coverage inventory version");
}
console.log("test_ffi_surface_stdlib_io: ok");

/*
@covers
crates/perry-stdlib/src/fetch.rs:
  - js_blob_array_buffer
  - js_blob_bytes
  - js_blob_size
  - js_blob_slice
  - js_blob_stream
  - js_blob_text
  - js_blob_type
  - js_fetch_get
  - js_fetch_get_with_auth
  - js_fetch_post
  - js_fetch_post_with_auth
  - js_fetch_response_count
  - js_fetch_response_json
  - js_fetch_response_ok
  - js_fetch_response_status
  - js_fetch_response_status_text
  - js_fetch_response_text
  - js_fetch_stream_close
  - js_fetch_stream_poll
  - js_fetch_stream_start
  - js_fetch_stream_status
  - js_fetch_text
  - js_fetch_with_options
  - js_headers_delete
  - js_headers_entries
  - js_headers_for_each
  - js_headers_get
  - js_headers_has
  - js_headers_keys
  - js_headers_new
  - js_headers_set
  - js_headers_values
  - js_request_get_body
  - js_request_get_method
  - js_request_get_url
  - js_request_new
  - js_response_array_buffer
  - js_response_blob
  - js_response_body
  - js_response_clone
  - js_response_get_headers
  - js_response_new
  - js_response_static_json
  - js_response_static_redirect
crates/perry-stdlib/src/framework/multipart.rs:
  - js_multipart_parse
  - js_multipart_parse_with_sizes
crates/perry-stdlib/src/framework/request.rs:
  - js_http_request_body_length
  - js_http_request_content_type
  - js_http_request_has_header
  - js_http_request_headers_all
  - js_http_request_id
  - js_http_request_is_method
  - js_http_request_query_all
  - js_http_request_query_param
crates/perry-stdlib/src/framework/response.rs:
  - js_http_respond_error
  - js_http_respond_html
  - js_http_respond_json
  - js_http_respond_not_found
  - js_http_respond_redirect
  - js_http_respond_status_text
  - js_http_respond_text
  - js_http_respond_with_headers
crates/perry-stdlib/src/framework/server.rs:
  - js_http_request_body
  - js_http_request_header
  - js_http_request_method
  - js_http_request_path
  - js_http_request_query
  - js_http_respond
  - js_http_server_accept
  - js_http_server_accept_v2
  - js_http_server_close
  - js_http_server_create
crates/perry-stdlib/src/http.rs:
  - js_http_client_request_end
  - js_http_client_request_write
  - js_http_get
  - js_http_on
  - js_http_process_pending
  - js_http_request
  - js_http_response_headers
  - js_http_set_header
  - js_http_set_timeout
  - js_http_status_code
  - js_http_status_message
  - js_https_get
  - js_https_request
crates/perry-stdlib/src/net/mod.rs:
  - js_net_process_pending
  - js_net_socket_alloc
  - js_net_socket_connect
  - js_net_socket_destroy
  - js_net_socket_end
  - js_net_socket_method_connect
  - js_net_socket_on
  - js_net_socket_upgrade_tls
  - js_net_socket_write
  - js_tls_connect
crates/perry-stdlib/src/readline.rs:
  - js_readline_close
  - js_readline_create_interface
  - js_readline_has_active
  - js_readline_on
  - js_readline_question
crates/perry-stdlib/src/streams.rs:
  - js_readable_stream_cancel
  - js_readable_stream_controller_close
  - js_readable_stream_controller_desired_size
  - js_readable_stream_controller_enqueue
  - js_readable_stream_controller_error
  - js_readable_stream_from_blob
  - js_readable_stream_from_iterable
  - js_readable_stream_from_response
  - js_readable_stream_get_reader
  - js_readable_stream_locked
  - js_readable_stream_pipe_through
  - js_readable_stream_pipe_to
  - js_readable_stream_subclass_init
  - js_readable_stream_tee
  - js_reader_cancel
  - js_reader_closed
  - js_reader_read
  - js_reader_release_lock
  - js_stream_unwrap_handle
  - js_streams_throw_byob_not_implemented
  - js_streams_throw_byte_length_not_implemented
  - js_transform_stream_new
  - js_transform_stream_readable
  - js_transform_stream_subclass_init
  - js_transform_stream_writable
  - js_writable_stream_abort
  - js_writable_stream_close
  - js_writable_stream_get_writer
  - js_writable_stream_locked
  - js_writable_stream_new
  - js_writable_stream_subclass_init
  - js_writer_abort
  - js_writer_close
  - js_writer_closed
  - js_writer_desired_size
  - js_writer_ready
  - js_writer_release_lock
  - js_writer_write
crates/perry-stdlib/src/worker_threads.rs:
  - js_worker_threads_get_worker_data
  - js_worker_threads_has_pending
  - js_worker_threads_on
  - js_worker_threads_parent_port
  - js_worker_threads_post_message
  - js_worker_threads_process_pending
crates/perry-stdlib/src/ws.rs:
  - js_ws_connect
  - js_ws_connect_start
  - js_ws_handle_to_i64
  - js_ws_is_open
  - js_ws_message_count
  - js_ws_process_pending
  - js_ws_receive
  - js_ws_send_to_client
  - js_ws_server_close
  - js_ws_server_new
  - js_ws_wait_for_message
*/
