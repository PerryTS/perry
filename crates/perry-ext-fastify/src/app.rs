//! FastifyApp model + FFI for application creation, route registration,
//! lifecycle hooks, error handlers, and plugin registration.

use std::collections::HashMap;

use perry_ffi::{
    get_handle_mut, register_handle, Handle, JsClosure, JsValue, ObjectHeader, RawClosureHeader,
    StringHeader,
};

use crate::context::string_from_nanboxed;
use crate::ensure_gc_scanner_registered;
use crate::router::RoutePattern;

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Closure pointer (matches the runtime's `*const ClosureHeader`).
pub type ClosurePtr = i64;

/// Route registration entry — method, parsed pattern, user closure.
#[derive(Clone)]
pub struct Route {
    pub method: String,
    pub pattern: RoutePattern,
    pub handler: ClosurePtr,
}

/// Lifecycle hook callbacks. Each hook list is iterated in FIFO order;
/// any hook that calls `reply.send(...)` aborts the chain (sets
/// `ctx.sent = true`) so subsequent hooks + the route handler are
/// skipped.
#[derive(Default, Clone)]
pub struct Hooks {
    pub on_request: Vec<ClosurePtr>,
    pub pre_parsing: Vec<ClosurePtr>,
    pub pre_validation: Vec<ClosurePtr>,
    pub pre_handler: Vec<ClosurePtr>,
    pub pre_serialization: Vec<ClosurePtr>,
    pub on_send: Vec<ClosurePtr>,
    pub on_response: Vec<ClosurePtr>,
    pub on_error: Vec<ClosurePtr>,
}

/// Plugin registration record. Today only `prefix` is honored — the
/// plugin's body is invoked synchronously at registration time and
/// any routes it adds end up on the parent app under the prefix.
#[derive(Clone)]
pub struct Plugin {
    pub handler: ClosurePtr,
    pub prefix: String,
}

/// FastifyApp — registered routes + lifecycle hooks + plugins +
/// configuration.
pub struct FastifyApp {
    pub routes: Vec<Route>,
    /// O(1) index over the subset of `routes` whose pattern is fully static
    /// (no `Param`/`Wildcard` segments). Key: `"{METHOD} {full_path}"` (matching
    /// what `match_route` builds at lookup time); value: the index into `routes`.
    /// Built incrementally in `add_route`; parametric/wildcard routes fall
    /// through to the linear scan. Purely additive — `routes` (and the GC
    /// scanner's walk over it) is unchanged — turning the dominant static-route
    /// lookup from O(N) per request into one hash + comparison.
    pub static_index: HashMap<String, usize>,
    pub hooks: Hooks,
    pub error_handler: Option<ClosurePtr>,
    pub plugins: Vec<Plugin>,
    pub prefix: String,
    pub config: FastifyConfig,
    /// #1113: callbacks registered via `app.server.on("upgrade", cb)`.
    /// See the doc-comment on `js_fastify_app_server` for the full
    /// rationale — the short version is that `app.server` returns the
    /// FastifyApp handle pointer-tagged so `.on(…)` dispatches back
    /// into this same struct's `upgrade_handlers` Vec. Today only
    /// stores the callbacks — the hyper accept-loop in `server.rs`
    /// doesn't yet route `Upgrade:` requests through
    /// `hyper::upgrade::on(req)` and hand the raw socket + head
    /// bytes back to TypeScript. Full bidirectional WebSocket
    /// upgrade dispatch through perry-ext-ws's `noServer` mode is
    /// the tracked #1113 follow-up.
    pub upgrade_handlers: Vec<ClosurePtr>,
}

/// Server configuration parsed from the `Fastify({ ... })` call.
#[derive(Clone, Default)]
pub struct FastifyConfig {
    pub logger: bool,
    pub trust_proxy: bool,
    pub body_limit: Option<usize>,
}

/// Canonicalize a path into the normalized, leading-slash form the route
/// matcher compares against: drop any query string, then keep only the
/// non-empty `/`-separated segments (so `/api//users`, `api/users`, and
/// `/api/users/` all collapse to `/api/users`, and `/` / `` collapse to `/`).
/// `static_index` keys and lookups both go through this, so the index can never
/// diverge from `RoutePattern`'s own segment view — which likewise ignores
/// empty segments — and a route registered with redundant slashes still
/// resolves through the O(1) fast path instead of falling back to the scan.
fn canonical_path(path: &str) -> String {
    let path = path.split('?').next().unwrap_or(path);
    // Fast path: an already-canonical `/a/b/c` (leading slash, no empty
    // segments, no trailing slash) is by far the common case — copy it as-is
    // with a single allocation and skip the segment rebuild. Only paths with
    // missing/redundant/trailing slashes fall through to normalization.
    if path.len() > 1 && path.starts_with('/') && !path.ends_with('/') && !path.contains("//") {
        return path.to_string();
    }
    // The normalized form only ever drops bytes (query string, redundant
    // slashes), so the input length is a safe upper bound — pre-size to keep
    // this to a single allocation too.
    let mut out = String::with_capacity(path.len() + 1);
    for part in path.split('/').filter(|p| !p.is_empty()) {
        out.push('/');
        out.push_str(part);
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

impl FastifyApp {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            static_index: HashMap::new(),
            hooks: Hooks::default(),
            error_handler: None,
            plugins: Vec::new(),
            prefix: String::new(),
            config: FastifyConfig::default(),
            upgrade_handlers: Vec::new(),
        }
    }

    pub fn with_prefix(prefix: String) -> Self {
        let mut app = Self::new();
        app.prefix = prefix;
        app
    }

    /// Register a route. The method is upper-cased and the path is parsed into a
    /// [`RoutePattern`]. Fully-static routes (no `Param`/`Wildcard` segments) are
    /// additionally recorded in `static_index` for O(1) lookup in `match_route`,
    /// but *only* when doing so cannot change which handler `match_route` returns
    /// (see below); everything else resolves through the linear scan.
    pub fn add_route(&mut self, method: &str, path: &str, handler: ClosurePtr) {
        let full_path = if self.prefix.is_empty() {
            path.to_string()
        } else {
            format!("{}{}", self.prefix, path)
        };
        let method_upper = method.to_uppercase();
        let pattern = RoutePattern::parse(&full_path);
        let is_static = pattern
            .segments
            .iter()
            .all(|s| matches!(s, crate::router::Segment::Static(_)));
        let new_idx = self.routes.len();
        self.routes.push(Route {
            method: method_upper,
            pattern,
            handler,
        });
        if is_static {
            // Normalize the path through the same `canonical_path` the lookup
            // side uses, so the key matches regardless of leading/redundant
            // slashes (`/foo`, `foo`, `/foo//bar`).
            let method_for_key = self.routes[new_idx].method.clone();
            let canon_path = canonical_path(&full_path);
            let key = format!("{} {}", method_for_key, canon_path);
            // Precedence guard: `match_route` consults the index *before* the
            // linear scan, so only index this static route if no earlier-
            // registered route (another static, or a parametric/wildcard pattern
            // like `/:slug` or `/static/*`) already matches the same path. When
            // one does, skip the index so the lookup falls through to the scan and
            // the first-registered route still wins — preserving the adapter's
            // existing first-match semantics. `or_insert` keeps the first static
            // registration on exact-duplicate keys for the same reason.
            let shadowed_by_prior = self.routes[..new_idx].iter().any(|route| {
                route.method == method_for_key && route.pattern.match_path(&canon_path).is_some()
            });
            if !shadowed_by_prior {
                self.static_index.entry(key).or_insert(new_idx);
            }
        }
    }

    pub fn add_hook(&mut self, hook_name: &str, handler: ClosurePtr) {
        match hook_name {
            "onRequest" => self.hooks.on_request.push(handler),
            "preParsing" => self.hooks.pre_parsing.push(handler),
            "preValidation" => self.hooks.pre_validation.push(handler),
            "preHandler" => self.hooks.pre_handler.push(handler),
            "preSerialization" => self.hooks.pre_serialization.push(handler),
            "onSend" => self.hooks.on_send.push(handler),
            "onResponse" => self.hooks.on_response.push(handler),
            "onError" => self.hooks.on_error.push(handler),
            _ => eprintln!("Unknown hook: {}", hook_name),
        }
    }

    pub fn set_error_handler(&mut self, handler: ClosurePtr) {
        self.error_handler = Some(handler);
    }

    /// Resolve a request `(method, path)` to its handler and extracted path
    /// params. Fully-static routes resolve in O(1) through `static_index`;
    /// everything else (and any static route the index deliberately omits to
    /// preserve precedence — see `add_route`) falls through to a linear scan that
    /// returns the first registered match.
    pub fn match_route(
        &self,
        method: &str,
        path: &str,
    ) -> Option<(&Route, HashMap<String, String>)> {
        // Normalize the method the same way `add_route` does (it stores
        // `to_uppercase()`), so a lowercase/mixed-case caller hits both the index
        // and the scan rather than silently missing. Requests off the hyper accept
        // loop are already upper-case (the hot path), so only allocate when a
        // direct caller actually passes a lower-case letter.
        let method_upper: std::borrow::Cow<'_, str> =
            if method.bytes().any(|b| b.is_ascii_lowercase()) {
                std::borrow::Cow::Owned(method.to_uppercase())
            } else {
                std::borrow::Cow::Borrowed(method)
            };
        // Canonicalize the path (drops the query string + redundant slashes) the
        // same way `add_route` builds the key and `RoutePattern::match_path`
        // normalizes on the scan side, so e.g. `/ping?x=1` still hits the static
        // `/ping` index entry.
        let canon_path = canonical_path(path);
        let key = format!("{} {}", method_upper, canon_path);
        // Fast path: fully-static routes resolve in O(1) via `static_index`.
        if let Some(&idx) = self.static_index.get(&key) {
            // Static patterns never extract path params — return an empty map,
            // matching `RoutePattern::match_path`'s behaviour for static patterns.
            return Some((&self.routes[idx], HashMap::new()));
        }
        // Slow path: parametric/wildcard routes still go through the linear scan,
        // preserving first-registered-wins semantics.
        for route in &self.routes {
            if route.method == *method_upper {
                if let Some(params) = route.pattern.match_path(path) {
                    return Some((route, params));
                }
            }
        }
        None
    }
}

impl Default for FastifyApp {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// FFI: app creation
// ============================================================================

/// Strip a NaN-box `POINTER_TAG` envelope off a closure pointer if
/// present. Codegen sometimes hands us the raw pointer, sometimes the
/// NaN-boxed form — we accept both.
fn strip_pointer_tag(value: i64) -> i64 {
    if (value as u64 & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        (value as u64 & PTR_MASK) as i64
    } else {
        value
    }
}

/// Pull a string field out of an options object via perry-ffi's
/// `json_stringify` round-trip. Same trick mongodb / http use.
unsafe fn opts_string(opts_f64: f64, key: &str) -> Option<String> {
    let v = JsValue::from_bits(opts_f64.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    let json = perry_ffi::json_stringify(v)?;
    let parsed: serde_json::Value = serde_json::from_str(&json).ok()?;
    parsed.get(key).and_then(|v| v.as_str()).map(String::from)
}

unsafe fn opts_bool(opts_f64: f64, key: &str) -> Option<bool> {
    let v = JsValue::from_bits(opts_f64.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    let json = perry_ffi::json_stringify(v)?;
    let parsed: serde_json::Value = serde_json::from_str(&json).ok()?;
    parsed.get(key).and_then(|v| v.as_bool())
}

unsafe fn opts_u64(opts_f64: f64, key: &str) -> Option<u64> {
    let v = JsValue::from_bits(opts_f64.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    let json = perry_ffi::json_stringify(v)?;
    let parsed: serde_json::Value = serde_json::from_str(&json).ok()?;
    parsed.get(key).and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_i64().map(|n| n.max(0) as u64))
            .or_else(|| v.as_f64().map(|n| n.max(0.0) as u64))
    })
}

/// `Fastify()` — create a new application with default config.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_create() -> Handle {
    ensure_gc_scanner_registered();
    register_handle(FastifyApp::new())
}

/// `Fastify({ logger, trustProxy, bodyLimit })` — create with options.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_create_with_opts(opts_f64: f64) -> Handle {
    ensure_gc_scanner_registered();
    let mut config = FastifyConfig::default();
    if let Some(b) = opts_bool(opts_f64, "logger") {
        config.logger = b;
    }
    if let Some(b) = opts_bool(opts_f64, "trustProxy") {
        config.trust_proxy = b;
    }
    if let Some(n) = opts_u64(opts_f64, "bodyLimit") {
        config.body_limit = Some(n as usize);
    }
    let mut app = FastifyApp::new();
    app.config = config;
    register_handle(app)
}

