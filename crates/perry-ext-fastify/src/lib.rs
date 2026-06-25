//! Native bindings for the npm `fastify` HTTP server framework.
//!
//! Replaces `perry-stdlib`'s in-tree `fastify/` module — same FFI
//! surface (`js_fastify_*` symbols), implemented on top of `perry-ffi`
//! v0.5 only (handle registry + JsValue + JsClosure + GC scanner +
//! spawn_blocking + notify_main_thread). hyper provides the HTTP
//! transport.
//!
//! # Architecture
//!
//! - `Fastify(opts?)` returns a `FastifyApp` handle. Routes / hooks /
//!   plugins / error handler are registered via the per-method FFI
//!   calls, all mutating that single handle.
//! - `app.listen({ port })` spawns a perry-ffi blocking task that
//!   runs the hyper accept loop on the shared tokio runtime, then
//!   enters a main-thread event loop that drains pending requests
//!   from an mpsc channel.
//! - Each request is matched against the snapshot of routes captured
//!   at `listen()` time, then dispatched: lifecycle hooks fire first
//!   (any hook that calls `reply.send` aborts the chain), then the
//!   route handler runs, then the response (which may carry a value
//!   from the handler's return or an explicit `reply.send`) is sent
//!   back via a oneshot channel.
//! - User closures (route handlers, hooks, error handler, plugin
//!   bodies) are stored as raw `i64` pointers inside the
//!   `FastifyApp`. A mutable GC root scanner keeps each closure live
//!   and rewrites moved pointers after copied-minor GC so a `gc()`
//!   call between registration and incoming-request dispatch can't
//!   free them (issue #35 pattern in perry-stdlib's existing copy).
//!
//! # Punted gaps
//!
//! Documented here so future ports know what to extend:
//!
//! - **HTTP/2** — hyper's `http2` builder isn't wired up; we use
//!   `http1::Builder::new()`. Adding a configurable
//!   `http2: true` option-flag would require switching to
//!   `hyper_util::server::conn::auto::Builder` for upgrade
//!   negotiation. perry-stdlib's existing copy has the same gap.
//! - **WebSocket upgrade** — fastify exposes `app.register(websocket)`
//!   for protocol upgrades; we don't support that path. Programs
//!   that need a server-side WebSocket should reach for `ws` directly
//!   (perry-ext-ws).
//! - **Multipart / file upload parsing** — `req.body` exposes raw
//!   bytes; multipart structuring is left to user code (or
//!   `perry-stdlib::framework::multipart`, which is not part of
//!   the well-known flip).
//! - **Schema validation** — fastify's `schema` option (JSON Schema
//!   on routes for input validation + serialization) is parsed but
//!   not enforced. Programs needing validation should call
//!   `validator` (perry-ext-validator) inside their handler.
//! - **Plugins** — `app.register(plugin, opts)` runs the plugin
//!   synchronously and inherits a temporary prefix; nested plugin
//!   isolation (separate `app` handles per plugin) is deferred.
//! - **Streaming bodies** — `req.body` collects the entire body
//!   before dispatching. Cooperative streaming would require a
//!   `spawn_async` perry-ffi surface (v0.6.0 followup, same gap as
//!   perry-ext-http / perry-ext-ws).

use std::sync::Once;

use perry_ffi::{gc_register_mutable_root_scanner_named, iter_handles_of_mut, GcRootVisitor};

mod app;
mod context;
mod router;
mod server;
mod upgrade;

pub use app::*;
pub use context::*;
pub use router::*;
pub use server::*;

// ============================================================================
// GC root scanner
// ============================================================================

static GC_REGISTERED: Once = Once::new();

/// Register the fastify GC root scanner exactly once. User closures
/// (route handlers, lifecycle hooks, error handler, plugin bodies)
/// are stored as raw `i64` pointers inside `FastifyApp` handles.
/// Without this scanner, a malloc-triggered GC between registration
/// and incoming-request dispatch would sweep the handler closures —
/// same root cause as issue #35 for net.Socket listeners.
pub(crate) fn ensure_gc_scanner_registered() {
    GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ext-fastify", scan_fastify_roots);
    });
}

