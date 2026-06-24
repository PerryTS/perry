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
pub(crate) unsafe fn nm_dispatch_punycode(
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
        ("punycode", "decode") => crate::punycode::js_punycode_decode(arg(0)),
        ("punycode", "encode") => crate::punycode::js_punycode_encode(arg(0)),
        ("punycode", "toASCII") => crate::punycode::js_punycode_to_ascii(arg(0)),
        ("punycode", "toUnicode") => crate::punycode::js_punycode_to_unicode(arg(0)),
        // ── punycode.ucs2 sub-namespace (#2607) ──
        ("punycode.ucs2", "decode") => crate::punycode::js_punycode_ucs2_decode(arg(0)),
        ("punycode.ucs2", "encode") => crate::punycode::js_punycode_ucs2_encode(arg(0)),

        // ── dgram namespace (`node:dgram` / `dgram`) ──
        // Gated behind `mod-dgram`: `crate::dgram` is only compiled when the
        // program imports `dgram` (the compiler enables the feature on
        // `module: "dgram"` usage), so this arm — and the `js_dgram_*` externs
        // it calls — are absent otherwise. Unreachable when off (a dgram
        // namespace can't exist without the import that enables the feature).
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
pub(crate) unsafe fn nm_dispatch_querystring(
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
        (
            "querystring",
            "unescapeBuffer" | "unescape" | "escape" | "stringify" | "encode" | "parse" | "decode",
        ) => {
            let ptr = crate::value::JS_NATIVE_QUERYSTRING_DISPATCH
                .load(std::sync::atomic::Ordering::SeqCst);
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
pub(crate) unsafe fn nm_dispatch_readline(
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
        ("readline", "clearLine") => {
            crate::readline_helpers::js_readline_clear_line_args(pack_args())
        }
        ("readline", "clearScreenDown") => {
            crate::readline_helpers::js_readline_clear_screen_down_args(pack_args())
        }
        ("readline", "cursorTo") => {
            crate::readline_helpers::js_readline_cursor_to_args(pack_args())
        }
        ("readline", "moveCursor") => {
            crate::readline_helpers::js_readline_move_cursor_args(pack_args())
        }
        ("readline", "emitKeypressEvents") => {
            crate::readline_helpers::js_readline_emit_keypress_events_args(pack_args())
        }

        // ── node:dns / node:dns/promises configuration ──
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
pub(crate) unsafe fn nm_dispatch_repl(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("repl", "start") => crate::node_repl::js_repl_start(arg(0)),
        ("repl", "REPLServer") => crate::node_repl::js_repl_repl_server_new(arg(0)),
        ("repl", "Recoverable") => crate::node_repl::js_repl_recoverable_new(arg(0)),

        // #3680: `v8.Serializer` / `v8.DefaultSerializer` instance methods.
        // The registry id lives in field[1] of the namespace object; the
        // runtime re-derives it from the receiver value.
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
pub(crate) unsafe fn nm_dispatch_sea(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("sea", "isSea") => crate::node_sea::js_sea_is_sea(),
        ("sea", "getAsset") => crate::node_sea::js_sea_get_asset(arg(0), arg(1)),
        ("sea", "getAssetAsBlob") => crate::node_sea::js_sea_get_asset_as_blob(arg(0), arg(1)),
        ("sea", "getRawAsset") => crate::node_sea::js_sea_get_raw_asset(arg(0)),
        ("sea", "getAssetKeys") => crate::node_sea::js_sea_get_asset_keys(),
        // ── Buffer constructor static API ──
        // `class MyBuffer extends Buffer {}; MyBuffer.from(...)` reaches this
        // path through js_class_static_method_call's native-superclass
        // fallback. Return plain Buffer instances, matching Node's internal
        // FastBuffer behavior rather than species/subclass construction.
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
pub(crate) unsafe fn nm_dispatch_sqlite(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("sqlite", _) => {
            let ptr =
                crate::value::JS_NATIVE_SQLITE_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: crate::value::JsNativeSqliteDispatchFn = std::mem::transmute(ptr);
                dispatch(
                    method_name.as_ptr(),
                    method_name.len(),
                    args_ptr,
                    args_len,
                    0,
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
pub(crate) unsafe fn nm_dispatch_stream(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("stream", _) => dispatch_stream_native_module_method(method_name, args_ptr, args_len)
            .unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits())),
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
pub(crate) unsafe fn nm_dispatch_timers(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("timers", "setTimeout") if args_len >= 2 => {
            let cb = arg(0);
            let delay = arg(1);
            let cb_handle = {
                let bits = cb.to_bits();
                if (bits >> 48) >= 0x7FF8 {
                    (bits & 0x0000_FFFF_FFFF_FFFF) as i64
                } else {
                    bits as i64
                }
            };
            if args_len > 2 {
                let extra_ptr = unsafe { args_ptr.add(2) };
                return f64::from_bits(
                    JSValue::pointer(crate::timer::js_set_timeout_callback_args(
                        cb_handle,
                        delay,
                        extra_ptr,
                        (args_len - 2) as i32,
                    ) as *mut u8)
                    .bits(),
                );
            }
            return f64::from_bits(JSValue::pointer(
                crate::timer::js_set_timeout_callback(cb_handle, delay) as *mut u8,
            ).bits());
        }
        ("timers", "setImmediate") if args_len >= 1 => {
            let cb = arg(0);
            let cb_handle = {
                let bits = cb.to_bits();
                if (bits >> 48) >= 0x7FF8 {
                    (bits & 0x0000_FFFF_FFFF_FFFF) as i64
                } else {
                    bits as i64
                }
            };
            if args_len > 1 {
                let extra_ptr = unsafe { args_ptr.add(1) };
                return f64::from_bits(
                    JSValue::pointer(crate::timer::js_set_immediate_callback_args(
                        cb_handle,
                        extra_ptr,
                        (args_len - 1) as i32,
                    ) as *mut u8)
                    .bits(),
                );
            }
            return f64::from_bits(
                JSValue::pointer(crate::timer::js_set_immediate_callback(cb_handle) as *mut u8)
                    .bits(),
            );
        }
        ("timers", "setInterval") if args_len >= 2 => {
            let cb = arg(0);
            let delay = arg(1);
            let bits = cb.to_bits();
            let cb_handle = if (bits >> 48) >= 0x7FF8 {
                (bits & 0x0000_FFFF_FFFF_FFFF) as i64
            } else {
                bits as i64
            };
            if args_len > 2 {
                let extra_ptr = unsafe { args_ptr.add(2) };
                return f64::from_bits(
                    JSValue::pointer(crate::timer::js_set_interval_callback_args(
                        cb_handle,
                        delay,
                        extra_ptr,
                        (args_len - 2) as i32,
                    ) as *mut u8)
                    .bits(),
                );
            }
            return f64::from_bits(
                JSValue::pointer(crate::timer::setInterval(cb_handle, delay) as *mut u8).bits(),
            );
        }
        ("timers", "clearTimeout") if args_len >= 1 => {
            crate::timer::js_clear_timeout_value(arg(0));
            return f64::from_bits(JSValue::undefined().bits());
        }
        ("timers", "clearImmediate") if args_len >= 1 => {
            crate::timer::js_clear_immediate_value(arg(0));
            return f64::from_bits(JSValue::undefined().bits());
        }
        ("timers", "clearInterval") if args_len >= 1 => {
            crate::timer::js_clear_interval_value(arg(0));
            return f64::from_bits(JSValue::undefined().bits());
        }
        // ── assert module ──
        // Root-callable `assert(x, msg)` / `assert.strict(x, msg)` —
        // HIR lowers these to method "default".
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
pub(crate) unsafe fn nm_dispatch_tls(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("tls", "getCiphers") => crate::tls::js_tls_get_ciphers(),
        ("tls", "getCACertificates") => crate::tls::js_tls_get_ca_certificates(arg(0)),
        ("tls", "setDefaultCACertificates") => {
            crate::tls::js_tls_set_default_ca_certificates(arg(0))
        }
        ("tls", "checkServerIdentity") => crate::tls::js_tls_check_server_identity(arg(0), arg(1)),
        ("tls", "createSecureContext") => crate::tls::js_tls_create_secure_context(arg(0)),
        ("tls", "SecureContext") => crate::tls::js_tls_secure_context_new(arg(0)),

        // ── wasi module ──
        ("tls", _) => {
            let ptr =
                crate::value::JS_NATIVE_TLS_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if ptr.is_null() {
                f64::from_bits(JSValue::undefined().bits())
            } else {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                dispatch(method_name.as_ptr(), method_name.len(), args_ptr, args_len)
            }
        }

        // #2533: captured / aliased server factories
        // (`const createServer = options.createServer || createServerHTTP;
        // createServer(opts, handler)` — `@hono/node-server`'s `serve()`). The
        // method-call form (`http.createServer(...)`) already lowers through a
        // dedicated codegen NATIVE_MODULE_TABLE path; the value-read form yields
        // a bound-method closure (see `is_native_module_callable_export`) that
        // lands here when invoked. The impls live in perry-ext-http-server, so
        // route through the dispatcher perry-stdlib registers at startup under
        // `external-http-server-pump` (enabled whenever http/https/http2 is
        // imported). Null when the http ext crate isn't linked → undefined. The
        // dispatcher takes the module name so one callback serves all three.
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
pub(crate) unsafe fn nm_dispatch_tty(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("tty", "isatty") => crate::tty::js_tty_isatty(arg(0)),
        ("tty", "ReadStream") => crate::tty::js_tty_read_stream_new(arg(0)),
        ("tty", "WriteStream") => crate::tty::js_tty_write_stream_new(arg(0)),

        // ── tls module helpers ──
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
pub(crate) unsafe fn nm_dispatch_url(ctx: &NmCtx, module_name: &str, method_name: &str) -> f64 {
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
        ("url", "fileURLToPath") => crate::url::js_url_file_url_to_path(arg(0), arg(1)),
        ("url", "fileURLToPathBuffer") => {
            crate::url::js_url_file_url_to_path_buffer(arg(0), arg(1))
        }
        ("url", "pathToFileURL") => crate::url::js_url_path_to_file_url(arg(0), arg(1)),
        ("url", "domainToASCII") => crate::url::js_url_domain_to_ascii(arg(0)),
        ("url", "domainToUnicode") => crate::url::js_url_domain_to_unicode(arg(0)),
        ("url", "urlToHttpOptions") => crate::url::js_url_to_http_options(arg(0)),
        ("url", "URLPattern") => crate::url::js_url_pattern_constructor_call(arg(0), arg(1)),
        ("url", "Url") => crate::url::js_url_legacy_url_new(),
        ("url", "format") => crate::url::js_url_format(arg(0), arg(1)),
        ("url", "parse") => crate::url::js_url_legacy_parse(arg(0), arg(1), arg(2)),
        ("url", "resolve") => crate::url::js_url_legacy_resolve(arg(0), arg(1)),
        ("url", "resolveObject") => crate::url::js_url_legacy_resolve_object(arg(0), arg(1)),

        // ── punycode module (deprecated, #2513) ──
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}