// ============================================================================
// FFI: route registration
// ============================================================================

/// Internal helper — append a route to the app.
unsafe fn register_route(app_handle: Handle, method: &str, path: i64, handler: i64) -> bool {
    let path_str = match string_from_nanboxed(path) {
        Some(p) => p,
        None => return false,
    };
    let raw_handler = strip_pointer_tag(handler);
    if let Some(app) = get_handle_mut::<FastifyApp>(app_handle) {
        app.add_route(method, &path_str, raw_handler);
        return true;
    }
    false
}

/// `app.get(path, handler)`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_get(app: Handle, path: i64, handler: i64) -> bool {
    register_route(app, "GET", path, handler)
}

/// `app.post(path, handler)`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_post(app: Handle, path: i64, handler: i64) -> bool {
    register_route(app, "POST", path, handler)
}

/// `app.put(path, handler)`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_put(app: Handle, path: i64, handler: i64) -> bool {
    register_route(app, "PUT", path, handler)
}

/// `app.delete(path, handler)`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_delete(app: Handle, path: i64, handler: i64) -> bool {
    register_route(app, "DELETE", path, handler)
}

/// `app.patch(path, handler)`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_patch(app: Handle, path: i64, handler: i64) -> bool {
    register_route(app, "PATCH", path, handler)
}

