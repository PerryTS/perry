//! The wrapper archives' symbols this crate's `external-*` adapters call.
//!
//! The adapters for perry-ext-net / perry-ext-http (routed `net`, `tls`,
//! `http`, `https`, `http2`) are compiled into every archive and installed
//! only for a program that links the wrapper (`feature_hooks`), so outside
//! such a program nothing reachable references these symbols. ELF and
//! Mach-O linkers report only references from code they keep; MSVC resolves
//! every reference of every object it loads. For MSVC each symbol therefore
//! has a weak default here, which the wrapper's definition replaces when it
//! is linked; the default is never reached, because only an installed
//! adapter calls these, and it aborts if it is.
//!
//! Every adapter names these symbols through this module, so each has one
//! declaration.

#[cfg(not(target_env = "msvc"))]
macro_rules! wrapper_symbols {
    ($(fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)?;)*) => {
        #[allow(dead_code, improper_ctypes, clippy::too_many_arguments)]
        unsafe extern "C" {
            $(pub(crate) fn $name($($arg: $ty),*) $(-> $ret)?;)*
        }
    };
}

#[cfg(target_env = "msvc")]
macro_rules! wrapper_symbols {
    ($(fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)?;)*) => {
        $(
            #[allow(dead_code, unused_variables, clippy::too_many_arguments)]
            #[no_mangle]
            #[linkage = "weak"]
            pub(crate) unsafe extern "C" fn $name($($arg: $ty),*) $(-> $ret)? {
                wrapper_not_linked(stringify!($name))
            }
        )*
    };
}

#[cfg(target_env = "msvc")]
#[cold]
fn wrapper_not_linked(symbol: &str) -> ! {
    eprintln!("perry: internal error: {symbol} was called but its wrapper archive is not linked");
    std::process::abort()
}

