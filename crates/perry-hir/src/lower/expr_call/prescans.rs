//! Pre-scan helpers for `lower_call_inner` — register native-instance
//! tags for handler params (fastify, http, ws, streams) and run the
//! perry/ui reactive desugars BEFORE the call's args are lowered.
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move; the only
//! consumer is `lower_call_inner` inside this module.

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::Expr;
use crate::lower_patterns::{
    pre_scan_fastify_handler_params, pre_scan_node_http_client_callback_params,
    pre_scan_node_http_client_request_socket_params, pre_scan_node_http_create_server_params,
    pre_scan_node_http_upgrade_params,
};

use super::super::{try_desugar_reactive_animate, try_desugar_reactive_text, LoweringContext};
use super::stream::register_super_stream_controller_params;

/// Run all pre-scan registrations and reactive desugars that must
/// happen BEFORE this call's args are lowered. Returns `Some(expr)`
/// if a reactive desugar fired (caller should return that directly);
/// otherwise `None` and the caller continues with normal lowering.
pub(super) fn run_call_prescans(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
) -> Result<Option<Expr>> {
    // Pre-scan: if this call is `<fastify app>.get|post|...|addHook(path, handler)`,
    // the handler is an arrow function whose first two params are
    // the FastifyRequest and FastifyReply. Register them as native
    // instances BEFORE lowering the arrow so that `request.header(...)`
    // and `request.headers[...]` inside the handler dispatch through
    // `Expr::NativeMethodCall` instead of generic object access.
    //
    // In v0.4.51 this was (presumably) handled by the old codegen's
    // per-method dispatch table; in v0.5.x the dispatch happens at
    // HIR lower time via `lookup_native_instance(name)`, so we need
    // the annotation here for the lookup to succeed.
    let fastify_handler_names: Option<(String, String)> =
        pre_scan_fastify_handler_params(ctx, call);
    if let Some((req_name, reply_name)) = &fastify_handler_names {
        ctx.register_native_instance(
            req_name.clone(),
            "fastify".to_string(),
            "Request".to_string(),
        );
        if !reply_name.is_empty() {
            ctx.register_native_instance(
                reply_name.clone(),
                "fastify".to_string(),
                "Reply".to_string(),
            );
        }
    }

    // Issue #577 — `http.createServer((req, res) => …)` /
    // `https.createServer({...}, (req, res) => …)` /
    // `http2.createSecureServer({...}, (req, res) => …)`. Register
    // the (req, res) handler params as IncomingMessage /
    // ServerResponse native instances BEFORE the arrow body is
    // lowered, so `req.method` / `res.end(...)` inside the handler
    // dispatch through NATIVE_MODULE_TABLE instead of falling
    // through to generic property access.
    if let Some((req_name, res_name)) = pre_scan_node_http_create_server_params(ctx, call) {
        ctx.register_native_instance(req_name, "http".to_string(), "IncomingMessage".to_string());
        ctx.register_native_instance(res_name, "http".to_string(), "ServerResponse".to_string());
    }

    // Issue #1124 followup — `http.get(url, (res) => …)` /
    // `http.request(opts, (res) => …)` / `https.{get,request}`.
    // Register the `res` arrow param as a `("http",
    // "IncomingMessage")` native instance BEFORE the arrow body is
    // lowered, so `res.on('data', cb)` / `res.on('end', cb)` inside
    // the callback dispatch through NATIVE_MODULE_TABLE's
    // class_filter = Some("IncomingMessage") rows (which call
    // `js_node_http_im_on` — the same dispatcher the server-side
    // request handler's `req.on(...)` uses, which fans out to
    // perry-ext-http's `js_http_on` for client-side IncomingMessage
    // handles too via the cross-module on-listener path).
    if let Some(res_name) = pre_scan_node_http_client_callback_params(ctx, call) {
        // Protect only when registration actually happened — it no-ops under a
        // `perry.compilePackages` override, and protecting without a fresh tag
        // would let a later same-named param skip tombstoning a stale one.
        if ctx.register_native_instance(
            res_name.clone(),
            "http".to_string(),
            "IncomingMessage".to_string(),
        ) {
            ctx.protect_native_param(res_name);
        }
    }

    // Issue #577 Phase 4 — `httpServer.on('upgrade', (req, wsId, head) => …)`
    // — register `wsId` as a `("ws", "Client")` native instance BEFORE
    // the arrow body is lowered, so `wsId.send(...)` / `wsId.on(...)` /
    // `wsId.close()` inside the handler dispatch via the Client-class
    // entries in NATIVE_MODULE_TABLE.
    if let Some(ws_id_name) = pre_scan_node_http_upgrade_params(ctx, call) {
        // The arrow's own `wsId` param binding would otherwise tombstone this
        // fresh tag via shadow_native_instance_if_present — protect it so
        // `wsId.send`/`.on` inside the handler keep the Client-class dispatch.
        // Protect only when registration actually happened (it no-ops under a
        // compile-package override), else a later same-named param would skip
        // tombstoning a genuinely stale tag.
        if ctx.register_native_instance(ws_id_name.clone(), "ws".to_string(), "Client".to_string())
        {
            ctx.protect_native_param(ws_id_name);
        }
    }

    // Issue #2211 — `request.on('socket', sock => …)` on a `ClientRequest`
    // hands the consumer the underlying TCP socket; pre-tag the arrow param
    // as a `("net", "Socket")` native instance so EventEmitter introspection
    // (`sock.listeners('timeout')`, `sock.eventNames()`, etc.) inside the
    // handler dispatches through the class-filtered Socket rows in
    // NATIVE_MODULE_TABLE.
    if let Some(sock_name) = pre_scan_node_http_client_request_socket_params(ctx, call) {
        // Protect only on successful registration (no-ops under a
        // compile-package override) — see the `wsId` site above.
        if ctx.register_native_instance(sock_name.clone(), "net".to_string(), "Socket".to_string())
        {
            ctx.protect_native_param(sock_name);
        }
    }

    // perry/ui reactive Text: `Text(\`...${state.value}...\`)` where at least one
    // interpolation is `<ident>.value` on a State binding. Desugars to
    // `{ __h = Text(concat); stateOnChange(state, v => textSetString(__h, concat)); __h }`
    // so the label updates when state.set(...) fires subscribers. Closes #104.
    if let Some(desugared) = try_desugar_reactive_text(ctx, call)? {
        return Ok(Some(desugared));
    }

    // perry/ui reactive animation: `widget.animateOpacity(<expr reading
    // state.value>, dur)` or `.animatePosition(...)` desugars to an IIFE
    // that runs the initial animation and registers a `stateOnChange`
    // subscriber per referenced State so the animation re-fires when
    // any read state changes. Closes the follow-up to #109.
    if let Some(desugared) = try_desugar_reactive_animate(ctx, call)? {
        return Ok(Some(desugared));
    }

    // Issue #562: `super({ start, pull, transform, flush, ... })` inside
    // a class extending `ReadableStream` / `TransformStream` —
    // pre-register the controller param of each callback as a
    // `readable_stream` native instance BEFORE the args are lowered, so
    // `controller.enqueue(...)` inside those callback bodies dispatches
    // through the streams arms in `lower_call.rs`. Mirrors the existing
    // pre-scan in `expr_new.rs::lower_new` for `new ReadableStream({...})`
    // / `new TransformStream({...})`. Idempotent for non-matching shapes.
    if matches!(&call.callee, ast::Callee::Super(_)) {
        if let Some(parent_ident) = ctx.current_class_super_ident.clone() {
            register_super_stream_controller_params(ctx, &parent_ident, call);
        }
    }

    Ok(None)
}
