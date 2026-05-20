//! Native stdlib module dispatch table (`NATIVE_MODULE_TABLE`) + the
//! arg/return kind types and manifest-introspection helpers.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. Holds the ~5k-row static dispatch table that
//! maps `(module, method)` to a runtime symbol + arg/return coercion
//! recipe, plus `iter_native_module_table` (consumed by `lib.rs`'s
//! public manifest API). The dispatch *consumers* (`native_module_lookup`,
//! `lower_native_module_dispatch`) stay in `mod.rs` and import the
//! `pub(super)` items below.

// ============================================================================
// Native stdlib module dispatch (fastify, mysql2, ws, pg, ioredis, mongodb,
// better-sqlite3, etc.). Ported from the old Cranelift codegen's dispatch
// table that was lost in the v0.5.0 LLVM cutover.
// ============================================================================

/// How each argument should be coerced before passing to the runtime fn.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum NativeArgKind {
    /// NaN-boxed f64 — pass as-is (objects, generic JSValues).
    F64,
    /// NaN-boxed string → extract raw i64 pointer via js_get_string_pointer_unified.
    /// Use for Rust signatures like `*const StringHeader`.
    StrPtr,
    /// NaN-boxed closure/pointer → unbox to i64 via the standard mask.
    PtrI64,
    /// Pass the NaN-boxed JSValue bits as-is (bitcast f64 → i64, no
    /// unboxing). Use for Rust signatures where the function receives
    /// `name: i64` and internally calls `string_from_nanboxed(name)` or
    /// similar — the callee expects the full NaN-boxed value, not an
    /// unboxed raw pointer. Common pattern in fastify context methods.
    JsvalI64,
    /// Pack all remaining user-supplied args (from this position onward)
    /// into a freshly allocated JS array and pass a single i64
    /// `*const ArrayHeader` to the runtime. Must be the last entry in
    /// `sig.args`. When the user supplies no args at this position, an
    /// empty array is passed (a real allocated header, not a null
    /// pointer — callees that walk `*arr_ptr` unconditionally are safe).
    /// Used for variadic JS-side call shapes like
    /// `stmt.all(...params)` / `stmt.run(...)` / `stmt.get(...)` that
    /// the runtime consumes as a single `*const ArrayHeader`.
    VarArgsAsArray,
}

/// What the runtime function returns.
#[derive(Copy, Clone, Debug)]
pub(super) enum NativeRetKind {
    /// Returns i64 handle → NaN-box as POINTER.
    Ptr,
    /// Returns `*mut StringHeader` → NaN-box as STRING. Use for runtime
    /// functions whose Rust signature returns a raw string pointer; the
    /// caller (and `JSON.stringify`, string-comparison, etc.) needs the
    /// STRING_TAG to recognize it as a string rather than a heap object.
    Str,
    /// Returns `*mut StringHeader` containing JSON → automatically pipe
    /// through `js_json_parse` so the user-visible value is a parsed
    /// object/array, not the JSON-encoded string. Symmetric to `NA_JSON`
    /// on the argument side (#915). Null pointer → TAG_NULL so a failed
    /// verify (`jwt.verify` on bad signature) still reads as `null`
    /// rather than dereferencing a dangling pointer. Issue #927.
    ObjFromJsonStr,
    /// Returns `*mut BigIntHeader` → NaN-box as BIGINT (0x7FFA tag). Use
    /// for functions like `parseEther`/`parseUnits` that return bigint values.
    BigInt,
    /// Returns f64 → pass through (NaN-boxed JSValue).
    F64,
    /// Returns i32 → ignored, return TAG_UNDEFINED.
    I32Void,
    /// Returns void → return TAG_UNDEFINED.
    Void,
}

#[derive(Copy, Clone, Debug)]
pub(super) struct NativeModSig {
    pub(super) module: &'static str,
    pub(super) has_receiver: bool,
    pub(super) method: &'static str,
    /// Optional class_name filter. When Some, only matches if the HIR's
    /// class_name equals this value (e.g. "Pool" vs "Connection" for mysql2).
    /// When None, matches regardless of class_name.
    pub(super) class_filter: Option<&'static str>,
    pub(super) runtime: &'static str,
    pub(super) args: &'static [NativeArgKind],
    pub(super) ret: NativeRetKind,
}

// Short aliases to keep the table compact without wildcard imports
// (wildcard would clash with crate::types::* names like I64, DOUBLE).
const NA_F64: NativeArgKind = NativeArgKind::F64;
const NA_STR: NativeArgKind = NativeArgKind::StrPtr;
const NA_PTR: NativeArgKind = NativeArgKind::PtrI64;
const NA_JSV: NativeArgKind = NativeArgKind::JsvalI64;
const NA_VARARGS: NativeArgKind = NativeArgKind::VarArgsAsArray;
const NR_PTR: NativeRetKind = NativeRetKind::Ptr;
const NR_STR: NativeRetKind = NativeRetKind::Str;
const NR_OBJ_FROM_JSON_STR: NativeRetKind = NativeRetKind::ObjFromJsonStr;
const NR_BIGINT: NativeRetKind = NativeRetKind::BigInt;
const NR_F64: NativeRetKind = NativeRetKind::F64;
const NR_I32: NativeRetKind = NativeRetKind::I32Void;
const NR_VOID: NativeRetKind = NativeRetKind::Void;

