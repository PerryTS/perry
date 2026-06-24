//! Per-module native-module dispatch buckets, relocated from
//! `native_module_dispatch.rs` to keep each file under the size budget
//! (issue #1103 split). Pure relocation — no logic change. The
//! `nm_general_closures!` macro is supplied by the parent module.
use super::*;

#[allow(
    unused_variables,
    unused_mut,
    unused_unsafe,
    clippy::let_and_return,
    clippy::all
)]
pub(crate) unsafe fn nm_dispatch_dgram(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
    let NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    } = *ctx;
    let _ = (obj, args_ptr, args_len, assert_skip_prototype);
    nm_general_closures!(
        obj,
        args_ptr,
        args_len,
        arg,
        i32_arg,
        bool_to_f64,
        str_to_f64,
        pack_args,
        pack_args_from,
        bool_tag,
        ptr_addr,
        optional_ptr_addr,
        _arg_event_ptr,
        arg_bits,
        _arg_closure_ptr,
        ptr_to_f64,
        typed_kind
    );
    match (module_name, method_name) {
        #[cfg(feature = "mod-dgram")]
        ("dgram", "createSocket") | ("dgram", "Socket") => {
            crate::dgram::js_dgram_create_socket(pack_args())
        }

        // ── console module namespace (`node:console` / `console`) ──
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}

#[allow(
    unused_variables,
    unused_mut,
    unused_unsafe,
    clippy::let_and_return,
    clippy::all
)]
pub(crate) unsafe fn nm_dispatch_dns(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
    let NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    } = *ctx;
    let _ = (obj, args_ptr, args_len, assert_skip_prototype);
    nm_general_closures!(
        obj,
        args_ptr,
        args_len,
        arg,
        i32_arg,
        bool_to_f64,
        str_to_f64,
        pack_args,
        pack_args_from,
        bool_tag,
        ptr_addr,
        optional_ptr_addr,
        _arg_event_ptr,
        arg_bits,
        _arg_closure_ptr,
        ptr_to_f64,
        typed_kind
    );
    match (module_name, method_name) {
        ("dns", "getServers") => crate::dns::dns_get_servers_value(),
        ("dns", "setServers") => crate::dns::dns_set_servers_value(arg(0)),
        ("dns/promises", "getServers") => crate::dns::dns_promises_get_servers_value(),
        ("dns/promises", "setServers") => crate::dns::dns_promises_set_servers_value(arg(0)),
        ("dns" | "dns/promises", "getDefaultResultOrder") => {
            crate::dns::dns_get_default_result_order_value()
        }
        ("dns" | "dns/promises", "setDefaultResultOrder") => {
            crate::dns::dns_set_default_result_order_value(arg(0))
        }

        // #2130: captured-then-called child_process methods (`const spawn =
        // require('child_process').spawn; spawn(...)`, Node's canonical test
        // idiom). The bound-method closure produced by `cp.spawn` (and the
        // other entries allowlisted in `is_native_module_callable_export`)
        // funnels back here when invoked. The method-call form
        // (`cp.spawn(...)`) is lowered to the same FFIs through dedicated
        // codegen arms (`expr/child_proc.rs`); this arm mirrors them for the
        // value-call form. `cmd` / `file` / `module` strings come in NaN-boxed
        // (SSO-safe via `js_string_materialize_to_heap`); `args` is the array
        // pointer (or null); `opts` is the options-object pointer (or 0).
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}

