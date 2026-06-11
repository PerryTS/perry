//! Fastify application creation and route registration

use perry_runtime::{js_string_from_bytes, JSValue, StringHeader};

use super::context::string_from_nanboxed;
use super::{ensure_gc_scanner_registered, FastifyApp, FastifyConfig};
use crate::common::{get_handle_mut, register_handle, Handle};

// ============================================================================
// App Creation
// ============================================================================

/// Create a new Fastify application
#[no_mangle]
pub unsafe extern "C" fn js_fastify_create() -> Handle {
    ensure_gc_scanner_registered();
    register_handle(FastifyApp::new())
}

/// Create a new Fastify application with options
/// Options object fields:
/// - logger: boolean
/// - trustProxy: boolean
/// - bodyLimit: number
#[no_mangle]
pub unsafe extern "C" fn js_fastify_create_with_opts(opts: f64) -> Handle {
    ensure_gc_scanner_registered();
    let mut config = FastifyConfig::default();

    // Parse options if it's an object
    let jsv = JSValue::from_bits(opts.to_bits());
    if jsv.is_pointer() {
        let ptr = jsv.as_pointer::<perry_runtime::ObjectHeader>();

        // Check logger field
        let logger_key = js_string_from_bytes(b"logger".as_ptr(), 6);
        let logger_val = perry_runtime::js_object_get_field_by_name_f64(ptr, logger_key);
        let logger_jsv = JSValue::from_bits(logger_val.to_bits());
        if logger_jsv.is_bool() && logger_jsv.as_bool() {
            config.logger = true;
        }

        // Check trustProxy field
        let trust_key = js_string_from_bytes(b"trustProxy".as_ptr(), 10);
        let trust_val = perry_runtime::js_object_get_field_by_name_f64(ptr, trust_key);
        let trust_jsv = JSValue::from_bits(trust_val.to_bits());
        if trust_jsv.is_bool() && trust_jsv.as_bool() {
            config.trust_proxy = true;
        }

        // Check bodyLimit field
        let limit_key = js_string_from_bytes(b"bodyLimit".as_ptr(), 9);
        let limit_val = perry_runtime::js_object_get_field_by_name_f64(ptr, limit_key);
        let limit_jsv = JSValue::from_bits(limit_val.to_bits());
        if limit_jsv.is_number() {
            config.body_limit = Some(limit_val as usize);
        }
    }

    let mut app = FastifyApp::new();
    app.config = config;
    register_handle(app)
}

// ============================================================================
// Route Registration
// ============================================================================

/// Register a GET route
#[no_mangle]
pub unsafe extern "C" fn js_fastify_get(app_handle: Handle, path: i64, handler: i64) -> bool {
    register_route(app_handle, "GET", path, handler)
}

/// Register a POST route
#[no_mangle]
pub unsafe extern "C" fn js_fastify_post(app_handle: Handle, path: i64, handler: i64) -> bool {
    register_route(app_handle, "POST", path, handler)
}

/// Register a PUT route
#[no_mangle]
pub unsafe extern "C" fn js_fastify_put(app_handle: Handle, path: i64, handler: i64) -> bool {
    register_route(app_handle, "PUT", path, handler)
}

/// Register a DELETE route
#[no_mangle]
pub unsafe extern "C" fn js_fastify_delete(app_handle: Handle, path: i64, handler: i64) -> bool {
    register_route(app_handle, "DELETE", path, handler)
}

/// Register a PATCH route
#[no_mangle]
pub unsafe extern "C" fn js_fastify_patch(app_handle: Handle, path: i64, handler: i64) -> bool {
    register_route(app_handle, "PATCH", path, handler)
}

/// Register a HEAD route
#[no_mangle]
pub unsafe extern "C" fn js_fastify_head(app_handle: Handle, path: i64, handler: i64) -> bool {
    register_route(app_handle, "HEAD", path, handler)
}

/// Register an OPTIONS route
#[no_mangle]
pub unsafe extern "C" fn js_fastify_options(app_handle: Handle, path: i64, handler: i64) -> bool {
    register_route(app_handle, "OPTIONS", path, handler)
}

/// Register a route for all methods
#[no_mangle]
pub unsafe extern "C" fn js_fastify_all(app_handle: Handle, path: i64, handler: i64) -> bool {
    let methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
    let mut success = true;
    for method in methods {
        if !register_route(app_handle, method, path, handler) {
            success = false;
        }
    }
    success
}