/// `app.head(path, handler)`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_head(app: Handle, path: i64, handler: i64) -> bool {
    register_route(app, "HEAD", path, handler)
}

/// `app.options(path, handler)`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_options(app: Handle, path: i64, handler: i64) -> bool {
    register_route(app, "OPTIONS", path, handler)
}

/// `app.all(path, handler)` — registers the same handler under every
/// HTTP method.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_all(app: Handle, path: i64, handler: i64) -> bool {
    let methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
    let mut ok = true;
    for m in methods {
        if !register_route(app, m, path, handler) {
            ok = false;
        }
    }
    ok
}

/// `app.route({ method, url, handler })` — generic dispatch with
/// caller-supplied method. The first arg here is the method as a
/// NaN-boxed string; we support uppercased / lowercased variants.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_route(
    app: Handle,
    method: i64,
    path: i64,
    handler: i64,
) -> bool {
    let method = match string_from_nanboxed(method) {
        Some(m) => m.to_uppercase(),
        None => return false,
    };
    register_route(app, &method, path, handler)
}

// ============================================================================
// FFI: hooks
// ============================================================================

/// `app.addHook(event, handler)` — registers a lifecycle hook.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_add_hook(app: Handle, hook_name: i64, handler: i64) -> bool {
    let name = match string_from_nanboxed(hook_name) {
        Some(n) => n,
        None => return false,
    };
    let raw_handler = strip_pointer_tag(handler);
    if let Some(app) = get_handle_mut::<FastifyApp>(app) {
        app.add_hook(&name, raw_handler);
        return true;
    }
    false
}