#[allow(
    unused_variables,
    unused_mut,
    unused_unsafe,
    clippy::let_and_return,
    clippy::all
)]
pub(crate) unsafe fn nm_dispatch_domain(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
    let NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    } = *ctx;
    let _ = (obj, args_ptr, args_len, assert_skip_prototype);
    nm_general_closures!(
        obj,
        args_ptr,
        args_len,
        arg,
        i32_arg,
        bool_to_f64,
        str_to_f64,
        pack_args,
        pack_args_from,
        bool_tag,
        ptr_addr,
        optional_ptr_addr,
        _arg_event_ptr,
        arg_bits,
        _arg_closure_ptr,
        ptr_to_f64,
        typed_kind
    );
    match (module_name, method_name) {
        ("domain", "Domain" | "createDomain" | "create") => {
            let ptr =
                crate::value::JS_NATIVE_DOMAIN_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                dispatch(method_name.as_ptr(), method_name.len(), args_ptr, args_len)
            }
        }
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}

#[allow(
    unused_variables,
    unused_mut,
    unused_unsafe,
    clippy::let_and_return,
    clippy::all
)]
pub(crate) unsafe fn nm_dispatch_events(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
    let NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    } = *ctx;
    let _ = (obj, args_ptr, args_len, assert_skip_prototype);
    nm_general_closures!(
        obj,
        args_ptr,
        args_len,
        arg,
        i32_arg,
        bool_to_f64,
        str_to_f64,
        pack_args,
        pack_args_from,
        bool_tag,
        ptr_addr,
        optional_ptr_addr,
        _arg_event_ptr,
        arg_bits,
        _arg_closure_ptr,
        ptr_to_f64,
        typed_kind
    );
    match (module_name, method_name) {
        ("events", "init") => f64::from_bits(crate::value::TAG_UNDEFINED),
        ("events", "EventEmitterAsyncResource") => {
            let message =
                b"Class constructor EventEmitterAsyncResource cannot be invoked without 'new'";
            let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
            let err = crate::error::js_typeerror_new(msg);
            crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
        }
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}