/// Static dispatch table for native stdlib modules. Each entry maps
/// `(module, has_receiver, method)` → runtime function, with per-arg
/// coercion rules and return-value boxing.
///
/// The receiver (when `has_receiver = true`) is always NaN-unboxed to
/// an i64 pointer and passed as the first argument.
pub(super) const NATIVE_MODULE_TABLE: &[NativeModSig] = &[
    // ========== Node URL ==========
    NativeModSig {
        module: "url",
        has_receiver: false,
        method: "fileURLToPath",
        class_filter: None,
        runtime: "js_url_file_url_to_path",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "url",
        has_receiver: false,
        method: "pathToFileURL",
        class_filter: None,
        runtime: "js_url_path_to_file_url",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "url",
        has_receiver: false,
        method: "domainToASCII",
        class_filter: None,
        runtime: "js_url_domain_to_ascii",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "url",
        has_receiver: false,
        method: "domainToUnicode",
        class_filter: None,
        runtime: "js_url_domain_to_unicode",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "url",
        has_receiver: false,
        method: "urlToHttpOptions",
        class_filter: None,
        runtime: "js_url_to_http_options",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "url",
        has_receiver: false,
        method: "format",
        class_filter: None,
        runtime: "js_url_format",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "url",
        has_receiver: false,
        method: "parse",
        class_filter: None,
        runtime: "js_url_legacy_parse",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "url",
        has_receiver: false,
        method: "resolve",
        class_filter: None,
        runtime: "js_url_legacy_resolve",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    // ========== Node util ==========
    NativeModSig {
        module: "util",
        has_receiver: false,
        method: "inspect",
        class_filter: None,
        runtime: "js_util_inspect",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util",
        has_receiver: false,
        method: "isDeepStrictEqual",
        class_filter: None,
        runtime: "js_util_is_deep_strict_equal",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util",
        has_receiver: false,
        method: "stripVTControlCharacters",
        class_filter: None,
        runtime: "js_util_strip_vt_control_characters",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util/types",
        has_receiver: false,
        method: "isPromise",
        class_filter: None,
        runtime: "js_util_types_is_promise",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util/types",
        has_receiver: false,
        method: "isArrayBuffer",
        class_filter: None,
        runtime: "js_util_types_is_array_buffer",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util/types",
        has_receiver: false,
        method: "isAnyArrayBuffer",
        class_filter: None,
        runtime: "js_util_types_is_array_buffer",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util/types",
        has_receiver: false,
        method: "isArrayBufferView",
        class_filter: None,
        runtime: "js_util_types_is_array_buffer_view",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util/types",
        has_receiver: false,
        method: "isTypedArray",
        class_filter: None,
        runtime: "js_util_types_is_typed_array",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util/types",
        has_receiver: false,
        method: "isUint8Array",
        class_filter: None,
        runtime: "js_util_types_is_uint8_array",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util/types",
        has_receiver: false,
        method: "isUint16Array",
        class_filter: None,
        runtime: "js_util_types_is_uint16_array",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util/types",
        has_receiver: false,
        method: "isInt32Array",
        class_filter: None,
        runtime: "js_util_types_is_int32_array",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util/types",
        has_receiver: false,
        method: "isFloat64Array",
        class_filter: None,
        runtime: "js_util_types_is_float64_array",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util/types",
        has_receiver: false,
        method: "isMap",
        class_filter: None,
        runtime: "js_util_types_is_map",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util/types",
        has_receiver: false,
        method: "isSet",
        class_filter: None,
        runtime: "js_util_types_is_set",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util/types",
        has_receiver: false,
        method: "isDate",
        class_filter: None,
        runtime: "js_util_types_is_date",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "util/types",
        has_receiver: false,
        method: "isRegExp",
        class_filter: None,
        runtime: "js_util_types_is_reg_exp",
        args: &[NA_F64],
        ret: NR_F64,
    },
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
    // perry-stdlib/src/fastify/app.rs for the rationale and the gap
    // still owed (hyper accept-loop doesn't yet dispatch incoming
    // `Upgrade:` requests to the registered handlers).
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
    // ========== MySQL2 ==========
    NativeModSig {
        module: "mysql2",
        has_receiver: false,
        method: "createConnection",
        class_filter: None,
        runtime: "js_mysql2_create_connection",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2",
        has_receiver: false,
        method: "createPool",
        class_filter: None,
        runtime: "js_mysql2_create_pool",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: false,
        method: "createConnection",
        class_filter: None,
        runtime: "js_mysql2_create_connection",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: false,
        method: "createPool",
        class_filter: None,
        runtime: "js_mysql2_create_pool",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    // mysql2 Pool-specific methods (class_filter: Some("Pool"))
    NativeModSig {
        module: "mysql2",
        has_receiver: true,
        method: "query",
        class_filter: Some("Pool"),
        runtime: "js_mysql2_pool_query",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2",
        has_receiver: true,
        method: "execute",
        class_filter: Some("Pool"),
        runtime: "js_mysql2_pool_execute",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2",
        has_receiver: true,
        method: "end",
        class_filter: Some("Pool"),
        runtime: "js_mysql2_pool_end",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: true,
        method: "query",
        class_filter: Some("Pool"),
        runtime: "js_mysql2_pool_query",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: true,
        method: "execute",
        class_filter: Some("Pool"),
        runtime: "js_mysql2_pool_execute",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: true,
        method: "end",
        class_filter: Some("Pool"),
        runtime: "js_mysql2_pool_end",
        args: &[],
        ret: NR_PTR,
    },
    // mysql2 PoolConnection-specific methods
    NativeModSig {
        module: "mysql2",
        has_receiver: true,
        method: "query",
        class_filter: Some("PoolConnection"),
        runtime: "js_mysql2_pool_connection_query",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2",
        has_receiver: true,
        method: "execute",
        class_filter: Some("PoolConnection"),
        runtime: "js_mysql2_pool_connection_execute",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: true,
        method: "query",
        class_filter: Some("PoolConnection"),
        runtime: "js_mysql2_pool_connection_query",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: true,
        method: "execute",
        class_filter: Some("PoolConnection"),
        runtime: "js_mysql2_pool_connection_execute",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    // mysql2 generic instance methods (Connection fallback, class_filter: None)
    NativeModSig {
        module: "mysql2",
        has_receiver: true,
        method: "query",
        class_filter: None,
        runtime: "js_mysql2_connection_query",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2",
        has_receiver: true,
        method: "execute",
        class_filter: None,
        runtime: "js_mysql2_connection_execute",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2",
        has_receiver: true,
        method: "end",
        class_filter: None,
        runtime: "js_mysql2_connection_end",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2",
        has_receiver: true,
        method: "getConnection",
        class_filter: None,
        runtime: "js_mysql2_pool_get_connection",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2",
        has_receiver: true,
        method: "release",
        class_filter: None,
        runtime: "js_mysql2_pool_connection_release",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "mysql2",
        has_receiver: true,
        method: "beginTransaction",
        class_filter: None,
        runtime: "js_mysql2_connection_begin_transaction",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2",
        has_receiver: true,
        method: "commit",
        class_filter: None,
        runtime: "js_mysql2_connection_commit",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2",
        has_receiver: true,
        method: "rollback",
        class_filter: None,
        runtime: "js_mysql2_connection_rollback",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: true,
        method: "query",
        class_filter: None,
        runtime: "js_mysql2_connection_query",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: true,
        method: "execute",
        class_filter: None,
        runtime: "js_mysql2_connection_execute",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: true,
        method: "end",
        class_filter: None,
        runtime: "js_mysql2_connection_end",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: true,
        method: "getConnection",
        class_filter: None,
        runtime: "js_mysql2_pool_get_connection",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: true,
        method: "release",
        class_filter: None,
        runtime: "js_mysql2_pool_connection_release",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: true,
        method: "beginTransaction",
        class_filter: None,
        runtime: "js_mysql2_connection_begin_transaction",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: true,
        method: "commit",
        class_filter: None,
        runtime: "js_mysql2_connection_commit",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mysql2/promise",
        has_receiver: true,
        method: "rollback",
        class_filter: None,
        runtime: "js_mysql2_connection_rollback",
        args: &[],
        ret: NR_PTR,
    },
    // ========== PostgreSQL (pg) ==========
    // `new Client(config)` and `new Pool(config)` are dispatched by
    // `lower_builtin_new` (sync constructors that produce real handles).
    // The factory-style entries below stay wired for `pg.connect(config)` /
    // `pg.Pool(config)` patterns that some npm code uses.
    NativeModSig {
        module: "pg",
        has_receiver: false,
        method: "connect",
        class_filter: None,
        runtime: "js_pg_connect",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "pg",
        has_receiver: false,
        method: "Pool",
        class_filter: None,
        runtime: "js_pg_create_pool",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    // `client.connect()` — async, opens the TCP connection on a handle that
    // `new Client(config)` previously created in the pre-connect state.
    // No-op if the handle was already connected (e.g. came from the
    // older `pg.connect(config)` factory). Class-filtered to Client so
    // `pool.connect()` (which has different semantics — checkout a pooled
    // connection — not yet implemented) doesn't accidentally land here.
    NativeModSig {
        module: "pg",
        has_receiver: true,
        method: "connect",
        class_filter: Some("Client"),
        runtime: "js_pg_client_connect",
        args: &[],
        ret: NR_PTR,
    },
    // Pool-specific query/end — different runtime fns from the Client paths.
    // Pre-existing dispatch was unfiltered and routed both Pool and Client
    // through the Client query/end fns (latent bug: pool.query() against a
    // Pool handle would fail because js_pg_client_query expects a Connection
    // handle). Class-filtered Pool rows take precedence over the unfiltered
    // Client/default rows below thanks to native_module_lookup's two-pass
    // search (exact class_filter match first, then None fallback).
    NativeModSig {
        module: "pg",
        has_receiver: true,
        method: "query",
        class_filter: Some("Pool"),
        runtime: "js_pg_pool_query",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "pg",
        has_receiver: true,
        method: "end",
        class_filter: Some("Pool"),
        runtime: "js_pg_pool_end",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "pg",
        has_receiver: true,
        method: "query",
        class_filter: None,
        runtime: "js_pg_client_query",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "pg",
        has_receiver: true,
        method: "end",
        class_filter: None,
        runtime: "js_pg_client_end",
        args: &[],
        ret: NR_PTR,
    },
    // ========== ioredis ==========
    // NB: every row was previously emitting `js_redis_*` symbols which don't
    // exist in perry-stdlib (the actual fns are `js_ioredis_*`). The bug was
    // dormant because pre-#187 no codepath could land on a real Redis handle
    // — `new Redis()` fell into the empty-placeholder branch in lower_new and
    // every method dispatched against junk. With the v0.5.262 ctor branch
    // making the receiver real, these rows have to point at the actual
    // runtime symbols. Fixed throughout below.
    NativeModSig {
        module: "ioredis",
        has_receiver: false,
        method: "createClient",
        class_filter: None,
        // npm `redis`'s createClient(opts) and ioredis's `new Redis(opts)` are
        // shape-compatible (both produce a client; opts is host/port/etc.).
        // js_ioredis_new ignores its arg and reads env vars — same behavior.
        runtime: "js_ioredis_new",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ioredis",
        has_receiver: true,
        method: "set",
        class_filter: None,
        runtime: "js_ioredis_set",
        args: &[NA_STR, NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ioredis",
        has_receiver: true,
        method: "get",
        class_filter: None,
        runtime: "js_ioredis_get",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ioredis",
        has_receiver: true,
        method: "del",
        class_filter: None,
        runtime: "js_ioredis_del",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ioredis",
        has_receiver: true,
        method: "exists",
        class_filter: None,
        runtime: "js_ioredis_exists",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ioredis",
        has_receiver: true,
        method: "incr",
        class_filter: None,
        runtime: "js_ioredis_incr",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ioredis",
        has_receiver: true,
        method: "decr",
        class_filter: None,
        runtime: "js_ioredis_decr",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ioredis",
        has_receiver: true,
        method: "expire",
        class_filter: None,
        runtime: "js_ioredis_expire",
        args: &[NA_STR, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ioredis",
        has_receiver: true,
        method: "quit",
        class_filter: None,
        runtime: "js_ioredis_quit",
        args: &[],
        ret: NR_PTR,
    },
    // Issue #605 — npm `redis`'s `client.connect()` is async. ioredis
    // auto-connects in `new Redis()` and exposes `connect()` as a no-op
    // resolved-promise that the runtime returns. Without this row,
    // `await client.connect()` from `import { createClient } from
    // "redis"` dispatches against `undefined` and raises the user-
    // facing TypeError ("Cannot read properties of undefined …").
    NativeModSig {
        module: "ioredis",
        has_receiver: true,
        method: "connect",
        class_filter: None,
        runtime: "js_ioredis_connect",
        args: &[],
        ret: NR_PTR,
    },
    // npm `redis`'s `client.disconnect()` — alias for `.quit()`.
    NativeModSig {
        module: "ioredis",
        has_receiver: true,
        method: "disconnect",
        class_filter: None,
        runtime: "js_ioredis_quit",
        args: &[],
        ret: NR_PTR,
    },
    // ========== MongoDB ==========
    // `new MongoClient(uri)` is dispatched by `lower_builtin_new` (sync ctor
    // that stores the URI). `client.connect()` opens the connection on the
    // pre-connect handle. The receiver-less factory `mongodb.connect(uri)`
    // (combines new+connect, returns Promise<Handle>) stays wired below.
    NativeModSig {
        module: "mongodb",
        has_receiver: false,
        method: "connect",
        class_filter: None,
        runtime: "js_mongodb_connect",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mongodb",
        has_receiver: true,
        method: "connect",
        class_filter: None,
        runtime: "js_mongodb_client_connect",
        args: &[],
        ret: NR_PTR,
    },
    // Symbol-name fix: every row below previously emitted a stripped-name
    // form (`js_mongodb_db`, `js_mongodb_insert_one`, etc.) but the actual
    // stdlib functions carry a `_client_` / `_db_` / `_collection_` infix
    // (`js_mongodb_client_db`, `js_mongodb_collection_insert_one`, ...).
    // Pre-#187 nobody hit it because `new MongoClient()` produced a junk
    // handle and method calls against it never linked the symbols. With the
    // v0.5.270-era ctor making the receiver real, these dispatch rows now
    // actually link — so they have to point at the real functions. Same
    // family as the v0.5.270 ioredis row fix.
    NativeModSig {
        module: "mongodb",
        has_receiver: true,
        method: "db",
        class_filter: None,
        runtime: "js_mongodb_client_db",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mongodb",
        has_receiver: true,
        method: "collection",
        class_filter: None,
        runtime: "js_mongodb_db_collection",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    // `_value` wrapper variants — every collection method that accepts an
    // object/filter arg goes through a wrapper that JSON-stringifies the
    // NaN-boxed JSValue (NA_F64) before forwarding to the existing
    // JSON-string-taking runtime fn. Without the wrapper, codegen passed
    // the JSValue f64 bits directly into a fn signed to receive a
    // *const StringHeader — every doc/filter looked like garbage and the
    // user saw "Invalid document" / "Invalid JSON".
    NativeModSig {
        module: "mongodb",
        has_receiver: true,
        method: "insertOne",
        class_filter: None,
        runtime: "js_mongodb_collection_insert_one_value",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mongodb",
        has_receiver: true,
        method: "insertMany",
        class_filter: None,
        runtime: "js_mongodb_collection_insert_many_value",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mongodb",
        has_receiver: true,
        method: "find",
        class_filter: None,
        runtime: "js_mongodb_collection_find_value",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mongodb",
        has_receiver: true,
        method: "findOne",
        class_filter: None,
        runtime: "js_mongodb_collection_find_one_value",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mongodb",
        has_receiver: true,
        method: "updateOne",
        class_filter: None,
        runtime: "js_mongodb_collection_update_one_value",
        args: &[NA_F64, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mongodb",
        has_receiver: true,
        method: "updateMany",
        class_filter: None,
        runtime: "js_mongodb_collection_update_many_value",
        args: &[NA_F64, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mongodb",
        has_receiver: true,
        method: "deleteOne",
        class_filter: None,
        runtime: "js_mongodb_collection_delete_one_value",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mongodb",
        has_receiver: true,
        method: "deleteMany",
        class_filter: None,
        runtime: "js_mongodb_collection_delete_many_value",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "mongodb",
        has_receiver: true,
        method: "countDocuments",
        class_filter: None,
        runtime: "js_mongodb_collection_count_value",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    // aggregate / createIndex / toArray runtime functions don't exist in
    // perry-stdlib yet — listed as commented-out so the dispatch table
    // doesn't reference undefined symbols. User code calling these methods
    // falls through to the unknown-method sentinel returning TAG_UNDEFINED;
    // that's better than a hard link failure for code that happens to
    // import mongodb but doesn't call the methods.
    //   NativeModSig { module: "mongodb", method: "aggregate",   ... },
    //   NativeModSig { module: "mongodb", method: "createIndex", ... },
    //   NativeModSig { module: "mongodb", method: "toArray",     ... },
    NativeModSig {
        module: "mongodb",
        has_receiver: true,
        method: "close",
        class_filter: None,
        runtime: "js_mongodb_client_close",
        args: &[],
        ret: NR_PTR,
    },
    // ========== better-sqlite3 ==========
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: false,
        method: "default",
        class_filter: None,
        runtime: "js_sqlite_open",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "prepare",
        class_filter: None,
        runtime: "js_sqlite_prepare",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    // stmt.run/get/all/iterate take JS-side variadic params. The runtime
    // consumes them as a single `*const ArrayHeader`, so VarArgsAsArray
    // packs every user-supplied arg into a real JS array before the call.
    // Pre-#339 these used `NA_F64` and the runtime had to defensively
    // bail when the high-16 bits looked like a NaN-box tag — fine for
    // the no-arg case (TAG_UNDEFINED), but `.all('a')` passed a
    // STRING-tagged f64 that also tripped the bail and the params were
    // silently dropped.
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "run",
        class_filter: None,
        runtime: "js_sqlite_stmt_run",
        args: &[NA_VARARGS],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "get",
        class_filter: None,
        runtime: "js_sqlite_stmt_get",
        args: &[NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "all",
        class_filter: None,
        runtime: "js_sqlite_stmt_all",
        args: &[NA_VARARGS],
        ret: NR_PTR,
    },
    // `stmt.raw([toggle])` — flips the statement into raw mode and
    // returns the same handle so `stmt.raw().all(...)` chains. drizzle's
    // PreparedQuery.values() relies on this; without it `stmt.raw` is
    // undefined and the call surfaces as `(number).all is not a
    // function` deeper in the chain. Refs #643. The optional `toggle`
    // arg isn't threaded through the dispatch yet (always enables);
    // extend `args` if a real downstream needs `.raw(false)`.
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "raw",
        class_filter: None,
        runtime: "js_sqlite_stmt_raw",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "exec",
        class_filter: None,
        runtime: "js_sqlite_exec",
        args: &[NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "close",
        class_filter: None,
        runtime: "js_sqlite_close",
        args: &[],
        ret: NR_VOID,
    },
    // ========== WebSocket (ws) ==========
    NativeModSig {
        module: "ws",
        has_receiver: false,
        method: "Server",
        class_filter: None,
        runtime: "js_ws_server_new",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ws",
        has_receiver: false,
        method: "WebSocket",
        class_filter: None,
        runtime: "js_ws_connect",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "on",
        class_filter: None,
        runtime: "js_ws_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "send",
        class_filter: None,
        runtime: "js_ws_send",
        args: &[NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "close",
        class_filter: None,
        runtime: "js_ws_close",
        args: &[],
        ret: NR_VOID,
    },
    // Issue #577 Phase 4 — `("ws", "Client")` instance methods.
    // The wsId delivered to `Server.on('upgrade', (req, wsId, head) => …)`
    // is NaN-boxed POINTER_TAG so unbox_to_i64 (called by the dispatch
    // helper) extracts the original integer ws_id; user code writing
    // `wsId.send("…")` / `wsId.on("message", cb)` / `wsId.close()`
    // dispatches via these class-filtered entries to the dedicated
    // i64-taking Client variants.
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "send",
        class_filter: Some("Client"),
        runtime: "js_ws_send_client_i64",
        args: &[NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "close",
        class_filter: Some("Client"),
        runtime: "js_ws_close_client_i64",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "on",
        class_filter: Some("Client"),
        runtime: "js_ws_on_client_i64",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "addListener",
        class_filter: Some("Client"),
        runtime: "js_ws_on_client_i64",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    // Server-side helpers — the user receives a client handle as a plain
    // f64 number from `wss.on('connection', (handle) => …)`, then passes
    // it back to these free functions to write/close that specific peer.
    // Without these entries the receiver-less call falls through to the
    // silent stub a few hundred lines down, evaluates the args for side
    // effects, and returns TAG_UNDEFINED — so frames silently never ship
    // (issue #136).
    NativeModSig {
        module: "ws",
        has_receiver: false,
        method: "sendToClient",
        class_filter: None,
        runtime: "js_ws_send_to_client",
        args: &[NA_F64, NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "ws",
        has_receiver: false,
        method: "closeClient",
        class_filter: None,
        runtime: "js_ws_close_client",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    // ========== Raw TCP sockets (net) + TLS ==========
    // Factory: `net.createConnection(...)` / `net.connect(...)` returns
    // a Socket handle. Supports both Node overloads:
    //   - `net.connect(port, host)` — positional
    //   - `net.connect({ host, port }, cb?)` — options object (issue #770)
    // Both args are passed through as `NA_F64` so the runtime sees the
    // raw NaN-boxed bits and can discriminate the overload by tag.
    // Pre-#770 the second arg was `NA_STR`, which silently corrupted the
    // options-object call site: codegen tried to coerce the callback
    // function to a string pointer, the runtime read garbage bytes as
    // the host name, and `getaddrinfo`'s internal `CString::new()`
    // panicked with "file name contained an unexpected NUL byte".
    //
    // HIR lowering at crates/perry-hir/src/lower.rs registers the
    // return value as class "Socket" so subsequent methods dispatch via
    // the class_filter entries below.
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "createConnection",
        class_filter: None,
        runtime: "js_net_socket_connect",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_PTR,
    },
    // Factory alias: `net.connect(...)` is the spec'd alias for
    // `net.createConnection(...)`. Pre-issue-#422 only the
    // `createConnection` form was wired; `net.connect(...)` fell through
    // to the receiver-less unknown-method path which returns
    // TAG_UNDEFINED, so user code reading `typeof net.connect(...)`
    // saw `"undefined"` (issue #422 reproducer 3).
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "connect",
        class_filter: None,
        runtime: "js_net_socket_connect",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_PTR,
    },
    // Constructor: `new net.Socket()` allocates an unconnected socket
    // handle whose TCP connection is deferred until `sock.connect(port,
    // host)` runs. The HIR's `lower_new` arm rewrites `new net.Socket()`
    // (Member callee) to a receiver-less `Expr::NativeMethodCall` so it
    // reaches this dispatch entry; the matching let-stmt registration in
    // `lower.rs` tags the binding as a `("net", "Socket")` native instance
    // so subsequent `sock.connect/.write/.on/.end/.destroy` calls find
    // the class-filtered entries below (issue #422 reproducer 1 + 2).
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "Socket",
        class_filter: None,
        runtime: "js_net_socket_alloc",
        args: &[],
        ret: NR_PTR,
    },
    // Issue #810/#811 — IP classification helpers + Happy-Eyeballs default
    // accessors. Pure string/global-flag functions, no sockets or I/O.
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "isIP",
        class_filter: None,
        runtime: "js_net_is_ip",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "isIPv4",
        class_filter: None,
        runtime: "js_net_is_ipv4",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "isIPv6",
        class_filter: None,
        runtime: "js_net_is_ipv6",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "getDefaultAutoSelectFamily",
        class_filter: None,
        runtime: "js_net_get_default_auto_select_family",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "setDefaultAutoSelectFamily",
        class_filter: None,
        runtime: "js_net_set_default_auto_select_family",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "getDefaultAutoSelectFamilyAttemptTimeout",
        class_filter: None,
        runtime: "js_net_get_default_auto_select_family_attempt_timeout",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "setDefaultAutoSelectFamilyAttemptTimeout",
        class_filter: None,
        runtime: "js_net_set_default_auto_select_family_attempt_timeout",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // Instance method: `sock.connect(port, host)` initiates the deferred
    // TCP connection on a `new net.Socket()`-allocated handle. Twin of
    // the `createConnection` factory above — both end up in the same
    // tokio task body via `run_socket_task`.
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "connect",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_method_connect",
        args: &[NA_F64, NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "write",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_write",
        // Issue #1131 — pass the full NaN-boxed JS value (NA_JSV) so
        // the runtime can probe Buffer-vs-string-vs-number and read
        // through the correct header layout. NA_PTR pre-stripped the
        // tag, so `sock.write("ping")` handed the runtime a bare
        // StringHeader pointer that it reinterpreted as a
        // BufferHeader → garbage on the wire.
        args: &[NA_JSV],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "end",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_end",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "destroy",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_destroy",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "on",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_VOID,
    },
    // upgradeToTLS returns a Promise (handle pointer) — await it to wait
    // for the TLS handshake before sending anything over the upgraded stream.
    // upgradeToTLS(servername, verify): verify is 0/1 (number, not bool).
    // verify=1 uses the system trust store + hostname check (sslmode=verify-full);
    // verify=0 accepts any cert (sslmode=require, for local self-signed DBs).
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "upgradeToTLS",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_upgrade_tls",
        args: &[NA_STR, NA_F64],
        ret: NR_PTR,
    },
    // Factory: `tls.connect(host, port, servername, verify)` opens plain TCP
    // then runs a full TLS handshake before firing 'connect'. Returns a Socket
    // handle that behaves identically to one produced by net.createConnection
    // (same write/end/destroy/on surface).
    NativeModSig {
        module: "tls",
        has_receiver: false,
        method: "connect",
        class_filter: None,
        runtime: "js_tls_connect",
        args: &[NA_STR, NA_F64, NA_STR, NA_F64],
        ret: NR_PTR,
    },
    // ========== net.Server (issue #1123 followup) ==========
    // Server-side TCP via `net.createServer(...).listen(port, cb)`. The
    // factory itself is wired through `Expr::NetCreateServer` in
    // perry-codegen/src/expr.rs (not this table); the instance methods
    // dispatch here once the let-binding gets registered as
    // `("net", "Server")` in HIR lowering. Shape mirrors
    // `js_node_http_server_*` from perry-ext-http-server (signatures
    // are deliberately parallel so the codegen side reads the same).
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "listen",
        class_filter: Some("Server"),
        runtime: "js_net_server_listen",
        args: &[NA_F64, NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "close",
        class_filter: Some("Server"),
        runtime: "js_net_server_close",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "address",
        class_filter: Some("Server"),
        runtime: "js_net_server_address",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "on",
        class_filter: Some("Server"),
        runtime: "js_net_server_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "addListener",
        class_filter: Some("Server"),
        runtime: "js_net_server_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_VOID,
    },
    // ========== node:stream — Readable.from(iterable) (#631) ==========
    // The other stream constructors (`new Readable(opts)` etc.) are wired
    // via `lower_builtin_new` so the codegen can carry the closure-fields
    // ObjectHeader with NaN-boxed POINTER_TAG; they never reach this
    // table. `Readable.from` is a static factory call surfaced as
    // `Readable.from(...)` → `stream.from(...)`, so it lives here.
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "from",
        class_filter: None,
        runtime: "js_node_stream_readable_from",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // ========== Events ==========
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "EventEmitter",
        class_filter: None,
        runtime: "js_event_emitter_new",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "on",
        class_filter: None,
        runtime: "js_event_emitter_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "emit",
        class_filter: None,
        runtime: "js_event_emitter_emit",
        args: &[NA_STR, NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "removeListener",
        class_filter: None,
        runtime: "js_event_emitter_remove_listener",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "removeAllListeners",
        class_filter: None,
        runtime: "js_event_emitter_remove_all_listeners",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    // EventEmitter additions (#850) — `once` / `addListener` (alias for
    // `on`) / `prependListener` / `prependOnceListener` / `listenerCount`
    // / `listeners` / `rawListeners` / `eventNames` / `setMaxListeners` /
    // `getMaxListeners`. Pre-fix `.once(...)` and the prepend variants
    // silently no-op'd and the read-only accessors returned undefined.
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "once",
        class_filter: None,
        runtime: "js_event_emitter_once",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "addListener",
        class_filter: None,
        runtime: "js_event_emitter_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "prependListener",
        class_filter: None,
        runtime: "js_event_emitter_prepend_listener",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "prependOnceListener",
        class_filter: None,
        runtime: "js_event_emitter_prepend_once_listener",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "off",
        class_filter: None,
        runtime: "js_event_emitter_remove_listener",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "listenerCount",
        class_filter: None,
        runtime: "js_event_emitter_listener_count",
        args: &[NA_STR, NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "listeners",
        class_filter: None,
        runtime: "js_event_emitter_listeners",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "rawListeners",
        class_filter: None,
        runtime: "js_event_emitter_raw_listeners",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "eventNames",
        class_filter: None,
        runtime: "js_event_emitter_event_names",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "setMaxListeners",
        class_filter: None,
        runtime: "js_event_emitter_set_max_listeners",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "getMaxListeners",
        class_filter: None,
        runtime: "js_event_emitter_get_max_listeners",
        args: &[],
        ret: NR_F64,
    },
    // Module-level helpers (`events.once` / `events.getEventListeners` /
    // `events.listenerCount` / `events.getMaxListeners` /
    // `events.setMaxListeners`). All take the emitter handle as a
    // positional arg, so `has_receiver: false`.
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "once",
        class_filter: None,
        runtime: "js_events_once",
        args: &[NA_PTR, NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "addAbortListener",
        class_filter: None,
        runtime: "js_events_add_abort_listener",
        args: &[NA_PTR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "getEventListeners",
        class_filter: None,
        runtime: "js_events_get_event_listeners",
        args: &[NA_PTR, NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "listenerCount",
        class_filter: None,
        runtime: "js_events_listener_count",
        args: &[NA_PTR, NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "getMaxListeners",
        class_filter: None,
        runtime: "js_events_get_max_listeners",
        args: &[NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "setMaxListeners",
        class_filter: None,
        runtime: "js_events_set_max_listeners",
        args: &[NA_F64, NA_VARARGS],
        ret: NR_F64,
    },
    // ========== StringDecoder (issue #848) ==========
    // The typed-receiver path: `const d = new StringDecoder("utf8");
    // d.write(buf)` enters here because `d` is registered as a native
    // instance in HIR (`("string_decoder", "StringDecoder")`). The
    // any-typed receiver path (`(d as any).write(buf)` /
    // `Map.get("d").write(...)`) goes through HANDLE_METHOD_DISPATCH
    // instead — both routes call the same underlying handle dispatch,
    // so behavior is identical. `NR_F64` because we return a STRING_TAG-
    // NaN-boxed value directly from the FFI (NR_STR would re-NaN-box a
    // raw pointer and produce nonsense).
    NativeModSig {
        module: "string_decoder",
        has_receiver: true,
        method: "write",
        class_filter: Some("StringDecoder"),
        runtime: "js_string_decoder_write",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "string_decoder",
        has_receiver: true,
        method: "end",
        class_filter: Some("StringDecoder"),
        runtime: "js_string_decoder_end",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // ========== node:querystring ==========
    // Module-level functions. `decode` / `encode` route to the same
    // runtime symbols as `parse` / `stringify` so the test's
    // `decode === parse` identity-equality check passes.
    NativeModSig {
        module: "querystring",
        has_receiver: false,
        method: "escape",
        class_filter: None,
        runtime: "js_querystring_escape",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "querystring",
        has_receiver: false,
        method: "unescape",
        class_filter: None,
        runtime: "js_querystring_unescape",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "querystring",
        has_receiver: false,
        method: "parse",
        class_filter: None,
        runtime: "js_querystring_parse",
        args: &[NA_F64, NA_F64, NA_F64, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "querystring",
        has_receiver: false,
        method: "decode",
        class_filter: None,
        runtime: "js_querystring_parse",
        args: &[NA_F64, NA_F64, NA_F64, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "querystring",
        has_receiver: false,
        method: "stringify",
        class_filter: None,
        runtime: "js_querystring_stringify",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "querystring",
        has_receiver: false,
        method: "encode",
        class_filter: None,
        runtime: "js_querystring_stringify",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_F64,
    },
    // ========== LRU Cache ==========
    NativeModSig {
        module: "lru-cache",
        has_receiver: false,
        method: "default",
        class_filter: None,
        runtime: "js_lru_cache_new",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lru-cache",
        has_receiver: true,
        method: "get",
        class_filter: None,
        runtime: "js_lru_cache_get",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lru-cache",
        has_receiver: true,
        method: "set",
        class_filter: None,
        runtime: "js_lru_cache_set",
        args: &[NA_F64, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lru-cache",
        has_receiver: true,
        method: "has",
        class_filter: None,
        runtime: "js_lru_cache_has",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lru-cache",
        has_receiver: true,
        method: "delete",
        class_filter: None,
        runtime: "js_lru_cache_delete",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lru-cache",
        has_receiver: true,
        method: "clear",
        class_filter: None,
        runtime: "js_lru_cache_clear",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "lru-cache",
        has_receiver: true,
        method: "size",
        class_filter: None,
        runtime: "js_lru_cache_size",
        args: &[],
        ret: NR_F64,
    },
    // ========== commander (CLI parsing) ==========
    // `new Command()` is dispatched separately by `lower_builtin_new` so it
    // produces a real CommanderHandle instead of an empty placeholder. The
    // entries below cover the fluent chain methods + the parse() entry that
    // actually reads argv and fires the registered .action() callback.
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "name",
        class_filter: None,
        runtime: "js_commander_name",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "description",
        class_filter: None,
        runtime: "js_commander_description",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "version",
        class_filter: None,
        runtime: "js_commander_version",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "command",
        class_filter: None,
        runtime: "js_commander_command",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "option",
        class_filter: None,
        runtime: "js_commander_option",
        args: &[NA_STR, NA_STR, NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "requiredOption",
        class_filter: None,
        runtime: "js_commander_required_option",
        args: &[NA_STR, NA_STR, NA_STR],
        ret: NR_PTR,
    },
    // .action(cb) — NA_PTR coerces the NaN-boxed closure to its raw i64
    // pointer so the runtime can call back through `js_closure_call1`.
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "action",
        class_filter: None,
        runtime: "js_commander_action",
        args: &[NA_PTR],
        ret: NR_PTR,
    },
    // .parse(argv) — runtime reads std::env::args() directly; user-provided
    // argv expression evaluates for side effects but is not forwarded.
    // NA_F64 keeps the LLVM call signature aligned with the runtime decl
    // (`(I64, DOUBLE) -> I64`).
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "parse",
        class_filter: None,
        runtime: "js_commander_parse",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "opts",
        class_filter: None,
        runtime: "js_commander_opts",
        args: &[],
        ret: NR_PTR,
    },
    // ========== async_hooks.AsyncLocalStorage ==========
    // `new AsyncLocalStorage()` is dispatched by `lower_builtin_new`; the rows
    // below cover the instance methods. `run(store, cb)` and `exit(cb)` need
    // the closure pointer arg coerced via NA_PTR (the runtime function takes
    // it as a raw `i64` ClosureHeader pointer + invokes `js_closure_call0`
    // internally). Pre-fix every method silently no-op'd through the
    // unknown-method sentinel.
    NativeModSig {
        module: "async_hooks",
        has_receiver: true,
        method: "run",
        class_filter: None,
        runtime: "js_async_local_storage_run",
        args: &[NA_F64, NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: true,
        method: "getStore",
        class_filter: None,
        runtime: "js_async_local_storage_get_store",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: true,
        method: "enterWith",
        class_filter: None,
        runtime: "js_async_local_storage_enter_with",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: true,
        method: "exit",
        class_filter: None,
        runtime: "js_async_local_storage_exit",
        args: &[NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: true,
        method: "disable",
        class_filter: None,
        runtime: "js_async_local_storage_disable",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: false,
        method: "createHook",
        class_filter: None,
        runtime: "js_async_hooks_create_hook",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: false,
        method: "executionAsyncId",
        class_filter: None,
        runtime: "js_async_hooks_execution_async_id",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: false,
        method: "triggerAsyncId",
        class_filter: None,
        runtime: "js_async_hooks_trigger_async_id",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: true,
        method: "enable",
        class_filter: Some("AsyncHook"),
        runtime: "js_async_hook_enable",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: true,
        method: "disable",
        class_filter: Some("AsyncHook"),
        runtime: "js_async_hook_disable",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: true,
        method: "asyncId",
        class_filter: Some("AsyncResource"),
        runtime: "js_async_resource_async_id",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: true,
        method: "triggerAsyncId",
        class_filter: Some("AsyncResource"),
        runtime: "js_async_resource_trigger_async_id",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: true,
        method: "emitDestroy",
        class_filter: Some("AsyncResource"),
        runtime: "js_async_resource_emit_destroy",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: true,
        method: "runInAsyncScope",
        class_filter: Some("AsyncResource"),
        runtime: "js_async_resource_run_in_async_scope",
        args: &[NA_PTR, NA_F64, NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "async_hooks",
        has_receiver: true,
        method: "bind",
        class_filter: Some("AsyncResource"),
        runtime: "js_async_resource_bind",
        args: &[NA_PTR],
        ret: NR_PTR,
    },
    // ========== decimal.js (arbitrary-precision math) ==========
    // `new Decimal(value)` is dispatched by `lower_builtin_new` (calls
    // `js_decimal_coerce_to_handle` to handle string/number/Decimal args).
    // The instance methods below all operate on a registered DecimalHandle.
    // Binary-op wrappers (`*_value`) coerce the second arg via the same
    // helper so `a.plus(2)` and `a.plus("0.1")` work as well as `a.plus(b)`.
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "plus",
        class_filter: None,
        runtime: "js_decimal_plus_value",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "minus",
        class_filter: None,
        runtime: "js_decimal_minus_value",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "times",
        class_filter: None,
        runtime: "js_decimal_times_value",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "div",
        class_filter: None,
        runtime: "js_decimal_div_value",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "mod",
        class_filter: None,
        runtime: "js_decimal_mod_value",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "pow",
        class_filter: None,
        runtime: "js_decimal_pow",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "sqrt",
        class_filter: None,
        runtime: "js_decimal_sqrt",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "abs",
        class_filter: None,
        runtime: "js_decimal_abs",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "neg",
        class_filter: None,
        runtime: "js_decimal_neg",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "round",
        class_filter: None,
        runtime: "js_decimal_round",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "floor",
        class_filter: None,
        runtime: "js_decimal_floor",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "ceil",
        class_filter: None,
        runtime: "js_decimal_ceil",
        args: &[],
        ret: NR_PTR,
    },
    // Formatting — return strings (NR_STR NaN-boxes the *StringHeader).
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "toFixed",
        class_filter: None,
        runtime: "js_decimal_to_fixed",
        args: &[NA_F64],
        ret: NR_STR,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "toString",
        class_filter: None,
        runtime: "js_decimal_to_string",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "toNumber",
        class_filter: None,
        runtime: "js_decimal_to_number",
        args: &[],
        ret: NR_F64,
    },
    // `valueOf()` is what JS uses for implicit number coercion (e.g. `+a`,
    // `a < 5`); decimal.js documents it as an alias for toNumber.
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "valueOf",
        class_filter: None,
        runtime: "js_decimal_to_number",
        args: &[],
        ret: NR_F64,
    },
    // Comparisons — `*_value` wrappers coerce rhs so a.eq(0) works.
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "eq",
        class_filter: None,
        runtime: "js_decimal_eq_value",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "lt",
        class_filter: None,
        runtime: "js_decimal_lt_value",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "lte",
        class_filter: None,
        runtime: "js_decimal_lte_value",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "gt",
        class_filter: None,
        runtime: "js_decimal_gt_value",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "gte",
        class_filter: None,
        runtime: "js_decimal_gte_value",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "cmp",
        class_filter: None,
        runtime: "js_decimal_cmp_value",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // Predicates — return booleans encoded as f64 (TAG_TRUE / TAG_FALSE).
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "isZero",
        class_filter: None,
        runtime: "js_decimal_is_zero",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "isPositive",
        class_filter: None,
        runtime: "js_decimal_is_positive",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "decimal.js",
        has_receiver: true,
        method: "isNegative",
        class_filter: None,
        runtime: "js_decimal_is_negative",
        args: &[],
        ret: NR_F64,
    },
    // ========== uuid ==========
    NativeModSig {
        module: "uuid",
        has_receiver: false,
        method: "v4",
        class_filter: None,
        runtime: "js_uuid_v4",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "uuid",
        has_receiver: false,
        method: "v1",
        class_filter: None,
        runtime: "js_uuid_v1",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "uuid",
        has_receiver: false,
        method: "v7",
        class_filter: None,
        runtime: "js_uuid_v7",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "uuid",
        has_receiver: false,
        method: "validate",
        class_filter: None,
        runtime: "js_uuid_validate",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // ========== jsonwebtoken ==========
    // `sign` and `verify` are intentionally handled in
    // lower_call/native.rs — both need option-dependent runtime
    // selection (HS256 / ES256 / RS256) that the generic table can't
    // express. `decode` stays here because it has no algorithm options.
    NativeModSig {
        module: "jsonwebtoken",
        has_receiver: false,
        method: "decode",
        class_filter: None,
        runtime: "js_jwt_decode",
        // js_jwt_decode(token_ptr) -> *mut StringHeader (JSON of payload).
        // NR_OBJ_FROM_JSON_STR pipes the returned JSON through
        // js_json_parse_or_null so user code sees an object (mirrors
        // `verify`'s post-#927 contract). Issue #927.
        args: &[NA_STR],
        ret: NR_OBJ_FROM_JSON_STR,
    },
    // ========== nodemailer ==========
    NativeModSig {
        module: "nodemailer",
        has_receiver: false,
        method: "createTransport",
        class_filter: None,
        runtime: "js_nodemailer_create_transport",
        args: &[NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "nodemailer",
        has_receiver: true,
        method: "sendMail",
        class_filter: None,
        runtime: "js_nodemailer_send_mail",
        args: &[NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "nodemailer",
        has_receiver: true,
        method: "verify",
        class_filter: None,
        runtime: "js_nodemailer_verify",
        args: &[],
        ret: NR_PTR,
    },
    // ========== dotenv ==========
    NativeModSig {
        module: "dotenv",
        has_receiver: false,
        method: "config",
        class_filter: None,
        runtime: "js_dotenv_config",
        args: &[],
        ret: NR_F64,
    },
    // ========== nanoid ==========
    // js_nanoid_sized(NaN) → size=0 → falls back to js_nanoid() (21-char default),
    // so nanoid() and nanoid(N) both route through the same entry safely.
    NativeModSig {
        module: "nanoid",
        has_receiver: false,
        method: "nanoid",
        class_filter: None,
        runtime: "js_nanoid_sized",
        args: &[NA_F64],
        ret: NR_STR,
    },
    // ========== slugify ==========
    // Three-arg form handles both slugify(s) and slugify(s, replacement_char).
    // Missing args pad to null ptr → runtime uses "-" default separator.
    // "default" for `import slugify from 'slugify'; slugify(s)` (HIR emits method:"default").
    // "slugify" for `import { slugify } from 'slugify'; slugify(s)` (named import).
    NativeModSig {
        module: "slugify",
        has_receiver: false,
        method: "default",
        class_filter: None,
        runtime: "js_slugify_with_options",
        args: &[NA_STR, NA_STR, NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "slugify",
        has_receiver: false,
        method: "slugify",
        class_filter: None,
        runtime: "js_slugify_with_options",
        args: &[NA_STR, NA_STR, NA_STR],
        ret: NR_STR,
    },
    // ========== validator ==========
    NativeModSig {
        module: "validator",
        has_receiver: false,
        method: "isEmail",
        class_filter: None,
        runtime: "js_validator_is_email",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "validator",
        has_receiver: false,
        method: "isURL",
        class_filter: None,
        runtime: "js_validator_is_url",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "validator",
        has_receiver: false,
        method: "isUUID",
        class_filter: None,
        runtime: "js_validator_is_uuid",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "validator",
        has_receiver: false,
        method: "isJSON",
        class_filter: None,
        runtime: "js_validator_is_json",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "validator",
        has_receiver: false,
        method: "isEmpty",
        class_filter: None,
        runtime: "js_validator_is_empty",
        args: &[NA_STR],
        ret: NR_F64,
    },
    // ========== exponential-backoff ==========
    NativeModSig {
        module: "exponential-backoff",
        has_receiver: false,
        method: "backOff",
        class_filter: None,
        runtime: "backOff",
        args: &[NA_PTR, NA_F64],
        ret: NR_PTR,
    },
    // ========== argon2 ==========
    // Runtime FFI signatures take `*const StringHeader`, NOT NaN-boxed f64.
    // NA_STR routes through `js_get_string_pointer_unified` to extract the
    // raw pointer; NA_F64 would pass the f64 in d0 while the callee reads
    // x0 → null/garbage StringHeader → "Invalid password" (#591).
    NativeModSig {
        module: "argon2",
        has_receiver: false,
        method: "hash",
        class_filter: None,
        runtime: "js_argon2_hash",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "argon2",
        has_receiver: false,
        method: "verify",
        class_filter: None,
        runtime: "js_argon2_verify",
        args: &[NA_STR, NA_STR],
        ret: NR_PTR,
    },
    // ========== bcrypt ==========
    // Same ABI rule as argon2 above: password / hash args are
    // `*const StringHeader`. The salt-rounds arg of bcrypt.hash is a
    // genuine f64 number and stays NA_F64.
    NativeModSig {
        module: "bcrypt",
        has_receiver: false,
        method: "hash",
        class_filter: None,
        runtime: "js_bcrypt_hash",
        args: &[NA_STR, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "bcrypt",
        has_receiver: false,
        method: "compare",
        class_filter: None,
        runtime: "js_bcrypt_compare",
        args: &[NA_STR, NA_STR],
        ret: NR_PTR,
    },
    // ========== perry/thread (parallelMap, parallelFilter, spawn) ==========
    // Runtime expects both args as NaN-boxed f64 values and returns the same
    // — no unboxing/reboxing needed on either side. Closure is a POINTER_TAG'd
    // ClosureHeader; the runtime reads `func_ptr` and calls it per element.
    NativeModSig {
        module: "perry/thread",
        has_receiver: false,
        method: "parallelMap",
        class_filter: None,
        runtime: "js_thread_parallel_map",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "perry/thread",
        has_receiver: false,
        method: "parallelFilter",
        class_filter: None,
        runtime: "js_thread_parallel_filter",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "perry/thread",
        has_receiver: false,
        method: "spawn",
        class_filter: None,
        runtime: "js_thread_spawn",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // ========== lodash (named-import form: import { chunk } from 'lodash') ==========
    // Default-import form (import _ from 'lodash'; _.chunk(...)) needs has_receiver:true
    // but would pass the module object as first arg, breaking the C signature.
    // Named imports produce object:None HIR nodes and route here correctly.
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "chunk",
        class_filter: None,
        runtime: "js_lodash_chunk",
        args: &[NA_PTR, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "compact",
        class_filter: None,
        runtime: "js_lodash_compact",
        args: &[NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "drop",
        class_filter: None,
        runtime: "js_lodash_drop",
        args: &[NA_PTR, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "first",
        class_filter: None,
        runtime: "js_lodash_first",
        args: &[NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "head",
        class_filter: None,
        runtime: "js_lodash_first",
        args: &[NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "last",
        class_filter: None,
        runtime: "js_lodash_last",
        args: &[NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "flatten",
        class_filter: None,
        runtime: "js_lodash_flatten",
        args: &[NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "uniq",
        class_filter: None,
        runtime: "js_lodash_uniq",
        args: &[NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "reverse",
        class_filter: None,
        runtime: "js_lodash_reverse",
        args: &[NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "take",
        class_filter: None,
        runtime: "js_lodash_take",
        args: &[NA_PTR, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "camelCase",
        class_filter: None,
        runtime: "js_lodash_camel_case",
        args: &[NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "kebabCase",
        class_filter: None,
        runtime: "js_lodash_kebab_case",
        args: &[NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "snakeCase",
        class_filter: None,
        runtime: "js_lodash_snake_case",
        args: &[NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "clamp",
        class_filter: None,
        runtime: "js_lodash_clamp",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "range",
        class_filter: None,
        runtime: "js_lodash_range",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "times",
        class_filter: None,
        runtime: "js_lodash_times",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "size",
        class_filter: None,
        runtime: "js_lodash_size",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "tail",
        class_filter: None,
        runtime: "js_lodash_tail",
        args: &[NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "sum",
        class_filter: None,
        runtime: "js_lodash_sum",
        args: &[NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "mean",
        class_filter: None,
        runtime: "js_lodash_mean",
        args: &[NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "sumBy",
        class_filter: None,
        runtime: "js_lodash_sum_by",
        args: &[NA_PTR, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "meanBy",
        class_filter: None,
        runtime: "js_lodash_mean_by",
        args: &[NA_PTR, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "max",
        class_filter: None,
        runtime: "js_lodash_max",
        args: &[NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "min",
        class_filter: None,
        runtime: "js_lodash_min",
        args: &[NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "maxBy",
        class_filter: None,
        runtime: "js_lodash_max_by",
        args: &[NA_PTR, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "minBy",
        class_filter: None,
        runtime: "js_lodash_min_by",
        args: &[NA_PTR, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lodash",
        has_receiver: false,
        method: "inRange",
        class_filter: None,
        runtime: "js_lodash_in_range",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_F64,
    },
    // ========== dayjs ==========
    // Factory: `import dayjs from 'dayjs'; dayjs()` → method:"default".
    // Named import: `import { dayjs } from 'dayjs'; dayjs()` → method:"dayjs".
    // Instance methods: handle is a small i64 stored in f64 bits; unbox_to_i64
    // does bitcast+mask which is identity for small values, so has_receiver:true works.
    // dayjs handle args (isBefore/isAfter/diff) use NA_JSV (bitcast, no mask).
    // Note: moment instance methods use f64 handle ABI so cannot use this path.
    NativeModSig {
        module: "dayjs",
        has_receiver: false,
        method: "default",
        class_filter: None,
        runtime: "js_dayjs_now",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: false,
        method: "dayjs",
        class_filter: None,
        runtime: "js_dayjs_now",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "format",
        class_filter: None,
        runtime: "js_dayjs_format",
        args: &[NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "year",
        class_filter: None,
        runtime: "js_dayjs_year",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "month",
        class_filter: None,
        runtime: "js_dayjs_month",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "date",
        class_filter: None,
        runtime: "js_dayjs_date",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "day",
        class_filter: None,
        runtime: "js_dayjs_day",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "hour",
        class_filter: None,
        runtime: "js_dayjs_hour",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "minute",
        class_filter: None,
        runtime: "js_dayjs_minute",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "second",
        class_filter: None,
        runtime: "js_dayjs_second",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "millisecond",
        class_filter: None,
        runtime: "js_dayjs_millisecond",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "valueOf",
        class_filter: None,
        runtime: "js_dayjs_value_of",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "unix",
        class_filter: None,
        runtime: "js_dayjs_unix",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "toISOString",
        class_filter: None,
        runtime: "js_dayjs_to_iso_string",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "add",
        class_filter: None,
        runtime: "js_dayjs_add",
        args: &[NA_F64, NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "subtract",
        class_filter: None,
        runtime: "js_dayjs_subtract",
        args: &[NA_F64, NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "startOf",
        class_filter: None,
        runtime: "js_dayjs_start_of",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "endOf",
        class_filter: None,
        runtime: "js_dayjs_end_of",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "isBefore",
        class_filter: None,
        runtime: "js_dayjs_is_before",
        args: &[NA_JSV],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "isAfter",
        class_filter: None,
        runtime: "js_dayjs_is_after",
        args: &[NA_JSV],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "isSame",
        class_filter: None,
        runtime: "js_dayjs_is_same",
        args: &[NA_JSV],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "isValid",
        class_filter: None,
        runtime: "js_dayjs_is_valid",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "diff",
        class_filter: None,
        runtime: "js_dayjs_diff",
        args: &[NA_JSV, NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "dayjs",
        has_receiver: true,
        method: "clone",
        class_filter: None,
        runtime: "js_dayjs_value_of",
        args: &[],
        ret: NR_F64,
    },
    // ========== date-fns ==========
    // date-fns exports free functions: `format(date, pattern)`,
    // `addDays(date, n)`, etc. The first argument is a Date (NaN-boxed
    // f64 timestamp from `new Date(...)`). The manifest entries surface
    // these as receiver-less NativeMethodCalls on module "date-fns".
    // Without these rows the dispatch returns None and the call falls
    // through to undefined. Refs date-fns format() blocker.
    NativeModSig {
        module: "date-fns",
        has_receiver: false,
        method: "format",
        class_filter: None,
        runtime: "js_datefns_format",
        args: &[NA_F64, NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "date-fns",
        has_receiver: false,
        method: "parseISO",
        class_filter: None,
        runtime: "js_datefns_parse_iso",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "date-fns",
        has_receiver: false,
        method: "addDays",
        class_filter: None,
        runtime: "js_datefns_add_days",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "date-fns",
        has_receiver: false,
        method: "addMonths",
        class_filter: None,
        runtime: "js_datefns_add_months",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "date-fns",
        has_receiver: false,
        method: "addYears",
        class_filter: None,
        runtime: "js_datefns_add_years",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "date-fns",
        has_receiver: false,
        method: "differenceInDays",
        class_filter: None,
        runtime: "js_datefns_difference_in_days",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "date-fns",
        has_receiver: false,
        method: "differenceInHours",
        class_filter: None,
        runtime: "js_datefns_difference_in_hours",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "date-fns",
        has_receiver: false,
        method: "differenceInMinutes",
        class_filter: None,
        runtime: "js_datefns_difference_in_minutes",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "date-fns",
        has_receiver: false,
        method: "isAfter",
        class_filter: None,
        runtime: "js_datefns_is_after",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "date-fns",
        has_receiver: false,
        method: "isBefore",
        class_filter: None,
        runtime: "js_datefns_is_before",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "date-fns",
        has_receiver: false,
        method: "startOfDay",
        class_filter: None,
        runtime: "js_datefns_start_of_day",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "date-fns",
        has_receiver: false,
        method: "endOfDay",
        class_filter: None,
        runtime: "js_datefns_end_of_day",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // ========== moment ==========
    // Only factory wired: moment instance methods take f64 handle (not i64),
    // incompatible with the has_receiver:true i64-first-arg dispatch ABI.
    NativeModSig {
        module: "moment",
        has_receiver: false,
        method: "default",
        class_filter: None,
        runtime: "js_moment_now",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "moment",
        has_receiver: false,
        method: "moment",
        class_filter: None,
        runtime: "js_moment_now",
        args: &[],
        ret: NR_F64,
    },
    // ========== sharp ==========
    // Factory: sharp(path) → js_sharp_from_file. Instance methods take
    // Handle (i64), compatible with the has_receiver:true dispatch path.
    NativeModSig {
        module: "sharp",
        has_receiver: false,
        method: "default",
        class_filter: None,
        runtime: "js_sharp_from_file",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: false,
        method: "sharp",
        class_filter: None,
        runtime: "js_sharp_from_file",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "resize",
        class_filter: None,
        runtime: "js_sharp_resize",
        args: &[NA_F64, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "rotate",
        class_filter: None,
        runtime: "js_sharp_rotate",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "flip",
        class_filter: None,
        runtime: "js_sharp_flip",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "flop",
        class_filter: None,
        runtime: "js_sharp_flop",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "grayscale",
        class_filter: None,
        runtime: "js_sharp_grayscale",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "blur",
        class_filter: None,
        runtime: "js_sharp_blur",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "jpeg",
        class_filter: None,
        runtime: "js_sharp_jpeg",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "png",
        class_filter: None,
        runtime: "js_sharp_png",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "webp",
        class_filter: None,
        runtime: "js_sharp_webp",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "toFile",
        class_filter: None,
        runtime: "js_sharp_to_file",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "toBuffer",
        class_filter: None,
        runtime: "js_sharp_to_buffer",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "metadata",
        class_filter: None,
        runtime: "js_sharp_metadata",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "width",
        class_filter: None,
        runtime: "js_sharp_width",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "sharp",
        has_receiver: true,
        method: "height",
        class_filter: None,
        runtime: "js_sharp_height",
        args: &[],
        ret: NR_F64,
    },
    // ========== cheerio ==========
    // cheerio.load(html) → doc handle (NR_PTR). Instance methods take Handle (i64).
    NativeModSig {
        module: "cheerio",
        has_receiver: false,
        method: "load",
        class_filter: None,
        runtime: "js_cheerio_load",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "cheerio",
        has_receiver: true,
        method: "select",
        class_filter: None,
        runtime: "js_cheerio_select",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "cheerio",
        has_receiver: true,
        method: "text",
        class_filter: None,
        runtime: "js_cheerio_selection_text",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "cheerio",
        has_receiver: true,
        method: "html",
        class_filter: None,
        runtime: "js_cheerio_selection_html",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "cheerio",
        has_receiver: true,
        method: "attr",
        class_filter: None,
        runtime: "js_cheerio_selection_attr",
        args: &[NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "cheerio",
        has_receiver: true,
        method: "length",
        class_filter: None,
        runtime: "js_cheerio_selection_length",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "cheerio",
        has_receiver: true,
        method: "first",
        class_filter: None,
        runtime: "js_cheerio_selection_first",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "cheerio",
        has_receiver: true,
        method: "last",
        class_filter: None,
        runtime: "js_cheerio_selection_last",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "cheerio",
        has_receiver: true,
        method: "eq",
        class_filter: None,
        runtime: "js_cheerio_selection_eq",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "cheerio",
        has_receiver: true,
        method: "find",
        class_filter: None,
        runtime: "js_cheerio_selection_find",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "cheerio",
        has_receiver: true,
        method: "children",
        class_filter: None,
        runtime: "js_cheerio_selection_children",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "cheerio",
        has_receiver: true,
        method: "parent",
        class_filter: None,
        runtime: "js_cheerio_selection_parent",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "cheerio",
        has_receiver: true,
        method: "hasClass",
        class_filter: None,
        runtime: "js_cheerio_selection_has_class",
        args: &[NA_STR],
        ret: NR_F64,
    },
    // ========== zlib ==========
    NativeModSig {
        module: "zlib",
        has_receiver: false,
        method: "gzipSync",
        class_filter: None,
        runtime: "js_zlib_gzip_sync",
        args: &[NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "zlib",
        has_receiver: false,
        method: "gunzipSync",
        class_filter: None,
        runtime: "js_zlib_gunzip_sync",
        args: &[NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "zlib",
        has_receiver: false,
        method: "deflateSync",
        class_filter: None,
        runtime: "js_zlib_deflate_sync",
        args: &[NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "zlib",
        has_receiver: false,
        method: "inflateSync",
        class_filter: None,
        runtime: "js_zlib_inflate_sync",
        args: &[NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "zlib",
        has_receiver: false,
        method: "gzip",
        class_filter: None,
        runtime: "js_zlib_gzip",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "zlib",
        has_receiver: false,
        method: "gunzip",
        class_filter: None,
        runtime: "js_zlib_gunzip",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    // `zlib.createBrotliDecompress(options?)` — axios feature-checks
    // this at module init. The runtime stub returns a registered
    // Buffer-shaped handle (NaN-boxed as a pointer) so callers see
    // a truthy non-null object; the real Brotli decode path is a
    // follow-up. `options` is NaN-boxed as f64.
    NativeModSig {
        module: "zlib",
        has_receiver: false,
        method: "createBrotliDecompress",
        class_filter: None,
        runtime: "js_zlib_create_brotli_decompress",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    // ========== cron ==========
    // schedule() returns a Handle (i64) → NR_PTR. Instance methods take Handle (i64).
    // Callback arg uses NA_JSV (bitcast) to pass the full NaN-boxed closure i64.
    NativeModSig {
        module: "cron",
        has_receiver: false,
        method: "validate",
        class_filter: None,
        runtime: "js_cron_validate",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "cron",
        has_receiver: false,
        method: "schedule",
        class_filter: None,
        runtime: "js_cron_schedule",
        args: &[NA_STR, NA_JSV],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "cron",
        has_receiver: false,
        method: "describe",
        class_filter: None,
        runtime: "js_cron_describe",
        args: &[NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "cron",
        has_receiver: true,
        method: "start",
        class_filter: None,
        runtime: "js_cron_job_start",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "cron",
        has_receiver: true,
        method: "stop",
        class_filter: None,
        runtime: "js_cron_job_stop",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "cron",
        has_receiver: true,
        method: "isRunning",
        class_filter: None,
        runtime: "js_cron_job_is_running",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "cron",
        has_receiver: true,
        method: "nextDate",
        class_filter: None,
        runtime: "js_cron_next_date",
        args: &[],
        ret: NR_STR,
    },
    // ========== perry/tui (#358 Phase 1) ==========
    // Text(content) and Box() return widget handles (NaN-boxed POINTER).
    // The Box(children: Widget[]) shape is intercepted earlier in
    // lower_call/native.rs and lowered as Box() + add_child*N; this
    // table only matches the bare-arg shapes.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Text",
        class_filter: None,
        runtime: "js_perry_tui_text",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Box",
        class_filter: None,
        runtime: "js_perry_tui_box",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "render",
        class_filter: None,
        runtime: "js_perry_tui_render",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "enter",
        class_filter: None,
        runtime: "js_perry_tui_enter",
        args: &[],
        ret: NR_VOID,
    },
    // perry/tui Phase 2 — state container, useInput, run loop, exit.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "state",
        class_filter: None,
        runtime: "js_perry_tui_state_alloc",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    // state.get() — receiver call, dispatches against class "State"
    // registered by destructuring.rs.
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "get",
        class_filter: Some("State"),
        runtime: "js_perry_tui_state_get",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "set",
        class_filter: Some("State"),
        runtime: "js_perry_tui_state_set",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useInput",
        class_filter: None,
        runtime: "js_perry_tui_use_input",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "run",
        class_filter: None,
        runtime: "js_perry_tui_run",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "exit",
        class_filter: None,
        runtime: "js_perry_tui_exit",
        args: &[],
        ret: NR_VOID,
    },
    // perry/tui Phase 3 — Box style setters. The codegen at
    // lower_call/native.rs intercepts `Box(opts, children)` and emits
    // these explicitly per style field; they're not normally called
    // directly from user code but are listed here so the dispatch
    // table also handles direct hand-emission cases (e.g. a future
    // `box.setFlexDirection(...)` imperative API).
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetFlexDirection",
        class_filter: None,
        runtime: "js_perry_tui_box_set_flex_direction",
        args: &[NA_PTR, NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetJustifyContent",
        class_filter: None,
        runtime: "js_perry_tui_box_set_justify_content",
        args: &[NA_PTR, NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetAlignItems",
        class_filter: None,
        runtime: "js_perry_tui_box_set_align_items",
        args: &[NA_PTR, NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetGap",
        class_filter: None,
        runtime: "js_perry_tui_box_set_gap",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetPadding",
        class_filter: None,
        runtime: "js_perry_tui_box_set_padding",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetWidth",
        class_filter: None,
        runtime: "js_perry_tui_box_set_width",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetHeight",
        class_filter: None,
        runtime: "js_perry_tui_box_set_height",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetFlexGrow",
        class_filter: None,
        runtime: "js_perry_tui_box_set_flex_grow",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    // perry/tui Phase 3.5 — per-side padding, flex-shrink/basis,
    // percentage units. (#405.) Codegen-emitted from the Box-options
    // path; not normally called directly from user code.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetPaddingEach",
        class_filter: None,
        runtime: "js_perry_tui_box_set_padding_each",
        args: &[NA_PTR, NA_F64, NA_F64, NA_F64, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetFlexShrink",
        class_filter: None,
        runtime: "js_perry_tui_box_set_flex_shrink",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetFlexBasis",
        class_filter: None,
        runtime: "js_perry_tui_box_set_flex_basis",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetFlexBasisPct",
        class_filter: None,
        runtime: "js_perry_tui_box_set_flex_basis_pct",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetWidthPct",
        class_filter: None,
        runtime: "js_perry_tui_box_set_width_pct",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "boxSetHeightPct",
        class_filter: None,
        runtime: "js_perry_tui_box_set_height_pct",
        args: &[NA_PTR, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "TextStyled",
        class_filter: None,
        runtime: "js_perry_tui_text_styled",
        args: &[NA_STR, NA_STR, NA_STR, NA_F64],
        ret: NR_PTR,
    },
    // perry/tui Phase 4 — Spacer + ProgressBar widgets.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Spacer",
        class_filter: None,
        runtime: "js_perry_tui_spacer",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "ProgressBar",
        class_filter: None,
        runtime: "js_perry_tui_progress_bar",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_PTR,
    },
    // perry/tui Phase 4.5 — Spinner / Input / List / Select / TextArea.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Spinner",
        class_filter: None,
        runtime: "js_perry_tui_spinner",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Input",
        class_filter: None,
        runtime: "js_perry_tui_input",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "List",
        class_filter: None,
        runtime: "js_perry_tui_list",
        args: &[NA_PTR, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Select",
        class_filter: None,
        runtime: "js_perry_tui_select",
        args: &[NA_PTR, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "TextArea",
        class_filter: None,
        runtime: "js_perry_tui_text_area",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    // perry/tui Phase 4.6 — Table + Tabs widgets. Direct-FFI shapes
    // (positional args); object-literal `Table({headers, rows, selected})`
    // is unpacked at the codegen level (lower_call/native.rs).
    // (#402.)
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Table",
        class_filter: None,
        runtime: "js_perry_tui_table",
        args: &[NA_PTR, NA_PTR, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "Tabs",
        class_filter: None,
        runtime: "js_perry_tui_tabs",
        args: &[NA_PTR, NA_F64, NA_PTR],
        ret: NR_PTR,
    },
    // perry/tui Phase 4.7 — Input(value, cursor). Direct-call shape;
    // codegen also dispatches to this from the 2-arg form so the
    // table acts as a fallback for hand-emitted calls. (#404.)
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "InputAt",
        class_filter: None,
        runtime: "js_perry_tui_input_at",
        args: &[NA_STR, NA_F64],
        ret: NR_PTR,
    },
    // perry/tui Phase 4.7 — AnimatedSpinner. Bare `AnimatedSpinner()`
    // hits this row with both args defaulted; object-literal opts
    // form is unpacked at the codegen level. (#403.)
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "AnimatedSpinner",
        class_filter: None,
        runtime: "js_perry_tui_animated_spinner",
        args: &[NA_F64, NA_PTR],
        ret: NR_PTR,
    },
    // ========== perry/tui Phase 1 — ink-API ergonomics hooks (#679) ==========
    // useState(initial) — call-site-indexed state cell. Returns the
    // current value (initialised to `initial` on the first call). Pair
    // with useStateSet(slot_idx, v) to write. Slot index === hook
    // index seen by this useState call (matches React rule-of-hooks).
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useState",
        class_filter: None,
        runtime: "js_perry_tui_use_state",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // useStateSet(slot_idx, value) — write to a useState slot + flip
    // STATE_DIRTY when the bits change.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useStateSet",
        class_filter: None,
        runtime: "js_perry_tui_use_state_set",
        args: &[NA_F64, NA_F64],
        ret: NR_VOID,
    },
    // useStateTuple(initial) — returns a [value, setter] array. This
    // is the back-end the destructuring rewriter (destructuring.rs:
    // rewrite_use_state_tuple) emits when user code writes
    // `const [v, setV] = useState(initial)`. NR_PTR so the returned
    // array handle gets POINTER-tagged like a normal Perry array.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useStateTuple",
        class_filter: None,
        runtime: "js_perry_tui_use_state_tuple",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    // useEffect(fn, deps?). Runs fn() on first call or when deps change.
    // fn is an unboxed closure pointer (NA_PTR); deps is an unboxed
    // array pointer (NA_PTR) or 0 for "no deps array → run every render".
    // The runtime hashes the deps array elements bit-identity-style.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useEffect",
        class_filter: None,
        runtime: "js_perry_tui_use_effect",
        args: &[NA_PTR, NA_PTR],
        ret: NR_VOID,
    },
    // useMemo(fn, deps) — same deps convention; runs fn and caches
    // the result. Returns the cached value when deps haven't changed.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useMemo",
        class_filter: None,
        runtime: "js_perry_tui_use_memo",
        args: &[NA_PTR, NA_PTR],
        ret: NR_F64,
    },
    // useRef(initial) — returns a stable handle. .get()/.set() do not
    // flip STATE_DIRTY (writes don't re-render). NR_PTR so the
    // returned slot-handle is NaN-boxed; receiver-method dispatch on
    // the result unboxes back to i64.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useRef",
        class_filter: None,
        runtime: "js_perry_tui_use_ref",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "get",
        class_filter: Some("RefBox"),
        runtime: "js_perry_tui_ref_get",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "set",
        class_filter: Some("RefBox"),
        runtime: "js_perry_tui_ref_set",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    // useApp() — returns the singleton App handle.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useApp",
        class_filter: None,
        runtime: "js_perry_tui_use_app",
        args: &[],
        ret: NR_PTR,
    },
    // app.exit() / app.waitUntilExit() — class_filter routes only when
    // the receiver was registered as a "TuiApp" instance (see
    // destructuring.rs). These match ink's useApp() shape.
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "exit",
        class_filter: Some("TuiApp"),
        runtime: "js_perry_tui_app_exit",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "waitUntilExit",
        class_filter: Some("TuiApp"),
        runtime: "js_perry_tui_app_wait_until_exit",
        args: &[],
        ret: NR_VOID,
    },
    // useStdout() — returns the singleton Stdout handle.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useStdout",
        class_filter: None,
        runtime: "js_perry_tui_use_stdout",
        args: &[],
        ret: NR_PTR,
    },
    // stdout.write(s) / stdout.columns() / stdout.rows().
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "write",
        class_filter: Some("TuiStdout"),
        runtime: "js_perry_tui_stdout_write",
        args: &[NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "columns",
        class_filter: Some("TuiStdout"),
        runtime: "js_perry_tui_stdout_columns",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "rows",
        class_filter: Some("TuiStdout"),
        runtime: "js_perry_tui_stdout_rows",
        args: &[],
        ret: NR_F64,
    },
    // Top-level `waitUntilExit()` — receiver-less convenience that
    // blocks until exit().
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "waitUntilExit",
        class_filter: None,
        runtime: "js_perry_tui_wait_until_exit",
        args: &[],
        ret: NR_VOID,
    },
    // ---- perry/tui Phase 3 — focus management (#679) ----
    // useFocus(autoFocus, isActive) — returns 1.0 when this widget is
    // currently focused, else 0.0. Auto-focus on first render when
    // autoFocus=1 and no widget is focused yet.
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useFocus",
        class_filter: None,
        runtime: "js_perry_tui_use_focus",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "focusNext",
        class_filter: None,
        runtime: "js_perry_tui_focus_next",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "focusPrevious",
        class_filter: None,
        runtime: "js_perry_tui_focus_previous",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "focus",
        class_filter: None,
        runtime: "js_perry_tui_focus",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: false,
        method: "useFocusManager",
        class_filter: None,
        runtime: "js_perry_tui_use_focus_manager",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "focusNext",
        class_filter: Some("FocusManager"),
        runtime: "js_perry_tui_focus_manager_focus_next",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "focusPrevious",
        class_filter: Some("FocusManager"),
        runtime: "js_perry_tui_focus_manager_focus_previous",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "perry/tui",
        has_receiver: true,
        method: "focus",
        class_filter: Some("FocusManager"),
        runtime: "js_perry_tui_focus_manager_focus",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    // ========== readline (#347 Phase 1) ==========
    // createInterface(opts) returns a Handle (i64, NaN-boxed POINTER).
    // Instance methods take that Handle as the first arg via has_receiver.
    // Callbacks come in as NA_PTR (unboxed *const ClosureHeader as i64).
    NativeModSig {
        module: "readline",
        has_receiver: false,
        method: "createInterface",
        class_filter: None,
        runtime: "js_readline_create_interface",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "readline",
        has_receiver: true,
        method: "question",
        class_filter: None,
        runtime: "js_readline_question",
        args: &[NA_STR, NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "readline",
        has_receiver: true,
        method: "on",
        class_filter: None,
        runtime: "js_readline_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "readline",
        has_receiver: true,
        method: "close",
        class_filter: None,
        runtime: "js_readline_close",
        args: &[],
        ret: NR_VOID,
    },
    // ========== worker_threads ==========
    NativeModSig {
        module: "worker_threads",
        has_receiver: false,
        method: "getWorkerData",
        class_filter: None,
        runtime: "js_worker_threads_get_worker_data",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "worker_threads",
        has_receiver: false,
        method: "workerData",
        class_filter: None,
        runtime: "js_worker_threads_get_worker_data",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "worker_threads",
        has_receiver: false,
        method: "parentPort",
        class_filter: None,
        runtime: "js_worker_threads_parent_port",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "worker_threads",
        has_receiver: true,
        method: "postMessage",
        class_filter: None,
        runtime: "js_worker_threads_post_message",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // ========== ethers ==========
    // Utility functions (receiver-less, no class filter).
    NativeModSig {
        module: "ethers",
        has_receiver: false,
        method: "getAddress",
        class_filter: None,
        runtime: "js_ethers_get_address",
        args: &[NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "ethers",
        has_receiver: false,
        method: "formatEther",
        class_filter: None,
        runtime: "js_ethers_format_ether",
        args: &[NA_PTR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "ethers",
        has_receiver: false,
        method: "formatUnits",
        class_filter: None,
        runtime: "js_ethers_format_units",
        args: &[NA_PTR, NA_F64],
        ret: NR_STR,
    },
    NativeModSig {
        module: "ethers",
        has_receiver: false,
        method: "parseEther",
        class_filter: None,
        runtime: "js_ethers_parse_ether",
        args: &[NA_STR],
        ret: NR_BIGINT,
    },
    NativeModSig {
        module: "ethers",
        has_receiver: false,
        method: "parseUnits",
        class_filter: None,
        runtime: "js_ethers_parse_units",
        args: &[NA_STR, NA_F64],
        ret: NR_BIGINT,
    },
    // Wallet.createRandom() — static method on the Wallet class.
    // class_filter matches `Wallet` so `ethers.Wallet.createRandom()` in
    // HIR (which lowers to class_name="Wallet", method="createRandom")
    // resolves here.
    NativeModSig {
        module: "ethers",
        has_receiver: false,
        method: "createRandom",
        class_filter: Some("Wallet"),
        runtime: "js_ethers_wallet_create_random",
        args: &[],
        ret: NR_PTR,
    },
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
    NativeModSig {
        module: "http",
        has_receiver: false,
        method: "request",
        class_filter: None,
        runtime: "js_http_request",
        args: &[NA_F64, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: false,
        method: "get",
        class_filter: None,
        runtime: "js_http_get",
        args: &[NA_F64, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "https",
        has_receiver: false,
        method: "request",
        class_filter: None,
        runtime: "js_https_request",
        args: &[NA_F64, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "https",
        has_receiver: false,
        method: "get",
        class_filter: None,
        runtime: "js_https_get",
        args: &[NA_F64, NA_PTR],
        ret: NR_PTR,
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
        runtime: "js_http_client_request_end",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "write",
        class_filter: Some("ClientRequest"),
        runtime: "js_http_client_request_write",
        args: &[NA_F64],
        ret: NR_PTR,
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
        runtime: "js_http_set_timeout",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    // ========== node:http server (issue #577) ==========
    // Module-level: `import { createServer } from "node:http"; createServer(handler)`
    NativeModSig {
        module: "http",
        has_receiver: false,
        method: "createServer",
        class_filter: None,
        runtime: "js_node_http_create_server",
        args: &[NA_PTR],
        ret: NR_PTR,
    },
    // HttpServer instance methods (class_filter: HttpServer)
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "listen",
        class_filter: Some("HttpServer"),
        runtime: "js_node_http_server_listen",
        args: &[NA_F64, NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "close",
        class_filter: Some("HttpServer"),
        runtime: "js_node_http_server_close",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "closeAllConnections",
        class_filter: Some("HttpServer"),
        runtime: "js_node_http_server_close_all_connections",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "closeIdleConnections",
        class_filter: Some("HttpServer"),
        runtime: "js_node_http_server_close_idle_connections",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "on",
        class_filter: Some("HttpServer"),
        runtime: "js_node_http_server_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "addListener",
        class_filter: Some("HttpServer"),
        runtime: "js_node_http_server_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_F64,
    },
    // IncomingMessage instance methods
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "on",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "addListener",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "pause",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_pause",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "resume",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_resume",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "destroy",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_destroy",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "read",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_read",
        args: &[],
        ret: NR_F64,
    },
    // ServerResponse instance methods
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "setHeader",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_set_header",
        args: &[NA_STR, NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "getHeader",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_get_header",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "removeHeader",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_remove_header",
        args: &[NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "hasHeader",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_has_header",
        args: &[NA_STR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "writeHead",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_write_head",
        args: &[NA_F64, NA_STR, NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "write",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_write",
        args: &[NA_F64],
        ret: NR_I32,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "end",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_end",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "flushHeaders",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_flush_headers",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "writeContinue",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_write_continue",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "writeProcessing",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_write_processing",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "on",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "addListener",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_F64,
    },
    // Method-call aliases for property-style accessors. Until the
    // HIR-level PropertyGet→__get_<name> rewrite lands (followup),
    // user code must use the method-call form: `req.method()` (calls
    // js_node_http_im_method) instead of `req.method` (property
    // read). Same shape as fastify's `request.method()` / `request.url()`.
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "method",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_method",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "url",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_url",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "httpVersion",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_http_version",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "setStatus",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_set_status",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "getStatus",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_get_status",
        args: &[],
        ret: NR_F64,
    },
    // Property accessors as `__get_<name>` / `__set_<name>` synthetic methods
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_method",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_method",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_url",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_url",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_httpVersion",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_http_version",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_complete",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_complete",
        args: &[],
        ret: NR_I32,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_aborted",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_aborted",
        args: &[],
        ret: NR_I32,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_destroyed",
        class_filter: Some("IncomingMessage"),
        runtime: "js_node_http_im_destroyed",
        args: &[],
        ret: NR_I32,
    },
    // Closes #769 followup — client-side `res.statusCode` /
    // `res.statusMessage` / `res.headers` after `http.request(url, cb)`.
    // The HIR registers the `res` arrow param as ("http",
    // "IncomingMessage"); these entries route the rewritten
    // `__get_<prop>` reads to perry-ext-http's accessor (the same
    // crate that owns the client-IncomingMessage registry the
    // response queue populates). (Restored after the #1099 squash-merge
    // dropped them via a conflict auto-resolution.)
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_statusCode",
        class_filter: Some("IncomingMessage"),
        runtime: "js_http_status_code",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_statusMessage",
        class_filter: Some("IncomingMessage"),
        runtime: "js_http_status_message",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_headers",
        class_filter: Some("IncomingMessage"),
        runtime: "js_http_response_headers",
        args: &[],
        ret: NR_F64,
    },
    // PR #1146 belt-and-braces: bare-name dispatch for the same three
    // client-IncomingMessage properties, for sites where the HIR rewrite
    // to `__get_<prop>` doesn't fire (receiver isn't statically tagged
    // as ("http", "IncomingMessage"), e.g. one assigned through a local
    // before the property read).
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "statusCode",
        class_filter: Some("IncomingMessage"),
        runtime: "js_http_status_code",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "statusMessage",
        class_filter: Some("IncomingMessage"),
        runtime: "js_http_status_message",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "headers",
        class_filter: Some("IncomingMessage"),
        runtime: "js_http_response_headers",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_statusCode",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_get_status",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__set_statusCode",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_set_status",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__set_statusMessage",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_set_status_message",
        args: &[NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_headersSent",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_headers_sent",
        args: &[],
        ret: NR_I32,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_writableEnded",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_writable_ended",
        args: &[],
        ret: NR_I32,
    },
    NativeModSig {
        module: "http",
        has_receiver: true,
        method: "__get_writableFinished",
        class_filter: Some("ServerResponse"),
        runtime: "js_node_http_res_writable_finished",
        args: &[],
        ret: NR_I32,
    },
    // ========== node:https server (issue #577 Phase 2) ==========
    NativeModSig {
        module: "https",
        has_receiver: false,
        method: "createServer",
        class_filter: None,
        runtime: "js_node_https_create_server",
        args: &[NA_F64, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "https",
        has_receiver: true,
        method: "listen",
        class_filter: Some("HttpsServer"),
        runtime: "js_node_https_server_listen",
        args: &[NA_F64, NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "https",
        has_receiver: true,
        method: "close",
        class_filter: Some("HttpsServer"),
        runtime: "js_node_https_server_close",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "https",
        has_receiver: true,
        method: "on",
        class_filter: Some("HttpsServer"),
        runtime: "js_node_https_server_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_F64,
    },
    // ========== node:http2 server (issue #577 Phase 3) ==========
    NativeModSig {
        module: "http2",
        has_receiver: false,
        method: "createSecureServer",
        class_filter: None,
        runtime: "js_node_http2_create_secure_server",
        args: &[NA_F64, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "http2",
        has_receiver: true,
        method: "listen",
        class_filter: Some("Http2SecureServer"),
        runtime: "js_node_http2_server_listen",
        args: &[NA_F64, NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http2",
        has_receiver: true,
        method: "close",
        class_filter: Some("Http2SecureServer"),
        runtime: "js_node_http2_server_close",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "http2",
        has_receiver: true,
        method: "on",
        class_filter: Some("Http2SecureServer"),
        runtime: "js_node_http2_server_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_F64,
    },
    // `@perryts/google-auth` is no longer bundled in perry-stdlib —
    // since v0.5.1015 the package is published as a standalone npm
    // module (https://github.com/PerryTS/google-auth). Codegen
    // dispatches `js_google_auth_*` symbols through `ffi_signatures`
    // built from the installed package's
    // `perry.nativeLibrary.functions`, same as any other external
    // nativeLibrary crate.
    // ========== perry/ads (issue #867) ==========
    // Six FFI entry points exported by `crates/perry-ext-ads`:
    //   - 4 promise-returning load/show pairs for interstitial +
    //     rewarded (NR_PTR — runtime sees a `*mut perry_ffi::Promise`
    //     and NaN-boxes via POINTER_TAG, same as bcrypt / argon2 /
    //     google-auth).
    //   - 2 synchronous banner create/destroy (NR_F64 / NR_VOID —
    //     banner_create returns a handle as a `number`, destroy is
    //     fire-and-forget).
    // MVP returns structured `{ error: "no-sdk-linked" }`
    // placeholders; real Google Mobile Ads SDK integration follows.
    NativeModSig {
        module: "perry/ads",
        has_receiver: false,
        method: "js_ads_interstitial_load",
        class_filter: None,
        runtime: "js_ads_interstitial_load",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/ads",
        has_receiver: false,
        method: "js_ads_interstitial_show",
        class_filter: None,
        runtime: "js_ads_interstitial_show",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/ads",
        has_receiver: false,
        method: "js_ads_rewarded_load",
        class_filter: None,
        runtime: "js_ads_rewarded_load",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/ads",
        has_receiver: false,
        method: "js_ads_rewarded_show",
        class_filter: None,
        runtime: "js_ads_rewarded_show",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "perry/ads",
        has_receiver: false,
        method: "js_ads_banner_create",
        class_filter: None,
        runtime: "js_ads_banner_create",
        args: &[NA_STR, NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "perry/ads",
        has_receiver: false,
        method: "js_ads_banner_destroy",
        class_filter: None,
        runtime: "js_ads_banner_destroy",
        args: &[NA_F64],
        ret: NR_VOID,
    },
];

/// Iterate the dispatch table, projected to manifest-relevant fields.
/// Used by `perry-codegen`'s public `iter_native_method_signatures()`
/// — see `lib.rs`. Stable order = declaration order in
/// `NATIVE_MODULE_TABLE`. Returns args/ret as opaque tag strings so
/// downstream crates (perry-api-manifest's drift test) don't have to
/// know about `NativeArgKind` / `NativeRetKind` (#512).
#[allow(clippy::type_complexity)]
pub(crate) fn iter_native_module_table() -> impl Iterator<
    Item = (
        &'static str,
        bool,
        &'static str,
        Option<&'static str>,
        &'static [&'static str],
        &'static str,
    ),
> {
    NATIVE_MODULE_TABLE.iter().map(|sig| {
        (
            sig.module,
            sig.has_receiver,
            sig.method,
            sig.class_filter,
            arg_kinds_for(sig.args),
            ret_kind_tag(&sig.ret),
        )
    })
}

/// Map a `NativeArgKind` slice to its `NA_*` tag-name slice. The
/// returned slice is `&'static` — keeping each lookup costless on the
/// dispatch-table iteration path. Per-arity buckets keep the static
/// arrays addressable without alloc.
fn arg_kinds_for(args: &'static [NativeArgKind]) -> &'static [&'static str] {
    // Map each arg to its tag string. Up to 6 args covers every row
    // in NATIVE_MODULE_TABLE today (tls.connect = 4 args is the max).
    static TAGS_0: &[&str] = &[];
    let tags: Vec<&'static str> = args.iter().map(|a| arg_kind_tag(a)).collect();
    // Lookup against a small set of static fan-outs — but since we
    // can't easily memoize without `OnceLock`, just leak. The dispatch
    // table is < 400 rows; the resulting Vec leak is bounded and
    // happens once per process.
    if tags.is_empty() {
        return TAGS_0;
    }
    Box::leak(tags.into_boxed_slice())
}

fn arg_kind_tag(a: &NativeArgKind) -> &'static str {
    match a {
        NativeArgKind::F64 => "NA_F64",
        NativeArgKind::StrPtr => "NA_STR",
        NativeArgKind::PtrI64 => "NA_PTR",
        NativeArgKind::JsvalI64 => "NA_JSV",
        NativeArgKind::VarArgsAsArray => "NA_VARARGS",
    }
}

fn ret_kind_tag(r: &NativeRetKind) -> &'static str {
    match r {
        NativeRetKind::Ptr => "NR_PTR",
        NativeRetKind::Str => "NR_STR",
        NativeRetKind::ObjFromJsonStr => "NR_OBJ_FROM_JSON_STR",
        NativeRetKind::BigInt => "NR_BIGINT",
        NativeRetKind::F64 => "NR_F64",
        NativeRetKind::I32Void => "NR_I32",
        NativeRetKind::Void => "NR_VOID",
    }
}
