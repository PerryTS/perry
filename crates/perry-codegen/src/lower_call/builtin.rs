//! Built-in `new C()` constructor lowering — `lower_builtin_new`.
//!
//! Tier 2.2 follow-up (v0.5.339) — extracts the 399-LOC dispatcher
//! that handles `new` calls against built-in classes (Date, Map, Set,
//! Buffer, fetch Headers / Request / Response, mongodb MongoClient,
//! redis Redis client, fastify App, ws WebSocketServer, pg Client /
//! Pool, perry/plugin Decimal, AsyncLocalStorage, AbortController,
//! Command, …). Each match arm emits a runtime call to the
//! corresponding `js_<lib>_<class>_new(...)` C symbol.
//!
//! Pattern matches `ui_styling.rs` (the prior lower_call/ extraction):
//! `pub(super) fn` entry point, recursion through `super::lower_expr`,
//! shared `extract_options_fields` and `build_headers_from_object`
//! reach into the parent module.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64};

use super::{build_headers_from_object, extract_options_fields, get_raw_string_ptr};

pub(super) fn lower_builtin_new(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    // Issue #602: ambiguously-named built-in constructors (Client / Pool /
    // Database / Redis / MongoClient / Decimal) collide with default-import
    // aliases from unrelated packages — `import Client from "better-sqlite3"`
    // would otherwise dispatch through pg's Client arm and emit an undefined
    // `js_pg_client_new` reference at link time. When `class_name` matches an
    // ambiguous arm AND we know the import source is NOT the package the arm
    // is for, return `None` so `lower_new` falls through to the generic path.
    // Names without a recorded import source (top-level globals, locally-
    // defined classes already filtered upstream, etc.) keep their pre-#602
    // behavior — the arm still fires.
    let import_src = ctx
        .imported_class_sources
        .get(class_name)
        .map(|s| s.as_str());
    let arm_mismatches_source = match (class_name, import_src) {
        ("Client", Some(src)) => src != "pg",
        ("Pool", Some(src)) => src != "pg",
        ("Database", Some(src)) => src != "better-sqlite3",
        ("Redis", Some(src)) => src != "ioredis" && src != "redis",
        ("MongoClient", Some(src)) => src != "mongodb",
        ("Decimal", Some(src)) => src != "decimal.js",
        _ => false,
    };
    if arm_mismatches_source {
        return Ok(None);
    }
    match class_name {
        "EvalError" | "URIError" => {
            let msg_box = if let Some(message) = args.first() {
                lower_expr(ctx, message)?
            } else {
                lower_expr(ctx, &Expr::String(String::new()))?
            };
            for arg in args.iter().skip(1) {
                let _ = lower_expr(ctx, arg)?;
            }
            let blk = ctx.block();
            let msg_handle = unbox_to_i64(blk, &msg_box);
            let runtime = if class_name == "EvalError" {
                "js_evalerror_new"
            } else {
                "js_urierror_new"
            };
            let err_handle = blk.call(I64, runtime, &[(I64, &msg_handle)]);
            Ok(Some(nanbox_pointer_inline(blk, &err_handle)))
        }
        // `new RegExp(pattern)` / `new RegExp(pattern, flags)` — call
        // js_regexp_new directly so the resulting object is a real
        // RegExpHeader (registered in REGEX_POINTERS, .test/.exec/etc
        // dispatch correctly). Refs #486 — hono's `buildWildcardRegExp`
        // does `new RegExp(path === "*" ? "" : ...)`. Pre-fix, the
        // generic Expr::New path fell through to the placeholder
        // js_object_alloc(0,0) and the resulting "fake regex" never
        // actually matched anything (`.test("/")` returned false on every
        // input — caused middleware-vs-route lookup in
        // RegExpRouter.add's wildcard branch to skip every push, leaving
        // matchResult[0] missing the middleware entry). Compile-time
        // RegExp LITERALS (`/foo/g`) already lower through Expr::RegExp
        // at expr.rs:4964 — this arm covers the runtime `new RegExp(arg)`
        // form where the pattern argument is a non-literal expression.
        // `new ArrayBuffer(size)` — issue #579. Pre-fix this fell through
        // to the empty-ObjectHeader placeholder and `new Uint8Array(ab)`
        // views silently allocated independent storage (no aliasing). The
        // runtime's `js_array_buffer_new` allocates a real BufferHeader
        // that subsequent Uint8Array views share by pointer (see
        // `js_uint8array_new` in `crates/perry-runtime/src/buffer.rs`:
        // sources that ARE registered buffers but NOT marked as
        // Uint8Array — i.e. ArrayBuffers — are aliased rather than
        // copied). SharedArrayBuffer uses the same storage allocation with a
        // separate runtime registry so util.types can distinguish it.
        // Non-numeric arg shapes (`new ArrayBuffer(undefined)` etc.) coerce to
        // 0 — matches Node/bun's `ToIndex(length)` step for the typical
        // undefined-arg case.
        "ArrayBuffer" | "SharedArrayBuffer" => {
            let size_box = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(0.0)
            };
            let blk = ctx.block();
            let size_i32 = blk.fptosi(DOUBLE, &size_box, I32);
            let runtime = if class_name == "SharedArrayBuffer" {
                "js_shared_array_buffer_new"
            } else {
                "js_array_buffer_new"
            };
            let handle = blk.call(I64, runtime, &[(I32, &size_i32)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "Uint8Array" if args.len() >= 2 => {
            let source = lower_expr(ctx, &args[0])?;
            let offset = lower_expr(ctx, &args[1])?;
            let offset_i32 = ctx.block().fptosi(DOUBLE, &offset, I32);
            let length_i32 = if args.len() >= 3 {
                let length = lower_expr(ctx, &args[2])?;
                ctx.block().fptosi(DOUBLE, &length, I32)
            } else {
                "-1".to_string()
            };
            let handle = ctx.block().call(
                I64,
                "js_uint8array_view",
                &[(DOUBLE, &source), (I32, &offset_i32), (I32, &length_i32)],
            );
            Ok(Some(nanbox_pointer_inline(ctx.block(), &handle)))
        }
        // Minimal DataView support for BufferSource consumers such as
        // StringDecoder: Perry models ArrayBuffer/Uint8Array storage as a
        // BufferHeader, so `new DataView(buffer)` can alias the same backing
        // pointer for byte-extraction call sites.
        "DataView" => {
            let view_box = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let offset_i32 = if args.len() >= 2 {
                let offset = lower_expr(ctx, &args[1])?;
                ctx.block().fptosi(DOUBLE, &offset, I32)
            } else {
                "0".to_string()
            };
            let length_i32 = if args.len() >= 3 {
                let length = lower_expr(ctx, &args[2])?;
                ctx.block().fptosi(DOUBLE, &length, I32)
            } else {
                "-1".to_string()
            };
            Ok(Some(ctx.block().call(
                DOUBLE,
                "js_data_view_new",
                &[(DOUBLE, &view_box), (I32, &offset_i32), (I32, &length_i32)],
            )))
        }
        "RegExp" => {
            let pattern_box = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(0.0)
            };
            let flags_box = if args.len() > 1 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(0.0)
            };
            let blk = ctx.block();
            let pattern_handle = unbox_to_i64(blk, &pattern_box);
            let flags_handle = unbox_to_i64(blk, &flags_box);
            let handle = blk.call(
                I64,
                "js_regexp_new",
                &[(I64, &pattern_handle), (I64, &flags_handle)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // commander Command — `new Command()` allocates a real CommanderHandle
        // via the runtime constructor so subsequent `.command(...).action(...)
        // .parse(...)` calls operate on a registered handle. Without this,
        // `lower_new` falls back to an empty placeholder ObjectHeader and the
        // entire fluent chain dispatches against junk (closes #187).
        "Command" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_commander_new", &[]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // events.EventEmitter — `new EventEmitter()` produces a real
        // EventEmitterHandle so `.on(...)` / `.emit(...)` find their
        // registered handle (NATIVE_MODULE_TABLE wires those methods
        // through `js_event_emitter_*`). Same #187-shape bug — pre-fix
        // every .on/.emit call dispatched against a junk pointer and
        // silently registered nothing / fired nothing.
        "EventEmitter" => {
            let opts = if let Some(a) = args.first() {
                lower_expr(ctx, a)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            for a in args.iter().skip(1) {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_event_emitter_new_with_options", &[(DOUBLE, &opts)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "EventTarget" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_event_target_new", &[]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "Console" => {
            let opts = if let Some(a) = args.first() {
                lower_expr(ctx, a)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            if let Some(stderr_arg) = args.get(1) {
                let stderr = lower_expr(ctx, stderr_arg)?;
                for a in args.iter().skip(2) {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(Some(ctx.block().call(
                    DOUBLE,
                    "js_console_new2",
                    &[(DOUBLE, &opts), (DOUBLE, &stderr)],
                )));
            }
            Ok(Some(ctx.block().call(
                DOUBLE,
                "js_console_new",
                &[(DOUBLE, &opts)],
            )))
        }
        // node:perf_hooks — `new PerformanceObserver(cb)` registers the
        // observer and returns its `perf_observer` namespace object (already
        // NaN-boxed). `cb` is passed through so the runtime can invoke it on
        // flush; absent → undefined.
        "PerformanceObserver" => {
            let cb = if let Some(a) = args.first() {
                lower_expr(ctx, a)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(Some(ctx.block().call(
                DOUBLE,
                "js_perf_observer_new",
                &[(DOUBLE, &cb)],
            )))
        }
        // string_decoder.StringDecoder — issue #848. `new StringDecoder("utf8")`
        // pre-fix fell through to the generic `js_object_alloc(0, 0)` placeholder,
        // so `dec.write` / `dec.end` were `undefined`. Allocate a real handle
        // here; `common/dispatch.rs` dispatches the instance methods + getters
        // through HANDLE_METHOD_DISPATCH / HANDLE_PROPERTY_DISPATCH. Encoding
        // arg is passed through so future non-UTF-8 backends can switch on it;
        // the current impl only tracks the UTF-8 partial-codepoint state.
        "StringDecoder" => {
            let enc_box = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            for a in args.iter().skip(1) {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let enc_handle = unbox_to_i64(blk, &enc_box);
            let handle = blk.call(I64, "js_string_decoder_new", &[(I64, &enc_handle)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // node:stream — `new Readable(opts)` / `new Writable(opts)` /
        // `new Duplex(opts)` / `new Transform(opts)` / `new PassThrough(opts)`.
        // Issue #631. Pre-fix the generic Expr::New path produced an empty
        // ObjectHeader, so `r.on`, `r.pipe`, `.read`, etc. were undefined and
        // any downstream call crashed. The runtime helpers in
        // `perry-runtime/src/node_stream.rs` build an ObjectHeader with each
        // method name keyed to a NaN-boxed closure pointer that captures the
        // host object — `typeof r.on === "function"` and chained
        // `.on(...).on(...).pipe(...)` calls return `this` so the chain
        // doesn't lose identity. Stub semantics only: no real data pump.
        "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough" => {
            let opts_box = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            // Lower any extra args for side effects.
            for a in args.iter().skip(1) {
                let _ = lower_expr(ctx, a)?;
            }
            let runtime_fn = match class_name {
                "Readable" => "js_node_stream_readable_new",
                "Writable" => "js_node_stream_writable_new",
                "Duplex" => "js_node_stream_duplex_new",
                "Transform" => "js_node_stream_transform_new",
                "PassThrough" => "js_node_stream_passthrough_new",
                _ => unreachable!(),
            };
            let result = ctx.block().call(DOUBLE, runtime_fn, &[(DOUBLE, &opts_box)]);
            Ok(Some(result))
        }
        // lru-cache LRUCache — `new LRUCache({ max: N })`. Runtime takes
        // a single `max: f64`. Extract the `max` field from the options
        // literal (handles both raw `Expr::Object(props)` and Phase 3's
        // `Expr::New { __AnonShape_N }` shape via `extract_options_fields`);
        // default to 100 when no options literal is detected (matches the
        // npm `lru-cache` library's behavior for `new LRUCache()` with
        // missing max — it warns + falls back, we just fall back).
        "LRUCache" => {
            let max_val = if let Some(opts_arg) = args.first() {
                let mut found_max: Option<String> = None;
                if let Some(props) = extract_options_fields(ctx, opts_arg) {
                    for (k, vexpr) in &props {
                        if k == "max" {
                            found_max = Some(lower_expr(ctx, vexpr)?);
                        } else {
                            // Lower other fields for side effects (e.g. ttl
                            // option's setter calls).
                            let _ = lower_expr(ctx, vexpr)?;
                        }
                    }
                } else {
                    // Non-literal arg (variable, dynamic shape) — lower for
                    // side effects only; cannot extract max statically.
                    let _ = lower_expr(ctx, opts_arg)?;
                }
                found_max.unwrap_or_else(|| "100.0".to_string())
            } else {
                "100.0".to_string()
            };
            let blk = ctx.block();
            let handle = blk.call(I64, "js_lru_cache_new", &[(DOUBLE, &max_val)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // (`WebSocketServer` is handled by an earlier branch lower in this
        // file — pre-existing from 2026-04-14. No new branch needed here.)
        // pg Client — `new Client(config)` matching npm pg's API: synchronous
        // constructor that stores the config; the user calls
        // `await client.connect()` separately to open the TCP connection.
        // Pre-fix `new Client(config)` fell into the empty-placeholder branch
        // and every chained method (.connect/.query/.end) dispatched against
        // junk. The runtime's older `js_pg_connect(config) -> Promise<Handle>`
        // (still wired as the receiver-less `pg.connect(config)` factory)
        // combines new+connect in one step; this branch maps the npm shape
        // through the new `js_pg_client_new` (sync, stores config) +
        // `js_pg_client_connect` (async, opens the connection) split.
        "Client" => {
            let config_val = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let handle = blk.call(I64, "js_pg_client_new", &[(DOUBLE, &config_val)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // pg Pool — `new Pool(config)`. sqlx's `connect_lazy` makes this
        // synchronous (no actual connections opened until first `.query()`),
        // matching npm pg Pool's auto-connect-on-first-use semantics. The
        // older `js_pg_create_pool` factory (returns Promise<Handle>) stays
        // wired for `pg.Pool(config)` and similar patterns.
        "Pool" => {
            let config_val = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let handle = blk.call(I64, "js_pg_pool_new", &[(DOUBLE, &config_val)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // better-sqlite3 Database — `new Database(filename)` opens a SQLite
        // connection. Without this, `new Database(...)` falls into lower_new's
        // empty-object placeholder, so `db` is a generic ObjectHeader pointer
        // instead of a real Handle from `js_sqlite_open`. `db.prepare(...)`
        // then unboxes that bogus pointer; `get_handle::<SqliteDbHandle>`
        // returns None; prepare returns -1; every chained `.run()`/`.get()`/
        // `.all()` dispatches against junk and silently produces undefined.
        "Database" => {
            let path_ptr = if let Some(arg) = args.first() {
                get_raw_string_ptr(ctx, arg)?
            } else {
                "0".to_string()
            };
            let blk = ctx.block();
            let handle = blk.call(I64, "js_sqlite_open", &[(I64, &path_ptr)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // mongodb MongoClient — `new MongoClient(uri)` matching npm mongodb's
        // API. URI is a string; runtime stores it and connects later via
        // `await client.connect()`.
        "MongoClient" => {
            let uri_ptr = if let Some(arg) = args.first() {
                get_raw_string_ptr(ctx, arg)?
            } else {
                "0".to_string()
            };
            let blk = ctx.block();
            let handle = blk.call(I64, "js_mongodb_client_new", &[(I64, &uri_ptr)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // ioredis Redis — `new Redis()` or `new Redis(opts)`. The runtime's
        // `js_ioredis_new` reads connection settings from REDIS_HOST /
        // REDIS_PORT / REDIS_PASSWORD / REDIS_TLS env vars and ignores its
        // config arg; connection is lazy (the handle is registered immediately
        // and the actual TCP/TLS connect runs on the first `.get`/`.set`/etc.).
        // Pre-fix `new Redis()` fell into the empty-placeholder branch and
        // every chained method (set/get/del/exists/incr/decr/expire/quit)
        // dispatched against junk. The instance methods are wired in
        // NATIVE_MODULE_TABLE for module: "ioredis"; this branch makes the
        // ctor produce a real RedisClient handle so the dispatch lands on it.
        "Redis" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            // The runtime sig takes one i64 (currently *const c_void, ignored).
            // Pass 0 — semantically "use env-var defaults".
            let handle = blk.call(I64, "js_ioredis_new", &[(I64, "0")]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // async_hooks.AsyncLocalStorage — `new AsyncLocalStorage()` produces a
        // real handle so `.run(store, cb)` / `.getStore()` / `.enterWith(store)`
        // / `.exit(cb)` / `.disable()` find their registered store stack.
        // Same #187-shape bug — pre-fix `new AsyncLocalStorage()` fell into the
        // empty-placeholder branch and `.run(store, cb)` dispatched against a
        // junk pointer (callback never fired, store never recorded).
        "AsyncLocalStorage" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_async_local_storage_new", &[]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // #1367: `new crypto.X509Certificate(pem | der)` — parse the cert
        // into a handle exposing subject/issuer/validFrom/validTo/
        // serialNumber/fingerprint/fingerprint256/ca/raw. The arg is a PEM
        // string or DER Buffer; unbox to its raw pointer for the runtime
        // parser (`js_crypto_x509_new` returns an already-NaN-boxed handle).
        "X509Certificate" => {
            let input = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let input_handle = unbox_to_i64(blk, &input);
            Ok(Some(blk.call(
                DOUBLE,
                "js_crypto_x509_new",
                &[(I64, &input_handle)],
            )))
        }
        "AsyncResource" => {
            let type_value = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let options_value = if let Some(arg) = args.get(1) {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_async_resource_new",
                &[(DOUBLE, &type_value), (DOUBLE, &options_value)],
            );
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // #2875: TC39 explicit-resource-management stacks. `new
        // DisposableStack()` / `new AsyncDisposableStack()` allocate a
        // GC-managed stack object (NaN-boxed pointer) so the instance methods
        // (`use` / `adopt` / `defer` / `dispose` / `move` / `disposed`)
        // dispatch through the `__disposable__` rows in native_table.
        "DisposableStack" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_disposable_stack_new", &[]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "AsyncDisposableStack" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_async_disposable_stack_new", &[]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // #2875: `new SuppressedError(error, suppressed, message?)` — an
        // Error-subclass object carrying `.error` / `.suppressed` /
        // `.message` / `.name`. The runtime ctor registers the class id as
        // extending Error (once) so `instanceof Error` holds; the property
        // reads flow through the ordinary by-name object getter.
        "SuppressedError" => {
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let error = if let Some(a) = args.first() {
                lower_expr(ctx, a)?
            } else {
                undef.clone()
            };
            let suppressed = if let Some(a) = args.get(1) {
                lower_expr(ctx, a)?
            } else {
                undef.clone()
            };
            let message = if let Some(a) = args.get(2) {
                lower_expr(ctx, a)?
            } else {
                undef.clone()
            };
            let blk = ctx.block();
            Ok(Some(blk.call(
                DOUBLE,
                "js_suppressed_error_new",
                &[(DOUBLE, &error), (DOUBLE, &suppressed), (DOUBLE, &message)],
            )))
        }
        // decimal.js Decimal — `new Decimal(value)` where value is a number,
        // string, or another Decimal. Routes through `js_decimal_coerce_to_handle`
        // which NaN-decodes the JSValue and dispatches to `from_number` /
        // `from_string` / passthrough for an existing Decimal handle. Without
        // this, `new Decimal("0.1")` falls into the empty-placeholder branch
        // and every chained method dispatches against a junk receiver.
        "Decimal" => {
            let val = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                // `new Decimal()` with no args — coerce undefined → 0.
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let handle = blk.call(I64, "js_decimal_coerce_to_handle", &[(DOUBLE, &val)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "Array" => {
            // `new Array()` → empty array, `new Array(n)` → length-n sparse
            // array after runtime validation, and `new Array(value)` with a
            // non-number argument → one-element array. Multi-arg calls fall
            // back to the generic Expr::New path.
            let blk = ctx.block();
            let handle = if args.is_empty() {
                blk.call(I64, "js_array_create", &[])
            } else if args.len() == 1 {
                let value = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                blk.call(I64, "js_array_constructor_single", &[(DOUBLE, &value)])
            } else {
                return Ok(None);
            };
            let blk = ctx.block();
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        "Response" => {
            // new Response(body?, init?) — init = { status?, statusText?, headers? }
            let body_ptr = if !args.is_empty() {
                get_raw_string_ptr(ctx, &args[0])?
            } else {
                "0".to_string()
            };

            // Default init: status=200, statusText=null, headers=0
            let mut status_val = "200.0".to_string();
            let mut status_text_ptr = "0".to_string();
            let mut headers_handle = "0.0".to_string();

            if args.len() >= 2 {
                if let Some(props) = extract_options_fields(ctx, &args[1]) {
                    for (k, vexpr) in &props {
                        match k.as_str() {
                            "status" => {
                                status_val = lower_expr(ctx, vexpr)?;
                            }
                            "statusText" => {
                                status_text_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "headers" => {
                                // Inline object → build a Headers handle.
                                // Phase 3 anon-class → same via extract_options.
                                // Other expressions → use as-is (handle f64).
                                if let Some(hprops) = extract_options_fields(ctx, vexpr) {
                                    headers_handle = build_headers_from_object(ctx, &hprops)?;
                                } else {
                                    headers_handle = lower_expr(ctx, vexpr)?;
                                }
                            }
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    // Fix #421 (v0.5.575): the second arg is a runtime
                    // object (not an object literal) — happens when
                    // user code does `new Response(body, opts)` where
                    // `opts` is bound from elsewhere. Hono's
                    // `c.text(body, status)` path builds `{ status,
                    // headers }` inside `#newResponse` and passes it
                    // here. Previously perry just evaluated the arg
                    // for side effects and dropped status/headers,
                    // so every hono response had perry-default
                    // status (200) and no headers — `res.status`
                    // read undefined because the response never
                    // got a status to begin with. Now we extract
                    // `.status` / `.statusText` / `.headers` at
                    // runtime via `js_object_get_field_by_name_f64`
                    // and feed them to `js_response_new`.
                    let opts_val = lower_expr(ctx, &args[1])?;
                    let blk = ctx.block();
                    let opts_handle = crate::expr::unbox_to_i64(blk, &opts_val);

                    // Helper: intern a key, load its raw string ptr,
                    // call js_object_get_field_by_name_f64.
                    let get_field = |ctx_inner: &mut FnCtx<'_>, key: &str| -> Result<String> {
                        let key_idx = ctx_inner.strings.intern(key);
                        let key_global =
                            format!("@{}", ctx_inner.strings.entry(key_idx).handle_global);
                        let blk = ctx_inner.block();
                        let key_box = blk.load(DOUBLE, &key_global);
                        let key_bits = blk.bitcast_double_to_i64(&key_box);
                        let key_raw = blk.and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
                        let opts_handle_local = opts_handle.clone();
                        Ok(blk.call(
                            DOUBLE,
                            "js_object_get_field_by_name_f64",
                            &[(I64, &opts_handle_local), (I64, &key_raw)],
                        ))
                    };

                    // status: NaN-boxed f64. The runtime treats NaN /
                    // 0 as "use default 200" so a missing field flows
                    // through cleanly.
                    status_val = get_field(ctx, "status")?;
                    // statusText: NaN-boxed string. Strip to raw ptr
                    // for the FFI signature.
                    let st_box = get_field(ctx, "statusText")?;
                    let blk = ctx.block();
                    status_text_ptr =
                        blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &st_box)]);
                    // headers: NaN-boxed Headers handle (an f64
                    // numeric id from `js_headers_new`). Pass through
                    // verbatim — js_response_new accepts the raw f64.
                    // Defensive: strip NaN-box tag if hono / user code
                    // wrapped it as a pointer.
                    headers_handle = get_field(ctx, "headers")?;
                }
            }

            let handle = ctx.block().call(
                DOUBLE,
                "js_response_new",
                &[
                    (I64, &body_ptr),
                    (DOUBLE, &status_val),
                    (I64, &status_text_ptr),
                    (DOUBLE, &headers_handle),
                ],
            );
            // Response handle is a plain numeric f64 (response-registry id).
            // DO NOT NaN-box — method dispatch expects raw f64.
            Ok(Some(handle))
        }

        // Issue #1211: `new Blob(parts, opts)` / `new File(parts, name, opts)`.
        // `parts` is a JS array of mixed string/Buffer/Blob inputs — the
        // runtime helper (`js_blob_new` / `js_file_new`) walks the array
        // and concatenates the bytes. Locals bound by `const b = new Blob(...)`
        // are tagged `("blob", "Blob")` in `destructuring/var_decl.rs` so
        // subsequent property/method access dispatches through the
        // `module == "blob"` arm above.
        "Blob" => {
            let parts = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let mut type_str = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            if args.len() >= 2 {
                if let Some(props) = extract_options_fields(ctx, &args[1]) {
                    for (k, vexpr) in &props {
                        if k == "type" {
                            type_str = lower_expr(ctx, vexpr)?;
                        } else {
                            let _ = lower_expr(ctx, vexpr)?;
                        }
                    }
                } else {
                    let _ = lower_expr(ctx, &args[1])?;
                }
            }
            let handle = ctx.block().call(
                DOUBLE,
                "js_blob_new",
                &[(DOUBLE, &parts), (DOUBLE, &type_str)],
            );
            Ok(Some(handle))
        }

        "File" => {
            let parts = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let name = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let mut type_str = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            // NaN signals "use Date.now()" inside the runtime helper.
            let mut last_modified = double_literal(f64::NAN);
            if args.len() >= 3 {
                if let Some(props) = extract_options_fields(ctx, &args[2]) {
                    for (k, vexpr) in &props {
                        match k.as_str() {
                            "type" => type_str = lower_expr(ctx, vexpr)?,
                            "lastModified" => last_modified = lower_expr(ctx, vexpr)?,
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    let _ = lower_expr(ctx, &args[2])?;
                }
            }
            let handle = ctx.block().call(
                DOUBLE,
                "js_file_new",
                &[
                    (DOUBLE, &parts),
                    (DOUBLE, &name),
                    (DOUBLE, &type_str),
                    (DOUBLE, &last_modified),
                ],
            );
            Ok(Some(handle))
        }

        "Headers" => {
            // new Headers(init?) — init can be an object literal or another
            // Headers/array iterable. Only inline object literals are
            // handled so far; anything else falls back to empty.
            let h = ctx.block().call(DOUBLE, "js_headers_new", &[]);
            if !args.is_empty() {
                if let Some(props) = extract_options_fields(ctx, &args[0]) {
                    for (k, vexpr) in &props {
                        let key_expr = Expr::String(k.clone());
                        let key_ptr = get_raw_string_ptr(ctx, &key_expr)?;
                        let val_ptr = get_raw_string_ptr(ctx, vexpr)?;
                        ctx.block().call(
                            DOUBLE,
                            "js_headers_set",
                            &[(DOUBLE, &h), (I64, &key_ptr), (I64, &val_ptr)],
                        );
                    }
                } else {
                    let _ = lower_expr(ctx, &args[0])?;
                }
            }
            Ok(Some(h))
        }

        "Request" => {
            // new Request(url, init?) — init = Fetch RequestInit subset.
            let url_ptr = if !args.is_empty() {
                get_raw_string_ptr(ctx, &args[0])?
            } else {
                "0".to_string()
            };

            let mut method_ptr = "0".to_string();
            let mut body_ptr = "0".to_string();
            let mut headers_handle = "0.0".to_string();
            let mut referrer_ptr = "0".to_string();
            let mut referrer_policy_ptr = "0".to_string();
            let mut mode_ptr = "0".to_string();
            let mut credentials_ptr = "0".to_string();
            let mut cache_ptr = "0".to_string();
            let mut redirect_ptr = "0".to_string();
            let mut integrity_ptr = "0".to_string();
            let mut keepalive = double_literal(f64::from_bits(crate::nanbox::TAG_FALSE));
            let mut duplex_ptr = "0".to_string();
            let mut signal = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));

            if args.len() >= 2 {
                if let Some(props) = extract_options_fields(ctx, &args[1]) {
                    for (k, vexpr) in &props {
                        match k.as_str() {
                            "method" => {
                                method_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "body" => {
                                body_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "headers" => {
                                if let Some(hprops) = extract_options_fields(ctx, vexpr) {
                                    headers_handle = build_headers_from_object(ctx, &hprops)?;
                                } else {
                                    headers_handle = lower_expr(ctx, vexpr)?;
                                }
                            }
                            "referrer" => {
                                referrer_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "referrerPolicy" => {
                                referrer_policy_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "mode" => {
                                mode_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "credentials" => {
                                credentials_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "cache" => {
                                cache_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "redirect" => {
                                redirect_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "integrity" => {
                                integrity_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "keepalive" => {
                                keepalive = lower_expr(ctx, vexpr)?;
                            }
                            "duplex" => {
                                duplex_ptr = get_raw_string_ptr(ctx, vexpr)?;
                            }
                            "signal" => {
                                signal = lower_expr(ctx, vexpr)?;
                            }
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    let _ = lower_expr(ctx, &args[1])?;
                }
            }

            let handle = ctx.block().call(
                DOUBLE,
                "js_request_new",
                &[
                    (I64, &url_ptr),
                    (I64, &method_ptr),
                    (I64, &body_ptr),
                    (DOUBLE, &headers_handle),
                    (I64, &referrer_ptr),
                    (I64, &referrer_policy_ptr),
                    (I64, &mode_ptr),
                    (I64, &credentials_ptr),
                    (I64, &cache_ptr),
                    (I64, &redirect_ptr),
                    (I64, &integrity_ptr),
                    (DOUBLE, &keepalive),
                    (I64, &duplex_ptr),
                    (DOUBLE, &signal),
                ],
            );
            Ok(Some(handle))
        }

        // Issue #237: Web Streams API constructors. Source / sink / transform
        // objects accept `start` / `pull` / `cancel` / `write` / `close` /
        // `abort` / `transform` / `flush` callbacks; missing ones are passed
        // as TAG_UNDEFINED so the runtime can no-op cleanly.
        "ReadableStream" => {
            let mut start = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut pull = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut cancel = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut source_type = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut strategy = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            if !args.is_empty() {
                if let Some(props) = extract_options_fields(ctx, &args[0]) {
                    for (k, vexpr) in &props {
                        match k.as_str() {
                            "start" => {
                                start = lower_expr(ctx, vexpr)?;
                            }
                            "pull" => {
                                pull = lower_expr(ctx, vexpr)?;
                            }
                            "cancel" => {
                                cancel = lower_expr(ctx, vexpr)?;
                            }
                            "type" => {
                                source_type = lower_expr(ctx, vexpr)?;
                            }
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    let _ = lower_expr(ctx, &args[0])?;
                }
            }
            if args.len() >= 2 {
                strategy = lower_expr(ctx, &args[1])?;
            }
            let h = ctx.block().call(
                DOUBLE,
                "js_readable_stream_new_with_strategy_and_source_type",
                &[
                    (DOUBLE, &start),
                    (DOUBLE, &pull),
                    (DOUBLE, &cancel),
                    (DOUBLE, &strategy),
                    (DOUBLE, &source_type),
                ],
            );
            Ok(Some(h))
        }

        "WritableStream" => {
            if matches!(args.first(), Some(Expr::Null)) {
                for arg in args {
                    let _ = lower_expr(ctx, arg)?;
                }
                let h = ctx
                    .block()
                    .call(DOUBLE, "js_writable_stream_throw_invalid_sink", &[]);
                return Ok(Some(h));
            }
            let mut start = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut write = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut close = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut abort = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut sink_type = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut hwm = double_literal(1.0);
            if !args.is_empty() {
                if let Some(props) = extract_options_fields(ctx, &args[0]) {
                    for (k, vexpr) in &props {
                        match k.as_str() {
                            "start" => {
                                start = lower_expr(ctx, vexpr)?;
                            }
                            "write" => {
                                write = lower_expr(ctx, vexpr)?;
                            }
                            "close" => {
                                close = lower_expr(ctx, vexpr)?;
                            }
                            "abort" => {
                                abort = lower_expr(ctx, vexpr)?;
                            }
                            "type" => {
                                sink_type = lower_expr(ctx, vexpr)?;
                            }
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    let _ = lower_expr(ctx, &args[0])?;
                }
            }
            if args.len() >= 2 {
                if let Some(qprops) = extract_options_fields(ctx, &args[1]) {
                    for (k, vexpr) in &qprops {
                        if k == "highWaterMark" {
                            hwm = lower_expr(ctx, vexpr)?;
                        }
                    }
                } else {
                    let strategy = lower_expr(ctx, &args[1])?;
                    hwm = ctx.block().call(
                        DOUBLE,
                        "js_streams_strategy_high_water_mark",
                        &[(DOUBLE, &strategy)],
                    );
                }
            }
            let h = ctx.block().call(
                DOUBLE,
                "js_writable_stream_new_with_sink_type",
                &[
                    (DOUBLE, &start),
                    (DOUBLE, &write),
                    (DOUBLE, &close),
                    (DOUBLE, &abort),
                    (DOUBLE, &hwm),
                    (DOUBLE, &sink_type),
                ],
            );
            Ok(Some(h))
        }

        "TransformStream" => {
            let mut start = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut transform = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut flush = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut hwm = double_literal(1.0);
            if !args.is_empty() {
                if let Some(props) = extract_options_fields(ctx, &args[0]) {
                    for (k, vexpr) in &props {
                        match k.as_str() {
                            "start" => {
                                start = lower_expr(ctx, vexpr)?;
                            }
                            "transform" => {
                                transform = lower_expr(ctx, vexpr)?;
                            }
                            "flush" => {
                                flush = lower_expr(ctx, vexpr)?;
                            }
                            _ => {
                                let _ = lower_expr(ctx, vexpr)?;
                            }
                        }
                    }
                } else {
                    let _ = lower_expr(ctx, &args[0])?;
                }
            }
            if args.len() >= 2 {
                if let Some(qprops) = extract_options_fields(ctx, &args[1]) {
                    for (k, vexpr) in &qprops {
                        if k == "highWaterMark" {
                            hwm = lower_expr(ctx, vexpr)?;
                        }
                    }
                }
            }
            let h = ctx.block().call(
                DOUBLE,
                "js_transform_stream_new",
                &[
                    (DOUBLE, &start),
                    (DOUBLE, &transform),
                    (DOUBLE, &flush),
                    (DOUBLE, &hwm),
                ],
            );
            Ok(Some(h))
        }

        "TextEncoderStream" | "TextDecoderStream" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let h = ctx.block().call(DOUBLE, "js_text_encoding_stream_new", &[]);
            Ok(Some(h))
        }

        // node:stream/web QueuingStrategy classes (#1545). Both take a single
        // `{ highWaterMark }` options object; the runtime reads
        // `opts.highWaterMark` and builds a `{ highWaterMark, size }` object.
        // CountQueuingStrategy.size() returns 1, ByteLengthQueuingStrategy.size(chunk)
        // returns chunk.byteLength. Pass the whole options expression through so
        // both literal (`{ highWaterMark: 5 }`) and dynamic option objects work.
        "CountQueuingStrategy" | "ByteLengthQueuingStrategy" => {
            let opts = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            for a in args.iter().skip(1) {
                let _ = lower_expr(ctx, a)?;
            }
            let func = if class_name == "CountQueuingStrategy" {
                "js_count_queuing_strategy_new"
            } else {
                "js_byte_length_queuing_strategy_new"
            };
            let h = ctx.block().call(DOUBLE, func, &[(DOUBLE, &opts)]);
            Ok(Some(h))
        }

        "Promise" => {
            // `new Promise((resolve, reject) => { ... })` — the runtime's
            // `js_promise_new_with_executor` takes the closure, allocates
            // the resolve/reject helper closures, and invokes the executor
            // synchronously. The executor must actually run to honor
            // imperative patterns like `new Promise(r => { setTimeout(r,1) })`
            // that are common in the tests.
            if args.is_empty() {
                let p = ctx.block().call(I64, "js_promise_new", &[]);
                return Ok(Some(nanbox_pointer_inline(ctx.block(), &p)));
            }
            let exec_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let exec_handle = unbox_to_i64(blk, &exec_box);
            let p = blk.call(I64, "js_promise_new_with_executor", &[(I64, &exec_handle)]);
            Ok(Some(nanbox_pointer_inline(blk, &p)))
        }
        "WeakMap" => {
            let lowered_args = args
                .iter()
                .map(|a| lower_expr(ctx, a))
                .collect::<Result<Vec<_>>>()?;
            let handle = ctx.block().call(I64, "js_weakmap_new", &[]);
            // js_weakmap_new returns a raw `*mut ObjectHeader` — NaN-box
            // with POINTER_TAG so subsequent `js_weakmap_*` calls can
            // `js_nanbox_get_pointer` on the f64.
            let boxed = nanbox_pointer_inline(ctx.block(), &handle);
            if let Some(iterable) = lowered_args.first() {
                Ok(Some(ctx.block().call(
                    DOUBLE,
                    "js_weakmap_init_iterable",
                    &[(DOUBLE, &boxed), (DOUBLE, iterable)],
                )))
            } else {
                Ok(Some(boxed))
            }
        }
        "WeakSet" => {
            let lowered_args = args
                .iter()
                .map(|a| lower_expr(ctx, a))
                .collect::<Result<Vec<_>>>()?;
            let handle = ctx.block().call(I64, "js_weakset_new", &[]);
            let boxed = nanbox_pointer_inline(ctx.block(), &handle);
            if let Some(iterable) = lowered_args.first() {
                Ok(Some(ctx.block().call(
                    DOUBLE,
                    "js_weakset_init_iterable",
                    &[(DOUBLE, &boxed), (DOUBLE, iterable)],
                )))
            } else {
                Ok(Some(boxed))
            }
        }
        "AbortController" => {
            // Lower any incidental args for side effects (shouldn't have any).
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let handle = ctx.block().call(I64, "js_abort_controller_new", &[]);
            // The runtime returns a raw *mut ObjectHeader — NaN-box with
            // POINTER_TAG so regular property get (`controller.signal`,
            // `controller.aborted`) works via js_object_get_field_by_name_f64.
            let boxed = nanbox_pointer_inline(ctx.block(), &handle);
            Ok(Some(boxed))
        }

        // new WebSocketServer({ port: N }) → js_ws_server_new(opts_f64)
        "WebSocketServer" => {
            // Lower the options object (first arg) as a NaN-boxed f64.
            let opts = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            ctx.pending_declares
                .push(("js_ws_server_new".to_string(), I64, vec![DOUBLE]));
            let blk = ctx.block();
            let handle = blk.call(I64, "js_ws_server_new", &[(DOUBLE, &opts)]);
            Ok(Some(nanbox_pointer_inline(blk, &handle)))
        }
        // Issue #606 — `new WebSocket(url)` from `import { WebSocket } from
        // "ws"`. npm ws's API is sync-ctor: returns the client handle
        // immediately and connects in the background; the user's
        // `client.on("open", cb)` then registers a listener that fires
        // once the connect completes. The previous lower path treated
        // `new WebSocket(...)` as a no-op `Expr::New` and let the
        // method-dispatch tower invoke `js_ws_connect` (which returns a
        // Promise, not a handle), so `client.on(...)` was being called
        // against a promise pointer and silently no-op'd. Routing
        // through `js_ws_connect_start` returns the handle synchronously
        // and the connect runs as a sibling tokio task that pushes an
        // Open / Error event when complete.
        "WebSocket" => {
            let url_box = if !args.is_empty() {
                lower_expr(ctx, &args[0])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            ctx.pending_declares
                .push(("js_ws_connect_start".to_string(), DOUBLE, vec![DOUBLE]));
            let blk = ctx.block();
            // js_ws_connect_start returns the ws_id as a plain f64
            // (1.0, 2.0, …). Convert to i64 then NaN-box with
            // POINTER_TAG so the standard `unbox_to_i64` receiver
            // contract recovers the right ws_id at every method call
            // site (`client.on(...)`, `.send(...)`, `.close()`).
            let raw_f64 = blk.call(DOUBLE, "js_ws_connect_start", &[(DOUBLE, &url_box)]);
            let raw_i64 = blk.fptosi(DOUBLE, &raw_f64, I64);
            Ok(Some(nanbox_pointer_inline(blk, &raw_i64)))
        }

        _ => Ok(None),
    }
}