#[allow(
    unused_variables,
    unused_mut,
    unused_unsafe,
    clippy::let_and_return,
    clippy::all
)]
pub(crate) unsafe fn nm_dispatch_fs(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
    let NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    } = *ctx;
    let _ = (obj, args_ptr, args_len, assert_skip_prototype);
    nm_general_closures!(
        obj,
        args_ptr,
        args_len,
        arg,
        i32_arg,
        bool_to_f64,
        str_to_f64,
        pack_args,
        pack_args_from,
        bool_tag,
        ptr_addr,
        optional_ptr_addr,
        _arg_event_ptr,
        arg_bits,
        _arg_closure_ptr,
        ptr_to_f64,
        typed_kind
    );
    match (module_name, method_name) {
        ("fs", "_toUnixTimestamp") => crate::fs::js_fs_to_unix_timestamp(arg(0)),
        ("fs", "existsSync") => bool_to_f64(crate::fs::js_fs_exists_sync(arg(0))),
        ("fs", "readFileSync") => crate::fs::js_fs_read_file_dispatch(arg(0), arg(1)),
        ("fs", "writeFileSync") => bool_to_f64(crate::fs::js_fs_write_file_sync_options(
            arg(0),
            arg(1),
            arg(2),
        )),
        ("fs", "appendFileSync") => bool_to_f64(crate::fs::js_fs_append_file_sync_options(
            arg(0),
            arg(1),
            arg(2),
        )),
        ("fs", "mkdirSync") => bool_to_f64(crate::fs::js_fs_mkdir_sync_options(arg(0), arg(1))),
        ("fs", "unlinkSync") => bool_to_f64(crate::fs::js_fs_unlink_sync(arg(0))),
        ("fs", "rmSync") => bool_to_f64(crate::fs::js_fs_rm_recursive_options(arg(0), arg(1))),
        ("fs", "rmdirSync") => bool_to_f64(crate::fs::js_fs_rmdir_sync_options(arg(0), arg(1))),
        ("fs", "readdirSync") => {
            let raw = crate::fs::js_fs_readdir_sync(arg(0), arg(1));
            f64::from_bits(JSValue::pointer(raw.to_bits() as *const u8).bits())
        }
        ("fs", "statSync") => crate::fs::js_fs_stat_sync_options(arg(0), arg(1)),
        ("fs", "lstatSync") => crate::fs::js_fs_lstat_sync_options(arg(0), arg(1)),
        ("fs", "renameSync") => bool_to_f64(crate::fs::js_fs_rename_sync(arg(0), arg(1))),
        ("fs", "copyFileSync") => bool_to_f64(crate::fs::js_fs_copy_file_sync_flags(
            arg(0),
            arg(1),
            arg(2),
        )),
        ("fs", "cpSync") => bool_to_f64(crate::fs::js_fs_cp_sync_options(arg(0), arg(1), arg(2))),
        ("fs", "accessSync") => crate::fs::js_fs_access_sync_throw_mode(arg(0), arg(1)),
        ("fs", "realpathSync") => crate::fs::js_fs_realpath_dispatch(arg(0), arg(1)),
        ("fs", "mkdtempSync") => crate::fs::js_fs_mkdtemp_dispatch(arg(0), arg(1)),
        ("fs", "mkdtempDisposableSync") => crate::fs::js_fs_mkdtemp_disposable_sync(arg(0), arg(1)),
        ("fs", "chmodSync") => bool_to_f64(crate::fs::js_fs_chmod_sync(arg(0), arg(1))),
        ("fs", "chownSync") => bool_to_f64(crate::fs::js_fs_chown_sync(arg(0), arg(1), arg(2))),
        ("fs", "lchownSync") => bool_to_f64(crate::fs::js_fs_lchown_sync(arg(0), arg(1), arg(2))),
        ("fs", "lchmodSync") => bool_to_f64(crate::fs::js_fs_lchmod_sync(arg(0), arg(1))),
        ("fs", "truncateSync") => bool_to_f64(crate::fs::js_fs_truncate_sync(arg(0), arg(1))),
        ("fs", "ftruncateSync") => bool_to_f64(crate::fs::js_fs_ftruncate_sync(arg(0), arg(1))),
        ("fs", "fsyncSync") => bool_to_f64(crate::fs::js_fs_fsync_sync(arg(0))),
        ("fs", "fdatasyncSync") => bool_to_f64(crate::fs::js_fs_fdatasync_sync(arg(0))),
        ("fs", "fchmodSync") => bool_to_f64(crate::fs::js_fs_fchmod_sync(arg(0), arg(1))),
        ("fs", "fchownSync") => bool_to_f64(crate::fs::js_fs_fchown_sync(arg(0), arg(1), arg(2))),
        ("fs", "fstatSync") => crate::fs::js_fs_fstat_sync_options(arg(0), arg(1)),
        ("fs", "utimesSync") => crate::fs::js_fs_utimes_sync(arg(0), arg(1), arg(2)) as f64,
        ("fs", "lutimesSync") => crate::fs::js_fs_lutimes_sync(arg(0), arg(1), arg(2)) as f64,
        ("fs", "futimesSync") => crate::fs::js_fs_futimes_sync(arg(0), arg(1), arg(2)) as f64,
        ("fs", "_toUnixTimestamp") => crate::fs::js_fs_to_unix_timestamp(arg(0)),
        ("fs", "readvSync") => crate::fs::js_fs_readv_sync(arg(0), arg(1), arg(2)),
        ("fs", "writevSync") => crate::fs::js_fs_writev_sync(arg(0), arg(1), arg(2)),
        ("fs", "statfsSync") => crate::fs::js_fs_statfs_sync_options(arg(0), arg(1)),
        ("fs", "opendirSync") => crate::fs::js_fs_opendir_sync(arg(0)),
        ("fs", "globSync") => {
            let raw = crate::fs::js_fs_glob_sync_options(arg(0), arg(1));
            f64::from_bits(JSValue::pointer(raw.to_bits() as *const u8).bits())
        }
        ("fs", "watch") => crate::fs::js_fs_watch(arg(0), arg(1), arg(2)),
        ("fs", "watchFile") => crate::fs::js_fs_watch_file(arg(0), arg(1), arg(2)),
        ("fs", "unwatchFile") => crate::fs::js_fs_unwatch_file(arg(0), arg(1)),
        ("fs", "linkSync") => bool_to_f64(crate::fs::js_fs_link_sync(arg(0), arg(1))),
        ("fs", "symlinkSync") => bool_to_f64(crate::fs::js_fs_symlink_sync(arg(0), arg(1))),
        ("fs", "readlinkSync") => crate::fs::js_fs_readlink_dispatch(arg(0), arg(1)),
        ("fs", "openSync") => crate::fs::js_fs_open_sync(arg(0), arg(1)),
        ("fs", "openAsBlob") => crate::fs::js_fs_open_as_blob(arg(0), arg(1)),
        ("fs", "closeSync") => bool_to_f64(crate::fs::js_fs_close_sync(arg(0))),
        ("fs", "readSync") if args_len == 3 => {
            crate::fs::js_fs_read_sync_options(arg(0), arg(1), arg(2))
        }
        ("fs", "readSync") => crate::fs::js_fs_read_sync(arg(0), arg(1), arg(2), arg(3), arg(4)),
        ("fs", "writeSync") if args_len >= 5 => {
            crate::fs::js_fs_write_buffer_sync(arg(0), arg(1), arg(2), arg(3), arg(4))
        }
        ("fs", "writeSync") if args_len >= 3 => {
            crate::fs::js_fs_write_sync_options_dispatch(arg(0), arg(1), arg(2))
        }
        ("fs", "writeSync") => crate::fs::js_fs_write_sync(arg(0), arg(1)),
        ("fs", "read") if args_len == 4 => {
            crate::fs::js_fs_read_callback_options(arg(0), arg(1), arg(2), arg(3))
        }
        ("fs", "read") => {
            crate::fs::js_fs_read_callback(arg(0), arg(1), arg(2), arg(3), arg(4), arg(5))
        }
        ("fs", "write") if args_len >= 6 => {
            crate::fs::js_fs_write_buffer_callback(arg(0), arg(1), arg(2), arg(3), arg(4), arg(5))
        }
        ("fs", "write") if args_len == 4 => {
            crate::fs::js_fs_write_buffer_callback_options(arg(0), arg(1), arg(2), arg(3))
        }
        ("fs", "write") => crate::fs::js_fs_write_callback(arg(0), arg(1), arg(2)),
        ("fs", "readv") => crate::fs::js_fs_readv_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "writev") => crate::fs::js_fs_writev_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "createWriteStream") => crate::fs::js_fs_create_write_stream(arg(0), arg(1)),
        ("fs", "createReadStream") => crate::fs::js_fs_create_read_stream(arg(0), arg(1)),
        ("fs", "WriteStream") | ("fs", "FileWriteStream") => {
            crate::fs::js_fs_create_write_stream(arg(0), arg(1))
        }
        ("fs", "ReadStream") | ("fs", "FileReadStream") => {
            crate::fs::js_fs_create_read_stream(arg(0), arg(1))
        }
        ("fs", "Utf8Stream") => crate::fs::js_fs_utf8_stream_call_without_new(arg(0)),
        ("fs", "readFile") => crate::fs::js_fs_read_file_callback(arg(0), arg(1), arg(2)),
        ("fs", "writeFile") => crate::fs::js_fs_write_file_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "appendFile") => {
            crate::fs::js_fs_append_file_callback(arg(0), arg(1), arg(2), arg(3))
        }
        ("fs", "chmod") => crate::fs::js_fs_chmod_callback(arg(0), arg(1), arg(2)),
        ("fs", "chown") => crate::fs::js_fs_chown_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "lchown") => crate::fs::js_fs_lchown_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "lchmod") => crate::fs::js_fs_lchmod_callback(arg(0), arg(1), arg(2)),
        ("fs", "truncate") => crate::fs::js_fs_truncate_callback(arg(0), arg(1), arg(2)),
        ("fs", "link") => crate::fs::js_fs_link_callback(arg(0), arg(1), arg(2)),
        ("fs", "symlink") => crate::fs::js_fs_symlink_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "readlink") => crate::fs::js_fs_readlink_callback(arg(0), arg(1), arg(2)),
        ("fs", "realpath") => crate::fs::js_fs_realpath_callback(arg(0), arg(1), arg(2)),
        ("fs", "mkdtemp") => crate::fs::js_fs_mkdtemp_callback(arg(0), arg(1), arg(2)),
        ("fs", "open") => crate::fs::js_fs_open_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "close") => crate::fs::js_fs_close_callback(arg(0), arg(1)),
        ("fs", "cp") => crate::fs::js_fs_cp_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "mkdir") => crate::fs::js_fs_mkdir_callback(arg(0), arg(1), arg(2)),
        ("fs", "unlink") => crate::fs::js_fs_unlink_callback(arg(0), arg(1)),
        ("fs", "rmdir") => crate::fs::js_fs_rmdir_callback(arg(0), arg(1), arg(2)),
        ("fs", "rm") => crate::fs::js_fs_rm_callback(arg(0), arg(1), arg(2)),
        ("fs", "access") => crate::fs::js_fs_access_callback(arg(0), arg(1), arg(2)),
        ("fs", "exists") => crate::fs::js_fs_exists_callback(arg(0), arg(1)),
        ("fs", "readdir") => crate::fs::js_fs_readdir_callback(arg(0), arg(1), arg(2)),
        ("fs", "stat") => crate::fs::js_fs_stat_callback(arg(0), arg(1), arg(2)),
        ("fs", "lstat") => crate::fs::js_fs_lstat_callback(arg(0), arg(1), arg(2)),
        ("fs", "statfs") => crate::fs::js_fs_statfs_callback(arg(0), arg(1), arg(2)),
        ("fs", "opendir") => crate::fs::js_fs_opendir_callback(arg(0), arg(1), arg(2)),
        ("fs", "glob") => crate::fs::js_fs_glob_callback(arg(0), arg(1), arg(2)),
        ("fs", "fstat") => crate::fs::js_fs_fstat_callback(arg(0), arg(1), arg(2)),
        ("fs", "ftruncate") => crate::fs::js_fs_ftruncate_callback(arg(0), arg(1), arg(2)),
        ("fs", "fsync") => crate::fs::js_fs_fsync_callback(arg(0), arg(1)),
        ("fs", "fdatasync") => crate::fs::js_fs_fdatasync_callback(arg(0), arg(1)),
        ("fs", "fchmod") => crate::fs::js_fs_fchmod_callback(arg(0), arg(1), arg(2)),
        ("fs", "fchown") => crate::fs::js_fs_fchown_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "utimes") => crate::fs::js_fs_utimes_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "lutimes") => crate::fs::js_fs_lutimes_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "futimes") => crate::fs::js_fs_futimes_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "rename") => crate::fs::js_fs_rename_callback(arg(0), arg(1), arg(2)),
        ("fs", "copyFile") => crate::fs::js_fs_copy_file_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "isDirectory") => bool_to_f64(crate::fs::js_fs_is_directory(arg(0))),

        // ── os module (no args, return string or f64) ──
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}