/// GC root scanner — walk every registered FastifyApp and visit every
/// closure pointer the wrapper has stashed. Closures are stored as raw
/// `i64`s so copied-minor GC can rewrite the slots directly.
fn scan_fastify_roots(visitor: &mut GcRootVisitor<'_>) {
    iter_handles_of_mut::<FastifyApp, _>(|app| {
        for route in app.routes.iter_mut() {
            visitor.visit_i64_slot(&mut route.handler);
        }
        for cb in app
            .hooks
            .on_request
            .iter_mut()
            .chain(app.hooks.pre_parsing.iter_mut())
            .chain(app.hooks.pre_validation.iter_mut())
            .chain(app.hooks.pre_handler.iter_mut())
            .chain(app.hooks.pre_serialization.iter_mut())
            .chain(app.hooks.on_send.iter_mut())
            .chain(app.hooks.on_response.iter_mut())
            .chain(app.hooks.on_error.iter_mut())
        {
            visitor.visit_i64_slot(cb);
        }
        if let Some(eh) = app.error_handler.as_mut() {
            visitor.visit_i64_slot(eh);
        }
        for plugin in app.plugins.iter_mut() {
            visitor.visit_i64_slot(&mut plugin.handler);
        }
        // #1113: upgrade handlers registered via
        // `app.server.on("upgrade", cb)`. Visit slots mutably so
        // copied-minor GC can rewrite closures if they move.
        for cb in app.upgrade_handlers.iter_mut() {
            visitor.visit_i64_slot(cb);
        }
    });

    // Per-request `req.params` / `req.query` JS objects are cached on each live
    // FastifyContext (`params_object_cache` / `query_object_cache`). Visit them
    // as NaN-boxed value slots so a copying GC between the first accessor build
    // and a later read keeps the object alive AND relocates the cached pointer.
    // `0` means uncached; only a real object pointer (POINTER_TAG `0x7FFD`) is
    // visited. `get_mut()` is sound here — the GC scan has exclusive access to
    // each handle.
    iter_handles_of_mut::<FastifyContext, _>(|ctx| {
        let params_slot = ctx.params_object_cache.get_mut();
        if *params_slot >> 48 == 0x7FFD {
            visitor.visit_nanbox_u64_slot(params_slot);
        }
        let query_slot = ctx.query_object_cache.get_mut();
        if *query_slot >> 48 == 0x7FFD {
            visitor.visit_nanbox_u64_slot(query_slot);
        }
    });
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::{drop_handle, get_handle, register_handle};
    use std::collections::HashMap;
    use std::sync::{Mutex, MutexGuard};

    static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct GcTestGuard {
        frame: u64,
        _lock: MutexGuard<'static, ()>,
    }

    impl GcTestGuard {
        fn new() -> Self {
            let lock = GC_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            perry_runtime::gc::js_gc_write_barriers_emitted(1);
            let frame = perry_runtime::gc::js_shadow_frame_push(0);
            Self { frame, _lock: lock }
        }
    }

    impl Drop for GcTestGuard {
        fn drop(&mut self) {
            perry_runtime::gc::js_shadow_frame_pop(self.frame);
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
        }
    }

    fn young_gc_root() -> i64 {
        perry_runtime::arena::arena_alloc_gc(32, 8, perry_runtime::gc::GC_TYPE_STRING) as i64
    }

    fn assert_rewritten(before: i64, after: i64) {
        assert_ne!(after, before);
        assert!(perry_runtime::arena::pointer_in_nursery(after as usize));
    }

    #[test]
    fn gc_scanner_registers_idempotently() {
        // Calling ensure_gc_scanner_registered multiple times must
        // not panic and must not register the scanner twice (Once
        // guarantees).
        ensure_gc_scanner_registered();
        ensure_gc_scanner_registered();
        ensure_gc_scanner_registered();
    }

    #[test]
    fn gc_mutable_scanner_rewrites_registered_roots() {
        let _guard = GcTestGuard::new();
        perry_ffi::gc_register_mutable_root_scanner_named("perry-ext-fastify", scan_fastify_roots);

        let route = young_gc_root();
        let hook = young_gc_root();
        let error = young_gc_root();
        let plugin = young_gc_root();
        let mut app = FastifyApp::new();
        app.add_route("GET", "/gc", route);
        app.add_hook("onRequest", hook);
        app.set_error_handler(error);
        app.plugins.push(Plugin {
            handler: plugin,
            prefix: "/api".to_string(),
        });
        let handle = register_handle(app);

        let _ = perry_runtime::gc::gc_collect_minor();

        {
            let app = get_handle::<FastifyApp>(handle).expect("fastify handle should remain live");
            assert_rewritten(route, app.routes[0].handler);
            assert_rewritten(hook, app.hooks.on_request[0]);
            assert_rewritten(error, app.error_handler.expect("error handler"));
            assert_rewritten(plugin, app.plugins[0].handler);
        }
        drop_handle(handle);
    }

    /// A fresh `FastifyContext` must have both per-request object caches empty
    /// (`0`), and the slots round-trip the NaN-boxed bits they're built to hold.
    #[test]
    fn context_object_caches_start_empty() {
        use std::sync::atomic::Ordering;
        let ctx = FastifyContext::new(
            7,
            "GET".to_string(),
            "/users/:id".to_string(),
            HashMap::new(),
            None,
            HashMap::new(),
        );
        assert_eq!(ctx.params_object_cache.load(Ordering::Acquire), 0);
        assert_eq!(ctx.query_object_cache.load(Ordering::Acquire), 0);

        // Round-trip a synthetic NaN-boxed object pointer through each slot, so a
        // wiring regression in either cache is caught.
        let nan_boxed = 0x7FFD_0000_DEAD_BEEFu64;
        ctx.params_object_cache.store(nan_boxed, Ordering::Release);
        assert_eq!(ctx.params_object_cache.load(Ordering::Acquire), nan_boxed);
        ctx.query_object_cache.store(nan_boxed, Ordering::Release);
        assert_eq!(ctx.query_object_cache.load(Ordering::Acquire), nan_boxed);
    }

    /// The `req.params` / `req.query` object accessors build the object once and
    /// return the cached NaN-boxed pointer on subsequent calls within a request.
    #[test]
    fn req_object_accessors_cache_within_request() {
        use std::sync::atomic::Ordering;
        let _guard = GcTestGuard::new();
        let mut params = HashMap::new();
        params.insert("id".to_string(), "42".to_string());
        let ctx = FastifyContext::new(
            0,
            "GET".to_string(),
            "/users/42?trace=1".to_string(),
            HashMap::new(),
            None,
            params,
        );
        let h = register_handle(ctx);
        unsafe {
            // params: two reads return identical (cached) bits.
            let a = js_fastify_req_params_object(h);
            let b = js_fastify_req_params_object(h);
            assert_eq!(a.to_bits(), b.to_bits(), "second params read is cached");
            assert_eq!(a.to_bits() >> 48, 0x7FFD, "params object is POINTER_TAG'd");
            let cached_p = get_handle::<FastifyContext>(h)
                .unwrap()
                .params_object_cache
                .load(Ordering::Acquire);
            assert_eq!(
                cached_p,
                a.to_bits(),
                "params cache holds the returned object"
            );

            // query: same, and a distinct slot from params.
            let c = js_fastify_req_query_object(h);
            let d = js_fastify_req_query_object(h);
            assert_eq!(c.to_bits(), d.to_bits(), "second query read is cached");
            assert_eq!(c.to_bits() >> 48, 0x7FFD, "query object is POINTER_TAG'd");
            assert_ne!(
                a.to_bits(),
                c.to_bits(),
                "query cache must not reuse the params object"
            );
            let cached_q = get_handle::<FastifyContext>(h)
                .unwrap()
                .query_object_cache
                .load(Ordering::Acquire);
            assert_eq!(
                cached_q,
                c.to_bits(),
                "query cache holds the returned object"
            );
        }
        drop_handle(h);
    }

    /// GC soundness: the root scanner must visit each live FastifyContext's
    /// cached object slots so a copying GC keeps them alive AND relocates the
    /// cached pointer in place — and must preserve the NaN-box `POINTER_TAG`
    /// (a raw `visit_i64_slot` would strip it). Without this rooting a GC between
    /// the first `req.params` build and a later read would dangle the cache.
    #[test]
    fn gc_scanner_rewrites_cached_object_slots() {
        use std::sync::atomic::Ordering;
        let _guard = GcTestGuard::new();
        perry_ffi::gc_register_mutable_root_scanner_named("perry-ext-fastify", scan_fastify_roots);

        let params_obj = young_gc_root();
        let query_obj = young_gc_root();
        let nanbox = |addr: i64| 0x7FFD_0000_0000_0000u64 | (addr as u64 & 0x0000_FFFF_FFFF_FFFF);
        let mask = 0x0000_FFFF_FFFF_FFFFu64;

        let ctx = FastifyContext::new(
            0,
            "GET".to_string(),
            "/x".to_string(),
            HashMap::new(),
            None,
            HashMap::new(),
        );
        ctx.params_object_cache
            .store(nanbox(params_obj), Ordering::Release);
        ctx.query_object_cache
            .store(nanbox(query_obj), Ordering::Release);
        let handle = register_handle(ctx);

        let _ = perry_runtime::gc::gc_collect_minor();

        {
            let ctx =
                get_handle::<FastifyContext>(handle).expect("context handle should remain live");
            let p = ctx.params_object_cache.load(Ordering::Acquire);
            let q = ctx.query_object_cache.load(Ordering::Acquire);
            // Tag preserved (visit_nanbox_u64_slot, not visit_i64_slot).
            assert_eq!(p >> 48, 0x7FFD, "params cache keeps POINTER_TAG after GC");
            assert_eq!(q >> 48, 0x7FFD, "query cache keeps POINTER_TAG after GC");
            // Address relocated to the moved object, still in the nursery.
            assert_rewritten(params_obj, (p & mask) as i64);
            assert_rewritten(query_obj, (q & mask) as i64);
        }
        drop_handle(handle);
    }

    #[test]
    fn route_registration_round_trip() {
        let mut app = FastifyApp::new();
        app.add_route("GET", "/", 0);
        app.add_route("GET", "/users", 1);
        app.add_route("GET", "/users/:id", 2);
        app.add_route("POST", "/users", 3);
        assert_eq!(app.routes.len(), 4);

        // Match
        let (route, params) = app.match_route("GET", "/users/42").unwrap();
        assert_eq!(route.handler, 2);
        assert_eq!(params.get("id"), Some(&"42".to_string()));

        // Wrong method → no match
        assert!(app.match_route("DELETE", "/users").is_none());
    }

    #[test]
    fn hooks_register_in_order() {
        let mut app = FastifyApp::new();
        app.add_hook("onRequest", 1);
        app.add_hook("preHandler", 2);
        app.add_hook("preHandler", 3);
        app.add_hook("onResponse", 4);
        assert_eq!(app.hooks.on_request, vec![1]);
        assert_eq!(app.hooks.pre_handler, vec![2, 3]);
        assert_eq!(app.hooks.on_response, vec![4]);

        // Unknown hook is silently ignored (matches perry-stdlib).
        app.add_hook("notARealHook", 999);
        assert!(!app.hooks.on_request.contains(&999));
    }

    #[test]
    fn request_response_shape() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert("authorization".to_string(), "Bearer x".to_string());
        let mut params = HashMap::new();
        params.insert("id".to_string(), "42".to_string());

        let ctx = FastifyContext::new(
            7,
            "GET".to_string(),
            "/users/42?foo=bar".to_string(),
            headers,
            Some(b"{}".to_vec()),
            params,
        );

        // URL gets split
        assert_eq!(ctx.method, "GET");
        assert_eq!(ctx.url, "/users/42");
        assert_eq!(ctx.query_string, "foo=bar");
        // Defaults
        assert_eq!(ctx.status_code, 200);
        assert!(!ctx.sent);
        assert!(ctx.response_body.is_none());

        // Query param parsing
        assert_eq!(ctx.get_query_param("foo"), Some("bar".to_string()));
        assert_eq!(ctx.get_query_param("missing"), None);

        // Param accessor
        assert_eq!(ctx.params.get("id"), Some(&"42".to_string()));

        // Body
        assert_eq!(ctx.body_string(), Some("{}".to_string()));
    }

    #[test]
    fn req_url_includes_query_string() {
        // #1705 — `c.req.url` / `req.url` must carry the query string like
        // Node/Fastify (`@hono/perry-server` builds `new Request(host + req.url)`
        // from it). The context splits the target internally, so the FFI getter
        // reconstructs `path?query`.
        let with_query = FastifyContext::new(
            0,
            "GET".to_string(),
            "/u3?a=1&b=2".to_string(),
            HashMap::new(),
            None,
            HashMap::new(),
        );
        let h = register_handle(with_query);
        let got = unsafe { crate::context::string_from_header(super::js_fastify_req_url(h)) };
        assert_eq!(got.as_deref(), Some("/u3?a=1&b=2"));
        drop_handle(h);

        // No query → bare path, no trailing '?'.
        let no_query = FastifyContext::new(
            0,
            "GET".to_string(),
            "/u3".to_string(),
            HashMap::new(),
            None,
            HashMap::new(),
        );
        let h2 = register_handle(no_query);
        let got2 = unsafe { crate::context::string_from_header(super::js_fastify_req_url(h2)) };
        assert_eq!(got2.as_deref(), Some("/u3"));
        drop_handle(h2);
    }

    #[test]
    fn ext_fastify_is_context_handle_membership() {
        // #1293 — the membership probe perry-stdlib's external-fastify
        // dispatch arms consult before forwarding `(request as any).json()`
        // / `(request as any).body` to our `js_fastify_*` exports.
        let ctx = FastifyContext::new(
            0,
            "POST".to_string(),
            "/x".to_string(),
            HashMap::new(),
            Some(b"{}".to_vec()),
            HashMap::new(),
        );
        let ctx_handle = register_handle(ctx);
        // A non-existent handle id is never ours.
        assert_eq!(unsafe { js_ext_fastify_is_context_handle(0) }, 0);
        assert_eq!(
            unsafe { js_ext_fastify_is_context_handle(ctx_handle + 9_999) },
            0
        );
        // A live FastifyContext handle is ours.
        assert_eq!(unsafe { js_ext_fastify_is_context_handle(ctx_handle) }, 1);
        // A FastifyApp handle is NOT a context (type-discriminated downcast).
        let app_handle = register_handle(FastifyApp::new());
        assert_eq!(unsafe { js_ext_fastify_is_context_handle(app_handle) }, 0);

        drop_handle(ctx_handle);
        drop_handle(app_handle);
        // After the context is dropped the probe reports it gone.
        assert_eq!(unsafe { js_ext_fastify_is_context_handle(ctx_handle) }, 0);
    }

    #[test]
    fn port_extraction_safe_defaults() {
        // Object literal pattern verified through the wider unit
        // tests; here we exercise the bare-number pattern + the
        // missing-port pattern indirectly via FastifyApp::new() +
        // manual exec. We can't call extract_port directly (unsafe
        // + needs a NaN-boxed JsValue), so we just verify the
        // FastifyApp default-ports correctly.
        let app = FastifyApp::new();
        assert!(app.routes.is_empty());
        assert!(app.hooks.on_request.is_empty());
        assert!(app.hooks.pre_handler.is_empty());
        assert!(app.error_handler.is_none());
        assert!(app.prefix.is_empty());

        // Plugin prefix path
        let app = FastifyApp::with_prefix("/api/v1".to_string());
        assert_eq!(app.prefix, "/api/v1");

        // Routes inherit prefix
        let mut app = FastifyApp::with_prefix("/api".to_string());
        app.add_route("GET", "/users", 1);
        // Match through the full path.
        let (route, _) = app.match_route("GET", "/api/users").unwrap();
        assert_eq!(route.handler, 1);
        // The bare path must NOT match (prefix is required).
        assert!(app.match_route("GET", "/users").is_none());
    }

    #[test]
    fn error_handler_setter() {
        let mut app = FastifyApp::new();
        assert!(app.error_handler.is_none());
        app.set_error_handler(42);
        assert_eq!(app.error_handler, Some(42));
    }

    #[test]
    fn static_index_population_and_lookup() {
        let mut app = FastifyApp::new();
        app.add_route("GET", "/ping", 11);
        app.add_route("POST", "/users", 22);
        app.add_route("GET", "/users/:id", 33); // parametric — not indexed
        app.add_route("GET", "/static/*", 44); // wildcard — not indexed

        // Only the two fully-static routes are indexed.
        assert!(app.static_index.contains_key("GET /ping"));
        assert!(app.static_index.contains_key("POST /users"));
        assert_eq!(app.static_index.len(), 2);

        // Indexed lookup returns the correct handler and empty params.
        let (route, params) = app.match_route("GET", "/ping").expect("static hit");
        assert_eq!(route.handler, 11);
        assert!(params.is_empty());

        // A query string on the request must not break the static-index hit.
        let (route, params) = app
            .match_route("GET", "/ping?trace=1")
            .expect("static hit with query string");
        assert_eq!(route.handler, 11);
        assert!(params.is_empty());

        // Parametric routes still match via the linear-scan fallback, with params.
        let (route, params) = app.match_route("GET", "/users/42").expect("parametric hit");
        assert_eq!(route.handler, 33);
        assert_eq!(params.get("id").map(String::as_str), Some("42"));

        // Method mismatch must not short-circuit: only POST /users is registered,
        // so GET /users falls through to None rather than hitting the POST entry.
        assert!(app.match_route("GET", "/users").is_none());
    }

    #[test]
    fn static_index_respects_prefix() {
        let mut app = FastifyApp::with_prefix("/api".to_string());
        app.add_route("GET", "/users", 7);
        // The index key is the full prefixed path, not the bare registration path.
        assert!(app.static_index.contains_key("GET /api/users"));
        assert!(!app.static_index.contains_key("GET /users"));
        let (route, _) = app
            .match_route("GET", "/api/users")
            .expect("prefixed static hit");
        assert_eq!(route.handler, 7);
    }

    /// The index is consulted before the linear scan, so it must not let a later
    /// static route jump ahead of an earlier parametric/wildcard route that also
    /// matches — first-registered-wins has to survive.
    #[test]
    fn static_index_preserves_registration_precedence() {
        // Parametric `/:slug` registered FIRST, colliding static `/static` second.
        let mut app = FastifyApp::new();
        app.add_route("GET", "/:slug", 1); // parametric, registered first
        app.add_route("GET", "/static", 2); // static, collides with /:slug

        // The shadowed static route is deliberately NOT indexed...
        assert!(!app.static_index.contains_key("GET /static"));
        // ...so the earlier parametric route still wins through the linear scan.
        let (route, params) = app.match_route("GET", "/static").expect("hit");
        assert_eq!(route.handler, 1);
        assert_eq!(params.get("slug").map(String::as_str), Some("static"));

        // Reverse order: static registered first IS indexed and wins.
        let mut app2 = FastifyApp::new();
        app2.add_route("GET", "/static", 2); // static first
        app2.add_route("GET", "/:slug", 1); // parametric second
        assert!(app2.static_index.contains_key("GET /static"));
        assert_eq!(app2.match_route("GET", "/static").unwrap().0.handler, 2);

        // Wildcard `/static/*` registered FIRST: the later static `/static/foo`
        // must not bypass it via the index — the wildcard still wins and captures.
        let mut app3 = FastifyApp::new();
        app3.add_route("GET", "/static/*", 3); // wildcard, registered first
        app3.add_route("GET", "/static/foo", 4); // static, shadowed by the wildcard
        assert!(!app3.static_index.contains_key("GET /static/foo"));
        let (route, params) = app3.match_route("GET", "/static/foo").expect("hit");
        assert_eq!(route.handler, 3);
        assert_eq!(params.get("*").map(String::as_str), Some("foo"));
    }

    /// Redundant slashes in a registered path must not push a static route off
    /// the O(1) index: `RoutePattern` ignores empty segments, so the index key
    /// has to normalize the same way (`/api//users` → `GET /api/users`).
    #[test]
    fn static_index_normalizes_redundant_slashes() {
        let mut app = FastifyApp::new();
        app.add_route("GET", "/api//users", 9);
        // Indexed under the normalized key, not the raw double-slash path.
        assert!(app.static_index.contains_key("GET /api/users"));
        assert!(!app.static_index.contains_key("GET /api//users"));
        // And a normal request resolves through the fast path.
        assert_eq!(app.match_route("GET", "/api/users").unwrap().0.handler, 9);
    }

    /// Exact-duplicate static routes keep the first registration (matching the
    /// linear scan's first-match behaviour) rather than flipping to last-wins.
    #[test]
    fn static_index_duplicate_routes_keep_first() {
        let mut app = FastifyApp::new();
        app.add_route("GET", "/dup", 100);
        app.add_route("GET", "/dup", 200);
        assert_eq!(app.static_index.get("GET /dup"), Some(&0));
        assert_eq!(app.match_route("GET", "/dup").unwrap().0.handler, 100);
    }

    /// `add_route` stores the method upper-cased, so `match_route` must normalize
    /// the lookup method too — a lowercase/mixed-case caller resolves through both
    /// the static index and the linear-scan fallback.
    #[test]
    fn match_route_normalizes_method_case() {
        let mut app = FastifyApp::new();
        app.add_route("GET", "/ping", 11); // static -> index
        app.add_route("GET", "/users/:id", 22); // parametric -> scan

        // Static, indexed: lowercase + mixed-case both hit.
        assert_eq!(app.match_route("get", "/ping").unwrap().0.handler, 11);
        assert_eq!(app.match_route("GeT", "/ping").unwrap().0.handler, 11);
        // Parametric, scanned: same normalization on the slow path.
        let (route, params) = app.match_route("get", "/users/7").expect("hit");
        assert_eq!(route.handler, 22);
        assert_eq!(params.get("id").map(String::as_str), Some("7"));
    }

    /// Regression: the per-request dispatch in `process_request` must MOVE the
    /// pending request's headers/body/params into `FastifyContext` (via
    /// `mem::take` / `Option::take`) rather than clone them — each is consumed
    /// exactly once, so cloning fires three redundant per-request allocations
    /// (two `HashMap`s + one `Vec`, dominated by the O(headers) header-map clone).
    ///
    /// This drives `build_context_from_pending` — the *same* helper
    /// `process_request` uses to construct the context — so the test fails if
    /// that construction reverts to cloning: after the call the source `pending`
    /// collections must be empty (a clone would leave them populated, the canary),
    /// while `method`/`path` must survive (they're reused for the route match).
    #[test]
    fn process_request_moves_pending_fields_not_clone() {
        use crate::server::{build_context_from_pending, FastifyPendingRequest};

        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert("x-trace-id".to_string(), "abc-123".to_string());
        let mut params = HashMap::new();
        params.insert("id".to_string(), "42".to_string());

        // The response channel is irrelevant to context construction; a dropped
        // receiver is fine — the helper never touches `response_tx`.
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
        let mut pending = FastifyPendingRequest {
            method: "POST".to_string(),
            path: "/users/42".to_string(),
            headers,
            body: Some(b"{\"hello\":\"world\"}".to_vec()),
            params,
            response_tx,
        };

        // Exercise the production construction path.
        let ctx = build_context_from_pending(7, &mut pending);

        // (a) The context received the original values (incl. the request_id).
        assert_eq!(ctx.request_id, 7);
        assert_eq!(ctx.method, "POST");
        assert_eq!(ctx.headers.len(), 2);
        assert_eq!(
            ctx.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
        assert_eq!(ctx.params.get("id").map(String::as_str), Some("42"));
        assert_eq!(ctx.body.as_deref(), Some(&b"{\"hello\":\"world\"}"[..]));

        // (b) Move semantics — `pending`'s collections are empty post-construction.
        // A clone-based helper would leave them populated, failing the test.
        assert!(pending.headers.is_empty());
        assert!(pending.params.is_empty());
        assert!(pending.body.is_none());

        // (c) method/path are intentionally NOT consumed — reused for the route match.
        assert_eq!(pending.method, "POST");
        assert_eq!(pending.path, "/users/42");
    }
}
