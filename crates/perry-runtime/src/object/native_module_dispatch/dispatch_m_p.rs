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
pub(crate) unsafe fn nm_dispatch_module(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("module", "createRequire") => crate::module_require::js_module_create_require(arg(0)),
        ("module", "enableCompileCache") => crate::process::js_module_enable_compile_cache(arg(0)),
        ("module", "flushCompileCache") => crate::process::js_module_flush_compile_cache(),
        ("module", "getCompileCacheDir") => crate::process::js_module_get_compile_cache_dir(),
        ("module", "getSourceMapsSupport") => crate::process::js_module_get_source_maps_support(),
        ("module", "isBuiltin") => crate::process::js_module_is_builtin(arg(0)),
        ("module", "Module") => crate::process::js_module_module_new(arg(0)),
        ("module", "_findPath") => crate::process::js_module_find_path(arg(0), arg(1), arg(2)),
        ("module", "_initPaths") => crate::process::js_module_init_paths(),
        ("module", "_load") => crate::process::js_module_load(arg(0), arg(1), arg(2)),
        ("module", "_nodeModulePaths") => crate::process::js_module_node_module_paths(arg(0)),
        ("module", "_preloadModules") => crate::process::js_module_preload_modules(arg(0)),
        ("module", "_resolveFilename") => {
            crate::process::js_module_resolve_filename(arg(0), arg(1), arg(2), arg(3))
        }
        ("module", "_resolveLookupPaths") => {
            crate::process::js_module_resolve_lookup_paths(arg(0), arg(1))
        }
        ("module", "register") => crate::process::js_module_register(arg(0), arg(1), arg(2)),
        ("module", "registerHooks") => crate::process::js_module_register_hooks(arg(0)),
        ("module", "setSourceMapsSupport") => {
            crate::process::js_module_set_source_maps_support(arg(0), arg(1))
        }
        ("module", "stripTypeScriptTypes") => {
            crate::process::js_module_strip_typescript_types(arg(0), arg(1))
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
pub(crate) unsafe fn nm_dispatch_net(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        // `net.connect(port, host)` / `net.createConnection(...)` as a bound
        // VALUE (mysql2-via-turbopack's externals wrapper requires 'net' and
        // calls the export dynamically) — the socket factory lives in
        // perry-stdlib, so route through the registered stdlib dispatcher,
        // the same bridge the http client entry points use.
        ("net", "connect") | ("net", "createConnection") => {
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
        ("net", "_normalizeArgs") => crate::net_validate::js_net_normalize_args(arg(0)),
        ("net", "_createServerHandle") => crate::net_validate::js_net_create_server_handle_stub(
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4),
        ),

        // ── perf_hooks module (performance.*) ──
        // Statically lowered at call sites (module_static.rs); these arms
        // also serve the generic namespace-object method-dispatch path.
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
pub(crate) unsafe fn nm_dispatch_os(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("os", "tmpdir") => str_to_f64(crate::os::js_os_tmpdir()),
        ("os", "homedir") => str_to_f64(crate::os::js_os_homedir()),
        ("os", "platform") => str_to_f64(crate::os::js_os_platform()),
        ("os", "arch") => str_to_f64(crate::os::js_os_arch()),
        ("os", "hostname") => str_to_f64(crate::os::js_os_hostname()),
        ("os", "type") => str_to_f64(crate::os::js_os_type()),
        ("os", "release") => str_to_f64(crate::os::js_os_release()),
        ("os", "eol") => str_to_f64(crate::os::js_os_eol()),
        ("os", "devNull") => str_to_f64(crate::os::js_os_dev_null()),
        ("os", "totalmem") => crate::os::js_os_totalmem(),
        ("os", "freemem") => crate::os::js_os_freemem(),
        ("os", "uptime") => crate::os::js_os_uptime(),
        ("os", "availableParallelism") => crate::os::js_os_available_parallelism(),
        ("os", "endianness") => str_to_f64(crate::os::js_os_endianness()),
        ("os", "machine") => str_to_f64(crate::os::js_os_machine()),
        ("os", "loadavg") => {
            f64::from_bits(JSValue::pointer(crate::os::js_os_loadavg() as *const u8).bits())
        }
        ("os", "version") => str_to_f64(crate::os::js_os_version()),
        ("os", "cpus") => {
            f64::from_bits(JSValue::pointer(crate::os::js_os_cpus() as *const u8).bits())
        }
        ("os", "networkInterfaces") => f64::from_bits(
            JSValue::pointer(crate::os::js_os_network_interfaces() as *const u8).bits(),
        ),
        ("os", "userInfo") => {
            // #3004 — honor a runtime `options.encoding === "buffer"` value
            // (variable / function-return / computed-key options object).
            let opts_bits = arg(0).to_bits() as i64;
            f64::from_bits(
                JSValue::pointer(crate::os::js_os_user_info_options(opts_bits) as *const u8).bits(),
            )
        }
        ("os", "getPriority") => crate::os::js_os_get_priority(arg(0)),
        ("os", "setPriority") => crate::os::js_os_set_priority(arg(0), arg(1)),

        // ── path module (args are NaN-boxed strings → extract raw StringHeader ptr) ──
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
pub(crate) unsafe fn nm_dispatch_path(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
    let require_path_str_ptr = |n: usize| -> *const crate::StringHeader {
        if n < args_len {
            let v = arg(n);
            let ptr = crate::string::js_string_materialize_to_heap(v);
            if !ptr.is_null() {
                return ptr;
            }
        }
        crate::path::throw_invalid_path_arg_type()
    };
    let optional_path_str_ptr = |n: usize| -> *const crate::StringHeader {
        if n >= args_len {
            return std::ptr::null();
        }
        let v = arg(n);
        let jsv = JSValue::from_bits(v.to_bits());
        if jsv.is_undefined() {
            return std::ptr::null();
        }
        let ptr = crate::string::js_string_materialize_to_heap(v);
        if !ptr.is_null() {
            return ptr;
        }
        crate::path::throw_invalid_path_arg_type()
    };
    let path_join_value = |win32: bool| -> f64 {
        if args_len == 0 {
            let result = if win32 {
                crate::path::js_path_win32_join_unchecked(std::ptr::null(), std::ptr::null())
            } else {
                crate::path::js_path_join_unchecked(std::ptr::null(), std::ptr::null())
            };
            return str_to_f64(result);
        }
        let first = require_path_str_ptr(0);
        let mut result = if win32 {
            crate::path::js_path_win32_join_unchecked(first, std::ptr::null())
        } else {
            crate::path::js_path_join_unchecked(first, std::ptr::null())
        };
        for i in 1..args_len {
            let segment = require_path_str_ptr(i);
            result = if win32 {
                crate::path::js_path_win32_join_unchecked(result, segment)
            } else {
                crate::path::js_path_join_unchecked(result, segment)
            };
        }
        str_to_f64(result)
    };
    let path_resolve_value = |win32: bool| -> f64 {
        let mut result = if args_len == 0 {
            if win32 {
                crate::path::js_path_win32_join_unchecked(std::ptr::null(), std::ptr::null())
            } else {
                crate::path::js_path_join_unchecked(std::ptr::null(), std::ptr::null())
            }
        } else {
            require_path_str_ptr(0) as *mut crate::StringHeader
        };
        for i in 1..args_len {
            let segment = require_path_str_ptr(i);
            result = if win32 {
                crate::path::js_path_win32_resolve_join(result, segment)
            } else {
                crate::path::js_path_resolve_join(result, segment)
            };
        }
        if win32 {
            str_to_f64(crate::path::js_path_win32_resolve(result))
        } else {
            str_to_f64(crate::path::js_path_resolve(result))
        }
    };
    let path_basename_value = |win32: bool| -> f64 {
        let path = require_path_str_ptr(0);
        let ext = optional_path_str_ptr(1);
        if win32 {
            if ext.is_null() {
                str_to_f64(crate::path::js_path_win32_basename(path))
            } else {
                str_to_f64(crate::path::js_path_win32_basename_ext(path, ext))
            }
        } else if ext.is_null() {
            str_to_f64(crate::path::js_path_basename(path))
        } else {
            str_to_f64(crate::path::js_path_basename_ext(path, ext))
        }
    };
    match (module_name, method_name) {
        ("path", "dirname") => str_to_f64(crate::path::js_path_dirname(require_path_str_ptr(0))),
        ("path", "basename") => path_basename_value(false),
        ("path", "extname") => str_to_f64(crate::path::js_path_extname(require_path_str_ptr(0))),
        ("path", "normalize") => {
            str_to_f64(crate::path::js_path_normalize(require_path_str_ptr(0)))
        }
        ("path", "resolve") => path_resolve_value(false),
        ("path", "join") => path_join_value(false),
        ("path", "relative") => str_to_f64(crate::path::js_path_relative(
            require_path_str_ptr(0),
            require_path_str_ptr(1),
        )),
        ("path", "isAbsolute") => {
            bool_to_f64(crate::path::js_path_is_absolute(require_path_str_ptr(0)))
        }
        ("path", "toNamespacedPath") => crate::path::js_path_to_namespaced_path_value(arg(0)),
        ("path", "_makeLong") => crate::path::js_path_to_namespaced_path_value(arg(0)),
        ("path", "matchesGlob") => bool_to_f64(crate::path::js_path_matches_glob(
            require_path_str_ptr(0),
            require_path_str_ptr(1),
        )),
        ("path", "parse") => f64::from_bits(
            JSValue::pointer(crate::path::js_path_parse(require_path_str_ptr(0)) as *const u8)
                .bits(),
        ),
        ("path", "format") => str_to_f64(crate::path::js_path_format(arg(0))),

        // #1740: dynamic sub-namespace method dispatch — `path[k].method(...)`
        // where `k` resolves to "win32"/"posix" at runtime. `path[k].sep`
        // (property reads) already worked, but method calls landed here with
        // module_name "path.win32" / "path.posix" and no matching arm, so they
        // returned undefined. win32 routes to the `js_path_win32_*` family;
        // posix routes to the base `js_path_*` family (POSIX `/` semantics),
        // mirroring how the static `path.win32.X()` / `path.posix.X()` forms
        // lower in codegen.
        ("path.win32", "dirname") => {
            str_to_f64(crate::path::js_path_win32_dirname(require_path_str_ptr(0)))
        }
        ("path.win32", "basename") => path_basename_value(true),
        ("path.win32", "extname") => {
            str_to_f64(crate::path::js_path_win32_extname(require_path_str_ptr(0)))
        }
        ("path.win32", "normalize") => str_to_f64(crate::path::js_path_win32_normalize(
            require_path_str_ptr(0),
        )),
        ("path.win32", "resolve") => path_resolve_value(true),
        ("path.win32", "join") => path_join_value(true),
        ("path.win32", "relative") => str_to_f64(crate::path::js_path_win32_relative(
            require_path_str_ptr(0),
            require_path_str_ptr(1),
        )),
        ("path.win32", "toNamespacedPath") => {
            crate::path::js_path_win32_to_namespaced_path_value(arg(0))
        }
        ("path.win32", "_makeLong") => crate::path::js_path_win32_to_namespaced_path_value(arg(0)),
        ("path.win32", "isAbsolute") => bool_to_f64(crate::path::js_path_win32_is_absolute(
            require_path_str_ptr(0),
        )),
        ("path.win32", "matchesGlob") => bool_to_f64(crate::path::js_path_win32_matches_glob(
            require_path_str_ptr(0),
            require_path_str_ptr(1),
        )),
        ("path.win32", "parse") => {
            ptr_to_f64(crate::path::js_path_win32_parse(require_path_str_ptr(0)) as *const u8)
        }
        ("path.win32", "format") => str_to_f64(crate::path::js_path_win32_format(arg(0))),
        ("path.posix", "dirname") => {
            str_to_f64(crate::path::js_path_dirname(require_path_str_ptr(0)))
        }
        ("path.posix", "basename") => path_basename_value(false),
        ("path.posix", "extname") => {
            str_to_f64(crate::path::js_path_extname(require_path_str_ptr(0)))
        }
        ("path.posix", "normalize") => {
            str_to_f64(crate::path::js_path_normalize(require_path_str_ptr(0)))
        }
        ("path.posix", "resolve") => path_resolve_value(false),
        ("path.posix", "join") => path_join_value(false),
        ("path.posix", "relative") => str_to_f64(crate::path::js_path_relative(
            require_path_str_ptr(0),
            require_path_str_ptr(1),
        )),
        ("path.posix", "toNamespacedPath") => crate::path::js_path_to_namespaced_path_value(arg(0)),
        ("path.posix", "_makeLong") => crate::path::js_path_to_namespaced_path_value(arg(0)),
        ("path.posix", "isAbsolute") => {
            bool_to_f64(crate::path::js_path_is_absolute(require_path_str_ptr(0)))
        }
        ("path.posix", "matchesGlob") => bool_to_f64(crate::path::js_path_matches_glob(
            require_path_str_ptr(0),
            require_path_str_ptr(1),
        )),
        ("path.posix", "parse") => {
            ptr_to_f64(crate::path::js_path_parse(require_path_str_ptr(0)) as *const u8)
        }
        ("path.posix", "format") => str_to_f64(crate::path::js_path_format(arg(0))),

        // ── util module ──
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
pub(crate) unsafe fn nm_dispatch_perf(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("perf_hooks", "now") => crate::date::js_performance_now(),
        ("perf_hooks", "mark") => crate::perf_hooks::js_perf_mark(arg(0), arg(1)),
        ("perf_hooks", "measure") => crate::perf_hooks::js_perf_measure(arg(0), arg(1), arg(2)),
        ("perf_hooks", "getEntries") => crate::perf_hooks::js_perf_get_entries(),
        ("perf_hooks", "getEntriesByType") => {
            crate::perf_hooks::js_perf_get_entries_by_type(arg(0))
        }
        ("perf_hooks", "getEntriesByName") => {
            crate::perf_hooks::js_perf_get_entries_by_name(arg(0), arg(1))
        }
        ("perf_hooks", "clearMarks") => crate::perf_hooks::js_perf_clear_marks(arg(0)),
        ("perf_hooks", "clearMeasures") => crate::perf_hooks::js_perf_clear_measures(arg(0)),
        ("perf_hooks", "eventLoopUtilization") => {
            crate::perf_hooks::js_perf_event_loop_utilization(arg(0), arg(1))
        }
        ("perf_hooks", "toJSON") => crate::perf_hooks::js_perf_to_json(),
        ("perf_hooks", "clearResourceTimings") => {
            crate::perf_hooks::js_perf_clear_resource_timings()
        }
        ("perf_hooks", "setResourceTimingBufferSize") => {
            crate::perf_hooks::js_perf_set_resource_timing_buffer_size(arg(0))
        }
        ("perf_hooks", "markResourceTiming") => crate::perf_hooks::js_perf_mark_resource_timing(
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4),
            arg(5),
            arg(6),
            arg(7),
        ),
        ("perf_hooks", "timerify") => crate::perf_hooks::js_perf_timerify(arg(0), arg(1)),

        // ── PerformanceObserver instance (perf_observer) ──
        // The registry index lives in field[1] of the namespace object; the
        // runtime fns re-derive it from the object value.
        ("perf_observer", "observe") => {
            let obs_val = crate::value::js_nanbox_pointer(obj as i64);
            crate::perf_hooks::js_perf_observer_observe(obs_val, arg(0))
        }
        ("perf_observer", "disconnect") => {
            let obs_val = crate::value::js_nanbox_pointer(obj as i64);
            crate::perf_hooks::js_perf_observer_disconnect(obs_val)
        }
        ("perf_observer", "takeRecords") => {
            let obs_val = crate::value::js_nanbox_pointer(obj as i64);
            crate::perf_hooks::js_perf_observer_take_records(obs_val)
        }

        // ── PerformanceObserverEntryList (the callback `list` arg) ──
        ("perf_observer_list", "getEntries") => crate::perf_hooks::current_list_get_entries(),
        ("perf_observer_list", "getEntriesByType") => {
            crate::perf_hooks::current_list_get_by_type(arg(0))
        }
        ("perf_observer_list", "getEntriesByName") => {
            crate::perf_hooks::current_list_get_by_name(arg(0))
        }

        // ── Histogram instance methods (#1336) ──
        // Every method is a no-op on the stub — `enable`/`disable`/`reset`
        // don't sample anything, `record`/`recordDelta`/`add` discard input.
        // `percentile(p)` returns 0 (no samples => no rank).
        ("perf_histogram", "enable")
        | ("perf_histogram", "disable")
        | ("perf_histogram", "reset")
        | ("perf_histogram", "record")
        | ("perf_histogram", "recordDelta")
        | ("perf_histogram", "add") => crate::perf_hooks::js_perf_histogram_noop(),
        ("perf_histogram", "percentile") | ("perf_histogram", "percentileBigInt") => {
            crate::perf_hooks::js_perf_histogram_percentile(arg(0))
        }

        // ── timers module ──
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
pub(crate) unsafe fn nm_dispatch_process(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("process", "on") => crate::os::js_process_on(arg_bits(0), arg_bits(1)),
        ("process", "addListener") => crate::os::js_process_add_listener(arg_bits(0), arg_bits(1)),
        ("process", "once") => crate::os::js_process_once(arg_bits(0), arg_bits(1)),
        ("process", "prependListener") => {
            crate::os::js_process_prepend_listener(arg_bits(0), arg_bits(1))
        }
        ("process", "prependOnceListener") => {
            crate::os::js_process_prepend_once_listener(arg_bits(0), arg_bits(1))
        }
        ("process", "emit") => crate::os::js_process_emit(arg_bits(0), pack_args_from(1)),
        ("process", "removeListener") => {
            crate::os::js_process_remove_listener(arg_bits(0), arg_bits(1))
        }
        ("process", "off") => crate::os::js_process_off(arg_bits(0), arg_bits(1)),
        ("process", "removeAllListeners") => {
            crate::os::js_process_remove_all_listeners(arg_bits(0))
        }
        ("process", "listenerCount") => {
            crate::os::js_process_listener_count(arg_bits(0), arg_bits(1))
        }
        ("process", "listeners") => {
            ptr_to_f64(crate::os::js_process_listeners(arg_bits(0)) as *const u8)
        }
        ("process", "rawListeners") => {
            ptr_to_f64(crate::os::js_process_raw_listeners(arg_bits(0)) as *const u8)
        }
        ("process", "eventNames") => ptr_to_f64(crate::os::js_process_event_names() as *const u8),
        ("process", "setMaxListeners") => crate::os::js_process_set_max_listeners(arg(0)),
        ("process", "getMaxListeners") => crate::os::js_process_get_max_listeners(),
        ("process", "send") => {
            crate::process::process_ipc_send_call(arg(0), arg(1), arg(2), arg(3))
        }
        ("process", "disconnect") => crate::process::process_ipc_disconnect_call(),
        ("process", "emitWarning") => {
            crate::process::js_process_emit_warning(arg(0), arg(1), arg(2));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "getBuiltinModule") => crate::process::js_process_get_builtin_module(arg(0)),
        ("process", "execve") => crate::process::js_process_execve(arg(0), arg(1), arg(2)),
        ("process", "cwd") => str_to_f64(crate::os::js_process_cwd()),
        ("process", "uptime") => crate::os::js_process_uptime(),
        ("process", "memoryUsage") => crate::process::js_process_memory_usage(),
        ("process", "threadCpuUsage") => crate::process::js_process_thread_cpu_usage(arg(0)),
        ("process", "availableMemory") => crate::process::js_process_available_memory(),
        ("process", "constrainedMemory") => crate::process::js_process_constrained_memory(),
        ("process", "resourceUsage") => crate::process::js_process_resource_usage(),
        ("process", "getActiveResourcesInfo") => crate::process::js_process_active_resources_info(),
        ("process", "binding") => crate::process::js_process_binding(arg(0)),
        ("process", "_linkedBinding") => crate::process::js_process_linked_binding(arg(0)),
        ("process", "dlopen") => crate::process::js_process_dlopen(),
        ("process", "_rawDebug") => crate::process::js_process_raw_debug(),
        ("process", "_debugProcess") => crate::process::js_process_debug_process(),
        ("process", "_debugEnd") => crate::process::js_process_debug_end(),
        ("process", "_startProfilerIdleNotifier") => {
            crate::process::js_process_start_profiler_idle_notifier()
        }
        ("process", "_stopProfilerIdleNotifier") => {
            crate::process::js_process_stop_profiler_idle_notifier()
        }
        ("process", "reallyExit") => crate::process::js_process_really_exit(),
        ("process", "_fatalException") => {
            crate::process::js_process_fatal_exception(arg(0), arg(1))
        }
        ("process", "_tickCallback") => crate::process::js_process_tick_callback(),
        ("process", "_getActiveHandles") => crate::process::js_process_get_active_handles(),
        ("process", "_getActiveRequests") => crate::process::js_process_get_active_requests(),
        ("process", "openStdin") => crate::process::js_process_open_stdin(),
        ("process", "_kill") => crate::process::js_process_internal_kill(),
        ("process", "getuid") => crate::process::js_process_getuid(),
        ("process", "geteuid") => crate::process::js_process_geteuid(),
        ("process", "getgid") => crate::process::js_process_getgid(),
        ("process", "getegid") => crate::process::js_process_getegid(),
        ("process", "sourceMapsEnabled") => crate::process::js_process_source_maps_enabled(),
        ("process", "setSourceMapsEnabled") => {
            crate::process::js_process_set_source_maps_enabled(arg(0))
        }
        ("process", "ref") => crate::process::js_process_ref(arg(0)),
        ("process", "unref") => crate::process::js_process_unref(arg(0)),
        ("process", "hasUncaughtExceptionCaptureCallback") => {
            crate::process::js_process_has_uncaught_exception_capture_callback()
        }
        ("process", "setUncaughtExceptionCaptureCallback") => {
            crate::process::js_process_set_uncaught_exception_capture_callback(arg(0))
        }
        ("process", "addUncaughtExceptionCaptureCallback") => {
            crate::process::js_process_add_uncaught_exception_capture_callback(arg(0))
        }
        ("process", "nextTick") => {
            // Validate the callback and forward trailing args (#3046).
            unsafe { crate::os::js_process_next_tick(arg_bits(0), pack_args_from(1)) };
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "chdir") => {
            // #3043 — route dynamic/method-value chdir calls through the
            // full-value validator (matching the static codegen path) so a
            // non-string argument throws TypeError [ERR_INVALID_ARG_TYPE]
            // instead of silently no-oping on a null string pointer.
            unsafe {
                crate::process::js_process_chdir_jsv(arg(0));
            }
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "loadEnvFile") => {
            crate::process::js_process_load_env_file(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        // #3712: node:http module-level header validation helpers. These mirror
        // Node's `validateHeaderName` / `validateHeaderValue` (lib/_http_common
        // + lib/_http_outgoing): on invalid input they throw the matching error
        // codes, otherwise they return undefined.
        ("process", "getgroups") => crate::process::js_process_getgroups(),
        ("process", "setuid") => {
            crate::process::js_process_setuid(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "seteuid") => {
            crate::process::js_process_seteuid(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "setgid") => {
            crate::process::js_process_setgid(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "setegid") => {
            crate::process::js_process_setegid(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "setgroups") => {
            crate::process::js_process_setgroups(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "initgroups") => {
            crate::process::js_process_initgroups(arg(0), arg(1));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "kill") => crate::os::js_process_kill(arg(0), arg(1)),
        ("process", "exit") => {
            crate::process::js_process_exit(arg(0));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "abort") => {
            crate::process::js_process_abort();
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "umask") => {
            let mask = arg(0);
            let mask_value = JSValue::from_bits(mask.to_bits());
            if mask_value.is_undefined() {
                crate::process::js_process_umask()
            } else {
                crate::process::js_process_umask_set(mask)
            }
        }
        ("process", "emitWarning") => {
            crate::process::js_process_emit_warning(arg(0), arg(1), arg(2));
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        ("process", "hrtime") => crate::os::js_process_hrtime(arg(0)),
        ("process", "cpuUsage") => crate::process::js_process_cpu_usage(arg(0)),
        // ── crypto module ──
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}