#[allow(
    unused_variables,
    unused_mut,
    unused_unsafe,
    clippy::let_and_return,
    clippy::all
)]
pub(crate) unsafe fn nm_dispatch_http(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
    let NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    } = *ctx;
    let _ = (obj, args_ptr, args_len, assert_skip_prototype);
    nm_general_closures!(
        obj,
        args_ptr,
        args_len,
        arg,
        i32_arg,
        bool_to_f64,
        str_to_f64,
        pack_args,
        pack_args_from,
        bool_tag,
        ptr_addr,
        optional_ptr_addr,
        _arg_event_ptr,
        arg_bits,
        _arg_closure_ptr,
        ptr_to_f64,
        typed_kind
    );
    match (module_name, method_name) {
        ("http", "validateHeaderName") => js_http_validate_header_name(arg(0), arg(1)),
        ("http", "validateHeaderValue") => js_http_validate_header_value(arg(0), arg(1)),
        // #3712: parser/proxy setters are deterministic no-ops in Perry's
        // runtime (no shared parser pool / env-driven proxy state), matching
        // Node's `undefined` return for valid inputs.
        ("http", "setMaxIdleHTTPParsers") | ("http", "setGlobalProxyFromEnv") => {
            js_http_setter_noop(arg(0))
        }
        ("http", "_connectionListener") => js_http_connection_listener_noop(arg(0)),
        ("http", "createServer")
        | ("http", "Server")
        // #4904: captured / aliased client entry points (`const { get } =
        // require('http'); get(opts, cb)`) — same bound-value mechanism as
        // the server factories; the stdlib dispatcher routes them to
        // `js_http_get` / `js_http_request` (and https twins).
        | ("http", "request")
        | ("http", "get")
        | ("https", "request")
        | ("https", "get")
        | ("https", "createServer")
        | ("https", "Server")
        | ("http2", "createServer")
        | ("http2", "createSecureServer")
        | ("http2", "Server") => {
            let ptr =
                crate::value::JS_NATIVE_HTTP_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(
                    *const u8,
                    usize,
                    *const u8,
                    usize,
                    *const f64,
                    usize,
                ) -> f64 = std::mem::transmute(ptr);
                dispatch(
                    module_name.as_ptr(),
                    module_name.len(),
                    method_name.as_ptr(),
                    method_name.len(),
                    args_ptr,
                    args_len,
                )
            }
        }
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}

