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
pub(crate) unsafe fn nm_dispatch_assert(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("assert", "default") | ("assert/strict", "default") => js_assert_ok(arg(0), arg(1)),
        ("assert", "strict") | ("assert/strict", "strict") => js_assert_ok(arg(0), arg(1)),
        ("assert", "ok") | ("assert/strict", "ok") => js_assert_ok(arg(0), arg(1)),
        ("assert", "fail") | ("assert/strict", "fail") => js_assert_fail(arg(0)),
        ("assert", "equal") => js_assert_equal(arg(0), arg(1), arg(2)),
        ("assert", "notEqual") => js_assert_not_equal(arg(0), arg(1), arg(2)),
        ("assert", "strictEqual")
        | ("assert/strict", "strictEqual")
        | ("assert/strict", "equal") => js_assert_strict_equal(arg(0), arg(1), arg(2)),
        ("assert", "notStrictEqual")
        | ("assert/strict", "notStrictEqual")
        | ("assert/strict", "notEqual") => js_assert_not_strict_equal(arg(0), arg(1), arg(2)),
        ("assert", "deepEqual") if assert_skip_prototype => {
            js_assert_deep_equal_skip_prototype(arg(0), arg(1), arg(2))
        }
        ("assert", "notDeepEqual") if assert_skip_prototype => {
            js_assert_not_deep_equal_skip_prototype(arg(0), arg(1), arg(2))
        }
        ("assert", "deepStrictEqual")
        | ("assert/strict", "deepStrictEqual")
        | ("assert/strict", "deepEqual")
            if assert_skip_prototype =>
        {
            js_assert_deep_strict_equal_skip_prototype(arg(0), arg(1), arg(2))
        }
        ("assert", "notDeepStrictEqual")
        | ("assert/strict", "notDeepStrictEqual")
        | ("assert/strict", "notDeepEqual")
            if assert_skip_prototype =>
        {
            js_assert_not_deep_strict_equal_skip_prototype(arg(0), arg(1), arg(2))
        }
        ("assert", "deepEqual") => js_assert_deep_equal(arg(0), arg(1), arg(2)),
        ("assert", "notDeepEqual") => js_assert_not_deep_equal(arg(0), arg(1), arg(2)),
        ("assert", "deepStrictEqual")
        | ("assert/strict", "deepStrictEqual")
        | ("assert/strict", "deepEqual") => js_assert_deep_strict_equal(arg(0), arg(1), arg(2)),
        ("assert", "partialDeepStrictEqual") | ("assert/strict", "partialDeepStrictEqual") => {
            js_assert_partial_deep_strict_equal(arg(0), arg(1), arg(2))
        }
        ("assert", "notDeepStrictEqual")
        | ("assert/strict", "notDeepStrictEqual")
        | ("assert/strict", "notDeepEqual") => {
            js_assert_not_deep_strict_equal(arg(0), arg(1), arg(2))
        }
        ("assert", "match") | ("assert/strict", "match") => js_assert_match(arg(0), arg(1), arg(2)),
        ("assert", "doesNotMatch") | ("assert/strict", "doesNotMatch") => {
            js_assert_does_not_match(arg(0), arg(1), arg(2))
        }
        ("assert", "throws") | ("assert/strict", "throws") => {
            js_assert_throws(arg(0), arg(1), arg(2))
        }
        ("assert", "doesNotThrow") | ("assert/strict", "doesNotThrow") => {
            js_assert_does_not_throw(arg(0), arg(1), arg(2))
        }
        ("assert", "rejects") | ("assert/strict", "rejects") => {
            js_assert_rejects(arg(0), arg(1), arg(2))
        }
        ("assert", "doesNotReject") | ("assert/strict", "doesNotReject") => {
            js_assert_does_not_reject(arg(0), arg(1), arg(2))
        }
        ("assert", "ifError") | ("assert/strict", "ifError") => js_assert_if_error(arg(0)),
        ("assert", "Assert") | ("assert/strict", "Assert") => {
            crate::fs::validate::throw_type_error_with_code(
                "Class constructor Assert cannot be invoked without 'new'",
                "ERR_CONSTRUCT_CALL_REQUIRED",
            )
        }

        // ── fs module (args are NaN-boxed f64, booleans return as i32→f64) ──
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
pub(crate) unsafe fn nm_dispatch_async_hooks(
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
        ("async_hooks", "createHook") => {
            ptr_to_f64(crate::async_hooks::js_async_hooks_create_hook(arg(0)) as *const u8)
        }
        ("async_hooks", "executionAsyncId") => {
            crate::async_hooks::js_async_hooks_execution_async_id()
        }
        ("async_hooks", "triggerAsyncId") => crate::async_hooks::js_async_hooks_trigger_async_id(),
        ("async_hooks", "executionAsyncResource") => {
            crate::async_hooks::js_async_hooks_execution_async_resource()
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
pub(crate) unsafe fn nm_dispatch_bigint(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("bigint", "asIntN") => crate::object::bigint_as_n_dispatch(arg(0), arg(1), true),
        ("bigint", "asUintN") => crate::object::bigint_as_n_dispatch(arg(0), arg(1), false),
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}

/// bun:ffi (#6562) — dlopen / ptr / CString / FFIType / suffix. Method
/// resolution lives in `crate::bun_ffi::dispatch`; this bucket just forwards
/// the raw NaN-boxed args.
#[allow(unused_variables, unused_mut, unused_unsafe, clippy::all)]
pub(crate) unsafe fn nm_dispatch_bun_ffi(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
    let NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    } = *ctx;
    let _ = (obj, assert_skip_prototype);
    if module_name != "bun:ffi" {
        return f64::from_bits(JSValue::undefined().bits());
    }
    match crate::bun_ffi::dispatch(method_name, args_ptr, args_len) {
        Some(v) => v,
        None => f64::from_bits(JSValue::undefined().bits()),
    }
}

/// `"bun"` module shim pack (#6560). Callable exports; `stdin`/`stdout`/
/// `stderr` are property reads handled in `native_module.rs`, and
/// `pathToFileURL`/`fileURLToPath` alias the `node:url` implementations.
#[allow(clippy::all)]
pub(crate) unsafe fn nm_dispatch_bun(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("bun", "stringWidth") => crate::bun_compat::js_bun_string_width(arg(0), arg(1)),
        ("bun", "hash") => crate::bun_compat::js_bun_hash(arg(0), arg(1)),
        ("bun", "file") => crate::bun_compat::js_bun_file(arg(0)),
        ("bun", "write") => crate::bun_compat::js_bun_write(arg(0), arg(1)),
        ("bun", "pathToFileURL") => crate::url::js_url_path_to_file_url(arg(0), arg(1)),
        ("bun", "fileURLToPath") => crate::url::js_url_file_url_to_path(arg(0), arg(1)),
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
pub(crate) unsafe fn nm_dispatch_buffer(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("buffer.Buffer", "from") => {
            let data = arg(0);
            let second = JSValue::from_bits(arg(1).to_bits());
            let second_is_offset = args_len >= 2
                && !second.is_undefined()
                && !second.is_null()
                && !second.is_string()
                && !second.is_short_string();
            let buf = if args_len >= 3 || second_is_offset {
                let len = if args_len >= 3 { i32_arg(2) } else { -1 };
                crate::buffer::js_buffer_from_arraybuffer_slice(
                    data.to_bits() as i64,
                    i32_arg(1),
                    len,
                )
            } else {
                let enc = if args_len >= 2 {
                    crate::buffer::js_encoding_tag_from_value(arg(1))
                } else {
                    0
                };
                crate::buffer::js_buffer_from_value(data.to_bits() as i64, enc)
            };
            ptr_to_f64(buf as *const u8)
        }
        ("buffer.Buffer", "alloc") => {
            let buf = if args_len >= 2 {
                let enc = if args_len >= 3 {
                    crate::buffer::js_encoding_tag_from_value(arg(2))
                } else {
                    0
                };
                crate::buffer::js_buffer_alloc_fill_value(i32_arg(0), arg(1), enc)
            } else {
                crate::buffer::js_buffer_alloc(i32_arg(0), 0)
            };
            ptr_to_f64(buf as *const u8)
        }
        ("buffer.Buffer", "allocUnsafe") | ("buffer.Buffer", "allocUnsafeSlow") => {
            let buf = crate::buffer::js_buffer_alloc_unsafe(i32_arg(0));
            ptr_to_f64(buf as *const u8)
        }
        ("buffer.Buffer", "concat") => {
            let arr = ptr_addr(arg(0)) as *const crate::array::ArrayHeader;
            let buf = if args_len >= 2 {
                crate::buffer::js_buffer_concat_with_length(arr, arg(1))
            } else {
                crate::buffer::js_buffer_concat(arr)
            };
            ptr_to_f64(buf as *const u8)
        }
        ("buffer.Buffer", "copyBytesFrom") => {
            let buf = crate::buffer::js_buffer_copy_bytes_from(arg(0), arg(1), arg(2));
            ptr_to_f64(buf as *const u8)
        }
        ("buffer.Buffer", "of") => {
            let arr = pack_args();
            ptr_to_f64(crate::buffer::js_buffer_from_array(arr) as *const u8)
        }
        ("buffer.Buffer", "isBuffer") => {
            bool_to_f64(crate::buffer::js_buffer_is_buffer(arg(0).to_bits() as i64))
        }
        ("buffer.Buffer", "isEncoding") => {
            bool_to_f64(crate::buffer::js_buffer_is_encoding(arg(0)))
        }
        ("buffer.Buffer", "byteLength") => {
            crate::buffer::js_buffer_byte_length_value(arg(0), arg(1)) as f64
        }
        ("buffer.Buffer", "compare") => {
            let a = ptr_addr(arg(0));
            let b = ptr_addr(arg(1));
            if crate::buffer::is_registered_buffer(a) && crate::buffer::is_registered_buffer(b) {
                crate::buffer::js_buffer_compare(
                    a as *const crate::buffer::BufferHeader,
                    b as *const crate::buffer::BufferHeader,
                ) as f64
            } else {
                0.0
            }
        }
        ("buffer", "isAscii") => crate::buffer::js_buffer_is_ascii(arg(0)),
        ("buffer", "isUtf8") => crate::buffer::js_buffer_is_utf8(arg(0)),

        // ── process EventEmitter API ──
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
pub(crate) unsafe fn nm_dispatch_child_process(
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
        ("child_process", "spawn") => {
            let cmd = crate::string::js_string_materialize_to_heap(arg(0)) as i64;
            let args_p = optional_ptr_addr(arg(1)) as i64;
            let opts_p = optional_ptr_addr(arg(2)) as i64;
            crate::child_process::reactor::js_child_process_spawn_streams(cmd, args_p, opts_p)
        }
        ("child_process", "spawnSync") => {
            let cmd = crate::string::js_string_materialize_to_heap(arg(0));
            let args_p = optional_ptr_addr(arg(1)) as *const crate::array::ArrayHeader;
            let opts_p = optional_ptr_addr(arg(2)) as *const ObjectHeader;
            let result = crate::child_process::js_child_process_spawn_sync(cmd, args_p, opts_p);
            ptr_to_f64(result as *const u8)
        }
        ("child_process", "execSync") => {
            let cmd = crate::string::js_string_materialize_to_heap(arg(0));
            let opts_p = optional_ptr_addr(arg(1)) as *const ObjectHeader;
            crate::child_process::js_child_process_exec_sync(cmd, opts_p)
        }
        ("child_process", "exec") => {
            let cmd = crate::string::js_string_materialize_to_heap(arg(0));
            crate::child_process::js_child_process_exec(cmd, arg(1), arg(2))
        }
        ("child_process", "execFile") => {
            let file = crate::string::js_string_materialize_to_heap(arg(0)) as i64;
            crate::child_process::js_child_process_exec_file(file, arg(1), arg(2), arg(3))
        }
        ("child_process", "execFileSync") => {
            let file = crate::string::js_string_materialize_to_heap(arg(0)) as i64;
            crate::child_process::js_child_process_exec_file_sync(file, arg(1), arg(2))
        }
        ("child_process", "_forkChild") => crate::child_process::js_fork_child(args_len),
        ("child_process", "fork") => {
            let module = crate::string::js_string_materialize_to_heap(arg(0)) as i64;
            let args_p = optional_ptr_addr(arg(1)) as i64;
            let opts_p = optional_ptr_addr(arg(2)) as i64;
            crate::child_process::fork::js_child_process_fork(module, args_p, opts_p)
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
pub(crate) unsafe fn nm_dispatch_cluster(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("cluster", "setupPrimary") | ("cluster", "setupMaster") => {
            crate::cluster::js_cluster_setup_primary(arg(0))
        }
        ("cluster", "fork") => crate::cluster::js_cluster_fork(arg(0)),
        ("cluster", "disconnect") => crate::cluster::js_cluster_disconnect(arg(0)),
        ("cluster", "Worker") => f64::from_bits(JSValue::undefined().bits()),
        // #3687: node:cluster default-import EventEmitter surface.
        ("cluster", "on") | ("cluster", "addListener") => {
            crate::cluster::js_cluster_on(arg(0), arg(1))
        }
        ("cluster", "once") => crate::cluster::js_cluster_once(arg(0), arg(1)),
        ("cluster", "prependListener") => {
            crate::cluster::js_cluster_prepend_listener(arg(0), arg(1))
        }
        ("cluster", "prependOnceListener") => {
            crate::cluster::js_cluster_prepend_once_listener(arg(0), arg(1))
        }
        ("cluster", "emit") => crate::cluster::js_cluster_emit(arg(0), pack_args_from(1)),
        ("cluster", "eventNames") => crate::cluster::js_cluster_event_names(),
        ("cluster", "listenerCount") => crate::cluster::js_cluster_listener_count(arg(0)),
        ("cluster", "removeListener") | ("cluster", "off") => {
            crate::cluster::js_cluster_remove_listener(arg(0), arg(1))
        }
        ("cluster", "removeAllListeners") => {
            crate::cluster::js_cluster_remove_all_listeners(arg(0))
        }

        // #1577: captured-then-called crypto methods (`const f =
        // crypto.createHash; f(...)`). The impls live in perry-stdlib (which
        // depends on this crate), so route through the dispatcher stdlib
        // registers at startup via `js_set_native_crypto_dispatch`. Null when
        // stdlib isn't linked (e.g. runtime-only tests) → undefined. The
        // `randomFillSync` arm above is handled inline and never reaches here.
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
pub(crate) unsafe fn nm_dispatch_console(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("console", "Console") => crate::builtins::js_console_new2(arg(0), arg(1)),
        ("console", "log") | ("console", "info") | ("console", "debug") | ("console", "dirxml") => {
            crate::builtins::js_console_log_spread(pack_args());
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "error") => {
            crate::builtins::js_console_error_spread(pack_args());
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "warn") => {
            crate::builtins::js_console_warn_spread(pack_args());
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "assert") => {
            crate::builtins::js_console_assert_spread(arg(0), pack_args_from(1) as i64);
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "dir") => {
            crate::builtins::js_console_log_dynamic(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "trace") => {
            crate::builtins::js_console_trace_spread(pack_args());
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "table") => {
            if args_len > 1 {
                crate::builtins::js_console_table_with_properties(arg(0), arg(1));
            } else {
                crate::builtins::js_console_table(arg(0));
            }
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "clear") => {
            crate::builtins::js_console_clear();
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "count") => {
            crate::builtins::js_console_count_value(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "countReset") => {
            crate::builtins::js_console_count_reset_value(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "time") => {
            crate::builtins::js_console_time_value(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "timeEnd") => {
            crate::builtins::js_console_time_end_value(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "timeLog") => {
            if args_len > 1 {
                crate::builtins::js_console_time_log_spread(arg(0), pack_args_from(1));
            } else {
                crate::builtins::js_console_time_log_value(arg(0));
            }
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "group") | ("console", "groupCollapsed") => {
            if args_len > 0 {
                crate::builtins::js_console_log_dynamic(arg(0));
            }
            crate::builtins::js_console_group_begin();
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "groupEnd") => {
            crate::builtins::js_console_group_end();
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "profile") | ("console", "profileEnd") | ("console", "timeStamp") => {
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "context") => crate::builtins::js_console_context(arg(0)),
        ("console", "createTask") => crate::builtins::js_console_create_task(arg(0)),
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
pub(crate) unsafe fn nm_dispatch_crypto(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("crypto", "randomFillSync") if args_len >= 1 => {
            super::native_module_crypto_random::random_fill_sync(arg(0), arg(1), arg(2))
        }
        ("crypto", "KeyObject") => crate::fs::validate::throw_type_error_with_code(
            "Class constructor KeyObject cannot be invoked without 'new'",
            "ERR_CONSTRUCT_CALL_REQUIRED",
        ),
        ("crypto", "X509Certificate") => crate::fs::validate::throw_type_error_with_code(
            "Class constructor X509Certificate cannot be invoked without 'new'",
            "ERR_CONSTRUCT_CALL_REQUIRED",
        ),
        ("crypto.KeyObject", "from") => {
            super::native_module_crypto_key_object::key_object_from(arg(0))
        }
        ("crypto.webcrypto", "getRandomValues") if args_len >= 1 => {
            let undefined = f64::from_bits(JSValue::undefined().bits());
            super::native_module_crypto_random::random_fill_sync(arg(0), undefined, undefined)
        }
        // node:vm (createContext via #4050; rest #4079/#4087)
        ("crypto" | "crypto.webcrypto", _) => {
            let ptr =
                crate::value::JS_NATIVE_CRYPTO_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                dispatch(method_name.as_ptr(), method_name.len(), args_ptr, args_len)
            }
        }
        ("crypto.subtle", _) => {
            let ptr = crate::value::JS_NATIVE_WEBCRYPTO_DISPATCH
                .load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                dispatch(method_name.as_ptr(), method_name.len(), args_ptr, args_len)
            }
        }
        // Captured-then-called zlib methods (`const f = zlib.gzip; await f(buf)`,
        // `util.promisify(zlib.gzip)`). Mirrors the crypto arm above — the
        // impls live in perry-stdlib which depends on this crate, so route
        // through the dispatcher stdlib registers at startup via
        // `js_set_native_zlib_dispatch`. Null when stdlib isn't linked.
        ("crypto.Certificate", _) => {
            let qualified: &[u8] = match method_name {
                "verifySpkac" => b"Certificate.verifySpkac",
                "exportPublicKey" => b"Certificate.exportPublicKey",
                "exportChallenge" => b"Certificate.exportChallenge",
                _ => return f64::from_bits(JSValue::undefined().bits()),
            };
            let ptr =
                crate::value::JS_NATIVE_CRYPTO_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                dispatch(qualified.as_ptr(), qualified.len(), args_ptr, args_len)
            }
        }

        // #3906: top-level v8 helpers invoked through a bound callable
        // (`const s = v8.serialize; s(x)`). The method-call form
        // (`v8.serialize(x)`) already lowers through the codegen
        // NATIVE_MODULE_TABLE; these arms keep the value-read/bound-call form
        // coherent with the same FFI impls.
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}