/// Register a route with any method (generic)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_route(
    app_handle: Handle,
    method: i64,
    path: i64,
    handler: i64,
) -> bool {
    let method_str = match string_from_nanboxed(method) {
        Some(m) => m.to_uppercase(),
        None => return false,
    };
    register_route(app_handle, &method_str, path, handler)
}

/// Internal helper to register a route
unsafe fn register_route(app_handle: Handle, method: &str, path: i64, handler: i64) -> bool {
    let path_str = match string_from_nanboxed(path) {
        Some(p) => p,
        None => return false,
    };

    // Strip NaN-box tag from handler closure pointer if needed
    let raw_handler = if (handler as u64 & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
        (handler as u64 & 0x0000_FFFF_FFFF_FFFF) as i64
    } else {
        handler
    };

    if let Some(app) = get_handle_mut::<FastifyApp>(app_handle) {
        app.add_route(method, &path_str, raw_handler);
        return true;
    }
    false
}

// ============================================================================
// Hooks
// ============================================================================

/// Add a lifecycle hook
#[no_mangle]
pub unsafe extern "C" fn js_fastify_add_hook(
    app_handle: Handle,
    hook_name: i64,
    handler: i64,
) -> bool {
    let name = match string_from_nanboxed(hook_name) {
        Some(n) => n,
        None => return false,
    };

    // Strip NaN-box tag from handler closure pointer if needed
    let raw_handler = if (handler as u64 & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
        (handler as u64 & 0x0000_FFFF_FFFF_FFFF) as i64
    } else {
        handler
    };

    if let Some(app) = get_handle_mut::<FastifyApp>(app_handle) {
        app.add_hook(&name, raw_handler);
        return true;
    }
    false
}

// ============================================================================
// Error Handler
// ============================================================================

/// Set custom error handler
#[no_mangle]
pub unsafe extern "C" fn js_fastify_set_error_handler(app_handle: Handle, handler: i64) -> bool {
    // Strip NaN-box tag from handler closure pointer if needed
    let raw_handler = if (handler as u64 & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
        (handler as u64 & 0x0000_FFFF_FFFF_FFFF) as i64
    } else {
        handler
    };

    if let Some(app) = get_handle_mut::<FastifyApp>(app_handle) {
        app.set_error_handler(raw_handler);
        return true;
    }
    false
}

// ============================================================================
// Plugins
// ============================================================================

/// Register a plugin
/// plugin: closure that receives (fastify, opts) and can register routes
/// opts: options object (optional, may contain prefix)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_register(app_handle: Handle, plugin: i64, opts: f64) -> bool {
    // Extract prefix from opts if present
    let mut plugin_prefix = String::new();
    let jsv = JSValue::from_bits(opts.to_bits());
    if jsv.is_pointer() {
        let ptr = jsv.as_pointer::<perry_runtime::ObjectHeader>();
        let prefix_key = js_string_from_bytes(b"prefix".as_ptr(), 6);
        let prefix_val = perry_runtime::js_object_get_field_by_name_f64(ptr, prefix_key);
        if let Some(p) = extract_jsvalue_string(prefix_val) {
            plugin_prefix = p;
        }
    }

    // Save old prefix and set the combined prefix on the MAIN app.
    // Plugin routes will register directly on the main app handle (handle=1)
    // using the temporarily-set prefix, which add_route() prepends automatically.
    let old_prefix = {
        if let Some(app) = get_handle_mut::<FastifyApp>(app_handle) {
            let old = app.prefix.clone();
            app.prefix = if old.is_empty() {
                plugin_prefix.clone()
            } else if plugin_prefix.is_empty() {
                old.clone()
            } else {
                format!("{}{}", old, plugin_prefix)
            };
            old
        } else {
            return false;
        }
    };

    // NaN-box the MAIN app handle so Perry's runtime dispatches method calls on it
    let nanboxed_main =
        f64::from_bits(0x7FFD_0000_0000_0000 | (app_handle as u64 & 0x0000_FFFF_FFFF_FFFF));

    // Strip NaN-box tag from plugin closure pointer if needed
    let raw_closure_ptr = if (plugin as u64 & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
        (plugin as u64 & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::ClosureHeader
    } else {
        plugin as *const perry_runtime::ClosureHeader
    };

    // Call the plugin — async functions run the body synchronously and return a Promise
    perry_runtime::js_closure_call2(raw_closure_ptr, nanboxed_main, opts);

    // Flush the microtask queue (in case any async work was deferred)
    perry_runtime::js_promise_run_microtasks();

    // Restore the old prefix
    if let Some(app) = get_handle_mut::<FastifyApp>(app_handle) {
        app.prefix = old_prefix;
    }

    true
}