// ============================================================================
// FFI: error handler
// ============================================================================

/// `app.setErrorHandler(fn)`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_set_error_handler(app: Handle, handler: i64) -> bool {
    let raw = strip_pointer_tag(handler);
    if let Some(app) = get_handle_mut::<FastifyApp>(app) {
        app.set_error_handler(raw);
        return true;
    }
    false
}

// ============================================================================
// FFI: plugins
// ============================================================================

/// `app.register(plugin, opts?)` — invokes the plugin synchronously
/// with the parent app handle and the options object. Plugin routes
/// are registered onto the parent app under any `opts.prefix` value
/// (the prefix is temporarily appended to the parent app's prefix
/// while the plugin runs, then restored).
#[no_mangle]
pub unsafe extern "C" fn js_fastify_register(app_handle: Handle, plugin: i64, opts: f64) -> bool {
    let plugin_prefix = opts_string(opts, "prefix").unwrap_or_default();

    // Save old prefix and set the combined prefix on the parent app.
    let old_prefix = match get_handle_mut::<FastifyApp>(app_handle) {
        Some(app) => {
            let old = app.prefix.clone();
            app.prefix = if old.is_empty() {
                plugin_prefix.clone()
            } else if plugin_prefix.is_empty() {
                old.clone()
            } else {
                format!("{}{}", old, plugin_prefix)
            };
            old
        }
        None => return false,
    };

    // NaN-box the parent app handle so the plugin's method dispatch
    // (e.g. `app.get(...)`) sees a POINTER_TAG'd JS handle the
    // codegen-side dispatcher knows how to unbox.
    let nanboxed_app = f64::from_bits(POINTER_TAG | (app_handle as u64 & PTR_MASK));

    // Strip a NaN-box wrapper if codegen handed us one.
    let raw_closure = if (plugin as u64 & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        (plugin as u64 & PTR_MASK) as *const RawClosureHeader
    } else {
        plugin as *const RawClosureHeader
    };

    let closure = JsClosure::from_raw(raw_closure);
    if !closure.is_null() {
        let _ = closure.call2(nanboxed_app, opts);
    }

    // Restore the old prefix.
    if let Some(app) = get_handle_mut::<FastifyApp>(app_handle) {
        app.prefix = old_prefix;
    }

    true
}