#[allow(
    unused_variables,
    unused_mut,
    unused_unsafe,
    clippy::let_and_return,
    clippy::all
)]
pub(crate) unsafe fn nm_dispatch_inspector(
    ctx: &NmCtx,
    module_name: &str,
    method_name: &str,
) -> f64 {
    let NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    } = *ctx;
    let _ = (obj, args_ptr, args_len, assert_skip_prototype);
    nm_general_closures!(
        obj,
        args_ptr,
        args_len,
        arg,
        i32_arg,
        bool_to_f64,
        str_to_f64,
        pack_args,
        pack_args_from,
        bool_tag,
        ptr_addr,
        optional_ptr_addr,
        _arg_event_ptr,
        arg_bits,
        _arg_closure_ptr,
        ptr_to_f64,
        typed_kind
    );
    match (module_name, method_name) {
        ("inspector", "open") => {
            crate::node_inspector::js_node_inspector_open(arg(0), arg(1), arg(2))
        }
        ("inspector", "close") => crate::node_inspector::js_node_inspector_close(),
        ("inspector", "url") => crate::node_inspector::js_node_inspector_url(),
        ("inspector", "waitForDebugger") => {
            crate::node_inspector::js_node_inspector_wait_for_debugger()
        }
        ("inspector", "Session") => crate::node_inspector::js_node_inspector_session_new(),
        ("inspector/promises", "Session") => {
            crate::node_inspector::js_node_inspector_promises_session_new()
        }
        ("inspector.Network", "requestWillBeSent")
        | ("inspector.Network", "responseReceived")
        | ("inspector.Network", "loadingFinished")
        | ("inspector.Network", "loadingFailed")
        | ("inspector.Network", "dataSent")
        | ("inspector.Network", "dataReceived")
        | ("inspector.Network", "webSocketCreated")
        | ("inspector.Network", "webSocketClosed")
        | ("inspector.Network", "webSocketHandshakeResponseReceived") => {
            crate::node_inspector::js_node_inspector_network_notify(arg(0))
        }
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}
