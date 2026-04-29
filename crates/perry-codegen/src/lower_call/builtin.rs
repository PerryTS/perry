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
    match class_name {
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
            return Ok(Some(nanbox_pointer_inline(blk, &handle)));
        }
        // events.EventEmitter — `new EventEmitter()` produces a real
        // EventEmitterHandle so `.on(...)` / `.emit(...)` find their
        // registered handle (NATIVE_MODULE_TABLE wires those methods
        // through `js_event_emitter_*`). Same #187-shape bug — pre-fix
        // every .on/.emit call dispatched against a junk pointer and
        // silently registered nothing / fired nothing.
        "EventEmitter" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_event_emitter_new", &[]);
            return Ok(Some(nanbox_pointer_inline(blk, &handle)));
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
            return Ok(Some(nanbox_pointer_inline(blk, &handle)));
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
            return Ok(Some(nanbox_pointer_inline(blk, &handle)));
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
            return Ok(Some(nanbox_pointer_inline(blk, &handle)));
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
            return Ok(Some(nanbox_pointer_inline(blk, &handle)));
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
            return Ok(Some(nanbox_pointer_inline(blk, &handle)));
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
            return Ok(Some(nanbox_pointer_inline(blk, &handle)));
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
            return Ok(Some(nanbox_pointer_inline(blk, &handle)));
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
            return Ok(Some(nanbox_pointer_inline(blk, &handle)));
        }
        "Array" => {
            // `new Array()` → empty array, `new Array(n)` → length-n array
            // (zero-initialized slots), `new Array(a, b, c)` → 3-element array
            // [a, b, c]. We handle the no-arg and single-numeric-arg cases
            // here. Multi-arg / non-numeric single arg falls back to the
            // generic Expr::New path.
            let blk = ctx.block();
            let handle = if args.is_empty() {
                blk.call(I64, "js_array_create", &[])
            } else if args.len() == 1 {
                let cap = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let cap_i32 = blk.fptosi(DOUBLE, &cap, I32);
                blk.call(I64, "js_array_alloc_with_length", &[(I32, &cap_i32)])
            } else {
                return Ok(None);
            };
            let blk = ctx.block();
            return Ok(Some(nanbox_pointer_inline(blk, &handle)));
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
                    // Not an object literal — still evaluate for side effects.
                    let _ = lower_expr(ctx, &args[1])?;
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
            // new Request(url, init?) — init = { method?, body?, headers? }
            let url_ptr = if !args.is_empty() {
                get_raw_string_ptr(ctx, &args[0])?
            } else {
                "0".to_string()
            };

            let mut method_ptr = "0".to_string();
            let mut body_ptr = "0".to_string();
            let mut headers_handle = "0.0".to_string();

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
                ],
            );
            Ok(Some(handle))
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
            // Lower init iterable args for side effects; the runtime's
            // js_weakmap_new takes no args and the HIR lowering of
            // `.set(k,v)` calls dispatch on the resulting handle.
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let handle = ctx.block().call(I64, "js_weakmap_new", &[]);
            // js_weakmap_new returns a raw `*mut ObjectHeader` — NaN-box
            // with POINTER_TAG so subsequent `js_weakmap_*` calls can
            // `js_nanbox_get_pointer` on the f64.
            let boxed = nanbox_pointer_inline(ctx.block(), &handle);
            Ok(Some(boxed))
        }
        "WeakSet" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let handle = ctx.block().call(I64, "js_weakset_new", &[]);
            let boxed = nanbox_pointer_inline(ctx.block(), &handle);
            Ok(Some(boxed))
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

        _ => Ok(None),
    }
}
