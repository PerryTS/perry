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

use perry_ffi::{gc_register_mutable_root_scanner, iter_handles_of_mut, GcRootVisitor};

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
        gc_register_mutable_root_scanner(scan_fastify_roots);
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
        perry_ffi::gc_register_mutable_root_scanner(scan_fastify_roots);

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
}