wrapper_symbols! {
    fn js_ext_http2_session_dispatch_method(handle: i64, method_ptr: *const u8, method_len: usize, args_ptr: *const f64, args_len: usize) -> f64;
    fn js_ext_http2_session_dispatch_property(handle: i64, property_ptr: *const u8, property_len: usize) -> f64;
    fn js_ext_http2_session_is_handle(handle: i64) -> i32;
    fn js_ext_http2_stream_dispatch_method(handle: i64, method_ptr: *const u8, method_len: usize, args_ptr: *const f64, args_len: usize) -> f64;
    fn js_ext_http2_stream_dispatch_property(handle: i64, property_ptr: *const u8, property_len: usize) -> f64;
    fn js_ext_http2_stream_is_handle(handle: i64) -> i32;
    fn js_ext_http_agent_dispatch_method(handle: i64, method_ptr: *const u8, method_len: usize, args_ptr: *const f64, args_len: usize) -> f64;
    fn js_ext_http_agent_dispatch_property(handle: i64, property_ptr: *const u8, property_len: usize) -> f64;
    fn js_ext_http_agent_dispatch_property_set(handle: i64, property_ptr: *const u8, property_len: usize, value: f64) -> i32;
    fn js_ext_http_agent_is_handle(handle: i64) -> i32;
    fn js_ext_http_client_incoming_message_is_handle(handle: i64) -> i32;
    fn js_ext_http_client_request_dispatch_method(handle: i64, method_ptr: *const u8, method_len: usize, args_ptr: *const f64, args_len: usize) -> f64;
    fn js_ext_http_client_request_dispatch_property(handle: i64, property_ptr: *const u8, property_len: usize) -> f64;
    fn js_ext_http_client_request_is_handle(handle: i64) -> i32;
    fn js_ext_http_incoming_message_dispatch_method(handle: i64, method_ptr: *const u8, method_len: usize, args_ptr: *const f64, args_len: usize) -> f64;
    fn js_ext_http_incoming_message_dispatch_property(handle: i64, property_ptr: *const u8, property_len: usize) -> f64;
    fn js_ext_http_incoming_message_dispatch_property_set(handle: i64, property_ptr: *const u8, property_len: usize, value: f64) -> i32;
    fn js_ext_http_incoming_message_is_handle(handle: i64) -> i32;
    fn js_ext_http_native_dispatch(module_ptr: *const u8, module_len: usize, method_ptr: *const u8, method_len: usize, args_ptr: *const f64, args_len: usize) -> f64;
    fn js_ext_http_server_dispatch_method(handle: i64, method_ptr: *const u8, method_len: usize, args_ptr: *const f64, args_len: usize) -> f64;
    fn js_ext_http_server_dispatch_property(handle: i64, property_ptr: *const u8, property_len: usize) -> f64;
    fn js_ext_http_server_is_handle(handle: i64) -> i32;
    fn js_ext_http_server_response_dispatch_method(handle: i64, method_ptr: *const u8, method_len: usize, args_ptr: *const f64, args_len: usize) -> f64;
    fn js_ext_http_server_response_dispatch_property(handle: i64, property_ptr: *const u8, property_len: usize) -> f64;
    fn js_ext_http_server_response_dispatch_property_set(handle: i64, property_ptr: *const u8, property_len: usize, value: f64) -> i32;
    fn js_ext_http_server_response_is_handle(handle: i64) -> i32;
    fn js_ext_net_destroy_socket(handle: i64);
    fn js_ext_net_is_block_list_handle(handle: i64) -> i32;
    fn js_ext_net_is_server_handle(handle: i64) -> i32;
    fn js_ext_net_is_socket_address_handle(handle: i64) -> i32;
    fn js_ext_net_is_socket_handle(handle: i64) -> i32;
    fn js_ext_net_socket_emit(handle: i64, event_ptr: i64, args_ptr: *const f64, args_len: usize) -> f64;
    fn js_ext_net_socket_end(handle: i64, chunk_bits: i64);
    fn js_ext_net_socket_get_max_listeners(handle: i64) -> f64;
    fn js_ext_net_socket_listener_count(handle: i64, event_ptr: i64) -> f64;
    fn js_ext_net_socket_listeners(handle: i64, event_ptr: i64) -> i64;
    fn js_ext_net_socket_method_connect(handle: i64, arg1: f64, arg2: f64, arg3: f64);
    fn js_ext_net_socket_on(handle: i64, event_ptr: i64, cb_ptr: i64) -> i64;
    fn js_ext_net_socket_once(handle: i64, event_ptr: i64, cb_ptr: i64) -> i64;
    fn js_ext_net_socket_read(handle: i64, size: f64) -> f64;
    fn js_ext_net_socket_remove_all_listeners(handle: i64, event_ptr: i64) -> i64;
    fn js_ext_net_socket_remove_listener(handle: i64, event_ptr: i64, cb_ptr: i64) -> i64;
    fn js_ext_net_socket_set_max_listeners(handle: i64, n: f64) -> f64;
    fn js_ext_net_socket_write(handle: i64, buf_ptr: i64) -> f64;
    fn js_http_incoming_message_pipe(handle: i64, dest: f64) -> f64;
    fn js_http_incoming_message_req(handle: i64) -> f64;
    fn js_http_incoming_message_set_encoding(handle: i64, encoding_ptr: *const perry_runtime::StringHeader) -> i64;
    fn js_http_incoming_message_socket(handle: i64) -> f64;
    fn js_http_is_incoming_message(handle: i64) -> i32;
    fn js_http_on(handle: i64, event_ptr: *const perry_runtime::StringHeader, callback: i64) -> i64;
    fn js_http_once(handle: i64, event_ptr: *const perry_runtime::StringHeader, callback: i64) -> i64;
    fn js_http_response_complete(handle: i64) -> f64;
    fn js_http_response_headers(handle: i64) -> f64;
    fn js_http_response_http_version(handle: i64) -> *mut perry_runtime::StringHeader;
    fn js_http_response_http_version_major(handle: i64) -> f64;
    fn js_http_response_http_version_minor(handle: i64) -> f64;
    fn js_http_response_raw_headers(handle: i64) -> f64;
    fn js_http_response_trailers(handle: i64) -> f64;
    fn js_http_status_code(handle: i64) -> f64;
    fn js_http_status_message(handle: i64) -> *mut perry_runtime::StringHeader;
    fn js_net_block_list_add_address(handle: i64, address_ptr: i64, family_ptr: i64) -> f64;
    fn js_net_block_list_add_range(handle: i64, start_ptr: i64, end_ptr: i64, family_ptr: i64) -> f64;
    fn js_net_block_list_add_subnet(handle: i64, address_ptr: i64, prefix: f64, family_ptr: i64) -> f64;
    fn js_net_block_list_check(handle: i64, address_ptr: i64, family_ptr: i64) -> f64;
    fn js_net_block_list_from_json(handle: i64, value: f64) -> f64;
    fn js_net_block_list_rules(handle: i64) -> *mut perry_runtime::ArrayHeader;
    fn js_net_block_list_to_json(handle: i64) -> f64;
    fn js_net_server_address(handle: i64) -> *mut perry_runtime::StringHeader;
    fn js_net_server_close(handle: i64, callback_i64: i64);
    fn js_net_server_event_names(handle: i64) -> *mut perry_runtime::StringHeader;
    fn js_net_server_get_connections(handle: i64) -> f64;
    fn js_net_server_get_drop_max_connection(handle: i64) -> f64;
    fn js_net_server_get_listening(handle: i64) -> f64;
    fn js_net_server_get_max_connections(handle: i64) -> f64;
    fn js_net_server_listen(handle: i64, port: f64, arg2: f64, arg3: f64);
    fn js_net_server_listener_count(handle: i64, event_ptr: i64) -> f64;
    fn js_net_server_listeners(handle: i64, event_ptr: i64) -> i64;
    fn js_net_server_on(handle: i64, event_ptr: i64, cb: i64);
    fn js_net_server_once(handle: i64, event_ptr: i64, cb: i64) -> i64;
    fn js_net_server_raw_listeners(handle: i64, event_ptr: i64) -> i64;
    fn js_net_server_remove_all_listeners(handle: i64, event_ptr: i64) -> i64;
    fn js_net_server_remove_listener(handle: i64, event_ptr: i64, cb: i64) -> i64;
    fn js_net_server_set_drop_max_connection(handle: i64, value: f64) -> f64;
    fn js_net_server_set_max_connections(handle: i64, value: f64) -> f64;
    fn js_net_socket_address(handle: i64) -> *mut perry_runtime::StringHeader;
    fn js_net_socket_address_get_address(handle: i64) -> *mut perry_runtime::StringHeader;
    fn js_net_socket_address_get_family(handle: i64) -> *mut perry_runtime::StringHeader;
    fn js_net_socket_address_get_flowlabel(handle: i64) -> f64;
    fn js_net_socket_address_get_port(handle: i64) -> f64;
    fn js_net_socket_destroy(handle: i64);
    fn js_net_socket_end(handle: i64, chunk_bits: i64);
    fn js_net_socket_event_names(handle: i64) -> *mut perry_runtime::StringHeader;
    fn js_net_socket_get_type_of_service(handle: i64) -> f64;
    fn js_net_socket_listener_count(handle: i64, event_ptr: i64) -> f64;
    fn js_net_socket_on(handle: i64, event_ptr: i64, cb: i64);
    fn js_net_socket_once(handle: i64, event_ptr: i64, cb: i64) -> i64;
    fn js_net_socket_raw_listeners(handle: i64, event_ptr: i64) -> i64;
    fn js_net_socket_remove_all_listeners(handle: i64, event_ptr: i64) -> i64;
    fn js_net_socket_remove_listener(handle: i64, event_ptr: i64, cb: i64) -> i64;
    fn js_net_socket_reset_and_destroy(handle: i64) -> i64;
    fn js_net_socket_set_type_of_service(handle: i64, value: f64) -> i64;
    fn js_net_socket_upgrade_tls(handle: i64, servername_ptr: i64, verify: f64) -> *mut perry_runtime::Promise;
    fn js_net_socket_write(handle: i64, chunk_bits: i64);
    fn js_tls_connect(arg1: f64, arg2: f64, arg3: f64, arg4: f64) -> i64;
}
