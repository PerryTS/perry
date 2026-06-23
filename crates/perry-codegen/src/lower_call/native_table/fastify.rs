use super::*;

pub(super) const FASTIFY_ROWS: &[NativeModSig] = &[
    // ========== Fastify HTTP Framework ==========
    NativeModSig {
        module: "fastify",
        has_receiver: false,
        method: "default",
        class_filter: None,
        runtime: "js_fastify_create_with_opts",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "get",
        class_filter: None,
        runtime: "js_fastify_get",
        args: &[NA_STR, NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "post",
        class_filter: None,
        runtime: "js_fastify_post",
        args: &[NA_STR, NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "put",
        class_filter: None,
        runtime: "js_fastify_put",
        args: &[NA_STR, NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "delete",
        class_filter: None,
        runtime: "js_fastify_delete",
        args: &[NA_STR, NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "patch",
        class_filter: None,
        runtime: "js_fastify_patch",
        args: &[NA_STR, NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "head",
        class_filter: None,
        runtime: "js_fastify_head",
        args: &[NA_STR, NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "options",
        class_filter: None,
        runtime: "js_fastify_options",
        args: &[NA_STR, NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "all",
        class_filter: None,
        runtime: "js_fastify_all",
        args: &[NA_STR, NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "route",
        class_filter: None,
        runtime: "js_fastify_route",
        args: &[NA_STR, NA_STR, NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "addHook",
        class_filter: None,
        runtime: "js_fastify_add_hook",
        args: &[NA_STR, NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "setErrorHandler",
        class_filter: None,
        runtime: "js_fastify_set_error_handler",
        args: &[NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "register",
        class_filter: None,
        runtime: "js_fastify_register",
        args: &[NA_PTR, NA_F64],
        ret: NR_I32,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "listen",
        class_filter: None,
        runtime: "js_fastify_listen",
        args: &[NA_F64, NA_PTR],
        ret: NR_VOID,
    },
    // `app.close()` — shut down every server bound to this
    // FastifyApp. Pre-fix, this method had no entry in the dispatch
    // table at all, so codegen for the `NativeMethodCall` shape fell
    // through to the "unknown native method" arm and emitted a no-op
    // 0.0 return. With `listen()` now non-blocking, the program
    // doesn't exit until something marks the server as no-longer-
    // listening — `app.close()` is how user code does that. The
    // runtime fn walks the handle registry for matching
    // `FastifyServerHandle` rows and clears the listening flag.
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "close",
        class_filter: None,
        runtime: "js_fastify_app_close",
        args: &[],
        ret: NR_VOID,
    },
    // #1113 — `app.server` (property access, lowered to a zero-arg
    // NativeMethodCall by the HIR property-as-method path). Pre-fix,
    // this fell through to the "unknown native method" sentinel
    // (`double 0.0`), so `typeof app.server` was `"number"` and
    // `app.server.on("upgrade", …)` threw `(number).on is not a
    // function` at boot. Returns the same FastifyApp handle id
    // pointer-tagged (NR_PTR) so `typeof` resolves to `"object"` and
    // `.on(…)` routes through HANDLE_METHOD_DISPATCH back into the
    // FastifyApp arm. See `js_fastify_app_server` in
    // perry-ext-fastify (crates/perry-ext-fastify/src/app.rs) for the
    // rationale — fastify is served by that external crate (the in-stdlib
    // adapter was removed).
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "server",
        class_filter: None,
        runtime: "js_fastify_app_server",
        args: &[],
        ret: NR_PTR,
    },
    // #1113 — `app.server.on(event, cb)`. `app.server` returns the
    // FastifyApp handle (pointer-tagged), so `.on(…)` lowers as a
    // 2-arg NativeMethodCall on the same module. The runtime fn
    // records the callback for the recognised event names
    // (currently just `"upgrade"`); other names are silently
    // accepted so handlers like `app.server.on("error", …)`
    // registered at boot don't crash. Full EventEmitter parity is a
    // tracked follow-up.
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "on",
        class_filter: None,
        runtime: "js_fastify_app_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_VOID,
    },
    // Fastify request methods
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "method",
        class_filter: None,
        runtime: "js_fastify_req_method",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "url",
        class_filter: None,
        runtime: "js_fastify_req_url",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "params",
        class_filter: None,
        // Returns the parsed path-params object (e.g. `{id: "42"}` for /users/:id),
        // not the raw JSON string — `request.params.id` must be the value, not
        // undefined. `js_fastify_req_params` (string) is still available via
        // the lower-level FFI but isn't reachable from TypeScript.
        runtime: "js_fastify_req_params_object",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "param",
        class_filter: None,
        runtime: "js_fastify_req_param",
        args: &[NA_JSV],
        ret: NR_STR,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "query",
        class_filter: None,
        runtime: "js_fastify_req_query_object",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "rawBody",
        class_filter: None,
        runtime: "js_fastify_req_body",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "headers",
        class_filter: None,
        runtime: "js_fastify_req_headers",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "header",
        class_filter: None,
        runtime: "js_fastify_req_header",
        args: &[NA_JSV],
        ret: NR_STR,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "user",
        class_filter: None,
        runtime: "js_fastify_req_get_user_data",
        args: &[],
        ret: NR_F64,
    },
    // Fastify reply methods
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "status",
        class_filter: None,
        runtime: "js_fastify_reply_status",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    // `reply.code(N)` is an alias for `reply.status(N)` in npm Fastify. Without
    // this row, `reply.code(201)` silently no-op'd and the HTTP status stayed 200.
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "code",
        class_filter: None,
        runtime: "js_fastify_reply_status",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "send",
        class_filter: None,
        runtime: "js_fastify_reply_send",
        args: &[NA_F64],
        ret: NR_I32,
    },
    // `reply.header(name, value)` — chainable. Without this dispatch
    // entry, every `reply.header(...)` call silently no-op'd; the runtime
    // function existed in `runtime_decls.rs` but no NativeModSig routed
    // user code at it. CORS hooks, Cache-Control, and content-type
    // overrides all evaporated.
    //
    // `ret: NR_PTR` is critical — the Rust impl returns `Handle` (i64).
    // Previously `NR_F64` caused chained `.header(...).send(...)` to read
    // an uninitialized XMM0/D0 register as the receiver, producing
    // `(number).send is not a function` errors (#1048).
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "header",
        class_filter: None,
        runtime: "js_fastify_reply_header",
        args: &[NA_JSV, NA_JSV],
        ret: NR_PTR,
    },
    // `reply.type(value)` — Fastify alias for setting `content-type`.
    // Routes to `js_fastify_reply_type` (thin wrapper over reply_header).
    // `ret: NR_PTR` for the same reason as `reply.header` above (#1048).
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "type",
        class_filter: None,
        runtime: "js_fastify_reply_type",
        args: &[NA_JSV],
        ret: NR_PTR,
    },
    // Fastify context methods (Hono-style)
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "text",
        class_filter: None,
        runtime: "js_fastify_ctx_text",
        args: &[NA_JSV, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "html",
        class_filter: None,
        runtime: "js_fastify_ctx_html",
        args: &[NA_JSV, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "redirect",
        class_filter: None,
        runtime: "js_fastify_ctx_redirect",
        args: &[NA_JSV, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "json",
        class_filter: None,
        runtime: "js_fastify_ctx_json",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "fastify",
        has_receiver: true,
        method: "body",
        class_filter: None,
        runtime: "js_fastify_req_json",
        args: &[],
        ret: NR_F64,
    },
];