/// Helper to extract string from JSValue
unsafe fn extract_jsvalue_string(value: f64) -> Option<String> {
    let ptr = perry_runtime::js_get_string_pointer_unified(value);
    if ptr == 0 {
        return None;
    }
    let len = (*(ptr as *const StringHeader)).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// #1113: `app.server` — Node-compatible getter that returns an
/// object-shaped value supporting `.on("upgrade", cb)` etc.
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
/// Pre-fix `app.server` fell through to the generic property getter on
/// the FastifyApp handle and returned the raw handle id as a JS
/// number — so `typeof app.server` was `"number"` and `app.server.on`
/// was `undefined`. The user-side `.on("upgrade", …)` call then threw
/// `(number).on is not a function` and the program's boot `catch`
/// arm logged `fatal:` and exited.
///
/// The minimum fix is to expose `app.server` as a value that
/// supports `.on(eventName, cb)`. Storing the upgrade handler list
/// inside the FastifyApp itself lets us reuse the existing handle
/// dispatch — `app.server` returns the same handle id pointer-tagged
/// so `typeof` resolves to `"object"`, `.on` routes through
/// `js_handle_method_dispatch` -> `dispatch_fastify_app` -> the new
/// `"on"` arm, and `.on("upgrade", cb)` pushes the closure into
/// `app.upgrade_handlers`. See the corresponding doc comment on
/// `FastifyApp::upgrade_handlers` for the follow-up still owed:
/// the hyper accept loop doesn't yet route incoming `Upgrade:`
/// requests through `hyper::upgrade::on(req)` and hand the raw socket
/// + head bytes back to TS — so today registered handlers never
/// actually fire. The diagnostic at the request-dispatch site in
/// `server.rs` makes the gap visible at runtime; full bidirectional
/// upgrade dispatch through `perry-ext-ws`'s `noServer` mode is
/// the tracked follow-up.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_app_server(app_handle: Handle) -> Handle {
    // Return the same FastifyApp handle id. The codegen-side
    // `NativeModSig.ret = NR_PTR` arm in `NATIVE_MODULE_TABLE` (see
    // `lower_call.rs`) NaN-boxes the i64 with POINTER_TAG before
    // it reaches the JS world. Perry's typeof against POINTER_TAG
    // returns `"object"` (matches Node's
    // `typeof http.Server === "object"`); `.on(…)` then dispatches
    // through HANDLE_METHOD_DISPATCH which sees the FastifyApp at
    // this handle id and routes into the `"on"` arm in
    // `dispatch_fastify_app`.
    app_handle
}

/// #1113: register an event handler on the fastify app's underlying
/// "server" surface (`app.server.on(event, cb)` — see
/// `js_fastify_app_server` above for why `app.server` shares the
/// FastifyApp handle).
///
/// Only `"upgrade"` is meaningful today — handlers for other event
/// names ("connection", "error", "listening", …) are silently
/// accepted so user code paths that register them at boot don't
/// crash, but the hyper accept loop doesn't currently fire them.
/// Same diagnostic gap as `upgrade` itself — full EventEmitter
/// parity for `app.server` is a follow-up.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_app_on(app_handle: Handle, event_ptr: i64, callback: i64) {
    if event_ptr == 0 || callback == 0 {
        return;
    }
    let header = event_ptr as *const StringHeader;
    let len = (*header).byte_len as usize;
    let data = (event_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let event = std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("");

    if event == "upgrade" {
        // Warn at registration, not first upgrade request: storing the
        // callback used to be silent, and a registered-but-never-fired
        // handler is exactly the "works at boot, breaks in production"
        // lie #4917 targets. Upgrade requests themselves get a loud 501
        // in server.rs until #1113 lands real dispatch.
        perry_runtime::stub_diag::perry_stub_warn(
            "fastify app.server.on('upgrade')",
            "the handler is stored but never fires; upgrade requests get 501",
            Some("#1113"),
        );
        if let Some(app) = get_handle_mut::<FastifyApp>(app_handle) {
            app.upgrade_handlers.push(callback);
        }
    }
    // Other event names (e.g. "connection", "error", "listening") are
    // silently accepted — see the function-level comment.
}