// ============================================================================
// FFI: #1113 — app.server EventEmitter (minimum-viable shape)
// ============================================================================

/// `app.server` — Node-compatible getter that returns an
/// object-shaped value supporting `.on(event, cb)`.
///
/// In Node, `app.server` is the underlying `http.Server` (or
/// `http2.Http2Server`) which `extends EventEmitter`. The canonical
/// pattern for attaching a WebSocket server without opening a second
/// port is:
///
/// ```ts
/// import { WebSocketServer } from "ws";
/// const wss = new WebSocketServer({ noServer: true });
/// app.server.on("upgrade", (req, socket, head) => {
///   wss.handleUpgrade(req, socket, head, (sock) => { … });
/// });
/// ```
///
/// Pre-fix `app.server` fell through to the generic property getter
/// on the FastifyApp handle and returned the raw handle id as a JS
/// number — so `typeof app.server` was `"number"` and `app.server.on`
/// was `undefined`. The user-side `.on("upgrade", …)` call then threw
/// `(number).on is not a function` and the boot `catch` arm logged
/// `fatal:` and exited.
///
/// We return the same FastifyApp handle id — the codegen-side
/// `NATIVE_MODULE_TABLE` arm at `module: "fastify", method: "server"`
/// NaN-boxes it via `NR_PTR` before it reaches the JS world. Perry's
/// `typeof` against `POINTER_TAG` returns `"object"` and `.on(…)`
/// routes through the same FastifyApp method dispatch (the
/// `"on"` arm in `js_fastify_app_on` below).
///
/// **Today only stores the handler list** — the hyper accept loop in
/// `server.rs` doesn't yet route `Upgrade:` requests through
/// `hyper::upgrade::on(req)` and hand the raw socket + head bytes
/// back to TypeScript, so registered upgrade handlers never fire.
/// Full bidirectional WebSocket upgrade dispatch through
/// `perry-ext-ws`'s `noServer` mode is tracked as the #1113
/// follow-up. The diagnostic line emitted from `server.rs` at
/// request-dispatch time tells the user when an Upgrade arrived
/// despite the gap.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_app_server(app_handle: Handle) -> Handle {
    app_handle
}

/// `app.server.on(event, cb)` — register an event handler. Only
/// `"upgrade"` is meaningful today; other event names
/// (`"connection"`, `"error"`, `"listening"`, …) are silently
/// accepted so boot-time registrations don't crash, but the
/// hyper accept loop doesn't currently fire them. See
/// `js_fastify_app_server` for the broader rationale.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_app_on(app_handle: Handle, event: i64, callback: i64) {
    if callback == 0 {
        return;
    }
    let event_name = match string_from_nanboxed(event) {
        Some(s) => s,
        None => return,
    };
    let raw_cb = strip_pointer_tag(callback);
    if event_name == "upgrade" {
        if let Some(app) = get_handle_mut::<FastifyApp>(app_handle) {
            app.upgrade_handlers.push(raw_cb);
        }
    }
    // Other event names (e.g. "connection", "error", "listening") are
    // silently accepted — see the function-level comment.
}

// Suppress unused-import warnings for FFI surface re-exports the
// caller's code may need at link time.
#[allow(dead_code)]
fn _link_keepalive() -> Option<*mut ObjectHeader> {
    None
}
#[allow(dead_code)]
fn _link_string_keepalive() -> Option<*mut StringHeader> {
    None
}
