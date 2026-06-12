use super::*;

const fn cr(
    method: &'static str,
    runtime: &'static str,
    args: &'static [NativeArgKind],
    ret: NativeRetKind,
) -> NativeModSig {
    NativeModSig {
        module: "http",
        has_receiver: true,
        method,
        class_filter: Some("ClientRequest"),
        runtime,
        args,
        ret,
    }
}

pub(super) const HTTP_CLIENT_ROWS: &[NativeModSig] = &[
    // ========== node:http / node:https client (issue #769) ==========
    // `http.request(url_or_options, cb)` / `http.get(url_or_options, cb)`
    // and their `https.*` variants. Runtime impls live in
    // `crates/perry-stdlib/src/http.rs` and have been declared in the FFI
    // table for a while, but no `NativeModSig` entries existed — so user
    // code calling `http.request(...)` fell through to the unknown-method
    // path and got back `TAG_UNDEFINED`. Return is a `ClientRequest`
    // handle; the let-stmt arm in `crates/perry-hir/src/lower.rs` tags
    // the binding so `req.on/.end/.write/...` dispatch via the
    // class-filtered entries below.
    // #3226/#3227/#3228 — client factory overloads. Node accepts
    // `request(url[, cb])`, `request(options[, cb])`, and
    // `request(url, options[, cb])` (same for `get`). A fixed
    // `[NA_F64, NA_PTR]` shape mis-routed the options object into the
    // callback slot and dropped the real callback in the three-arg form.
    // Pass every user arg as a JS array (`NA_VARARGS`, mirroring the
    // `listen()` rows) and let the runtime `*_overload` entry points
    // resolve `(url, options, callback)` by value type. Runtime impls
    // live in `crates/perry-ext-http/src/lib.rs`.
    NativeModSig {
        module: "http",
        has_receiver: false,
        method: "request",
        class_filter: None,
        runtime: "js_http_request_overload",
        args: &[NA_VARARGS],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: false,
        method: "get",
        class_filter: None,
        runtime: "js_http_get_overload",
        args: &[NA_VARARGS],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "https",
        has_receiver: false,
        method: "request",
        class_filter: None,
        runtime: "js_https_request_overload",
        args: &[NA_VARARGS],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "https",
        has_receiver: false,
        method: "get",
        class_filter: None,
        runtime: "js_https_get_overload",
        args: &[NA_VARARGS],
        ret: NR_PTR,
    },
    // #3712 — module-level header-validation / parser-proxy helpers. Runtime
    // impls live in `crates/perry-runtime/src/object/native_module_dispatch.rs`.
    // `validateHeaderName`/`validateHeaderValue` throw Node-shaped error codes
    // on invalid input; the two setters are deterministic no-ops returning
    // undefined. Args are passed as NaN-boxed f64 (the helpers coerce/probe
    // them), return is the NaN-boxed undefined value.
    NativeModSig {
        module: "http",
        has_receiver: false,
        method: "validateHeaderName",
        class_filter: None,
        runtime: "js_http_validate_header_name",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: false,
        method: "validateHeaderValue",
        class_filter: None,
        runtime: "js_http_validate_header_value",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: false,
        method: "setMaxIdleHTTPParsers",
        class_filter: None,
        runtime: "js_http_setter_noop",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: false,
        method: "setGlobalProxyFromEnv",
        class_filter: None,
        runtime: "js_http_setter_noop",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: false,
        method: "_connectionListener",
        class_filter: None,
        runtime: "js_http_connection_listener_noop",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // ClientRequest instance methods (`req.on/.end/.write/.setHeader/.setTimeout`).
    // Shared between `http` and `https` factories — both register the
    // returned binding under module `"http"` in the HIR class table.
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "on",
        class_filter: Some("ClientRequest"),
        runtime: "js_http_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "end",
        class_filter: Some("ClientRequest"),
        // #4909: route to the callback-aware end so `req.end(chunk, cb)` /
        // `req.end(cb)` fire their callback (and the queued write callbacks
        // + `'finish'`) in Node's flush order. arg2/arg3 carry the
        // `(encoding?, callback?)` tail.
        runtime: "js_http_client_request_end_full",
        args: &[NA_F64, NA_JSV, NA_JSV],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "write",
        class_filter: Some("ClientRequest"),
        // #4909: pass the trailing `(encoding?, callback?)` args as raw
        // JSValues so the runtime can queue the write callback and return a
        // real boolean (NR_F64 → NaN-boxed bool) for backpressure. The
        // previous `js_http_client_request_write` (NA_F64 only / NR_PTR)
        // dropped the callback and returned the always-truthy handle, so
        // `while (req.write(buf))` producer loops never terminated.
        runtime: "js_http_client_request_write_full",
        args: &[NA_F64, NA_JSV, NA_JSV],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "setHeader",
        class_filter: Some("ClientRequest"),
        runtime: "js_http_set_header",
        args: &[NA_STR, NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "setTimeout",
        class_filter: Some("ClientRequest"),
        // #4909: the callback arg registers as a `'timeout'` listener and a
        // real timer is armed (previously the delay was only stored, so
        // `req.setTimeout(n, cb)` on a never-responding server hung forever).
        runtime: "js_http_set_timeout_full",
        args: &[NA_F64, NA_JSV],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "listenerCount",
        class_filter: Some("ClientRequest"),
        runtime: "js_http_client_request_listener_count",
        args: &[NA_STR],
        ret: NR_F64,
    },
    cr(
        "getHeader",
        "js_http_client_request_get_header",
        &[NA_STR],
        NR_F64,
    ),
    cr(
        "hasHeader",
        "js_http_client_request_has_header",
        &[NA_STR],
        NR_F64,
    ),
    cr(
        "removeHeader",
        "js_http_client_request_remove_header",
        &[NA_STR],
        NR_F64,
    ),
    cr(
        "getHeaderNames",
        "js_http_client_request_get_header_names",
        &[],
        NR_F64,
    ),
    cr(
        "getHeaders",
        "js_http_client_request_get_headers",
        &[],
        NR_F64,
    ),
    cr(
        "getRawHeaderNames",
        "js_http_client_request_get_raw_header_names",
        &[],
        NR_F64,
    ),
    cr("abort", "js_http_client_request_abort", &[], NR_F64),
    cr(
        "destroy",
        "js_http_client_request_destroy",
        &[NA_F64],
        NR_PTR,
    ),
    cr(
        "flushHeaders",
        "js_http_client_request_flush_headers",
        &[NA_F64, NA_F64],
        NR_F64,
    ),
    cr(
        "cork",
        "js_http_client_request_noop_undefined",
        &[NA_F64, NA_F64],
        NR_F64,
    ),
    cr(
        "uncork",
        "js_http_client_request_noop_undefined",
        &[NA_F64, NA_F64],
        NR_F64,
    ),
    cr(
        "setNoDelay",
        "js_http_client_request_noop_undefined",
        &[NA_F64, NA_F64],
        NR_F64,
    ),
    cr(
        "setSocketKeepAlive",
        "js_http_client_request_noop_undefined",
        &[NA_F64, NA_F64],
        NR_F64,
    ),
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_method",
        class_filter: Some("ClientRequest"),
        runtime: "js_http_client_request_method",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_protocol",
        class_filter: Some("ClientRequest"),
        runtime: "js_http_client_request_protocol",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_host",
        class_filter: Some("ClientRequest"),
        runtime: "js_http_client_request_host",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_path",
        class_filter: Some("ClientRequest"),
        runtime: "js_http_client_request_path",
        args: &[],
        ret: NR_STR,
    },
    cr(
        "__get_aborted",
        "js_http_client_request_aborted",
        &[],
        NR_F64,
    ),
    cr(
        "__get_destroyed",
        "js_http_client_request_destroyed",
        &[],
        NR_F64,
    ),
    cr(
        "__get_finished",
        "js_http_client_request_finished",
        &[],
        NR_F64,
    ),
    cr(
        "__get_reusedSocket",
        "js_http_client_request_reused_socket",
        &[],
        NR_F64,
    ),
    cr(
        "__get_maxHeadersCount",
        "js_http_client_request_max_headers_count",
        &[],
        NR_F64,
    ),
    cr(
        "__get_writableEnded",
        "js_http_client_request_writable_ended",
        &[],
        NR_F64,
    ),
    cr(
        "__get_writableFinished",
        "js_http_client_request_writable_finished",
        &[],
        NR_F64,
    ),
    cr("__get_socket", "js_http_client_request_socket", &[], NR_F64),
    cr(
        "__get_connection",
        "js_http_client_request_socket",
        &[],
        NR_F64,
    ),
    // ========== http.Agent / https.Agent (issue #2129) ==========
    // `new http.Agent(options?)` / `new https.Agent(options?)` — registered
    // via the Member-callee path in lower/expr_new.rs (mirrors the
    // `new net.Socket()` route). Both classes share the same instance
    // surface; only the constructor differs (default protocol).
    NativeModSig {
        module: "http",
        has_receiver: false,
        method: "Agent",
        class_filter: None,
        runtime: "js_http_agent_new",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "https",
        has_receiver: false,
        method: "Agent",
        class_filter: None,
        runtime: "js_https_agent_new",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    // Agent instance methods. Most are chainable no-ops today — Perry
    // doesn't pool sockets, but Node's test suite asserts the methods
    // exist and don't throw. `getName(options)` is the one method whose
    // exact output the tests check.
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "getName",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_get_name",
        args: &[NA_F64],
        ret: NR_STR,
    },
    // #2154: real `destroy()` flips the `destroyed` flag (no-op
    // pre-#2154 — the agent had nothing to destroy because it owned no
    // sockets). Still chainable: returns the receiver handle.
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "destroy",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_destroy",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "keepSocketAlive",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "reuseSocket",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    // Property accessors as `__get_<name>` synthetic methods. The HIR
    // rewrites bare `agent.maxSockets` reads to `agent.__get_maxSockets()`
    // when the receiver is tagged as ("http"|"https", "Agent").
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_maxSockets",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_max_sockets",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_maxFreeSockets",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_max_free_sockets",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_maxTotalSockets",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_max_total_sockets",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_keepAliveMsecs",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_keep_alive_msecs",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_keepAlive",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_keep_alive",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_protocol",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_protocol",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_defaultPort",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_default_port",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__set_protocol",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_set_protocol",
        args: &[NA_STR],
        ret: NR_VOID,
    },
    // Bare-name dispatch for the same property reads. Mirrors the
    // belt-and-braces approach used by IncomingMessage's `statusCode` /
    // `headers` rows below — covers sites where the HIR rewrite to
    // `__get_<prop>` doesn't fire (e.g. an agent assigned through a
    // local before the property read).
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "maxSockets",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_max_sockets",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "maxFreeSockets",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_max_free_sockets",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "maxTotalSockets",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_max_total_sockets",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "keepAliveMsecs",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_keep_alive_msecs",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "keepAlive",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_keep_alive",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "protocol",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_protocol",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "defaultPort",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_default_port",
        args: &[],
        ret: NR_F64,
    },
    // ========== http.Agent / https.Agent (issue #2154) ==========
    // Extra surface the #2129 first PR didn't cover: sockets /
    // freeSockets / requests accessors (return empty objects),
    // destroyed getter + destroy(), tunable property setters with
    // RangeError-throwing validation, and createConnection /
    // createSocket override storage.
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_sockets",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_sockets",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "sockets",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_sockets",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_freeSockets",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_free_sockets",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "freeSockets",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_free_sockets",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_requests",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_requests",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "requests",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_requests",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_destroyed",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_destroyed",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "destroyed",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_destroyed",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__set_maxSockets",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_set_max_sockets",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__set_maxFreeSockets",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_set_max_free_sockets",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__set_maxTotalSockets",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_set_max_total_sockets",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__set_keepAlive",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_set_keep_alive",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__set_keepAliveMsecs",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_set_keep_alive_msecs",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__set_createConnection",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_set_create_connection",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__set_createSocket",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_set_create_socket",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_createConnection",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_create_connection",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_createSocket",
        class_filter: Some("Agent"),
        runtime: "js_http_agent_create_socket",
        args: &[],
        ret: NR_PTR,
    },
];
