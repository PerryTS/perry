//! Function call expression lowering: `ast::Expr::Call`.
//!
//! Tier 2.3 round 4 (v0.5.339) — extracts the 3,986-LOC `Call` arm
//! from `lower_expr`. By far the largest single arm in the entire
//! codebase. This is a giant dispatcher: figure out what's being
//! called (built-in like Math.floor, native module method like
//! `mysql.query()`, user function, closure, etc.) and emit the right
//! HIR variant.
//!
//! Pattern matches the prior expr_*.rs extractions: free
//! `pub(super) fn` entry, recursion through `super::lower_expr`.
//! Module is intentionally one big function; further sub-extraction
//! by call category (Math / JSON / fetch / native / class-static /
//! …) is a follow-up — splitting them all in a single PR would
//! balloon the diff and the borrow-checker dance is non-trivial.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_patterns::{
    detect_native_instance_expr, pre_scan_fastify_handler_params,
    pre_scan_node_http_create_server_params, pre_scan_node_http_upgrade_params,
};
use crate::lower_types::extract_ts_type_with_ctx;

use super::{
    extract_typed_parse_source_order, is_generator_call_expr, is_widget_modifier_name, lower_expr,
    resolve_typed_parse_ty, try_desugar_reactive_animate, try_desugar_reactive_text,
    LoweringContext,
};

/// Build the `Expr::UrlSearchParams*` HIR variant for `<recv>.<method>(args)`.
/// Returns the populated `args` back when the (method, arity) combo doesn't
/// match a known URLSearchParams op so the caller can fall through.
///
/// Shared between the typed-local arm and the chained-`new URLSearchParams(...)`
/// arm so both surfaces dispatch identically (refs #575).
fn build_url_search_params_method_call(
    recv: Expr,
    method: &str,
    args: Vec<Expr>,
) -> std::result::Result<Expr, Vec<Expr>> {
    match method {
        "get" if !args.is_empty() => {
            let name = args.into_iter().next().unwrap();
            Ok(Expr::UrlSearchParamsGet {
                params: Box::new(recv),
                name: Box::new(name),
            })
        }
        "has" if !args.is_empty() => {
            let name = args.into_iter().next().unwrap();
            Ok(Expr::UrlSearchParamsHas {
                params: Box::new(recv),
                name: Box::new(name),
            })
        }
        "set" if args.len() >= 2 => {
            let mut iter = args.into_iter();
            let name = iter.next().unwrap();
            let value = iter.next().unwrap();
            Ok(Expr::UrlSearchParamsSet {
                params: Box::new(recv),
                name: Box::new(name),
                value: Box::new(value),
            })
        }
        "append" if args.len() >= 2 => {
            let mut iter = args.into_iter();
            let name = iter.next().unwrap();
            let value = iter.next().unwrap();
            Ok(Expr::UrlSearchParamsAppend {
                params: Box::new(recv),
                name: Box::new(name),
                value: Box::new(value),
            })
        }
        "delete" if !args.is_empty() => {
            let name = args.into_iter().next().unwrap();
            Ok(Expr::UrlSearchParamsDelete {
                params: Box::new(recv),
                name: Box::new(name),
            })
        }
        "toString" => Ok(Expr::UrlSearchParamsToString(Box::new(recv))),
        "getAll" if !args.is_empty() => {
            let name = args.into_iter().next().unwrap();
            Ok(Expr::UrlSearchParamsGetAll {
                params: Box::new(recv),
                name: Box::new(name),
            })
        }
        _ => Err(args),
    }
}

/// Issue #650: classify the static type of a method-call receiver well enough
/// to decide whether the ambiguous Date method arms (`toJSON`, `toString`,
/// `toLocaleString`, `valueOf`, `to{Date,Time,LocaleDate,LocaleTime}String`)
/// should fire. Returns `Some("Date")` for `new Date(...)` and locals typed
/// as `Date`; `Some("URL")` for `new URL(...)` and locals typed as `URL`;
/// `None` for everything else (in which case the call falls through to
/// generic dispatch). Matches receiver shapes by AST first, then by the
/// caller's `local_types` table — both source-level shapes the user typically
/// writes for these objects.
fn static_receiver_class(ctx: &LoweringContext, obj: &ast::Expr) -> Option<&'static str> {
    if let ast::Expr::New(new_expr) = obj {
        if let ast::Expr::Ident(ident) = new_expr.callee.as_ref() {
            return match ident.sym.as_ref() {
                "Date" => Some("Date"),
                "URL" => Some("URL"),
                _ => None,
            };
        }
    }
    // #809: an object literal receiver, or `Object.create(...)`, is
    // provably a plain object — never a Date. Returning `Some("Object")`
    // makes the ambiguous-Date-method gate skip the Date arms for
    // `({...}).toJSON()` / `Object.create(p).toJSON()` the same way it
    // does for URL, so the call falls through to generic dynamic dispatch
    // and finds the object's own method.
    if matches!(obj, ast::Expr::Object(_)) {
        return Some("Object");
    }
    if let ast::Expr::Call(call) = obj {
        if let ast::Callee::Expr(callee) = &call.callee {
            if let ast::Expr::Member(m) = callee.as_ref() {
                if matches!(m.obj.as_ref(), ast::Expr::Ident(o) if o.sym.as_ref() == "Object")
                    && matches!(&m.prop, ast::MemberProp::Ident(p) if p.sym.as_ref() == "create")
                {
                    return Some("Object");
                }
            }
        }
    }
    if let ast::Expr::Ident(ident) = obj {
        let name = ident.sym.as_ref();
        if ctx.plain_object_locals.contains(name) {
            return Some("Object");
        }
        if let Some(ty) = ctx.lookup_local_type(name) {
            if let Type::Named(n) = ty {
                return match n.as_str() {
                    "Date" => Some("Date"),
                    "URL" => Some("URL"),
                    _ => None,
                };
            }
        }
    }
    None
}

pub(super) fn lower_call(ctx: &mut LoweringContext, call: &ast::CallExpr) -> Result<Expr> {
    // Check if any argument has spread
    let has_spread = call.args.iter().any(|arg| arg.spread.is_some());

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

    // Issue #577 Phase 4 — `httpServer.on('upgrade', (req, wsId, head) => …)`
    // — register `wsId` as a `("ws", "Client")` native instance BEFORE
    // the arrow body is lowered, so `wsId.send(...)` / `wsId.on(...)` /
    // `wsId.close()` inside the handler dispatch via the Client-class
    // entries in NATIVE_MODULE_TABLE.
    if let Some(ws_id_name) = pre_scan_node_http_upgrade_params(ctx, call) {
        ctx.register_native_instance(ws_id_name, "ws".to_string(), "Client".to_string());
    }

    // perry/ui reactive Text: `Text(\`...${state.value}...\`)` where at least one
    // interpolation is `<ident>.value` on a State binding. Desugars to
    // `{ __h = Text(concat); stateOnChange(state, v => textSetString(__h, concat)); __h }`
    // so the label updates when state.set(...) fires subscribers. Closes #104.
    if let Some(desugared) = try_desugar_reactive_text(ctx, call)? {
        return Ok(desugared);
    }

    // perry/ui reactive animation: `widget.animateOpacity(<expr reading
    // state.value>, dur)` or `.animatePosition(...)` desugars to an IIFE
    // that runs the initial animation and registers a `stateOnChange`
    // subscriber per referenced State so the animation re-fires when
    // any read state changes. Closes the follow-up to #109.
    if let Some(desugared) = try_desugar_reactive_animate(ctx, call)? {
        return Ok(desugared);
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

    // Issue #668: AOT `require(stringLiteral)` from a user TypeScript file
    // currently lowers to `Call { callee: GlobalGet(0), ... }` (the unknown-ident
    // sentinel) and explodes at runtime as `TypeError: value is not a function`.
    // Until we wire up synthetic namespace-imports for `require(literal)`, fail
    // at compile time with a fix-it pointing at `import ...` so the user finds the
    // problem on the first build instead of the first prod request.
    if let ast::Callee::Expr(callee_expr) = &call.callee {
        if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
            // Issue #668: only enforce the compile-time error for user-written
            // source files. Many published packages (e.g. `@perryts/redis`)
            // deliberately use `require(literal)` inside a method body to break
            // import cycles; those calls only execute on opt-in code paths and
            // pre-fix simply returned undefined-and-failed-at-call-time. Failing
            // them at compile time would refuse to build any consumer of those
            // packages even if the require'd path is never reached. node_modules
            // sources keep the legacy behavior (silent fall-through to the
            // unknown-callee path) until we wire up real `require(literal)`
            // lowering.
            if !ctx.is_external_module
                && ident.sym.as_ref() == "require"
                && ctx.lookup_local("require").is_none()
                && ctx.lookup_func("require").is_none()
                && ctx.lookup_imported_func("require").is_none()
                && call.args.len() == 1
                && call.args[0].spread.is_none()
            {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = call.args[0].expr.as_ref() {
                    let spec = s.value.as_str().unwrap_or("");
                    // #925: when we have a module-specific hint (e.g.
                    // distinguishing "this is in stdlib, just swap to
                    // ESM" from "this isn't shimmed at all"), append it.
                    let hint = super::unimpl_hints::require_module_hint(spec)
                        .map(|h| format!(" {h}"))
                        .unwrap_or_default();
                    crate::lower_bail!(
                        call.span,
                        "CommonJS `require(\"{}\")` is not supported under `perry compile` \
                         — use a static `import` instead \
                         (e.g. `import * as m from \"{}\"` \
                         or `import {{ x }} from \"{}\"`). Closes #668.{}",
                        spec,
                        spec,
                        spec,
                        hint,
                    );
                }
            }
        }
    }

    // Issue #76 — `embedWasm("./file.wasm")` from `perry/build` is a
    // compile-time intrinsic that bakes the file's bytes directly into the
    // produced binary. Resolves the path relative to the current source
    // file (matches the maintainer's preferred MVP shape vs. the in-flight
    // import-attributes proposal). The argument MUST be a string literal —
    // dynamic paths defeat the whole purpose. Unknown failure (file not
    // found, etc.) bails the compile with a clear error.
    if let ast::Callee::Expr(callee_expr) = &call.callee {
        if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
            if ident.sym.as_ref() == "embedWasm"
                && ctx.lookup_local("embedWasm").is_none()
                && ctx.lookup_func("embedWasm").is_none()
                && call.args.len() == 1
                && call.args[0].spread.is_none()
            {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = call.args[0].expr.as_ref() {
                    let rel: String = s.value.as_str().unwrap_or("").to_string();
                    let base_dir = std::path::Path::new(&ctx.source_file_path)
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    let resolved = base_dir.join(&rel);
                    let bytes = std::fs::read(&resolved).map_err(|e| {
                        anyhow::anyhow!(
                            "embedWasm(\"{}\") failed to read {}: {}",
                            rel,
                            resolved.display(),
                            e
                        )
                    })?;
                    let elems: Vec<Expr> = bytes.iter().map(|b| Expr::Number(*b as f64)).collect();
                    return Ok(Expr::Uint8ArrayNew(Some(Box::new(Expr::Array(elems)))));
                }
                crate::lower_bail!(
                    call.span,
                    "embedWasm(...) requires a string-literal path argument so the bytes can be embedded at compile time"
                );
            }
        }
    }

    // Issue #957 — `(function(...) { ... }.call(<thisArg>, ...args))` IIFE
    // pattern used at the top of older CJS packages (lodash, underscore, and
    // every package that copies their UMD prelude). Pre-fix the inner
    // function expression lowers to a Closure, then `.call(thisArg, ...args)`
    // falls through to `js_native_call_method` on the closure handle which
    // doesn't recognize Function.prototype.call — the body never runs and
    // mutations to outer captures (e.g. `module.exports = _` inside the
    // wrap) are silently dropped, so `import _ from "lodash"` resolves to
    // `undefined` and `_.add` throws. Rewrite the AST shape directly to a
    // plain Call on the inner function expression, dropping the thisArg.
    //
    // Conservative scope: only fires when the callee's receiver is a
    // FunctionExpression or ArrowExpression literal AND the inner function
    // does NOT reference `this` (`captures_this == false` after lowering).
    // Method dispatch like `obj.fn.call(otherObj, args)` keeps its existing
    // semantics — those go through the generic property-call path. We can
    // safely drop the thisArg because `captures_this == false` means the
    // body has no `this` references that depend on the bound value.
    if !has_spread {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Member(member) = callee_expr.as_ref() {
                if let ast::MemberProp::Ident(prop) = &member.prop {
                    if prop.sym.as_ref() == "call" && !call.args.is_empty() {
                        // Unwrap `(`...`)` parens so `((a,b) => a+b).call(...)`
                        // matches the same shape as `(function(){...}).call(...)`.
                        let mut inner = member.obj.as_ref();
                        while let ast::Expr::Paren(p) = inner {
                            inner = p.expr.as_ref();
                        }
                        let is_fn_lit = matches!(inner, ast::Expr::Fn(_) | ast::Expr::Arrow(_));
                        if is_fn_lit {
                            let lowered_callee = lower_expr(ctx, inner)?;
                            if let Expr::Closure {
                                captures_this: false,
                                ..
                            } = &lowered_callee
                            {
                                let rest_args = call
                                    .args
                                    .iter()
                                    .skip(1)
                                    .map(|arg| lower_expr(ctx, &arg.expr))
                                    .collect::<Result<Vec<_>>>()?;
                                return Ok(Expr::Call {
                                    callee: Box::new(lowered_callee),
                                    args: rest_args,
                                    type_args: Vec::new(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Followup to #957 / PR #959 — `Function('return this')()`.
    //
    // Every CJS/UMD-shaped library (lodash, underscore, Effect, …)
    // computes its "give me whatever the host calls `globalThis` here"
    // root with the double-call idiom:
    //   var root = freeGlobal || freeSelf || Function('return this')();
    // Pre-fix the bare `Function` ident lowers to `Expr::GlobalGet(0)`
    // (the no-resolution sentinel), then the inner `Function('return this')`
    // lowers to `Call { callee: GlobalGet(0), args: [String("return this")] }`
    // which codegen treats as "call a non-callable" — the outer `()` then
    // tries to call the returned value and the closure validator throws
    // `TypeError: value is not a function` at module init, leaving the
    // import resolved to undefined.
    //
    // PR #959 closed the sibling `.call(this)` IIFE bug and called this
    // one out in its commit message ("the next runtime gap"); fix here.
    // Match the full two-call shape at the AST level (the inner `Function`
    // ident still carries its name, so we can verify it really is the
    // builtin) and fold to `Expr::GlobalThisExpr`, which lowers to the
    // runtime's `js_get_global_this()` singleton — the same object
    // `globalThis[X] = V` already writes to (see #611).
    //
    // Conservative: requires the LITERAL "return this" (with optional
    // semicolon / whitespace) AND the outer Call must have no args. Any
    // other `Function(...)` shape (e.g. dynamic body, real `new Function`)
    // falls through to the existing GlobalGet(0) path; arbitrary
    // `new Function(body)` is still not supported (an architectural
    // change — issue #960 / future work).
    if !has_spread && call.args.is_empty() {
        if let ast::Callee::Expr(outer_callee) = &call.callee {
            let mut inner = outer_callee.as_ref();
            while let ast::Expr::Paren(p) = inner {
                inner = p.expr.as_ref();
            }
            if let ast::Expr::Call(inner_call) = inner {
                let inner_args_ok =
                    inner_call.args.len() == 1 && inner_call.args[0].spread.is_none();
                if inner_args_ok {
                    if let ast::Callee::Expr(inner_callee) = &inner_call.callee {
                        let mut inner_target = inner_callee.as_ref();
                        while let ast::Expr::Paren(p) = inner_target {
                            inner_target = p.expr.as_ref();
                        }
                        if let ast::Expr::Ident(ident) = inner_target {
                            if ident.sym.as_ref() == "Function"
                                && ctx.lookup_local("Function").is_none()
                                && ctx.lookup_func("Function").is_none()
                            {
                                if let ast::Expr::Lit(ast::Lit::Str(s)) =
                                    inner_call.args[0].expr.as_ref()
                                {
                                    let body = s.value.as_str().unwrap_or("").trim();
                                    let body = body.trim_end_matches(';').trim();
                                    if body == "return this" {
                                        return Ok(Expr::GlobalThisExpr);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Followup to #957 / PR #959 — `RegExp(<args>)` as a bare function call.
    //
    // lodash 4 builds half a dozen of these at module init:
    //   var reEscapedHtml = /&(?:amp|lt|gt|quot|#39);/g,
    //       reHasEscapedHtml = RegExp(reEscapedHtml.source);
    // The bare `RegExp` ident lowers to `Expr::GlobalGet(0)` (no resolved
    // value), so the function-call form dispatches through
    // `js_closure_call1` with a null closure handle and throws
    // `TypeError: value is not a function`. Fold here to
    // `Expr::RegExpDynamic` which lowers to the same `js_regexp_new`
    // runtime entrypoint the static `/foo/g` arm uses.
    //
    // Conservative: only `RegExp(pattern)` and `RegExp(pattern, flags)`
    // with no spread. Any local/import named `RegExp` shadows the
    // builtin and falls through to its normal dispatch.
    if !has_spread && !call.args.is_empty() && call.args.len() <= 2 {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            let mut callee_inner = callee_expr.as_ref();
            while let ast::Expr::Paren(p) = callee_inner {
                callee_inner = p.expr.as_ref();
            }
            if let ast::Expr::Ident(ident) = callee_inner {
                if ident.sym.as_ref() == "RegExp"
                    && ctx.lookup_local("RegExp").is_none()
                    && ctx.lookup_func("RegExp").is_none()
                {
                    let pattern = lower_expr(ctx, &call.args[0].expr)?;
                    let flags = if call.args.len() == 2 {
                        Some(Box::new(lower_expr(ctx, &call.args[1].expr)?))
                    } else {
                        None
                    };
                    return Ok(Expr::RegExpDynamic {
                        pattern: Box::new(pattern),
                        flags,
                    });
                }
            }
        }
    }

    let mut args = call
        .args
        .iter()
        .map(|arg| lower_expr(ctx, &arg.expr))
        .collect::<Result<Vec<_>>>()?;

    // --- Proxy apply / revoke fast path ---
    if !has_spread {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
                let name = ident.sym.to_string();
                if ctx.proxy_locals.contains(&name) {
                    if let Some(id) = ctx.lookup_local(&name) {
                        return Ok(Expr::ProxyApply {
                            proxy: Box::new(Expr::LocalGet(id)),
                            args,
                        });
                    }
                }
                if let Some(proxy_name) = ctx.proxy_revoke_locals.get(&name).cloned() {
                    if let Some(id) = ctx.lookup_local(&proxy_name) {
                        return Ok(Expr::ProxyRevoke(Box::new(Expr::LocalGet(id))));
                    }
                }
            }
        }
    }

    // --- Issue #886: indirect call through an `Object.<staticMethod>` alias.
    //
    // esbuild's CJS-bundle prelude emits a constant-aliased dispatch table
    // at the top of every bundled package:
    //   var __defProp = Object.defineProperty;
    //   var __export = (target, all) => {
    //     for (var name in all) __defProp(target, name, { get: all[name], enumerable: true });
    //   };
    // Pre-fix the `__defProp(...)` call fell through to the generic
    // `LocalGet(__defProp)(args)` codegen path which evaluates the LocalGet
    // to whatever the init lowering produced — a PropertyGet on the
    // undefined-at-runtime `Object` constructor reference — and then tries
    // to invoke an undefined value, throwing `TypeError: value is not a
    // function`. The recogniser below for the literal `Object.defineProperty
    // (...)` shape (added by #891) didn't fire because the callee isn't a
    // member expression, just a local ident.
    //
    // The alias is populated in `destructuring.rs` when the AST shape is
    // `const X = Object.<method>` and `<method>` is in the whitelist of
    // methods that already have a dedicated HIR variant. Route here to
    // synthesize the same Expr the literal recogniser would produce.
    if !has_spread {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
                let name = ident.sym.to_string();
                if let Some(id) = ctx.lookup_local(&name) {
                    if let Some(method) = ctx.object_static_method_aliases.get(&id).cloned() {
                        return Ok(build_object_static_method_call(&method, args));
                    }
                }
            }
        }
    }

    // --- Object.hasOwnProperty.call(obj, key) → js_object_has_own(obj, key) ---
    //
    // Current NestJS `@Module()` uses this inherited Object.prototype helper
    // through the `Object` constructor value instead of spelling
    // `Object.prototype.hasOwnProperty.call(...)`.
    if !has_spread && args.len() == 2 {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Member(outer) = callee_expr.as_ref() {
                if let (ast::MemberProp::Ident(outer_prop), ast::Expr::Member(mid)) =
                    (&outer.prop, outer.obj.as_ref())
                {
                    if outer_prop.sym.as_ref() == "call" {
                        if let (ast::MemberProp::Ident(mid_prop), ast::Expr::Ident(mid_obj)) =
                            (&mid.prop, mid.obj.as_ref())
                        {
                            if mid_obj.sym.as_ref() == "Object"
                                && mid_prop.sym.as_ref() == "hasOwnProperty"
                            {
                                return Ok(Expr::Call {
                                    callee: Box::new(Expr::ExternFuncRef {
                                        name: "js_object_has_own".to_string(),
                                        param_types: Vec::new(),
                                        return_type: Type::Any,
                                    }),
                                    args,
                                    type_args: Vec::new(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // --- Object.prototype.toString.call(x) → js_object_to_string(x) ---
    // AST shape is a four-level member expression:
    //   call.call(x)
    //   ^^^^^^^^^^ outer member: (Object.prototype.toString).call
    // The runtime helper consults the class's `Symbol.toStringTag`
    // getter (registered at module init via `__perry_wk_tostringtag_*`)
    // and returns `[object <tag>]` or the default `[object Object]`.
    // `Object.prototype.<method>.call(...)` idioms — perry doesn't expose
    // `Object.prototype` so we rewrite to runtime helpers. Refs #420.
    if !has_spread && (args.len() == 1 || args.len() == 2) {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Member(outer) = callee_expr.as_ref() {
                if let (ast::MemberProp::Ident(outer_prop), ast::Expr::Member(mid)) =
                    (&outer.prop, outer.obj.as_ref())
                {
                    if outer_prop.sym.as_ref() == "call" {
                        if let (ast::MemberProp::Ident(mid_prop), ast::Expr::Member(inner)) =
                            (&mid.prop, mid.obj.as_ref())
                        {
                            let runtime_fn = match (mid_prop.sym.as_ref(), args.len()) {
                                ("toString", 1) => Some("js_object_to_string"),
                                ("hasOwnProperty", 2) => Some("js_object_has_own"),
                                _ => None,
                            };
                            if let Some(runtime_fn) = runtime_fn {
                                if let (
                                    ast::MemberProp::Ident(inner_prop),
                                    ast::Expr::Ident(inner_obj),
                                ) = (&inner.prop, inner.obj.as_ref())
                                {
                                    if inner_obj.sym.as_ref() == "Object"
                                        && inner_prop.sym.as_ref() == "prototype"
                                    {
                                        return Ok(Expr::Call {
                                            callee: Box::new(Expr::ExternFuncRef {
                                                name: runtime_fn.to_string(),
                                                param_types: Vec::new(),
                                                return_type: Type::Any,
                                            }),
                                            args,
                                            type_args: Vec::new(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // If spread is present, create CallSpread instead of Call
    let spread_args: Option<Vec<CallArg>> = if has_spread {
        Some(
            call.args
                .iter()
                .zip(args.iter())
                .map(|(ast_arg, lowered)| {
                    if ast_arg.spread.is_some() {
                        CallArg::Spread(lowered.clone())
                    } else {
                        CallArg::Expr(lowered.clone())
                    }
                })
                .collect(),
        )
    } else {
        None
    };

    match &call.callee {
        ast::Callee::Super(_) => {
            // super() call in constructor
            Ok(Expr::SuperCall(args))
        }
        ast::Callee::Expr(expr) => {
            // Check for super.method() call
            if let ast::Expr::SuperProp(super_prop) = expr.as_ref() {
                if let ast::SuperProp::Ident(ident) = &super_prop.prop {
                    return Ok(Expr::SuperMethodCall {
                        method: ident.sym.to_string(),
                        args,
                    });
                }
            }

            // Check for nested process member calls like process.hrtime.bigint()
            if let ast::Expr::Member(outer_member) = expr.as_ref() {
                if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
                    if let ast::Expr::Ident(inner_obj) = inner_member.obj.as_ref() {
                        if inner_obj.sym.as_ref() == "process" {
                            if let ast::MemberProp::Ident(inner_prop) = &inner_member.prop {
                                if inner_prop.sym.as_ref() == "hrtime" {
                                    if let ast::MemberProp::Ident(method_ident) = &outer_member.prop
                                    {
                                        if method_ident.sym.as_ref() == "bigint" {
                                            return Ok(Expr::ProcessHrtimeBigint);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Web Crypto API — `crypto.subtle.<method>(args)` (issue #561).
            // AST shape is the same nested-Member pattern as
            // `process.hrtime.bigint()` above. We resolve here BEFORE the
            // generic mod.X.Y() arm so the strict-API gate (#463) doesn't
            // reject `subtle` (which is a sub-namespace, not a class).
            if let ast::Expr::Member(outer_member) = expr.as_ref() {
                if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
                    if let ast::Expr::Ident(root_ident) = inner_member.obj.as_ref() {
                        let root_name = root_ident.sym.as_ref();
                        let is_crypto_root = root_name == "crypto"
                            || ctx.lookup_builtin_module_alias(root_name) == Some("crypto")
                            || ctx
                                .lookup_native_module(root_name)
                                .map(|(m, _)| m == "crypto")
                                .unwrap_or(false);
                        if is_crypto_root {
                            if let ast::MemberProp::Ident(inner_prop) = &inner_member.prop {
                                if inner_prop.sym.as_ref() == "subtle" {
                                    if let ast::MemberProp::Ident(method_ident) = &outer_member.prop
                                    {
                                        let method = method_ident.sym.as_ref();
                                        match method {
                                            "digest" if args.len() >= 2 => {
                                                let mut iter = args.into_iter();
                                                let algo = iter.next().unwrap();
                                                let data = iter.next().unwrap();
                                                return Ok(Expr::WebCryptoDigest {
                                                    algo: Box::new(algo),
                                                    data: Box::new(data),
                                                });
                                            }
                                            "importKey" if args.len() >= 5 => {
                                                let mut iter = args.into_iter();
                                                let format = iter.next().unwrap();
                                                let key = iter.next().unwrap();
                                                let algorithm = iter.next().unwrap();
                                                let extractable = iter.next().unwrap();
                                                let usages = iter.next().unwrap();
                                                return Ok(Expr::WebCryptoImportKey {
                                                    format: Box::new(format),
                                                    key: Box::new(key),
                                                    algorithm: Box::new(algorithm),
                                                    extractable: Box::new(extractable),
                                                    usages: Box::new(usages),
                                                });
                                            }
                                            "sign" if args.len() >= 3 => {
                                                let mut iter = args.into_iter();
                                                let algorithm = iter.next().unwrap();
                                                let key = iter.next().unwrap();
                                                let data = iter.next().unwrap();
                                                return Ok(Expr::WebCryptoSign {
                                                    algorithm: Box::new(algorithm),
                                                    key: Box::new(key),
                                                    data: Box::new(data),
                                                });
                                            }
                                            "verify" if args.len() >= 4 => {
                                                let mut iter = args.into_iter();
                                                let algorithm = iter.next().unwrap();
                                                let key = iter.next().unwrap();
                                                let signature = iter.next().unwrap();
                                                let data = iter.next().unwrap();
                                                return Ok(Expr::WebCryptoVerify {
                                                    algorithm: Box::new(algorithm),
                                                    key: Box::new(key),
                                                    signature: Box::new(signature),
                                                    data: Box::new(data),
                                                });
                                            }
                                            "encrypt" if args.len() >= 3 => {
                                                let mut iter = args.into_iter();
                                                let algorithm = iter.next().unwrap();
                                                let key = iter.next().unwrap();
                                                let data = iter.next().unwrap();
                                                return Ok(Expr::WebCryptoEncrypt {
                                                    algorithm: Box::new(algorithm),
                                                    key: Box::new(key),
                                                    data: Box::new(data),
                                                });
                                            }
                                            "decrypt" if args.len() >= 3 => {
                                                let mut iter = args.into_iter();
                                                let algorithm = iter.next().unwrap();
                                                let key = iter.next().unwrap();
                                                let data = iter.next().unwrap();
                                                return Ok(Expr::WebCryptoDecrypt {
                                                    algorithm: Box::new(algorithm),
                                                    key: Box::new(key),
                                                    data: Box::new(data),
                                                });
                                            }
                                            "generateKey" if args.len() >= 3 => {
                                                let mut iter = args.into_iter();
                                                let algorithm = iter.next().unwrap();
                                                let extractable = iter.next().unwrap();
                                                let usages = iter.next().unwrap();
                                                return Ok(Expr::WebCryptoGenerateKey {
                                                    algorithm: Box::new(algorithm),
                                                    extractable: Box::new(extractable),
                                                    usages: Box::new(usages),
                                                });
                                            }
                                            "wrapKey" if args.len() >= 4 => {
                                                let mut iter = args.into_iter();
                                                let format = iter.next().unwrap();
                                                let key = iter.next().unwrap();
                                                let wrapping_key = iter.next().unwrap();
                                                let wrap_algorithm = iter.next().unwrap();
                                                return Ok(Expr::WebCryptoWrapKey {
                                                    format: Box::new(format),
                                                    key: Box::new(key),
                                                    wrapping_key: Box::new(wrapping_key),
                                                    wrap_algorithm: Box::new(wrap_algorithm),
                                                });
                                            }
                                            "unwrapKey" if args.len() >= 7 => {
                                                let mut iter = args.into_iter();
                                                let format = iter.next().unwrap();
                                                let wrapped_key = iter.next().unwrap();
                                                let unwrapping_key = iter.next().unwrap();
                                                let unwrap_algorithm = iter.next().unwrap();
                                                let unwrapped_key_algorithm = iter.next().unwrap();
                                                let extractable = iter.next().unwrap();
                                                let usages = iter.next().unwrap();
                                                return Ok(Expr::WebCryptoUnwrapKey {
                                                    format: Box::new(format),
                                                    wrapped_key: Box::new(wrapped_key),
                                                    unwrapping_key: Box::new(unwrapping_key),
                                                    unwrap_algorithm: Box::new(unwrap_algorithm),
                                                    unwrapped_key_algorithm: Box::new(
                                                        unwrapped_key_algorithm,
                                                    ),
                                                    extractable: Box::new(extractable),
                                                    usages: Box::new(usages),
                                                });
                                            }
                                            _ => {
                                                // Unsupported subtle method —
                                                // fail loudly. The supported
                                                // surface is documented in the
                                                // d.ts and at #561; asymmetric
                                                // (RSA-PSS / ECDSA / RSA-OAEP),
                                                // deriveKey are still out of
                                                // scope per the issue.
                                                let allow_unimplemented =
                                                    std::env::var_os("PERRY_ALLOW_UNIMPLEMENTED")
                                                        .is_some();
                                                if !allow_unimplemented {
                                                    crate::lower_bail!(
                                                        outer_member.span,
                                                        "`crypto.subtle.{}` is not implemented in Perry — supported subtle methods are digest, importKey, sign, verify, encrypt, decrypt, generateKey, wrapKey, unwrapKey (HMAC + SHA-1/256/384/512; encrypt/decrypt/generateKey/wrapKey currently AES-GCM/AES-KW only). \
                                                         See `perry --print-api-manifest` and #561, or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore.",
                                                        method,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // `path.posix.<method>(args)` / `path.win32.<method>(args)` —
            // sub-namespace dispatch (issue #810). Without this arm the
            // generic mod.X.Y() block below skips the call (path.posix /
            // path.win32 are in its sub-namespace exclusion list to keep
            // them off the strict-API gate) and the call falls through
            // to the receiver-less dispatch, returning undefined.
            //
            // - `path.posix.X` routes to the existing Expr::PathX variant.
            //   The runtime js_path_* functions use POSIX (`/`) semantics,
            //   so this is a direct shape rewrite.
            // - `path.win32.join` routes to a dedicated Expr::PathWin32Join
            //   so the result uses `\` separators. Other path.win32.*
            //   methods are intentionally unwired here — falling through
            //   to a POSIX impl would silently mis-handle drive letters
            //   and `\` segments. Track those in a follow-up.
            if let ast::Expr::Member(outer_member) = expr.as_ref() {
                if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
                    if let ast::Expr::Ident(root_ident) = inner_member.obj.as_ref() {
                        let root_name = root_ident.sym.as_ref();
                        let is_path_root = root_name == "path"
                            || ctx.lookup_builtin_module_alias(root_name) == Some("path")
                            || ctx
                                .lookup_native_module(root_name)
                                .map(|(m, _)| m == "path")
                                .unwrap_or(false);
                        if is_path_root {
                            if let (
                                ast::MemberProp::Ident(sub_prop),
                                ast::MemberProp::Ident(method_prop),
                            ) = (&inner_member.prop, &outer_member.prop)
                            {
                                let sub = sub_prop.sym.as_ref();
                                let method = method_prop.sym.as_ref();
                                if sub == "posix" || sub == "win32" {
                                    // path.<sub>.join(...)
                                    if method == "join" {
                                        if args.is_empty() {
                                            return Ok(Expr::String(".".to_string()));
                                        }
                                        if sub == "win32" {
                                            let mut iter = args.into_iter();
                                            let mut result = iter.next().unwrap();
                                            for next_arg in iter {
                                                result = Expr::PathWin32Join(
                                                    Box::new(result),
                                                    Box::new(next_arg),
                                                );
                                            }
                                            return Ok(result);
                                        } else {
                                            // posix.join → existing PathJoin
                                            if args.len() == 1 {
                                                return Ok(Expr::PathNormalize(Box::new(
                                                    args.into_iter().next().unwrap(),
                                                )));
                                            }
                                            let mut iter = args.into_iter();
                                            let mut result = iter.next().unwrap();
                                            for next_arg in iter {
                                                result = Expr::PathJoin(
                                                    Box::new(result),
                                                    Box::new(next_arg),
                                                );
                                            }
                                            return Ok(result);
                                        }
                                    }

                                    // The remaining methods route to the
                                    // existing POSIX Expr::Path* variants
                                    // only for the `posix` sub-namespace.
                                    // For `win32` we deliberately fall
                                    // through to the receiver-less path
                                    // (returns undefined) rather than
                                    // give a wrong POSIX answer.
                                    if sub == "posix" {
                                        match method {
                                            "dirname" if !args.is_empty() => {
                                                return Ok(Expr::PathDirname(Box::new(
                                                    args.into_iter().next().unwrap(),
                                                )));
                                            }
                                            "basename" if args.len() >= 2 => {
                                                let mut it = args.into_iter();
                                                let p = it.next().unwrap();
                                                let e = it.next().unwrap();
                                                return Ok(Expr::PathBasenameExt(
                                                    Box::new(p),
                                                    Box::new(e),
                                                ));
                                            }
                                            "basename" if !args.is_empty() => {
                                                return Ok(Expr::PathBasename(Box::new(
                                                    args.into_iter().next().unwrap(),
                                                )));
                                            }
                                            "extname" if !args.is_empty() => {
                                                return Ok(Expr::PathExtname(Box::new(
                                                    args.into_iter().next().unwrap(),
                                                )));
                                            }
                                            "isAbsolute" if !args.is_empty() => {
                                                return Ok(Expr::PathIsAbsolute(Box::new(
                                                    args.into_iter().next().unwrap(),
                                                )));
                                            }
                                            "normalize" if !args.is_empty() => {
                                                return Ok(Expr::PathNormalize(Box::new(
                                                    args.into_iter().next().unwrap(),
                                                )));
                                            }
                                            "parse" if !args.is_empty() => {
                                                return Ok(Expr::PathParse(Box::new(
                                                    args.into_iter().next().unwrap(),
                                                )));
                                            }
                                            "format" if !args.is_empty() => {
                                                return Ok(Expr::PathFormat(Box::new(
                                                    args.into_iter().next().unwrap(),
                                                )));
                                            }
                                            "toNamespacedPath" if !args.is_empty() => {
                                                return Ok(Expr::PathToNamespacedPath(Box::new(
                                                    args.into_iter().next().unwrap(),
                                                )));
                                            }
                                            "relative" if args.len() >= 2 => {
                                                let mut it = args.into_iter();
                                                let from = it.next().unwrap();
                                                let to = it.next().unwrap();
                                                return Ok(Expr::PathRelative(
                                                    Box::new(from),
                                                    Box::new(to),
                                                ));
                                            }
                                            "resolve" if !args.is_empty() => {
                                                let mut it = args.into_iter();
                                                let first = it.next().unwrap();
                                                let mut joined = first;
                                                for next_arg in it {
                                                    joined = Expr::PathResolveJoin(
                                                        Box::new(joined),
                                                        Box::new(next_arg),
                                                    );
                                                }
                                                return Ok(Expr::PathResolve(Box::new(joined)));
                                            }
                                            "matchesGlob" if args.len() >= 2 => {
                                                let mut it = args.into_iter();
                                                let p = it.next().unwrap();
                                                let pat = it.next().unwrap();
                                                return Ok(Expr::PathMatchesGlob(
                                                    Box::new(p),
                                                    Box::new(pat),
                                                ));
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check for module.Class.staticMethod() pattern (e.g.,
            // ethers.Wallet.createRandom()). Modelled after the
            // process.hrtime.bigint() handler above.
            //
            // Some "module.foo.method()" shapes are NOT class statics —
            // they're sub-namespaces with dedicated codegen arms in
            // `crates/perry-codegen/src/expr.rs` (e.g. fs.promises.X
            // routes to the sync counterpart + js_promise_resolved).
            // Skip them here so the existing codegen path keeps working.
            // v0.5.385 (#299) introduced this arm; v0.5.386 (this fix)
            // adds the exclusion list after fs.promises.readFile silently
            // started returning `undefined` because the new HIR shape
            // bypassed the old codegen arm and fell into the
            // "unhandled fs.<method>()" warn-and-undef path.
            if let ast::Expr::Member(outer_member) = expr.as_ref() {
                if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
                    if let ast::Expr::Ident(mod_ident) = inner_member.obj.as_ref() {
                        let mod_name = mod_ident.sym.to_string();
                        if let Some((module_name, _)) = ctx.lookup_native_module(&mod_name) {
                            if let ast::MemberProp::Ident(class_ident) = &inner_member.prop {
                                let class_name = class_ident.sym.to_string();
                                let is_sub_namespace = matches!(
                                    (module_name, class_name.as_str()),
                                    ("fs", "promises")
                                        | ("fs", "constants")
                                        | ("path", "posix")
                                        | ("path", "win32")
                                );
                                // Unimplemented-API gate (#463) for the chained
                                // `mod.X.Y()` case. The lower_member gate fires
                                // for `mod.X` standalone but not when this arm
                                // short-circuits the chain into a single
                                // `NativeMethodCall` without recursing through
                                // lower_member. Without this, `crypto.subtle.encrypt(...)`
                                // built cleanly and silently returned undefined.
                                let allow_unimplemented =
                                    std::env::var_os("PERRY_ALLOW_UNIMPLEMENTED").is_some();
                                if !is_sub_namespace
                                    && !allow_unimplemented
                                    && perry_api_manifest::module_has_any_entries(module_name)
                                    && perry_api_manifest::module_has_symbol(
                                        module_name,
                                        &class_name,
                                    )
                                    .is_none()
                                {
                                    // #925: append a replacement hint if
                                    // we have one for this exact shape.
                                    let hint = super::unimpl_hints::module_member_hint(
                                        module_name,
                                        &class_name,
                                    )
                                    .map(|h| format!(" {h}"))
                                    .unwrap_or_default();
                                    crate::lower_bail!(
                                        outer_member.span,
                                        "`{}.{}` is not implemented in Perry — see `perry --print-api-manifest` for the supported surface, \
                                         or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore. (#463){}",
                                        module_name,
                                        class_name,
                                        hint,
                                    );
                                }
                                if !is_sub_namespace {
                                    if let ast::MemberProp::Ident(method_ident) = &outer_member.prop
                                    {
                                        let method_name = method_ident.sym.to_string();
                                        return Ok(Expr::NativeMethodCall {
                                            module: module_name.to_string(),
                                            class_name: Some(class_name),
                                            object: None,
                                            method: method_name,
                                            args,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // process.stdin.setRawMode/.on and process.stdout.on — methods
            // we recognize on the stdin/stdout stream objects. (#347
            // Phases 2 & 3.) Recognized BEFORE the generic
            // module.Class.staticMethod() arm because process.std{in,out}
            // are not classes. Falls through to the generic dispatch
            // (which lowers it as a closure call on the stub object) for
            // any other method name — `process.stdout.write` keeps
            // working through that path.
            if let ast::Expr::Member(outer_member) = expr.as_ref() {
                if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
                    if let ast::Expr::Ident(root_ident) = inner_member.obj.as_ref() {
                        if root_ident.sym.as_ref() == "process" {
                            if let ast::MemberProp::Ident(stream_ident) = &inner_member.prop {
                                let stream = stream_ident.sym.as_ref();
                                if let ast::MemberProp::Ident(method_ident) = &outer_member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match (stream, method_name) {
                                        ("stdin", "setRawMode") => {
                                            if !args.is_empty() {
                                                let arg = args.into_iter().next().unwrap();
                                                return Ok(Expr::ProcessStdinSetRawMode(Box::new(
                                                    arg,
                                                )));
                                            }
                                        }
                                        ("stdin", "on") => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let event = iter.next().unwrap();
                                                let handler = iter.next().unwrap();
                                                return Ok(Expr::ProcessStdinOn {
                                                    event: Box::new(event),
                                                    handler: Box::new(handler),
                                                });
                                            }
                                        }
                                        ("stdout", "on") => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let event = iter.next().unwrap();
                                                let handler = iter.next().unwrap();
                                                return Ok(Expr::ProcessStdoutOn {
                                                    event: Box::new(event),
                                                    handler: Box::new(handler),
                                                });
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check for native module method calls (e.g., mysql.createConnection())
            if let ast::Expr::Member(member) = expr.as_ref() {
                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                    let obj_name = obj_ident.sym.to_string();

                    // Check for process module methods
                    if obj_name == "process" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "uptime" => return Ok(Expr::ProcessUptime),
                                "cwd" => return Ok(Expr::ProcessCwd),
                                "memoryUsage" => return Ok(Expr::ProcessMemoryUsage),
                                "nextTick" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::ProcessNextTick(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "on" => {
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let event = iter.next().unwrap();
                                        let handler = iter.next().unwrap();
                                        return Ok(Expr::ProcessOn {
                                            event: Box::new(event),
                                            handler: Box::new(handler),
                                        });
                                    }
                                }
                                "chdir" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::ProcessChdir(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "kill" => {
                                    if !args.is_empty() {
                                        let mut iter = args.into_iter();
                                        let pid = iter.next().unwrap();
                                        let signal = iter.next().map(Box::new);
                                        return Ok(Expr::ProcessKill {
                                            pid: Box::new(pid),
                                            signal,
                                        });
                                    }
                                }
                                "exit" => {
                                    // process.exit() / process.exit(code) — never
                                    // returns, terminates the process. Until now this
                                    // fell through to generic NativeMethodCall which
                                    // silently no-op'd, so scripts that rely on it to
                                    // end the event loop (e.g. `main().then(() =>
                                    // process.exit(0))` in a net-socket driver) would
                                    // hang with the socket still keeping the loop alive.
                                    let code = if !args.is_empty() {
                                        Some(Box::new(args.into_iter().next().unwrap()))
                                    } else {
                                        None
                                    };
                                    return Ok(Expr::ProcessExit(code));
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for tty module methods (#347 Phase 3)
                    let is_tty_module = obj_name == "tty"
                        || ctx.lookup_builtin_module_alias(&obj_name) == Some("tty");
                    if is_tty_module {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            if method_ident.sym.as_ref() == "isatty" && !args.is_empty() {
                                let arg = args.into_iter().next().unwrap();
                                return Ok(Expr::TtyIsAtty(Box::new(arg)));
                            }
                        }
                    }

                    // Check for os module methods FIRST (before generic NativeMethodCall)
                    let is_os_module = obj_name == "os"
                        || ctx.lookup_builtin_module_alias(&obj_name) == Some("os");
                    if is_os_module {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "platform" => return Ok(Expr::OsPlatform),
                                "arch" => return Ok(Expr::OsArch),
                                "hostname" => return Ok(Expr::OsHostname),
                                "homedir" => return Ok(Expr::OsHomedir),
                                "tmpdir" => return Ok(Expr::OsTmpdir),
                                "totalmem" => return Ok(Expr::OsTotalmem),
                                "freemem" => return Ok(Expr::OsFreemem),
                                "uptime" => return Ok(Expr::OsUptime),
                                "type" => return Ok(Expr::OsType),
                                "release" => return Ok(Expr::OsRelease),
                                "cpus" => return Ok(Expr::OsCpus),
                                "networkInterfaces" => return Ok(Expr::OsNetworkInterfaces),
                                "userInfo" => return Ok(Expr::OsUserInfo),
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for Buffer static methods. Issue #831: aliased
                    // imports of Buffer (`import { Buffer as RuntimeBuffer } from
                    // "node:buffer"`) must route through the same dedicated
                    // BufferFrom/BufferAlloc/etc HIR variants as the global
                    // `Buffer`; otherwise the lowering falls through to a
                    // receiver-less `NativeMethodCall { module: "buffer", method:
                    // "from", object: None }` for which the codegen has no
                    // dispatch table entry — it would silently return
                    // TAG_UNDEFINED, and any caller that subsequently treats the
                    // result as a Buffer (e.g. `b[0]` → Uint8ArrayGet) segfaults
                    // on the undefined value.
                    let is_buffer_ref = obj_name == "Buffer"
                        || matches!(
                            ctx.lookup_native_module(&obj_name),
                            Some(("buffer", Some("Buffer")))
                        );
                    if is_buffer_ref {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "from" => {
                                    let data = args.first().cloned().unwrap_or(Expr::Undefined);
                                    let encoding = args.get(1).cloned().map(Box::new);
                                    return Ok(Expr::BufferFrom {
                                        data: Box::new(data),
                                        encoding,
                                    });
                                }
                                "alloc" => {
                                    let size = args.first().cloned().unwrap_or(Expr::Number(0.0));
                                    let fill = args.get(1).cloned().map(Box::new);
                                    return Ok(Expr::BufferAlloc {
                                        size: Box::new(size),
                                        fill,
                                    });
                                }
                                "allocUnsafe" => {
                                    let size = args.first().cloned().unwrap_or(Expr::Number(0.0));
                                    return Ok(Expr::BufferAllocUnsafe(Box::new(size)));
                                }
                                "concat" => {
                                    let list = args.first().cloned().unwrap_or(Expr::Array(vec![]));
                                    return Ok(Expr::BufferConcat(Box::new(list)));
                                }
                                "isBuffer" => {
                                    let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::BufferIsBuffer(Box::new(obj)));
                                }
                                "byteLength" => {
                                    let data = args
                                        .first()
                                        .cloned()
                                        .unwrap_or(Expr::String("".to_string()));
                                    return Ok(Expr::BufferByteLength(Box::new(data)));
                                }
                                // `Buffer.compare(a, b)` → `a.compare(b)` instance call
                                // (handled by runtime buffer dispatch).
                                "compare" => {
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let a = iter.next().unwrap();
                                        let b = iter.next().unwrap();
                                        return Ok(Expr::Call {
                                            callee: Box::new(Expr::PropertyGet {
                                                object: Box::new(a),
                                                property: "compare".to_string(),
                                            }),
                                            args: vec![b],
                                            type_args: vec![],
                                        });
                                    }
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for Uint8Array static methods
                    if obj_name == "Uint8Array" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "from" => {
                                    let data = args.first().cloned().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::Uint8ArrayFrom(Box::new(data)));
                                }
                                // Issue #871 (part 2): `Uint8Array.of(a, b, c, ...)` —
                                // uuid's `sha1.js` calls this with 20 args (the SHA-1
                                // hash output, byte by byte), which hit the
                                // `Call callee shape not supported (PropertyGet) with N args`
                                // bail in `crates/perry-codegen/src/lower_call.rs::~3226`
                                // because the receiver `Uint8Array` lowers to `GlobalGet(0)`
                                // (which `lower_expr` evaluates to `0.0`) so the closure-call
                                // fallback at `~3167` skipped past it, and there's no
                                // `js_closure_call17..20` to dispatch ≥17-arg calls anyway.
                                //
                                // Per ECMAScript: `Uint8Array.of(...items)` is `Uint8Array.from([...items])`
                                // — same shape as the existing `from` arm above, just wrap the
                                // varargs in an array literal first. Routes through `Expr::Array`
                                // → `Expr::Uint8ArrayFrom` which already lowers correctly for any
                                // arity (it's just `js_uint8array_from_array`-or-equivalent on the
                                // packed array). Mirrors the `Array.of` arm at :1618.
                                "of" => {
                                    return Ok(Expr::Uint8ArrayFrom(Box::new(Expr::Array(args))));
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for Object static methods
                    if obj_name == "Object" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "keys" => {
                                    let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ObjectKeys(Box::new(obj)));
                                }
                                "values" => {
                                    let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ObjectValues(Box::new(obj)));
                                }
                                "entries" => {
                                    let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ObjectEntries(Box::new(obj)));
                                }
                                // Object.assign(target, ...sources) — per ECMAScript spec, this
                                // MUTATES target with each source's own enumerable string-keyed
                                // and Symbol-keyed properties, and RETURNS target (preserving
                                // identity, class_id, and the SYMBOL_PROPERTIES side-table).
                                // Refs #590: the previous lowering folded the call into
                                // ObjectSpread which allocates a fresh object — that breaks
                                // `result === target` and orphans target's symbol-keyed
                                // properties since the side table is keyed by raw pointer.
                                //
                                // Special case: no target at all (`Object.assign()`) is
                                // a TypeError per spec; we coerce to an empty object literal.
                                // Special case: `Object.assign({}, ...)` with a fresh empty
                                // object-literal target — the user is explicitly asking for
                                // a fresh object, so we keep the old ObjectSpread path
                                // (matches `{...src1, ...src2}` semantics, no observable
                                // difference and avoids regressing the no-spread fold below).
                                "assign" => {
                                    if args.is_empty() {
                                        return Ok(Expr::Object(Vec::new()));
                                    }
                                    let mut iter = args.into_iter();
                                    let target = iter.next().unwrap();
                                    let sources: Vec<Expr> = iter.collect();
                                    // `Object.assign({}, ...sources)` — fresh empty object as
                                    // target. Preserve the literal-friendly fast paths
                                    // (no_spread fold to plain Object, otherwise ObjectSpread)
                                    // since there's no pre-existing identity / class_id to
                                    // preserve and downstream codegen has more aggressive
                                    // inlining for these shapes.
                                    if matches!(&target, Expr::Object(props) if props.is_empty()) {
                                        let mut parts: Vec<(Option<String>, Expr)> = Vec::new();
                                        for arg in &sources {
                                            match arg {
                                                Expr::Object(props) => {
                                                    for (key, val) in props {
                                                        parts
                                                            .push((Some(key.clone()), val.clone()));
                                                    }
                                                }
                                                _ => {
                                                    parts.push((None, arg.clone()));
                                                }
                                            }
                                        }
                                        let has_spread = parts.iter().any(|(k, _)| k.is_none());
                                        if !has_spread {
                                            let static_props: Vec<(String, Expr)> = parts
                                                .into_iter()
                                                .filter_map(|(k, v)| k.map(|key| (key, v)))
                                                .collect();
                                            return Ok(Expr::Object(static_props));
                                        }
                                        return Ok(Expr::ObjectSpread { parts });
                                    }
                                    // Real `Object.assign(target, ...sources)` — mutate target.
                                    return Ok(Expr::ObjectAssign {
                                        target: Box::new(target),
                                        sources,
                                    });
                                }
                                "fromEntries" => {
                                    let entries =
                                        args.into_iter().next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ObjectFromEntries(Box::new(entries)));
                                }
                                "groupBy" => {
                                    // Object.groupBy(items, keyFn) — Node 22+ static method
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let items = iter.next().unwrap();
                                        let key_fn = iter.next().unwrap();
                                        let key_fn =
                                            ctx.maybe_wrap_builtin_callback(key_fn, &call.args[1]);
                                        return Ok(Expr::ObjectGroupBy {
                                            items: Box::new(items),
                                            key_fn: Box::new(key_fn),
                                        });
                                    }
                                }
                                "is" => {
                                    let mut iter = args.into_iter();
                                    let a = iter.next().unwrap_or(Expr::Undefined);
                                    let b = iter.next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ObjectIs(Box::new(a), Box::new(b)));
                                }
                                "hasOwn" => {
                                    let mut iter = args.into_iter();
                                    let obj = iter.next().unwrap_or(Expr::Undefined);
                                    let key = iter.next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ObjectHasOwn(Box::new(obj), Box::new(key)));
                                }
                                "freeze" => {
                                    return Ok(Expr::ObjectFreeze(Box::new(
                                        args.into_iter().next().unwrap_or(Expr::Undefined),
                                    )));
                                }
                                "seal" => {
                                    return Ok(Expr::ObjectSeal(Box::new(
                                        args.into_iter().next().unwrap_or(Expr::Undefined),
                                    )));
                                }
                                "preventExtensions" => {
                                    return Ok(Expr::ObjectPreventExtensions(Box::new(
                                        args.into_iter().next().unwrap_or(Expr::Undefined),
                                    )));
                                }
                                "create" => {
                                    return Ok(Expr::ObjectCreate(Box::new(
                                        args.into_iter().next().unwrap_or(Expr::Undefined),
                                    )));
                                }
                                "isFrozen" => {
                                    return Ok(Expr::ObjectIsFrozen(Box::new(
                                        args.into_iter().next().unwrap_or(Expr::Undefined),
                                    )));
                                }
                                "isSealed" => {
                                    return Ok(Expr::ObjectIsSealed(Box::new(
                                        args.into_iter().next().unwrap_or(Expr::Undefined),
                                    )));
                                }
                                "isExtensible" => {
                                    return Ok(Expr::ObjectIsExtensible(Box::new(
                                        args.into_iter().next().unwrap_or(Expr::Undefined),
                                    )));
                                }
                                "getPrototypeOf" => {
                                    return Ok(Expr::ObjectGetPrototypeOf(Box::new(
                                        args.into_iter().next().unwrap_or(Expr::Undefined),
                                    )));
                                }
                                "setPrototypeOf" => {
                                    // `Object.setPrototypeOf(obj, proto)` is the foundation
                                    // of chalk's "callable + getter-bag" shape (a closure has
                                    // its `[[Prototype]]` reset to a Function-derived
                                    // accessor-bag). Pre-fix this fell through to a generic
                                    // `Object.setPrototypeOf` PropertyGet → Call where
                                    // `Object.setPrototypeOf` resolves to undefined and the
                                    // call throws `TypeError: value is not a function` —
                                    // chalk's `import chalk from "chalk"` died at module init.
                                    //
                                    // Perry's runtime doesn't track mutable per-instance
                                    // prototype chains (class IDs are baked at allocation),
                                    // so we model setPrototypeOf as a no-op that still
                                    // returns the target — matching the spec's "return obj"
                                    // contract. The runtime helper registers (obj, proto)
                                    // in a side-table that `Object.getPrototypeOf(obj)` is
                                    // free to consult later if a downstream pattern needs it.
                                    let mut iter = args.into_iter();
                                    let obj = iter.next().unwrap_or(Expr::Undefined);
                                    let proto = iter.next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ObjectSetPrototypeOf(
                                        Box::new(obj),
                                        Box::new(proto),
                                    ));
                                }
                                "defineProperty" => {
                                    let mut iter = args.into_iter();
                                    let obj = iter.next().unwrap_or(Expr::Undefined);
                                    let key = iter.next().unwrap_or(Expr::Undefined);
                                    let descriptor = iter.next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ObjectDefineProperty(
                                        Box::new(obj),
                                        Box::new(key),
                                        Box::new(descriptor),
                                    ));
                                }
                                "defineProperties" => {
                                    // `Object.defineProperties(target, descriptors)` — bulk
                                    // form of `defineProperty`. Used by chalk's index.js to
                                    // attach the `styles` getter-bag onto
                                    // `createChalk.prototype`. Pre-fix this fell through to a
                                    // generic `(Object).defineProperties(...)` call which
                                    // throws `TypeError: value is not a function` at module
                                    // init because `Object` isn't a runtime object with
                                    // method dispatch.
                                    //
                                    // Desugar to a sequence of `ObjectDefineProperty`
                                    // applications by reading `descriptors`'s own keys at
                                    // compile time when it's an object literal, otherwise
                                    // route through a runtime helper that iterates the
                                    // descriptor object's keys.
                                    let mut iter = args.into_iter();
                                    let target = iter.next().unwrap_or(Expr::Undefined);
                                    let descs = iter.next().unwrap_or(Expr::Undefined);
                                    if let Expr::Object(props) = &descs {
                                        // Static descriptor literal — desugar to a Sequence
                                        // of `defineProperty(target, key, desc)` calls and
                                        // yield `target` as the result value.
                                        let target = target;
                                        let mut exprs: Vec<Expr> =
                                            Vec::with_capacity(props.len() + 1);
                                        for (key_name, desc_expr) in props {
                                            exprs.push(Expr::ObjectDefineProperty(
                                                Box::new(target.clone()),
                                                Box::new(Expr::String(key_name.clone())),
                                                Box::new(desc_expr.clone()),
                                            ));
                                        }
                                        exprs.push(target);
                                        if exprs.len() == 1 {
                                            return Ok(exprs.into_iter().next().unwrap());
                                        }
                                        return Ok(Expr::Sequence(exprs));
                                    }
                                    return Ok(Expr::ObjectDefineProperties(
                                        Box::new(target),
                                        Box::new(descs),
                                    ));
                                }
                                "getOwnPropertyDescriptor" => {
                                    let mut iter = args.into_iter();
                                    let obj = iter.next().unwrap_or(Expr::Undefined);
                                    let key = iter.next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ObjectGetOwnPropertyDescriptor(
                                        Box::new(obj),
                                        Box::new(key),
                                    ));
                                }
                                "getOwnPropertyNames" => {
                                    return Ok(Expr::ObjectGetOwnPropertyNames(Box::new(
                                        args.into_iter().next().unwrap_or(Expr::Undefined),
                                    )));
                                }
                                "getOwnPropertySymbols" => {
                                    return Ok(Expr::ObjectGetOwnPropertySymbols(Box::new(
                                        args.into_iter().next().unwrap_or(Expr::Undefined),
                                    )));
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for Symbol static methods: Symbol.for / Symbol.keyFor
                    if obj_name == "Symbol" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "for" => {
                                    let key = args.into_iter().next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::SymbolFor(Box::new(key)));
                                }
                                "keyFor" => {
                                    let sym = args.into_iter().next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::SymbolKeyFor(Box::new(sym)));
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    if obj_name == "Reflect" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "get" => {
                                    let mut it = args.into_iter();
                                    let target = it.next().unwrap_or(Expr::Undefined);
                                    let key = it.next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ReflectGet {
                                        target: Box::new(target),
                                        key: Box::new(key),
                                    });
                                }
                                "set" => {
                                    let mut it = args.into_iter();
                                    let target = it.next().unwrap_or(Expr::Undefined);
                                    let key = it.next().unwrap_or(Expr::Undefined);
                                    let value = it.next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ReflectSet {
                                        target: Box::new(target),
                                        key: Box::new(key),
                                        value: Box::new(value),
                                    });
                                }
                                "has" => {
                                    let mut it = args.into_iter();
                                    let target = it.next().unwrap_or(Expr::Undefined);
                                    let key = it.next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ReflectHas {
                                        target: Box::new(target),
                                        key: Box::new(key),
                                    });
                                }
                                "deleteProperty" => {
                                    let mut it = args.into_iter();
                                    let target = it.next().unwrap_or(Expr::Undefined);
                                    let key = it.next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ReflectDelete {
                                        target: Box::new(target),
                                        key: Box::new(key),
                                    });
                                }
                                "ownKeys" => {
                                    let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ReflectOwnKeys(Box::new(target)));
                                }
                                "apply" => {
                                    let mut it = args.into_iter();
                                    let func = it.next().unwrap_or(Expr::Undefined);
                                    let this_arg = it.next().unwrap_or(Expr::Undefined);
                                    let args_arr = it.next().unwrap_or(Expr::Array(vec![]));
                                    return Ok(Expr::ReflectApply {
                                        func: Box::new(func),
                                        this_arg: Box::new(this_arg),
                                        args: Box::new(args_arr),
                                    });
                                }
                                "construct" => {
                                    // Special case: `Reflect.construct(ClassName, [args...])`
                                    // where ClassName is a known class — fold to a direct
                                    // `new ClassName(...args)` expression.
                                    if call.args.len() >= 2 {
                                        if let ast::Expr::Ident(cls_ident) =
                                            call.args[0].expr.as_ref()
                                        {
                                            let cls_name = cls_ident.sym.to_string();
                                            if ctx.lookup_class(&cls_name).is_some() {
                                                if let ast::Expr::Array(arr_lit) =
                                                    call.args[1].expr.as_ref()
                                                {
                                                    let new_args: Vec<Expr> = arr_lit
                                                        .elems
                                                        .iter()
                                                        .filter_map(|e| e.as_ref())
                                                        .map(|e| lower_expr(ctx, &e.expr))
                                                        .collect::<Result<Vec<_>>>()?;
                                                    return Ok(Expr::New {
                                                        class_name: cls_name,
                                                        args: new_args,
                                                        type_args: vec![],
                                                    });
                                                }
                                            }
                                        }
                                    }
                                    let mut it = args.into_iter();
                                    let target = it.next().unwrap_or(Expr::Undefined);
                                    let args_arr = it.next().unwrap_or(Expr::Array(vec![]));
                                    return Ok(Expr::ReflectConstruct {
                                        target: Box::new(target),
                                        args: Box::new(args_arr),
                                    });
                                }
                                "defineProperty" => {
                                    let mut it = args.into_iter();
                                    let target = it.next().unwrap_or(Expr::Undefined);
                                    let key = it.next().unwrap_or(Expr::Undefined);
                                    let descriptor = it.next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ReflectDefineProperty {
                                        target: Box::new(target),
                                        key: Box::new(key),
                                        descriptor: Box::new(descriptor),
                                    });
                                }
                                "getPrototypeOf" => {
                                    let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ReflectGetPrototypeOf(Box::new(target)));
                                }
                                "defineMetadata" => {
                                    let (key, value, target, property_key) =
                                        take_reflect_kvtp_args(args);
                                    return Ok(Expr::ReflectDefineMetadata {
                                        key: Box::new(key),
                                        value: Box::new(value),
                                        target: Box::new(target),
                                        property_key,
                                    });
                                }
                                "getMetadata" => {
                                    let (key, target, property_key) = take_reflect_ktp_args(args);
                                    return Ok(Expr::ReflectGetMetadata {
                                        key: Box::new(key),
                                        target: Box::new(target),
                                        property_key,
                                    });
                                }
                                "getOwnMetadata" => {
                                    let (key, target, property_key) = take_reflect_ktp_args(args);
                                    return Ok(Expr::ReflectGetOwnMetadata {
                                        key: Box::new(key),
                                        target: Box::new(target),
                                        property_key,
                                    });
                                }
                                "hasMetadata" => {
                                    let (key, target, property_key) = take_reflect_ktp_args(args);
                                    return Ok(Expr::ReflectHasMetadata {
                                        key: Box::new(key),
                                        target: Box::new(target),
                                        property_key,
                                    });
                                }
                                "hasOwnMetadata" => {
                                    let (key, target, property_key) = take_reflect_ktp_args(args);
                                    return Ok(Expr::ReflectHasOwnMetadata {
                                        key: Box::new(key),
                                        target: Box::new(target),
                                        property_key,
                                    });
                                }
                                "getMetadataKeys" => {
                                    let (target, property_key) = take_reflect_tp_args(args);
                                    return Ok(Expr::ReflectGetMetadataKeys {
                                        target: Box::new(target),
                                        property_key,
                                    });
                                }
                                "getOwnMetadataKeys" => {
                                    let (target, property_key) = take_reflect_tp_args(args);
                                    return Ok(Expr::ReflectGetOwnMetadataKeys {
                                        target: Box::new(target),
                                        property_key,
                                    });
                                }
                                "deleteMetadata" => {
                                    let (key, target, property_key) = take_reflect_ktp_args(args);
                                    return Ok(Expr::ReflectDeleteMetadata {
                                        key: Box::new(key),
                                        target: Box::new(target),
                                        property_key,
                                    });
                                }
                                "setPrototypeOf" => return Ok(Expr::Bool(true)),
                                "isExtensible" => {
                                    let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ObjectIsExtensible(Box::new(target)));
                                }
                                "preventExtensions" => {
                                    let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ObjectPreventExtensions(Box::new(target)));
                                }
                                _ => {}
                            }
                        }
                    }

                    if obj_name == "Proxy" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            if method_ident.sym.as_ref() == "revocable" {
                                let mut it = args.into_iter();
                                let target = it.next().unwrap_or(Expr::Undefined);
                                let handler = it.next().unwrap_or(Expr::Object(vec![]));
                                return Ok(Expr::ProxyRevocable {
                                    target: Box::new(target),
                                    handler: Box::new(handler),
                                });
                            }
                        }
                    }

                    // Check for Array static methods
                    if obj_name == "Array" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "isArray" => {
                                    let value = args.first().cloned().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::ArrayIsArray(Box::new(value)));
                                }
                                "from" => {
                                    let value = args.first().cloned().unwrap_or(Expr::Undefined);
                                    // `Array.from(iterable, mapFn)` uses a dedicated HIR
                                    // variant so codegen can handle Map/Set/Array sources
                                    // uniformly (materialize + js_array_map).
                                    if let Some(map_fn) = args.get(1).cloned() {
                                        return Ok(Expr::ArrayFromMapped {
                                            iterable: Box::new(value),
                                            map_fn: Box::new(map_fn),
                                        });
                                    }
                                    // Check if the source is a generator call — use iterator protocol
                                    let is_gen = is_generator_call_expr(ctx, &value);
                                    if is_gen {
                                        return Ok(Expr::IteratorToArray(Box::new(value)));
                                    }
                                    return Ok(Expr::ArrayFrom(Box::new(value)));
                                }
                                "of" => {
                                    // Array.of(1,2,3) is equivalent to [1,2,3]
                                    return Ok(Expr::Array(args));
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for net module methods
                    let is_net_module = obj_name == "net"
                        || ctx.lookup_builtin_module_alias(&obj_name) == Some("net");
                    if is_net_module {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "createServer" => {
                                    let options = args.first().cloned().map(Box::new);
                                    let connection_listener = args.get(1).cloned().map(Box::new);
                                    return Ok(Expr::NetCreateServer {
                                        options,
                                        connection_listener,
                                    });
                                }
                                // createConnection/connect fall through to generic NativeMethodCall
                                // so they dispatch via NATIVE_MODULE_TABLE to the new
                                // event-driven `js_net_socket_connect` in perry-stdlib (A1/A1.5).
                                // The dedicated `Expr::NetCreateConnection` variant was never
                                // lowered by the LLVM backend and remained as vestigial HIR;
                                // the generic path gives us working codegen for free.
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    if let Some((module_name, _imported_method)) =
                        ctx.lookup_native_module(&obj_name)
                    {
                        // Skip modules handled specifically below (path, fs, child_process, etc.)
                        // `net` used to be in this list back when its method calls
                        // were short-circuited into `Expr::NetCreateConnection` etc.
                        // After A1.5 `net` goes through the generic NativeMethodCall
                        // path so the LLVM backend's NATIVE_MODULE_TABLE dispatches
                        // to `js_net_socket_*` in perry-stdlib.
                        let is_handled_module = module_name == "path"
                            || module_name == "node:path"
                            || module_name == "fs"
                            || module_name == "node:fs"
                            || module_name == "child_process"
                            || module_name == "node:child_process"
                            || module_name == "crypto"
                            || module_name == "node:crypto"
                            || module_name == "os"
                            || module_name == "node:os";
                        if !is_handled_module {
                            // This is a call on a native module (e.g., mysql.createConnection)
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.to_string();
                                // Unimplemented-API gate (#463 / #525) for the 2-deep
                                // `mod.method()` call form. Without this, perry/* and
                                // other native-module call sites short-circuited past
                                // the `lower_member` gate that fires for the property-
                                // read form, then bailed in codegen with a per-module
                                // message (`'X' is not a known function`) — different
                                // wording, different escape hatch, harder for users to
                                // recognize as the same class of mistake. Mirrors the
                                // 3-deep gate above for `mod.X.Y()`.
                                let allow_unimplemented =
                                    std::env::var_os("PERRY_ALLOW_UNIMPLEMENTED").is_some();
                                if !allow_unimplemented
                                    && perry_api_manifest::module_has_any_entries(module_name)
                                    && perry_api_manifest::module_has_symbol(
                                        module_name,
                                        &method_name,
                                    )
                                    .is_none()
                                {
                                    // #925: this is the gate that fires
                                    // for `crypto.hmacSha256(data, key)`.
                                    let hint = super::unimpl_hints::module_member_hint(
                                        module_name,
                                        &method_name,
                                    )
                                    .map(|h| format!(" {h}"))
                                    .unwrap_or_default();
                                    crate::lower_bail!(
                                        member.span,
                                        "`{}.{}` is not implemented in Perry — see `perry --print-api-manifest` for the supported surface, \
                                         or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore. (#463){}",
                                        module_name,
                                        method_name,
                                        hint,
                                    );
                                }
                                return Ok(Expr::NativeMethodCall {
                                    module: module_name.to_string(),
                                    class_name: None, // Will be set by js_transform if needed
                                    object: None,     // Static call on module itself
                                    method: method_name,
                                    args,
                                });
                            }
                        }
                    }
                }
            }

            // Check for static method calls (e.g., Counter.increment())
            if let ast::Expr::Member(member) = expr.as_ref() {
                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                    let obj_name = obj_ident.sym.to_string();
                    // Treat uppercase imported identifiers as candidate classes —
                    // we don't have cross-module class metadata at HIR-lower
                    // time, so without this `import { MongoClient } from
                    // 'pkg'; MongoClient.connect(...)` falls through to the
                    // dynamic-dispatch path and reads garbage from the static
                    // ClosureHeader.  See compile.rs::imported_classes for the
                    // backing dispatch table that resolves these calls at
                    // codegen time.
                    let is_imported_upper = ctx.lookup_imported_func(&obj_name).is_some()
                        && obj_name
                            .chars()
                            .next()
                            .map(|c| c.is_uppercase())
                            .unwrap_or(false);
                    if ctx.lookup_class(&obj_name).is_some() || is_imported_upper {
                        match &member.prop {
                            ast::MemberProp::Ident(method_ident) => {
                                let method_name = method_ident.sym.to_string();
                                if ctx.has_static_method(&obj_name, &method_name)
                                    || is_imported_upper
                                {
                                    return Ok(Expr::StaticMethodCall {
                                        class_name: obj_name,
                                        method_name,
                                        args,
                                    });
                                }
                            }
                            // Private static method: WithPrivateStatic.#helper()
                            ast::MemberProp::PrivateName(priv_ident) => {
                                let method_name = format!("#{}", priv_ident.name);
                                if ctx.has_static_method(&obj_name, &method_name) {
                                    return Ok(Expr::StaticMethodCall {
                                        class_name: obj_name,
                                        method_name,
                                        args,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Check for native instance method calls (e.g., emitter.on(), ws.send())
            if let ast::Expr::Member(member) = expr.as_ref() {
                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                    let obj_name = obj_ident.sym.to_string();
                    // Clone module_name and class_name to avoid borrow issues
                    let native_instance = ctx
                        .lookup_native_instance(&obj_name)
                        .map(|(m, c)| (m.to_string(), c.to_string()));

                    if let Some((module_name, class_name)) = native_instance {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.to_string();
                            // Issue #562: stream subclass instances carry the
                            // bare-stream module/class tag for inherited-method
                            // dispatch (`w.pipeTo(...)`, `w.getWriter()`).
                            // Routing user-declared methods through the
                            // NativeMethodCall arm misses every dispatcher and
                            // falls through to the receiver-less zero-sentinel,
                            // so the method call returns undefined. Only route
                            // known stream-API methods through NativeMethodCall;
                            // anything else falls through to the user-class
                            // method dispatch path further down. Mirrors the
                            // PropertyGet gate in expr_member.rs.
                            let is_stream_module = matches!(
                                module_name.as_str(),
                                "readable_stream"
                                    | "writable_stream"
                                    | "transform_stream"
                                    | "readable_stream_reader"
                                    | "writable_stream_writer"
                            );
                            if is_stream_module && !is_stream_api_method(&module_name, &method_name)
                            {
                                // Fall through — let the regular method-call
                                // dispatch further down handle the user-class
                                // method.
                            } else {
                                // Get the object expression (the instance variable)
                                let object_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::NativeMethodCall {
                                    module: module_name,
                                    class_name: Some(class_name), // Use the registered class name
                                    object: Some(Box::new(object_expr)),
                                    method: method_name,
                                    args,
                                });
                            }
                        }
                    }
                }

                // issue #195: WidgetCtor(...).modifierName(...) is silently dropped.
                // Reject at compile time so users discover the options-object form.
                if let ast::Expr::Call(inner_call) = member.obj.as_ref() {
                    if let ast::Callee::Expr(inner_callee) = &inner_call.callee {
                        if let ast::Expr::Ident(widget_ident) = inner_callee.as_ref() {
                            let widget_name = widget_ident.sym.as_ref();
                            if matches!(
                                widget_name,
                                "Text"
                                    | "VStack"
                                    | "HStack"
                                    | "ZStack"
                                    | "Image"
                                    | "Spacer"
                                    | "Divider"
                                    | "ForEach"
                                    | "Label"
                                    | "Gauge"
                            ) && matches!(
                                ctx.lookup_native_module(widget_name),
                                Some(("perry/ui", _))
                            ) {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let modifier_name = method_ident.sym.as_ref();
                                    if is_widget_modifier_name(modifier_name) {
                                        return Err(anyhow!(
                                                        "modifier '{}' must be passed as an option-object on the widget constructor; use: {}(\"...\", {{ {}: ... }})",
                                                        modifier_name, widget_name, modifier_name
                                                    ));
                                    }
                                }
                            }
                        }
                    }
                }

                // Check for method calls on new Big/Decimal/BigNumber() expressions
                // e.g., new Big("100").div(2)
                if let Some(module_name) = detect_native_instance_expr(ctx, &member.obj) {
                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                        let method_name = method_ident.sym.to_string();
                        let object_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Expr::NativeMethodCall {
                            module: module_name.to_string(),
                            class_name: None, // Will be set by js_transform if needed
                            object: Some(Box::new(object_expr)),
                            method: method_name,
                            args,
                        });
                    }
                }

                // Check for chained method calls on registered native instances
                // e.g., r1.times(...).times(...) where r1 is a Big
                // The inner call might lower to a NativeMethodCall, and we need to chain properly
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.to_string();
                    // Lower the object expression first
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    // Check if it's a NativeMethodCall for a fluent-API native module
                    if let Expr::NativeMethodCall {
                        module,
                        class_name,
                        method: prior_method,
                        ..
                    } = &object_expr
                    {
                        // Methods that return the same type (builder pattern)
                        let is_math_lib =
                            matches!(module.as_str(), "big.js" | "decimal.js" | "bignumber.js");
                        let is_math_method = matches!(
                            method_name.as_str(),
                            // arithmetic + chainable rounding/formatting
                            "plus" | "minus" | "times" | "div" | "mod" |
                                    "pow" | "sqrt" | "abs" | "neg" | "round" | "floor" | "ceil" | "toFixed" |
                                    // decimal.js: terminal-shape methods that still need
                                    // NativeMethodCall dispatch (so a.plus(b).eq(c) etc.
                                    // doesn't fall back to the generic Call+PropertyGet path).
                                    "toString" | "toNumber" | "valueOf" |
                                    "eq" | "lt" | "lte" | "gt" | "gte" | "cmp" |
                                    "isZero" | "isPositive" | "isNegative"
                        );
                        // commander Command — every fluent method either
                        // returns the same handle (name/version/description/
                        // option/requiredOption/action) or a sub-Command with
                        // the same module + class (.command(name)). Either way
                        // the next chained call must dispatch through the
                        // commander NativeModSig table, not the generic
                        // dynamic-property fallback. Without this branch
                        // `program.name(...).version(...)` only the first
                        // call landed as a NativeMethodCall and the rest
                        // silently no-op'd at codegen — issue #187.
                        let is_commander = module.as_str() == "commander";
                        let is_commander_method = matches!(
                            method_name.as_str(),
                            "name"
                                | "version"
                                | "description"
                                | "option"
                                | "requiredOption"
                                | "action"
                                | "command"
                                | "parse"
                                | "opts"
                        );
                        // #1048 — fastify Reply chainable methods. `reply.code(201)
                        // .type("application/json").send(payload)` ships every method
                        // returning the same reply handle for chaining; without this
                        // branch only the inner `.code(...)` resolved as a
                        // NativeMethodCall and the rest of the chain fell through to
                        // the generic Call+PropertyGet path. That path routes through
                        // `js_native_call_method` → HANDLE_METHOD_DISPATCH, which has
                        // no fastify arm when the well-known flip strips
                        // `bundled-fastify` from stdlib — so `.type(...)` returned an
                        // untagged NaN that the next chain step read as a number
                        // ("(number).send is not a function"). Static NATIVE_MODULE_TABLE
                        // dispatch covers it correctly.
                        let is_fastify_reply =
                            module.as_str() == "fastify" && class_name.as_deref() == Some("Reply");
                        let is_fastify_reply_chain_method = matches!(
                            method_name.as_str(),
                            "code" | "status" | "header" | "type" | "send"
                        );
                        if (is_math_lib && is_math_method)
                            || (is_commander && is_commander_method)
                            || (is_fastify_reply && is_fastify_reply_chain_method)
                        {
                            return Ok(Expr::NativeMethodCall {
                                module: module.clone(),
                                class_name: class_name.clone(),
                                object: Some(Box::new(object_expr)),
                                method: method_name,
                                args,
                            });
                        }
                        // Database-driver chaining: methods like
                        // `db.prepare(sql).run()` / `db.prepare(sql).get()` /
                        // `db.prepare(sql).all()` where the inner call returns
                        // a *new* native class (Statement) — not the same
                        // handle as the receiver. Look up `(module,
                        // prior_method)` in the chaining table and dispatch
                        // the outer call against the resulting class. Without
                        // this, the outer `.run()`/`.get()`/`.all()` fell
                        // through to the generic js_native_call_method
                        // dispatcher: SQL never executed, returned objects
                        // had no keys_array, `Object.keys(row)` was `[]` and
                        // `row.id` was undefined.
                        let chained_class: Option<&'static str> =
                            match (module.as_str(), prior_method.as_str()) {
                                ("better-sqlite3", "prepare") => Some("Statement"),
                                ("mongodb", "db") => Some("Database"),
                                ("mongodb", "collection") => Some("Collection"),
                                ("mysql2", "getConnection")
                                | ("mysql2/promise", "getConnection") => Some("PoolConnection"),
                                ("pg", "connect") => Some("PoolClient"),
                                ("ioredis", "duplicate") => Some("Redis"),
                                _ => None,
                            };
                        if let Some(result_class) = chained_class {
                            return Ok(Expr::NativeMethodCall {
                                module: module.clone(),
                                class_name: Some(result_class.to_string()),
                                object: Some(Box::new(object_expr)),
                                method: method_name,
                                args,
                            });
                        }
                    }
                }
            }

            // Issue #76 — standard `<inst>.exports.<method>(args...)` shape
            // from the WebAssembly JS API. Sits OUTSIDE the Ident-receiver
            // gate below because `<inst>.exports` is itself a Member, not
            // an Ident. Only routes when `<inst>` resolves to a tagged
            // wasm-instance local (populated at var-decl time when the
            // initializer is `WebAssembly.instantiate(...)`) — this avoids
            // stealing `module.exports.foo()` etc. from the generic dispatch.
            if let ast::Expr::Member(outer_member) = expr.as_ref() {
                if let ast::MemberProp::Ident(method_ident) = &outer_member.prop {
                    if let ast::Expr::Member(inner) = outer_member.obj.as_ref() {
                        if let ast::MemberProp::Ident(inner_prop) = &inner.prop {
                            if inner_prop.sym.as_ref() == "exports" {
                                if let ast::Expr::Ident(inst_ident) = inner.obj.as_ref() {
                                    let inst_name = inst_ident.sym.as_ref();
                                    if ctx.wasm_instance_locals.contains(inst_name) {
                                        let instance_lowered = lower_expr(ctx, inner.obj.as_ref())?;
                                        ctx.uses_webassembly = true;
                                        return Ok(Expr::WebAssemblyCallExport {
                                            instance: Box::new(instance_lowered),
                                            name: Box::new(Expr::String(
                                                method_ident.sym.as_ref().to_string(),
                                            )),
                                            args,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check for fs.methodName() calls (including require('fs') aliases)
            if let ast::Expr::Member(member) = expr.as_ref() {
                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                    // Check if this is 'fs' directly or an alias from require('fs')
                    let obj_name = obj_ident.sym.as_ref();
                    let is_fs_module =
                        obj_name == "fs" || ctx.lookup_builtin_module_alias(obj_name) == Some("fs");
                    if is_fs_module {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "readFileSync" => {
                                    if args.len() >= 2 {
                                        // readFileSync(path, encoding) — returns string
                                        return Ok(Expr::FsReadFileSync(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    } else if args.len() == 1 {
                                        // readFileSync(path) without encoding — returns Buffer (Node parity)
                                        return Ok(Expr::FsReadFileBinary(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "writeFileSync" => {
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let path = iter.next().unwrap();
                                        let content = iter.next().unwrap();
                                        return Ok(Expr::FsWriteFileSync(
                                            Box::new(path),
                                            Box::new(content),
                                        ));
                                    }
                                }
                                "appendFileSync" => {
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let path = iter.next().unwrap();
                                        let content = iter.next().unwrap();
                                        return Ok(Expr::FsAppendFileSync(
                                            Box::new(path),
                                            Box::new(content),
                                        ));
                                    }
                                }
                                "existsSync" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::FsExistsSync(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "mkdirSync" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::FsMkdirSync(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "unlinkSync" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::FsUnlinkSync(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "readFileBuffer" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::FsReadFileBinary(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "rmRecursive" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::FsRmRecursive(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                // Issue #648 fallout: `fs.rmSync(path, opts?)` was
                                // historically silently no-op'd by `js_native_call_method`'s
                                // catch-all when the method wasn't statically wired. Now
                                // that catch-all throws, we need an explicit lowering.
                                // Routes to FsRmRecursive (recursive removal); the
                                // `{recursive,force,maxRetries,retryDelay}` opts arg is
                                // ignored — `js_fs_rm_recursive` already does recursive
                                // removal unconditionally.
                                "rmSync" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::FsRmRecursive(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for path.methodName() calls (including require('path') aliases)
                    let is_path_module = obj_name == "path"
                        || ctx.lookup_builtin_module_alias(obj_name) == Some("path");
                    if is_path_module {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "join" => {
                                    if args.is_empty() {
                                        return Ok(Expr::String(".".to_string()));
                                    }
                                    if args.len() == 1 {
                                        return Ok(Expr::PathNormalize(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                    let mut iter = args.into_iter();
                                    let mut result = iter.next().unwrap();
                                    for next_arg in iter {
                                        result =
                                            Expr::PathJoin(Box::new(result), Box::new(next_arg));
                                    }
                                    return Ok(result);
                                }
                                "dirname" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::PathDirname(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "basename" => {
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let path_arg = iter.next().unwrap();
                                        let ext_arg = iter.next().unwrap();
                                        return Ok(Expr::PathBasenameExt(
                                            Box::new(path_arg),
                                            Box::new(ext_arg),
                                        ));
                                    }
                                    if !args.is_empty() {
                                        return Ok(Expr::PathBasename(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "extname" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::PathExtname(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "resolve" => {
                                    if !args.is_empty() {
                                        // path.resolve(a, b, c): per Node, a later
                                        // absolute segment resets the accumulation —
                                        // distinct from path.join. Use PathResolveJoin
                                        // (reset-on-absolute) for the chain.
                                        let mut iter = args.into_iter();
                                        let first = iter.next().unwrap();
                                        let mut joined = first;
                                        for next_arg in iter {
                                            joined = Expr::PathResolveJoin(
                                                Box::new(joined),
                                                Box::new(next_arg),
                                            );
                                        }
                                        return Ok(Expr::PathResolve(Box::new(joined)));
                                    }
                                }
                                "isAbsolute" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::PathIsAbsolute(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "relative" => {
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let from = iter.next().unwrap();
                                        let to = iter.next().unwrap();
                                        return Ok(Expr::PathRelative(
                                            Box::new(from),
                                            Box::new(to),
                                        ));
                                    }
                                }
                                "normalize" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::PathNormalize(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "parse" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::PathParse(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "format" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::PathFormat(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "toNamespacedPath" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::PathToNamespacedPath(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "matchesGlob" => {
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let path_arg = iter.next().unwrap();
                                        let pattern = iter.next().unwrap();
                                        return Ok(Expr::PathMatchesGlob(
                                            Box::new(path_arg),
                                            Box::new(pattern),
                                        ));
                                    }
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for JSON.methodName() calls
                    if obj_ident.sym.as_ref() == "JSON" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "parse" => {
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let text = iter.next().unwrap();
                                        let reviver = iter.next().unwrap();
                                        return Ok(Expr::JsonParseWithReviver(
                                            Box::new(text),
                                            Box::new(reviver),
                                        ));
                                    } else if !args.is_empty() {
                                        let text = args.into_iter().next().unwrap();
                                        // Issue #179 typed-parse plan: if the call site
                                        // provides a TypeScript type argument (e.g.
                                        // `JSON.parse<Item[]>(blob)`), carry it into HIR
                                        // so codegen can emit a specialized parse path.
                                        // Semantically identical to JsonParse at runtime
                                        // (the `<T>` erases — Node-compatible).
                                        if let Some(type_args) = call.type_args.as_ref() {
                                            if let Some(ts_type) = type_args.params.first() {
                                                let ty =
                                                    extract_ts_type_with_ctx(ts_type, Some(ctx));
                                                // Resolve Named → structural (interface)
                                                // aliases so codegen sees the full
                                                // ObjectType without re-walking the alias
                                                // table. Array<Named> inner element
                                                // also gets resolved.
                                                let resolved = resolve_typed_parse_ty(ctx, ty);
                                                if !matches!(resolved, Type::Any | Type::Unknown) {
                                                    // Source-order field list for the
                                                    // inner Object type, if we can
                                                    // extract it from the AST. Codegen
                                                    // uses this for the fast-path
                                                    // per-field comparison.
                                                    let ordered_keys =
                                                        extract_typed_parse_source_order(
                                                            ts_type, ctx,
                                                        );
                                                    return Ok(Expr::JsonParseTyped {
                                                        text: Box::new(text),
                                                        ty: resolved,
                                                        ordered_keys,
                                                    });
                                                }
                                            }
                                        }
                                        return Ok(Expr::JsonParse(Box::new(text)));
                                    }
                                }
                                "stringify" => {
                                    if args.len() >= 2 {
                                        let mut it = args.into_iter();
                                        let value = it.next().unwrap();
                                        let replacer = it.next().unwrap();
                                        let spacer = it.next().unwrap_or(Expr::Null);
                                        return Ok(Expr::JsonStringifyFull(
                                            Box::new(value),
                                            Box::new(replacer),
                                            Box::new(spacer),
                                        ));
                                    } else if args.len() == 1 {
                                        let value = args.into_iter().next().unwrap();
                                        // Route ALL single-arg stringify through JsonStringifyFull
                                        // so the runtime can return TAG_UNDEFINED for undefined input
                                        return Ok(Expr::JsonStringifyFull(
                                            Box::new(value),
                                            Box::new(Expr::Null),
                                            Box::new(Expr::Null),
                                        ));
                                    }
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for performance.now()
                    if obj_ident.sym.as_ref() == "performance" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            if method_ident.sym.as_ref() == "now" {
                                return Ok(Expr::PerformanceNow);
                            }
                        }
                    }

                    // Check for Response.json(value) / Response.redirect(url, status?) static factories
                    if obj_ident.sym.as_ref() == "Response" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "json" | "redirect" => {
                                    ctx.uses_fetch = true;
                                    return Ok(Expr::NativeMethodCall {
                                        module: "fetch".to_string(),
                                        class_name: Some("Response".to_string()),
                                        object: None,
                                        method: format!("static_{}", method_name),
                                        args,
                                    });
                                }
                                _ => {}
                            }
                        }
                    }

                    // WebAssembly.* host runtime (issue #76). MVP surface:
                    // - `WebAssembly.validate(bytes)`
                    // - `WebAssembly.instantiate(bytes)` (sync, Perry-shape)
                    // - `WebAssembly.callExport(inst, name, ...args)` (legacy
                    //   helper kept for compatibility with the first PoC pass)
                    // The standard `inst.exports.<method>(...)` shape is
                    // recognised separately below as a syntactic pattern.
                    if obj_ident.sym.as_ref() == "WebAssembly" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "validate" => {
                                    if !args.is_empty() {
                                        ctx.uses_webassembly = true;
                                        return Ok(Expr::WebAssemblyValidate(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "instantiate" => {
                                    if !args.is_empty() {
                                        ctx.uses_webassembly = true;
                                        return Ok(Expr::WebAssemblyInstantiate(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "callExport" => {
                                    if args.len() >= 2 {
                                        ctx.uses_webassembly = true;
                                        let mut it = args.into_iter();
                                        let instance = it.next().unwrap();
                                        let name = it.next().unwrap();
                                        let rest: Vec<Expr> = it.collect();
                                        return Ok(Expr::WebAssemblyCallExport {
                                            instance: Box::new(instance),
                                            name: Box::new(name),
                                            args: rest,
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    // (Standard `<inst>.exports.<method>` shape moved to a
                    // dedicated block above — see `// Issue #76` near the
                    // fs-method dispatch — so it can match Member receivers
                    // without being gated on the Ident-receiver wrapper.)

                    // Check for Math.methodName() calls
                    if obj_ident.sym.as_ref() == "Math" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "floor" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathFloor(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "ceil" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathCeil(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "round" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathRound(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "trunc" => {
                                    // Math.trunc(x) = x >= 0 ? floor(x) : ceil(x)
                                    if !args.is_empty() {
                                        let arg = args.into_iter().next().unwrap();
                                        return Ok(Expr::Conditional {
                                            condition: Box::new(Expr::Compare {
                                                op: crate::CompareOp::Ge,
                                                left: Box::new(arg.clone()),
                                                right: Box::new(Expr::Number(0.0)),
                                            }),
                                            then_expr: Box::new(Expr::MathFloor(Box::new(
                                                arg.clone(),
                                            ))),
                                            else_expr: Box::new(Expr::MathCeil(Box::new(arg))),
                                        });
                                    }
                                }
                                "sign" => {
                                    // Math.sign(x) = x > 0 ? 1 : x < 0 ? -1 : 0 (or x for NaN)
                                    if !args.is_empty() {
                                        let arg = args.into_iter().next().unwrap();
                                        return Ok(Expr::Conditional {
                                            condition: Box::new(Expr::Compare {
                                                op: crate::CompareOp::Gt,
                                                left: Box::new(arg.clone()),
                                                right: Box::new(Expr::Number(0.0)),
                                            }),
                                            then_expr: Box::new(Expr::Number(1.0)),
                                            else_expr: Box::new(Expr::Conditional {
                                                condition: Box::new(Expr::Compare {
                                                    op: crate::CompareOp::Lt,
                                                    left: Box::new(arg.clone()),
                                                    right: Box::new(Expr::Number(0.0)),
                                                }),
                                                then_expr: Box::new(Expr::Number(-1.0)),
                                                else_expr: Box::new(arg),
                                            }),
                                        });
                                    }
                                }
                                "abs" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathAbs(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "sqrt" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathSqrt(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "log" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathLog(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "log2" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathLog2(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "log10" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathLog10(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "pow" => {
                                    if args.len() >= 2 {
                                        let mut args_iter = args.into_iter();
                                        let base = args_iter.next().unwrap();
                                        let exp = args_iter.next().unwrap();
                                        return Ok(Expr::MathPow(Box::new(base), Box::new(exp)));
                                    }
                                }
                                "min" => {
                                    if has_spread && args.len() == 1 {
                                        return Ok(Expr::MathMinSpread(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                    return Ok(Expr::MathMin(args));
                                }
                                "max" => {
                                    if has_spread && args.len() == 1 {
                                        return Ok(Expr::MathMaxSpread(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                    return Ok(Expr::MathMax(args));
                                }
                                "random" => {
                                    return Ok(Expr::MathRandom);
                                }
                                "imul" => {
                                    if args.len() >= 2 {
                                        let mut args_iter = args.into_iter();
                                        let a = args_iter.next().unwrap();
                                        let b = args_iter.next().unwrap();
                                        return Ok(Expr::MathImul(Box::new(a), Box::new(b)));
                                    }
                                }
                                "sin" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathSin(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "cos" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathCos(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "tan" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathTan(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "asin" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathAsin(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "acos" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathAcos(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "atan" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathAtan(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "atan2" => {
                                    if args.len() >= 2 {
                                        let mut args_iter = args.into_iter();
                                        let y = args_iter.next().unwrap();
                                        let x = args_iter.next().unwrap();
                                        return Ok(Expr::MathAtan2(Box::new(y), Box::new(x)));
                                    }
                                }
                                "cbrt" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathCbrt(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "hypot" => {
                                    return Ok(Expr::MathHypot(args));
                                }
                                "fround" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathFround(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "clz32" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathClz32(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "expm1" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathExpm1(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "log1p" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathLog1p(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "sinh" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathSinh(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "cosh" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathCosh(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "tanh" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathTanh(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "asinh" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathAsinh(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "acosh" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathAcosh(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "atanh" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathAtanh(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "exp" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::MathExp(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for Number.methodName() static calls
                    if obj_ident.sym.as_ref() == "Number" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "isNaN" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::NumberIsNaN(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "isFinite" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::NumberIsFinite(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "isInteger" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::NumberIsInteger(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "isSafeInteger" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::NumberIsSafeInteger(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "parseFloat" => {
                                    // Number.parseFloat is the same as global parseFloat
                                    if !args.is_empty() {
                                        return Ok(Expr::ParseFloat(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "parseInt" => {
                                    // Number.parseInt is the same as global parseInt
                                    let mut iter = args.into_iter();
                                    let string_arg = if let Some(s) = iter.next() {
                                        Box::new(s)
                                    } else {
                                        return Err(anyhow!(
                                            "Number.parseInt requires at least one argument"
                                        ));
                                    };
                                    let radix_arg = iter.next().map(Box::new);
                                    return Ok(Expr::ParseInt {
                                        string: string_arg,
                                        radix: radix_arg,
                                    });
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for String.methodName() static calls
                    if obj_ident.sym.as_ref() == "String" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "fromCharCode" => {
                                    if args.len() == 1 {
                                        return Ok(Expr::StringFromCharCode(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    } else if args.len() > 1 {
                                        // Multi-arg: concat each char as a separate fromCharCode call
                                        let mut iter = args.into_iter();
                                        let mut acc = Expr::StringFromCharCode(Box::new(
                                            iter.next().unwrap(),
                                        ));
                                        for arg in iter {
                                            acc = Expr::Binary {
                                                op: crate::ir::BinaryOp::Add,
                                                left: Box::new(acc),
                                                right: Box::new(Expr::StringFromCharCode(
                                                    Box::new(arg),
                                                )),
                                            };
                                        }
                                        return Ok(acc);
                                    }
                                }
                                "fromCodePoint" => {
                                    if args.len() == 1 {
                                        return Ok(Expr::StringFromCodePoint(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    } else if args.len() > 1 {
                                        let mut iter = args.into_iter();
                                        let mut acc = Expr::StringFromCodePoint(Box::new(
                                            iter.next().unwrap(),
                                        ));
                                        for arg in iter {
                                            acc = Expr::Binary {
                                                op: crate::ir::BinaryOp::Add,
                                                left: Box::new(acc),
                                                right: Box::new(Expr::StringFromCodePoint(
                                                    Box::new(arg),
                                                )),
                                            };
                                        }
                                        return Ok(acc);
                                    }
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for crypto.methodName() calls (including require('crypto') aliases)
                    let is_crypto_module = obj_name == "crypto"
                        || ctx.lookup_builtin_module_alias(obj_name) == Some("crypto");
                    if is_crypto_module {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "randomBytes" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::CryptoRandomBytes(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "randomUUID" => {
                                    return Ok(Expr::CryptoRandomUUID);
                                }
                                "sha256" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::CryptoSha256(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "md5" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::CryptoMd5(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                // `crypto.getRandomValues(buf)` fills the buffer
                                // in-place with random bytes and returns it.
                                // Lower as a synthetic instance method call so
                                // the runtime buffer dispatcher (added in
                                // perry-runtime/src/object.rs) handles it via
                                // `js_buffer_fill_random`.
                                "getRandomValues" => {
                                    if !args.is_empty() {
                                        let buf_arg = args.into_iter().next().unwrap();
                                        return Ok(Expr::Call {
                                            callee: Box::new(Expr::PropertyGet {
                                                object: Box::new(buf_arg),
                                                property: "$$cryptoFillRandom".to_string(),
                                            }),
                                            args: vec![],
                                            type_args: vec![],
                                        });
                                    }
                                }
                                // `crypto.randomFillSync(buf, offset?, size?)`
                                // — fills `buf` in-place with random bytes
                                // and returns it. Handles BufferHeader and
                                // TypedArrayHeader (Uint8Array, Uint32Array,
                                // etc — axios uses Uint32Array). The
                                // offset/size args are optional; absent
                                // values lower as Undefined and the runtime
                                // treats them as 0 / full-length.
                                "randomFillSync" => {
                                    if !args.is_empty() {
                                        let mut iter = args.into_iter();
                                        let buffer = iter.next().unwrap();
                                        let offset = iter.next().unwrap_or(Expr::Undefined);
                                        let size = iter.next().unwrap_or(Expr::Undefined);
                                        return Ok(Expr::CryptoRandomFillSync {
                                            buffer: Box::new(buffer),
                                            offset: Box::new(offset),
                                            size: Box::new(size),
                                        });
                                    }
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for os.methodName() calls (including require('os') aliases)
                    let is_os_module =
                        obj_name == "os" || ctx.lookup_builtin_module_alias(obj_name) == Some("os");
                    if is_os_module {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "platform" => {
                                    return Ok(Expr::OsPlatform);
                                }
                                "arch" => {
                                    return Ok(Expr::OsArch);
                                }
                                "hostname" => {
                                    return Ok(Expr::OsHostname);
                                }
                                "homedir" => {
                                    return Ok(Expr::OsHomedir);
                                }
                                "tmpdir" => {
                                    return Ok(Expr::OsTmpdir);
                                }
                                "totalmem" => {
                                    return Ok(Expr::OsTotalmem);
                                }
                                "freemem" => {
                                    return Ok(Expr::OsFreemem);
                                }
                                "uptime" => {
                                    return Ok(Expr::OsUptime);
                                }
                                "type" => {
                                    return Ok(Expr::OsType);
                                }
                                "release" => {
                                    return Ok(Expr::OsRelease);
                                }
                                "cpus" => {
                                    return Ok(Expr::OsCpus);
                                }
                                "networkInterfaces" => {
                                    return Ok(Expr::OsNetworkInterfaces);
                                }
                                "userInfo" => {
                                    return Ok(Expr::OsUserInfo);
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for Buffer.methodName() static calls — also accept
                    // aliased imports per #831 (see the longer note at the
                    // first Buffer site above).
                    let is_buffer_ref = obj_name == "Buffer"
                        || matches!(
                            ctx.lookup_native_module(&obj_name),
                            Some(("buffer", Some("Buffer")))
                        );
                    if is_buffer_ref {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "from" => {
                                    let data = args.first().cloned().unwrap_or(Expr::Undefined);
                                    let encoding = args.get(1).cloned().map(Box::new);
                                    return Ok(Expr::BufferFrom {
                                        data: Box::new(data),
                                        encoding,
                                    });
                                }
                                "alloc" => {
                                    if !args.is_empty() {
                                        let mut args_iter = args.into_iter();
                                        let size = args_iter.next().unwrap();
                                        let fill = args_iter.next().map(Box::new);
                                        return Ok(Expr::BufferAlloc {
                                            size: Box::new(size),
                                            fill,
                                        });
                                    }
                                }
                                "allocUnsafe" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::BufferAllocUnsafe(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "concat" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::BufferConcat(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "isBuffer" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::BufferIsBuffer(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "byteLength" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::BufferByteLength(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                // `Buffer.compare(a, b)` returns -1/0/1. The runtime
                                // dispatch already handles `a.compare(b)` as an
                                // instance method routing through `js_buffer_compare`.
                                // Synthesize that form so we don't need a dedicated
                                // HIR variant or runtime entry point.
                                "compare" => {
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let a = iter.next().unwrap();
                                        let b = iter.next().unwrap();
                                        return Ok(Expr::Call {
                                            callee: Box::new(Expr::PropertyGet {
                                                object: Box::new(a),
                                                property: "compare".to_string(),
                                            }),
                                            args: vec![b],
                                            type_args: vec![],
                                        });
                                    }
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for child_process named imports (execSync, spawnSync, spawn, exec)
                    let is_child_process_module =
                        ctx.lookup_builtin_module_alias(obj_name) == Some("child_process");
                    if is_child_process_module {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "execSync" => {
                                    if !args.is_empty() {
                                        let mut args_iter = args.into_iter();
                                        let command = args_iter.next().unwrap();
                                        let options = args_iter.next().map(Box::new);
                                        return Ok(Expr::ChildProcessExecSync {
                                            command: Box::new(command),
                                            options,
                                        });
                                    }
                                }
                                "spawnSync" => {
                                    if !args.is_empty() {
                                        let mut args_iter = args.into_iter();
                                        let command = args_iter.next().unwrap();
                                        let spawn_args = args_iter.next().map(Box::new);
                                        let options = args_iter.next().map(Box::new);
                                        return Ok(Expr::ChildProcessSpawnSync {
                                            command: Box::new(command),
                                            args: spawn_args,
                                            options,
                                        });
                                    }
                                }
                                "spawn" => {
                                    if !args.is_empty() {
                                        let mut args_iter = args.into_iter();
                                        let command = args_iter.next().unwrap();
                                        let spawn_args = args_iter.next().map(Box::new);
                                        let options = args_iter.next().map(Box::new);
                                        return Ok(Expr::ChildProcessSpawn {
                                            command: Box::new(command),
                                            args: spawn_args,
                                            options,
                                        });
                                    }
                                }
                                "exec" => {
                                    if !args.is_empty() {
                                        let mut args_iter = args.into_iter();
                                        let command = args_iter.next().unwrap();
                                        let options = args_iter.next().map(Box::new);
                                        let callback = args_iter.next().map(Box::new);
                                        return Ok(Expr::ChildProcessExec {
                                            command: Box::new(command),
                                            options,
                                            callback,
                                        });
                                    }
                                }
                                "spawnBackground" => {
                                    if args.len() >= 3 {
                                        let mut args_iter = args.into_iter();
                                        let command = args_iter.next().unwrap();
                                        let spawn_args = args_iter.next().map(Box::new);
                                        let log_file = args_iter.next().unwrap();
                                        let env_json = args_iter.next().map(Box::new);
                                        return Ok(Expr::ChildProcessSpawnBackground {
                                            command: Box::new(command),
                                            args: spawn_args,
                                            log_file: Box::new(log_file),
                                            env_json,
                                        });
                                    }
                                }
                                "getProcessStatus" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::ChildProcessGetProcessStatus(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                "killProcess" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::ChildProcessKillProcess(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    }
                                }
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for net.methodName() calls
                    let is_net_module = obj_name == "net"
                        || ctx.lookup_builtin_module_alias(obj_name) == Some("net");
                    if is_net_module {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "createServer" => {
                                    let mut args_iter = args.into_iter();
                                    let options = args_iter.next().map(Box::new);
                                    let connection_listener = args_iter.next().map(Box::new);
                                    return Ok(Expr::NetCreateServer {
                                        options,
                                        connection_listener,
                                    });
                                }
                                // createConnection/connect: see sibling site above —
                                // falls through to generic NativeMethodCall so the LLVM
                                // backend's NATIVE_MODULE_TABLE dispatch can handle it.
                                _ => {} // Fall through to generic handling
                            }
                        }
                    }

                    // Check for AbortSignal.timeout(ms) static method call
                    if obj_ident.sym.as_ref() == "AbortSignal" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            if method_name == "timeout" {
                                return Ok(Expr::StaticMethodCall {
                                    class_name: "AbortSignal".to_string(),
                                    method_name: "timeout".to_string(),
                                    args,
                                });
                            }
                        }
                    }

                    // Check for Date.now() / Date.parse() / Date.UTC() static method calls
                    if obj_ident.sym.as_ref() == "Date" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            if method_name == "now" {
                                return Ok(Expr::DateNow);
                            }
                            if method_name == "parse" && !args.is_empty() {
                                return Ok(Expr::DateParse(Box::new(
                                    args.into_iter().next().unwrap(),
                                )));
                            }
                            if method_name == "UTC" {
                                return Ok(Expr::DateUtc(args));
                            }
                        }
                    }

                    // Issue #650: URL.canParse(s) / URL.parse(s) static
                    // methods. URL is special-cased at HIR (the `URL`
                    // identifier resolves to a synthetic `0` value rather
                    // than a real ClassRef), so the static-method dispatch
                    // never reached the canonical class-method path. This
                    // arm intercepts both spellings and routes to dedicated
                    // HIR variants.
                    if obj_ident.sym.as_ref() == "URL" {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            if method_name == "canParse" && !args.is_empty() {
                                return Ok(Expr::UrlCanParse(Box::new(
                                    args.into_iter().next().unwrap(),
                                )));
                            }
                            if method_name == "parse" && !args.is_empty() {
                                return Ok(Expr::UrlParse(Box::new(
                                    args.into_iter().next().unwrap(),
                                )));
                            }
                        }
                    }
                }

                // Chained `new URLSearchParams(init).<method>(args)` — without
                // an intermediate let binding the typed-local arm below
                // doesn't fire and `.toString()` falls through to
                // Object.prototype.toString printing `"[object Object]"`.
                // Refs #575.
                if let ast::Expr::New(new_expr) = member.obj.as_ref() {
                    if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                        if class_ident.sym.as_ref() == "URLSearchParams" {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.as_ref();
                                let recv = lower_expr(ctx, &member.obj)?;
                                match build_url_search_params_method_call(recv, method_name, args) {
                                    Ok(expr) => return Ok(expr),
                                    Err(returned_args) => args = returned_args,
                                }
                            }
                        }
                    }
                }

                // Issue #650: URL instance toString / toJSON. Check this
                // before the Date arms so `urlInstance.toJSON()` doesn't
                // get rewritten as `DateToJSON(url)` and return a Date
                // string. Receiver-class detection only matches:
                //   `new URL(...).toString()` (immediate ctor)
                //   `let u = new URL(...); u.toString()` (typed local)
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    if static_receiver_class(ctx, member.obj.as_ref()) == Some("URL") {
                        match method_name {
                            "toString" => {
                                let url_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::UrlInstanceToString(Box::new(url_expr)));
                            }
                            "toJSON" => {
                                let url_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::UrlInstanceToJSON(Box::new(url_expr)));
                            }
                            _ => {}
                        }
                    }
                }

                // Issue #650: gate the AMBIGUOUS Date instance method arms
                // on a receiver-type check. Methods like `toJSON` /
                // `toString` / `toLocaleString` / `valueOf` exist on every
                // JS object — pre-fix the arms below fired unconditionally,
                // so calling any of them on a URL / class instance / array
                // got silently rewritten as a Date method, returning a Date
                // string for the URL.toJSON() case the issue tracks.
                let recv_class = if let ast::MemberProp::Ident(_) = &member.prop {
                    static_receiver_class(ctx, member.obj.as_ref())
                } else {
                    None
                };
                // #809: `Some("Object")` (object literal / `Object.create`)
                // joins URL as a "definitely not a Date" receiver.
                let allow_ambiguous_date = !matches!(recv_class, Some("URL") | Some("Object"));
                // Methods we treat as Date-only when the receiver is unambiguously
                // Date or unknown (current behavior). `toString` / `toJSON` etc.
                // skip these arms when `recv_class` proves the receiver is NOT a Date.

                // Check for Date instance method calls (date.getTime(), etc.)
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    let ambiguous = matches!(
                        method_name,
                        "toJSON"
                            | "toString"
                            | "toLocaleString"
                            | "toDateString"
                            | "toTimeString"
                            | "toLocaleDateString"
                            | "toLocaleTimeString"
                            | "toISOString"
                            | "valueOf"
                    );
                    if ambiguous && !allow_ambiguous_date {
                        // Receiver is statically a non-Date class (e.g. URL).
                        // Skip the Date arms below — fall through to generic.
                    } else {
                        match method_name {
                            "getTime" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetTime(Box::new(date_expr)));
                            }
                            "toISOString" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateToISOString(Box::new(date_expr)));
                            }
                            "getFullYear" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetFullYear(Box::new(date_expr)));
                            }
                            "getMonth" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetMonth(Box::new(date_expr)));
                            }
                            "getDate" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetDate(Box::new(date_expr)));
                            }
                            "getDay" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetDay(Box::new(date_expr)));
                            }
                            "getHours" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetHours(Box::new(date_expr)));
                            }
                            "getMinutes" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetMinutes(Box::new(date_expr)));
                            }
                            "getSeconds" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetSeconds(Box::new(date_expr)));
                            }
                            "getMilliseconds" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetMilliseconds(Box::new(date_expr)));
                            }
                            // UTC getters
                            "getUTCDay" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetUtcDay(Box::new(date_expr)));
                            }
                            "getUTCFullYear" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetUtcFullYear(Box::new(date_expr)));
                            }
                            "getUTCMonth" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetUtcMonth(Box::new(date_expr)));
                            }
                            "getUTCDate" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetUtcDate(Box::new(date_expr)));
                            }
                            "getUTCHours" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetUtcHours(Box::new(date_expr)));
                            }
                            "getUTCMinutes" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetUtcMinutes(Box::new(date_expr)));
                            }
                            "getUTCSeconds" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetUtcSeconds(Box::new(date_expr)));
                            }
                            "getUTCMilliseconds" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetUtcMilliseconds(Box::new(date_expr)));
                            }
                            // Other getters/methods
                            "valueOf" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateValueOf(Box::new(date_expr)));
                            }
                            "toDateString" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateToDateString(Box::new(date_expr)));
                            }
                            "toTimeString" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateToTimeString(Box::new(date_expr)));
                            }
                            "toLocaleDateString" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateToLocaleDateString(Box::new(date_expr)));
                            }
                            "toLocaleTimeString" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateToLocaleTimeString(Box::new(date_expr)));
                            }
                            "toLocaleString" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateToLocaleString(Box::new(date_expr)));
                            }
                            "getTimezoneOffset" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateGetTimezoneOffset(Box::new(date_expr)));
                            }
                            "toJSON" => {
                                let date_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::DateToJSON(Box::new(date_expr)));
                            }
                            // UTC setters — mutate the local variable in place
                            "setUTCFullYear" | "setUTCMonth" | "setUTCDate" | "setUTCHours"
                            | "setUTCMinutes" | "setUTCSeconds" | "setUTCMilliseconds" => {
                                if !args.is_empty() {
                                    let value_expr = args.into_iter().next().unwrap();
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    let setter_call = match method_name {
                                        "setUTCFullYear" => Expr::DateSetUtcFullYear {
                                            date: Box::new(date_expr.clone()),
                                            value: Box::new(value_expr),
                                        },
                                        "setUTCMonth" => Expr::DateSetUtcMonth {
                                            date: Box::new(date_expr.clone()),
                                            value: Box::new(value_expr),
                                        },
                                        "setUTCDate" => Expr::DateSetUtcDate {
                                            date: Box::new(date_expr.clone()),
                                            value: Box::new(value_expr),
                                        },
                                        "setUTCHours" => Expr::DateSetUtcHours {
                                            date: Box::new(date_expr.clone()),
                                            value: Box::new(value_expr),
                                        },
                                        "setUTCMinutes" => Expr::DateSetUtcMinutes {
                                            date: Box::new(date_expr.clone()),
                                            value: Box::new(value_expr),
                                        },
                                        "setUTCSeconds" => Expr::DateSetUtcSeconds {
                                            date: Box::new(date_expr.clone()),
                                            value: Box::new(value_expr),
                                        },
                                        "setUTCMilliseconds" => Expr::DateSetUtcMilliseconds {
                                            date: Box::new(date_expr.clone()),
                                            value: Box::new(value_expr),
                                        },
                                        _ => unreachable!(),
                                    };
                                    // If receiver is a local variable, mutate it in place by wrapping
                                    // the setter result in a LocalSet so the new timestamp is stored back.
                                    if let Expr::LocalGet(local_id) = &date_expr {
                                        return Ok(Expr::LocalSet(
                                            *local_id,
                                            Box::new(setter_call),
                                        ));
                                    }
                                    return Ok(setter_call);
                                }
                            }
                            _ => {} // Fall through to other handling
                        }
                    } // close `else` of `if ambiguous && !allow_ambiguous_date`
                }

                // Check for WeakRef.deref() / FinalizationRegistry.register() / .unregister()
                // dispatch BEFORE the generic array method dispatch — these receivers were
                // tracked in the pre-scan pass.
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    if let ast::Expr::Ident(recv_ident) = member.obj.as_ref() {
                        let recv_name = recv_ident.sym.to_string();
                        if ctx.weakref_locals.contains(&recv_name) && method_name == "deref" {
                            return Ok(Expr::WeakRefDeref(Box::new(Expr::LocalGet(
                                ctx.lookup_local(&recv_name).unwrap_or(0),
                            ))));
                        }
                        if ctx.finreg_locals.contains(&recv_name) {
                            let registry_id = ctx.lookup_local(&recv_name).unwrap_or(0);
                            match method_name {
                                "register" => {
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let target = iter.next().unwrap();
                                        let held = iter.next().unwrap();
                                        let token = iter.next().map(Box::new);
                                        return Ok(Expr::FinalizationRegistryRegister {
                                            registry: Box::new(Expr::LocalGet(registry_id)),
                                            target: Box::new(target),
                                            held: Box::new(held),
                                            token,
                                        });
                                    }
                                }
                                "unregister" => {
                                    if !args.is_empty() {
                                        let token = args.into_iter().next().unwrap();
                                        return Ok(Expr::FinalizationRegistryUnregister {
                                            registry: Box::new(Expr::LocalGet(registry_id)),
                                            token: Box::new(token),
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                        // WeakMap/WeakSet — route to dedicated runtime functions
                        // (NOT the regular Map/Set HIR variants) so reference-equality
                        // works for object keys. Primitive keys/values throw via
                        // js_weak_throw_primitive when the AST shows a bare literal.
                        let make_extern_call = |name: &str, args: Vec<Expr>| -> Expr {
                            Expr::Call {
                                callee: Box::new(Expr::ExternFuncRef {
                                    name: name.to_string(),
                                    param_types: Vec::new(),
                                    return_type: Type::Any,
                                }),
                                args,
                                type_args: Vec::new(),
                            }
                        };
                        let throw_primitive_expr = || -> Expr {
                            Expr::Call {
                                callee: Box::new(Expr::ExternFuncRef {
                                    name: "js_weak_throw_primitive".to_string(),
                                    param_types: Vec::new(),
                                    return_type: Type::Any,
                                }),
                                args: Vec::new(),
                                type_args: Vec::new(),
                            }
                        };
                        if ctx.weakmap_locals.contains(&recv_name) {
                            let map_id = ctx.lookup_local(&recv_name).unwrap_or(0);
                            let recv = Expr::LocalGet(map_id);
                            match method_name {
                                "set" if args.len() >= 2 => {
                                    let key_is_primitive_lit = matches!(
                                        call.args.first().map(|a| a.expr.as_ref()),
                                        Some(ast::Expr::Lit(_))
                                    );
                                    if key_is_primitive_lit {
                                        return Ok(throw_primitive_expr());
                                    }
                                    let mut iter = args.into_iter();
                                    let key = iter.next().unwrap();
                                    let value = iter.next().unwrap();
                                    return Ok(make_extern_call(
                                        "js_weakmap_set",
                                        vec![recv, key, value],
                                    ));
                                }
                                "get" if !args.is_empty() => {
                                    return Ok(make_extern_call(
                                        "js_weakmap_get",
                                        vec![recv, args.into_iter().next().unwrap()],
                                    ));
                                }
                                "has" if !args.is_empty() => {
                                    return Ok(make_extern_call(
                                        "js_weakmap_has",
                                        vec![recv, args.into_iter().next().unwrap()],
                                    ));
                                }
                                "delete" if !args.is_empty() => {
                                    return Ok(make_extern_call(
                                        "js_weakmap_delete",
                                        vec![recv, args.into_iter().next().unwrap()],
                                    ));
                                }
                                _ => {}
                            }
                        }
                        if ctx.weakset_locals.contains(&recv_name) {
                            let set_id = ctx.lookup_local(&recv_name).unwrap_or(0);
                            let recv = Expr::LocalGet(set_id);
                            match method_name {
                                "add" if !args.is_empty() => {
                                    let value_is_primitive_lit = matches!(
                                        call.args.first().map(|a| a.expr.as_ref()),
                                        Some(ast::Expr::Lit(_))
                                    );
                                    if value_is_primitive_lit {
                                        return Ok(throw_primitive_expr());
                                    }
                                    return Ok(make_extern_call(
                                        "js_weakset_add",
                                        vec![recv, args.into_iter().next().unwrap()],
                                    ));
                                }
                                "has" if !args.is_empty() => {
                                    return Ok(make_extern_call(
                                        "js_weakset_has",
                                        vec![recv, args.into_iter().next().unwrap()],
                                    ));
                                }
                                "delete" if !args.is_empty() => {
                                    return Ok(make_extern_call(
                                        "js_weakset_delete",
                                        vec![recv, args.into_iter().next().unwrap()],
                                    ));
                                }
                                _ => {}
                            }
                        }
                    }
                }

                // Check for array method calls (arr.push, arr.pop, etc.)
                // These are called on local variables, not global modules
                // IMPORTANT: Only apply to actual Array types, not String types
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    if let ast::Expr::Ident(arr_ident) = member.obj.as_ref() {
                        let arr_name = arr_ident.sym.to_string();
                        // Check that this is NOT a String type (Array, Set, Map are all OK)
                        // When type is unknown, only enter array block for array-only methods
                        // (push, pop, etc.), NOT for methods shared with strings (indexOf,
                        // includes, split) — those are handled by the general dispatch which
                        // checks is_string at codegen time.
                        let type_info = ctx.lookup_local_type(&arr_name);
                        // `Union<String, Void>` (e.g. `JSON.stringify` return type) is
                        // a possible-string — must NOT be treated as definitely not-a-
                        // string, otherwise `.indexOf`/`.includes` get routed through
                        // ArrayIndexOf/ArrayIncludes and return -1/false on a real
                        // string value.
                        let is_union_with_string = matches!(
                            type_info,
                            Some(Type::Union(variants)) if variants.iter().any(|v| matches!(v, Type::String))
                        );
                        let is_known_string = type_info
                            .map(|ty| matches!(ty, Type::String))
                            .unwrap_or(false)
                            || is_union_with_string;
                        // A user-defined class instance is NOT an array — must skip the array
                        // fast path so user-defined methods like Stack<T>.push() are dispatched
                        // to the class method, not runtime js_array_push. Map/Set/Promise are
                        // handled by explicit checks within the array block below.
                        let builtin_generic_bases = ["Map", "Set", "WeakMap", "WeakSet", "Promise"];
                        // Imported classes don't show up in `lookup_class`; treat any
                        // uppercase imported identifier as a candidate class so the
                        // array fast-path doesn't swallow `coll.find(filter)` etc.
                        let is_imported_class_name = |n: &str| -> bool {
                            if let Some(c) = n.chars().next() {
                                if c.is_uppercase() && ctx.lookup_imported_func(n).is_some() {
                                    return true;
                                }
                            }
                            false
                        };
                        let is_user_class_instance = match type_info {
                            Some(Type::Named(name)) => {
                                ctx.lookup_class(name).is_some() || is_imported_class_name(name)
                            }
                            Some(Type::Generic { base, .. }) => {
                                !builtin_generic_bases.contains(&base.as_str())
                                    && (ctx.lookup_class(base).is_some()
                                        || is_imported_class_name(base))
                            }
                            _ => false,
                        };
                        // When the receiver type is Any and the method name is one
                        // commonly defined on user classes too (e.g. mongo's
                        // `Collection.find(filter)`), skip the array fast-path so the
                        // dispatch falls through to class-method resolution. Without this
                        // guard, the lowering blindly emits `Expr::ArrayFind` and the
                        // call resolves to `js_array_find` at codegen time, returning 0.
                        let is_class_overlapping_method = matches!(
                            method_name,
                            "find"
                                | "findIndex"
                                | "findLast"
                                | "findLastIndex"
                                | "map"
                                | "filter"
                                | "some"
                                | "every"
                                | "forEach"
                                | "reduce"
                                | "reduceRight"
                                | "join"
                        );
                        let is_unknown_recv =
                            matches!(type_info, None | Some(Type::Any) | Some(Type::Unknown));
                        let is_known_not_string = type_info
                            .map(|ty| !matches!(ty, Type::String | Type::Any | Type::Unknown))
                            .unwrap_or(false)
                            && !is_union_with_string;
                        // Object type literals (e.g., { push: (v: number) => void; ... })
                        // are NOT arrays — they are plain objects with closure-valued
                        // properties and must NOT enter the array fast path.
                        let is_object_type = matches!(type_info, Some(Type::Object(_)));
                        // `Uint8Array`/`Buffer` instances must NOT enter the generic
                        // array fast path. They have a distinct runtime representation
                        // (raw `BufferHeader`, no f64 elements) and a different method
                        // family (`readUInt8`, `swap16`, byte-level `indexOf` matching
                        // string/buffer needles, etc.). The runtime's
                        // `dispatch_buffer_method` handles all of these via the
                        // universal `js_native_call_method` fallback path.
                        let is_buffer_type = matches!(
                            type_info,
                            Some(Type::Named(n))
                                if n == "Uint8Array" || n == "Buffer" || n == "Uint8ClampedArray"
                        );
                        let is_ambiguous_method =
                            matches!(method_name, "indexOf" | "includes" | "slice");
                        let is_not_string = if is_known_string {
                            false // definitely a string, skip array block
                        } else if is_user_class_instance {
                            false // user class — must dispatch to class method, skip array fast-path
                        } else if is_object_type {
                            false // object type literal — dispatch via method call, not array ops
                        } else if is_buffer_type {
                            false // Buffer/Uint8Array — runtime dispatch handles byte-level methods
                        } else if is_known_not_string {
                            true // definitely not a string, enter array block
                        } else if is_ambiguous_method {
                            false // type unknown + ambiguous method, skip array block (fall through to general dispatch)
                        } else if is_unknown_recv && is_class_overlapping_method {
                            false // type unknown + method commonly defined on user classes — fall through
                        } else {
                            true // type unknown + array-only method (push, pop, etc.), enter array block
                        };
                        // Helper: if the callback arg is a bare Boolean/Number/String identifier,
                        // desugar to a synthetic closure: x => Boolean(x) / Number(x) / String(x).
                        // This is needed because .filter(Boolean) etc. expect a closure pointer at
                        // runtime but built-in constructors aren't first-class closure objects.
                        if is_not_string {
                            if let Some(array_id) = ctx.lookup_local(&arr_name) {
                                match method_name {
                                    "push" => {
                                        if !args.is_empty() {
                                            // Check if any argument has spread operator —
                                            // when present, route through the spread path.
                                            // Multi-arg push without spread is desugared to a
                                            // Sequence of ArrayPush statements (one per arg);
                                            // JS spec returns the final array length, which is
                                            // exactly what the last ArrayPush returns.
                                            let any_spread =
                                                call.args.iter().any(|a| a.spread.is_some());
                                            if any_spread {
                                                if args.len() == 1 {
                                                    return Ok(Expr::ArrayPushSpread {
                                                        array_id,
                                                        source: Box::new(
                                                            args.into_iter().next().unwrap(),
                                                        ),
                                                    });
                                                }
                                                // Mixed regular + spread: bail to generic
                                                // dispatch (no current single-IR-shape).
                                            } else {
                                                if args.len() == 1 {
                                                    return Ok(Expr::ArrayPush {
                                                        array_id,
                                                        value: Box::new(
                                                            args.into_iter().next().unwrap(),
                                                        ),
                                                    });
                                                }
                                                let mut stmts: Vec<Expr> =
                                                    Vec::with_capacity(args.len());
                                                for a in args.into_iter() {
                                                    stmts.push(Expr::ArrayPush {
                                                        array_id,
                                                        value: Box::new(a),
                                                    });
                                                }
                                                return Ok(Expr::Sequence(stmts));
                                            }
                                        }
                                    }
                                    "pop" => {
                                        return Ok(Expr::ArrayPop(array_id));
                                    }
                                    "shift" => {
                                        return Ok(Expr::ArrayShift(array_id));
                                    }
                                    "unshift" => {
                                        if !args.is_empty() {
                                            return Ok(Expr::ArrayUnshift {
                                                array_id,
                                                value: Box::new(args.into_iter().next().unwrap()),
                                            });
                                        }
                                    }
                                    "indexOf" => {
                                        if !args.is_empty() {
                                            return Ok(Expr::ArrayIndexOf {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                value: Box::new(args.into_iter().next().unwrap()),
                                            });
                                        }
                                    }
                                    "includes" => {
                                        if !args.is_empty() {
                                            return Ok(Expr::ArrayIncludes {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                value: Box::new(args.into_iter().next().unwrap()),
                                            });
                                        }
                                    }
                                    "slice" => {
                                        // arr.slice(start, end?) - returns new array
                                        // Only convert to ArraySlice if we KNOW it's an Array type
                                        // (Type::Any could be a string, which has its own .slice() method)
                                        let is_definitely_array = ctx
                                            .lookup_local_type(&arr_name)
                                            .map(|ty| matches!(ty, Type::Array(_)))
                                            .unwrap_or(false);
                                        if is_definitely_array && !args.is_empty() {
                                            let mut args_iter = args.into_iter();
                                            let start = args_iter.next().unwrap();
                                            let end = args_iter.next();
                                            return Ok(Expr::ArraySlice {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                start: Box::new(start),
                                                end: end.map(Box::new),
                                            });
                                        }
                                        // Fall through to normal Call handling for strings or unknown types
                                    }
                                    "splice" => {
                                        // arr.splice(start, deleteCount?, ...items) - returns deleted elements
                                        if !args.is_empty() {
                                            let mut args_iter = args.into_iter();
                                            let start = args_iter.next().unwrap();
                                            let delete_count = args_iter.next();
                                            let items: Vec<Expr> = args_iter.collect();
                                            return Ok(Expr::ArraySplice {
                                                array_id,
                                                start: Box::new(start),
                                                delete_count: delete_count.map(Box::new),
                                                items,
                                            });
                                        }
                                    }
                                    "forEach" => {
                                        // Check if the receiver is a Map or Set - if so, don't use ArrayForEach.
                                        // Issue #542/#543: also reject `Map | undefined` / `Set | undefined`
                                        // so the same array/Map mismatch on for-of doesn't recur for
                                        // forEach calls on optional-Map parameters.
                                        let recv_ty = ctx.lookup_local_type(&arr_name);
                                        let is_map_or_set_variant = |ty: &Type| -> bool {
                                            matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set")
                                        };
                                        let is_map_or_set = match recv_ty {
                                            Some(ty) if is_map_or_set_variant(ty) => true,
                                            Some(Type::Union(variants)) => {
                                                variants.iter().any(is_map_or_set_variant)
                                            }
                                            _ => false,
                                        };
                                        if !is_map_or_set && !args.is_empty() {
                                            let cb = args.into_iter().next().unwrap();
                                            let cb =
                                                ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                            return Ok(Expr::ArrayForEach {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                callback: Box::new(cb),
                                            });
                                        }
                                    }
                                    "map" | "filter" | "find" | "findIndex" | "findLast"
                                    | "findLastIndex" | "some" | "every" | "at" => {
                                        // Skip the array-method fast path when the receiver
                                        // is a known class instance (e.g. mongo `Collection.find`).
                                        // Without this guard, `coll.find(filter)` lowers to
                                        // `Expr::ArrayFind` and dispatches to `js_array_find`,
                                        // which silently returns 0 on a class receiver.
                                        let recv_ty = ctx.lookup_local_type(&arr_name);
                                        // TypedArray types are Named but must NOT be treated as
                                        // class instances — they need the array-method fast path
                                        // so `.at()` / `.findLast()` emit the right HIR variants.
                                        let is_typed_array = recv_ty
                                            .as_ref()
                                            .map(|ty| {
                                                matches!(ty, Type::Named(n) if matches!(
                                                    n.as_str(),
                                                    "Int8Array" | "Int16Array" | "Int32Array"
                                                    | "Uint8Array" | "Uint8ClampedArray"
                                                    | "Uint16Array" | "Uint32Array"
                                                    | "Float32Array" | "Float64Array"
                                                    | "BigInt64Array" | "BigUint64Array"
                                                ))
                                            })
                                            .unwrap_or(false);
                                        let is_class_instance = !is_typed_array
                                            && recv_ty
                                                .as_ref()
                                                .map(|ty| {
                                                    matches!(
                                                        ty,
                                                        Type::Named(_) | Type::Generic { .. }
                                                    ) && !matches!(ty, Type::Array(_))
                                                })
                                                .unwrap_or(false);
                                        // Issue #514: gate `.at()` ArrayAt
                                        // emission on a statically-known
                                        // array type. `at` is shared between
                                        // String.prototype and Array.prototype,
                                        // so for `(s: any).at(-1)` codegen
                                        // can't tell which one the user means.
                                        // Pre-fix the HIR optimistically
                                        // lowered to `Expr::ArrayAt` →
                                        // `js_array_at`, which interprets
                                        // the NaN-boxed *StringHeader as an
                                        // *ArrayHeader and returns garbage.
                                        // Now: only emit ArrayAt for proven
                                        // arrays / typed-arrays; otherwise
                                        // fall through to the generic method
                                        // dispatch which lands in the runtime
                                        // tower's tag-aware string arm.
                                        let recv_is_array = is_typed_array
                                            || matches!(recv_ty, Some(Type::Array(_)));
                                        if !is_class_instance {
                                            if method_name == "at" && recv_is_array {
                                                if !args.is_empty() {
                                                    return Ok(Expr::ArrayAt {
                                                        array: Box::new(Expr::LocalGet(array_id)),
                                                        index: Box::new(
                                                            args.into_iter().next().unwrap(),
                                                        ),
                                                    });
                                                }
                                            } else if method_name != "at" && !args.is_empty() {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx
                                                    .maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                let array = Box::new(Expr::LocalGet(array_id));
                                                let callback = Box::new(cb);
                                                return Ok(match method_name {
                                                    "map" => Expr::ArrayMap { array, callback },
                                                    "filter" => {
                                                        Expr::ArrayFilter { array, callback }
                                                    }
                                                    "find" => Expr::ArrayFind { array, callback },
                                                    "findIndex" => {
                                                        Expr::ArrayFindIndex { array, callback }
                                                    }
                                                    "findLast" => {
                                                        Expr::ArrayFindLast { array, callback }
                                                    }
                                                    "findLastIndex" => {
                                                        Expr::ArrayFindLastIndex { array, callback }
                                                    }
                                                    "some" => Expr::ArraySome { array, callback },
                                                    "every" => Expr::ArrayEvery { array, callback },
                                                    _ => unreachable!(),
                                                });
                                            }
                                        }
                                    }
                                    "flatMap" => {
                                        if !args.is_empty() {
                                            let cb = args.into_iter().next().unwrap();
                                            let cb =
                                                ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                            return Ok(Expr::ArrayFlatMap {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                callback: Box::new(cb),
                                            });
                                        }
                                    }
                                    "sort" => {
                                        if !args.is_empty() {
                                            return Ok(Expr::ArraySort {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                comparator: Box::new(
                                                    args.into_iter().next().unwrap(),
                                                ),
                                            });
                                        }
                                    }
                                    "reduce" => {
                                        if !args.is_empty() {
                                            let mut args_iter = args.into_iter();
                                            let callback = args_iter.next().unwrap();
                                            let initial = args_iter.next().map(Box::new);
                                            return Ok(Expr::ArrayReduce {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                callback: Box::new(callback),
                                                initial,
                                            });
                                        }
                                    }
                                    "join" => {
                                        // arr.join(separator?) -> string
                                        let separator = args.into_iter().next().map(Box::new);
                                        return Ok(Expr::ArrayJoin {
                                            array: Box::new(Expr::LocalGet(array_id)),
                                            separator,
                                        });
                                    }
                                    "flat" => {
                                        // arr.flat() folds to depth=1 fast path;
                                        // arr.flat(depth) falls through so the
                                        // depth arg can reach the codegen
                                        // `lower_array_method.rs::flat` arm and
                                        // route to `js_array_flat_depth`.
                                        if args.is_empty() {
                                            return Ok(Expr::ArrayFlat {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                            });
                                        }
                                    }
                                    "reduceRight" => {
                                        if !args.is_empty() {
                                            let mut args_iter = args.into_iter();
                                            let callback = args_iter.next().unwrap();
                                            let initial = args_iter.next().map(Box::new);
                                            return Ok(Expr::ArrayReduceRight {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                callback: Box::new(callback),
                                                initial,
                                            });
                                        }
                                    }
                                    "toReversed" => {
                                        return Ok(Expr::ArrayToReversed {
                                            array: Box::new(Expr::LocalGet(array_id)),
                                        });
                                    }
                                    "toSorted" => {
                                        let comparator = args.into_iter().next().map(Box::new);
                                        return Ok(Expr::ArrayToSorted {
                                            array: Box::new(Expr::LocalGet(array_id)),
                                            comparator,
                                        });
                                    }
                                    "toSpliced" => {
                                        if args.len() >= 2 {
                                            let mut args_iter = args.into_iter();
                                            let start = args_iter.next().unwrap();
                                            let delete_count = args_iter.next().unwrap();
                                            let items: Vec<Expr> = args_iter.collect();
                                            return Ok(Expr::ArrayToSpliced {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                start: Box::new(start),
                                                delete_count: Box::new(delete_count),
                                                items,
                                            });
                                        }
                                    }
                                    "with" => {
                                        // Issue #515: only fold `arr.with(idx, val)` to
                                        // `Expr::ArrayWith` when the receiver is statically
                                        // typed as an array or typed-array. `with` is
                                        // heavily overloaded by user-defined builder
                                        // methods (`class Builder { with(a, b): this {
                                        // … } }`, `const obj = { with(a, b) { … } }`);
                                        // folding optimistically on unknown-type receivers
                                        // (the default for unannotated locals) silently
                                        // rewrote the call to a typed-array index-replace
                                        // and broke user code. Untyped-but-actually-array
                                        // callers fall through to the codegen
                                        // `lower_array_method` `with` arm (when
                                        // `is_array_expr` recognizes the receiver) or to
                                        // the runtime `js_native_call_method` arm.
                                        let recv_ty = ctx.lookup_local_type(&arr_name);
                                        let is_typed_array = recv_ty
                                            .as_ref()
                                            .map(|ty| {
                                                matches!(ty, Type::Named(n) if matches!(
                                                    n.as_str(),
                                                    "Int8Array" | "Int16Array" | "Int32Array"
                                                    | "Uint8Array" | "Uint8ClampedArray"
                                                    | "Uint16Array" | "Uint32Array"
                                                    | "Float32Array" | "Float64Array"
                                                    | "BigInt64Array" | "BigUint64Array"
                                                ))
                                            })
                                            .unwrap_or(false);
                                        let is_known_array = is_typed_array
                                            || recv_ty
                                                .map(|ty| {
                                                    matches!(ty, Type::Array(_) | Type::Tuple(_))
                                                })
                                                .unwrap_or(false);
                                        if is_known_array && args.len() >= 2 {
                                            let mut args_iter = args.into_iter();
                                            let index = args_iter.next().unwrap();
                                            let value = args_iter.next().unwrap();
                                            return Ok(Expr::ArrayWith {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                index: Box::new(index),
                                                value: Box::new(value),
                                            });
                                        }
                                        // Fall through to general method dispatch
                                    }
                                    "copyWithin" => {
                                        if args.len() >= 2 {
                                            let mut args_iter = args.into_iter();
                                            let target = args_iter.next().unwrap();
                                            let start = args_iter.next().unwrap();
                                            let end = args_iter.next().map(Box::new);
                                            return Ok(Expr::ArrayCopyWithin {
                                                array_id,
                                                target: Box::new(target),
                                                start: Box::new(start),
                                                end,
                                            });
                                        }
                                    }
                                    "entries" | "keys" | "values" => {
                                        // Issue #542/#543: `keys()`/`values()`/`entries()` are
                                        // shared between Array, Map, and Set. When the receiver's
                                        // static type is Any (e.g. an interface method return
                                        // whose signature wasn't tracked, or a `JSON.parse`
                                        // result), the optimistic fall-through to `ArrayKeys`/
                                        // `ArrayValues`/`ArrayEntries` runs `js_array_*` against
                                        // a real `MapHeader`. The map's `size` field aliases
                                        // `ArrayHeader.length`, so an N-entry Map produces
                                        // `[0..N-1]` for `keys()` and reads garbage from the
                                        // entries pointer for `values()`/`entries()`. Only fold
                                        // to the Array variant when the receiver is statically
                                        // known to be Array/Tuple; otherwise leave as a generic
                                        // method call so codegen routes through
                                        // `js_native_call_method`, which does the runtime
                                        // is_registered_map / is_registered_set check.
                                        let recv_ty = ctx.lookup_local_type(&arr_name);
                                        // Issue #542/#543 follow-up: accept `Type::Union` variants
                                        // containing the target (e.g. `Map<K,V> | undefined` after
                                        // an `if (!m) return;` narrow). The for-of path in lower.rs
                                        // already handles Union; the method-call path here did not,
                                        // so `m.keys()` on an optional-Map fell through to the
                                        // ArrayKeys fold. Mirrors lower.rs:6196.
                                        let ty_is_map = |t: &Type| matches!(t, Type::Generic { base, .. } if base == "Map" || base == "WeakMap");
                                        let ty_is_set = |t: &Type| matches!(t, Type::Generic { base, .. } if base == "Set" || base == "WeakSet");
                                        let ty_is_array =
                                            |t: &Type| matches!(t, Type::Array(_) | Type::Tuple(_));
                                        let is_map = match &recv_ty {
                                            Some(t) if ty_is_map(t) => true,
                                            Some(Type::Union(variants)) => {
                                                variants.iter().any(ty_is_map)
                                            }
                                            _ => false,
                                        };
                                        let is_set = match &recv_ty {
                                            Some(t) if ty_is_set(t) => true,
                                            Some(Type::Union(variants)) => {
                                                variants.iter().any(ty_is_set)
                                            }
                                            _ => false,
                                        };
                                        let is_known_array = match &recv_ty {
                                            Some(t) if ty_is_array(t) => true,
                                            Some(Type::Union(variants)) => {
                                                variants.iter().any(ty_is_array)
                                            }
                                            _ => false,
                                        };
                                        match method_name {
                                            "entries" => {
                                                if is_map {
                                                    return Ok(Expr::MapEntries(Box::new(
                                                        Expr::LocalGet(array_id),
                                                    )));
                                                }
                                                if is_known_array {
                                                    return Ok(Expr::ArrayEntries(Box::new(
                                                        Expr::LocalGet(array_id),
                                                    )));
                                                }
                                            }
                                            "keys" => {
                                                if is_map {
                                                    return Ok(Expr::MapKeys(Box::new(
                                                        Expr::LocalGet(array_id),
                                                    )));
                                                }
                                                if is_known_array {
                                                    return Ok(Expr::ArrayKeys(Box::new(
                                                        Expr::LocalGet(array_id),
                                                    )));
                                                }
                                            }
                                            "values" => {
                                                if is_map {
                                                    return Ok(Expr::MapValues(Box::new(
                                                        Expr::LocalGet(array_id),
                                                    )));
                                                }
                                                if is_set {
                                                    return Ok(Expr::SetValues(Box::new(
                                                        Expr::LocalGet(array_id),
                                                    )));
                                                }
                                                if is_known_array {
                                                    return Ok(Expr::ArrayValues(Box::new(
                                                        Expr::LocalGet(array_id),
                                                    )));
                                                }
                                            }
                                            _ => unreachable!(),
                                        }
                                        // Fall through: receiver type unknown — let general
                                        // dispatch (js_native_call_method) inspect at runtime.
                                    }
                                    // Map methods (only apply to actual Map/Set types)
                                    "set" => {
                                        // Check if this is a Map or Set type before treating as Map.set()
                                        let is_map_or_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set"))
                                                .unwrap_or(false);
                                        if is_map_or_set && args.len() >= 2 {
                                            // map.set(key, value) - returns the map for chaining
                                            let mut args_iter = args.into_iter();
                                            let key = args_iter.next().unwrap();
                                            let value = args_iter.next().unwrap();
                                            return Ok(Expr::MapSet {
                                                map: Box::new(Expr::LocalGet(array_id)),
                                                key: Box::new(key),
                                                value: Box::new(value),
                                            });
                                        }
                                    }
                                    "get" => {
                                        // Check if this is a Map type before treating as Map.get()
                                        let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                        if is_map && !args.is_empty() {
                                            // map.get(key) - returns value or undefined
                                            return Ok(Expr::MapGet {
                                                map: Box::new(Expr::LocalGet(array_id)),
                                                key: Box::new(args.into_iter().next().unwrap()),
                                            });
                                        }
                                    }
                                    "has" => {
                                        // Check if this is a Set or Map - only apply to actual Set/Map types
                                        let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                        let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                        if (is_set || is_map) && !args.is_empty() {
                                            let value = args.into_iter().next().unwrap();
                                            if is_set {
                                                return Ok(Expr::SetHas {
                                                    set: Box::new(Expr::LocalGet(array_id)),
                                                    value: Box::new(value),
                                                });
                                            } else {
                                                return Ok(Expr::MapHas {
                                                    map: Box::new(Expr::LocalGet(array_id)),
                                                    key: Box::new(value),
                                                });
                                            }
                                        }
                                    }
                                    "delete" => {
                                        // Check if this is a Set or Map - only apply to actual Set/Map types
                                        let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                        let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                        if (is_set || is_map) && !args.is_empty() {
                                            let value = args.into_iter().next().unwrap();
                                            if is_set {
                                                return Ok(Expr::SetDelete {
                                                    set: Box::new(Expr::LocalGet(array_id)),
                                                    value: Box::new(value),
                                                });
                                            } else {
                                                return Ok(Expr::MapDelete {
                                                    map: Box::new(Expr::LocalGet(array_id)),
                                                    key: Box::new(value),
                                                });
                                            }
                                        }
                                    }
                                    "clear" => {
                                        // Check if this is a Set or Map - only apply to actual Set/Map types
                                        let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                        let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                        if is_set {
                                            return Ok(Expr::SetClear(Box::new(Expr::LocalGet(
                                                array_id,
                                            ))));
                                        } else if is_map {
                                            return Ok(Expr::MapClear(Box::new(Expr::LocalGet(
                                                array_id,
                                            ))));
                                        }
                                        // Fall through if neither Set nor Map
                                    }
                                    // Map iterator methods: entries(), keys(), values()
                                    "entries" => {
                                        let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                        if is_map && args.is_empty() {
                                            return Ok(Expr::MapEntries(Box::new(Expr::LocalGet(
                                                array_id,
                                            ))));
                                        }
                                    }
                                    "keys" => {
                                        let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                        if is_map && args.is_empty() {
                                            return Ok(Expr::MapKeys(Box::new(Expr::LocalGet(
                                                array_id,
                                            ))));
                                        }
                                    }
                                    "values" => {
                                        let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                        if is_map && args.is_empty() {
                                            return Ok(Expr::MapValues(Box::new(Expr::LocalGet(
                                                array_id,
                                            ))));
                                        }
                                        let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                        if is_set && args.is_empty() {
                                            return Ok(Expr::SetValues(Box::new(Expr::LocalGet(
                                                array_id,
                                            ))));
                                        }
                                    }
                                    // Set methods
                                    "add" => {
                                        // Check if this is a Set type before treating as Set.add()
                                        let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                        if is_set && !args.is_empty() {
                                            // set.add(value) - returns the set for chaining
                                            let value = args.into_iter().next().unwrap();
                                            return Ok(Expr::SetAdd {
                                                set_id: array_id,
                                                value: Box::new(value),
                                            });
                                        }
                                    }
                                    _ => {} // Fall through to generic handling
                                }

                                // URLSearchParams methods
                                let is_url_search_params = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "URLSearchParams"))
                                        .unwrap_or(false);
                                if is_url_search_params {
                                    match build_url_search_params_method_call(
                                        Expr::LocalGet(array_id),
                                        method_name,
                                        args,
                                    ) {
                                        Ok(expr) => return Ok(expr),
                                        Err(returned_args) => args = returned_args,
                                    }
                                }

                                // TextEncoder methods
                                let is_text_encoder = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextEncoder"))
                                        .unwrap_or(false);
                                if is_text_encoder {
                                    if method_name == "encode" {
                                        if !args.is_empty() {
                                            return Ok(Expr::TextEncoderEncode(Box::new(
                                                args.into_iter().next().unwrap(),
                                            )));
                                        } else {
                                            // encode() with no args encodes empty string
                                            return Ok(Expr::TextEncoderEncode(Box::new(
                                                Expr::String(String::new()),
                                            )));
                                        }
                                    }
                                }

                                // TextDecoder methods
                                let is_text_decoder = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                                        .unwrap_or(false);
                                if is_text_decoder {
                                    if method_name == "decode" {
                                        if !args.is_empty() {
                                            return Ok(Expr::TextDecoderDecode(Box::new(
                                                args.into_iter().next().unwrap(),
                                            )));
                                        } else {
                                            // decode() with no args returns empty string
                                            return Ok(Expr::String(String::new()));
                                        }
                                    }
                                }
                            }
                        } // close is_array_type check
                    }

                    // Check for array methods on property access (e.g., this.items.push(value))
                    // This handles cases where the array is a property of an object, not a local variable
                    if let ast::Expr::Member(obj_member) = member.obj.as_ref() {
                        if let ast::MemberProp::Ident(obj_prop_ident) = &obj_member.prop {
                            let _property_name = obj_prop_ident.sym.to_string();
                            // Lower the object expression (e.g., 'this' or a local variable)
                            let _object_expr = lower_expr(ctx, &obj_member.obj)?;

                            if method_name == "push" {
                                if !args.is_empty() {
                                    // For now, fall through to generic Call handling
                                    // We'll compile this in codegen using inline property access
                                    // property-based push: object.{property}.push()
                                }
                            }
                        }
                    }
                }
            }

            // Check for array methods on imported variables (e.g., import { CHAIN_NAMES } from './module')
            // These don't have local IDs but are ExternFuncRef values
            if let ast::Callee::Expr(expr) = &call.callee {
                if let ast::Expr::Member(member) = expr.as_ref() {
                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                        let method_name = method_ident.sym.as_ref();
                        if let ast::Expr::Ident(arr_ident) = member.obj.as_ref() {
                            let arr_name = arr_ident.sym.to_string();
                            // Check if this is an imported variable (not a local)
                            if ctx.lookup_local(&arr_name).is_none() {
                                if let Some(orig_name) = ctx.lookup_imported_func(&arr_name) {
                                    // This is an imported variable - create ExternFuncRef for it
                                    let (param_types, return_type) = ctx
                                        .lookup_extern_func_types(orig_name)
                                        .map(|(p, r)| (p.clone(), r.clone()))
                                        .unwrap_or_else(|| (Vec::new(), Type::Any));
                                    let extern_ref = Expr::ExternFuncRef {
                                        name: orig_name.to_string(),
                                        param_types,
                                        return_type,
                                    };
                                    match method_name {
                                        "join" => {
                                            // Issue #420 (drizzle): `sql.join(arr)` — `sql`
                                            // is imported from drizzle-orm as a tag function
                                            // with a custom `.join` static method. Pre-fix
                                            // this path unconditionally folded to
                                            // ArrayJoin, so `sql.join(valuesSqlList)` was
                                            // dispatched as `js_array_join(sql, list)`
                                            // (treating `sql` as the array, list as the
                                            // separator). Result: empty string back.
                                            //
                                            // Only fold when the imported variable's
                                            // return_type is statically Array. Otherwise
                                            // fall through to generic dispatch which
                                            // respects the imported's own `.join` method.
                                            if matches!(extern_ref, Expr::ExternFuncRef { ref return_type, .. } if matches!(return_type, Type::Array(_)))
                                            {
                                                let separator =
                                                    args.into_iter().next().map(Box::new);
                                                return Ok(Expr::ArrayJoin {
                                                    array: Box::new(extern_ref),
                                                    separator,
                                                });
                                            }
                                            // Fall through.
                                        }
                                        "map" => {
                                            if !args.is_empty() {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx
                                                    .maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayMap {
                                                    array: Box::new(extern_ref),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "filter" => {
                                            if !args.is_empty() {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx
                                                    .maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayFilter {
                                                    array: Box::new(extern_ref),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "forEach" => {
                                            if !args.is_empty() {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx
                                                    .maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayForEach {
                                                    array: Box::new(extern_ref),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "find" => {
                                            if !args.is_empty() {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx
                                                    .maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayFind {
                                                    array: Box::new(extern_ref),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "sort" => {
                                            if !args.is_empty() {
                                                return Ok(Expr::ArraySort {
                                                    array: Box::new(extern_ref),
                                                    comparator: Box::new(
                                                        args.into_iter().next().unwrap(),
                                                    ),
                                                });
                                            }
                                        }
                                        "indexOf" => {
                                            if !args.is_empty() {
                                                return Ok(Expr::ArrayIndexOf {
                                                    array: Box::new(extern_ref),
                                                    value: Box::new(
                                                        args.into_iter().next().unwrap(),
                                                    ),
                                                });
                                            }
                                        }
                                        "includes" => {
                                            if !args.is_empty() {
                                                return Ok(Expr::ArrayIncludes {
                                                    array: Box::new(extern_ref),
                                                    value: Box::new(
                                                        args.into_iter().next().unwrap(),
                                                    ),
                                                });
                                            }
                                        }
                                        "slice" => {
                                            if !args.is_empty() {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let end = args_iter.next();
                                                return Ok(Expr::ArraySlice {
                                                    array: Box::new(extern_ref),
                                                    start: Box::new(start),
                                                    end: end.map(Box::new),
                                                });
                                            }
                                        }
                                        "reduce" => {
                                            if !args.is_empty() {
                                                let mut args_iter = args.into_iter();
                                                let callback = args_iter.next().unwrap();
                                                let initial = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayReduce {
                                                    array: Box::new(extern_ref),
                                                    callback: Box::new(callback),
                                                    initial,
                                                });
                                            }
                                        }
                                        "flat" => {
                                            // depth-aware calls fall through.
                                            if args.is_empty() {
                                                return Ok(Expr::ArrayFlat {
                                                    array: Box::new(extern_ref),
                                                });
                                            }
                                        }
                                        "reduceRight" => {
                                            if !args.is_empty() {
                                                let mut args_iter = args.into_iter();
                                                let callback = args_iter.next().unwrap();
                                                let initial = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayReduceRight {
                                                    array: Box::new(extern_ref),
                                                    callback: Box::new(callback),
                                                    initial,
                                                });
                                            }
                                        }
                                        "toReversed" => {
                                            return Ok(Expr::ArrayToReversed {
                                                array: Box::new(extern_ref),
                                            });
                                        }
                                        "toSorted" => {
                                            let comparator = args.into_iter().next().map(Box::new);
                                            return Ok(Expr::ArrayToSorted {
                                                array: Box::new(extern_ref),
                                                comparator,
                                            });
                                        }
                                        "toSpliced" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let delete_count = args_iter.next().unwrap();
                                                let items: Vec<Expr> = args_iter.collect();
                                                return Ok(Expr::ArrayToSpliced {
                                                    array: Box::new(extern_ref),
                                                    start: Box::new(start),
                                                    delete_count: Box::new(delete_count),
                                                    items,
                                                });
                                            }
                                        }
                                        "with" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let index = args_iter.next().unwrap();
                                                let value = args_iter.next().unwrap();
                                                return Ok(Expr::ArrayWith {
                                                    array: Box::new(extern_ref),
                                                    index: Box::new(index),
                                                    value: Box::new(value),
                                                });
                                            }
                                        }
                                        "entries" => {
                                            return Ok(Expr::ArrayEntries(Box::new(extern_ref)));
                                        }
                                        "keys" => {
                                            return Ok(Expr::ArrayKeys(Box::new(extern_ref)));
                                        }
                                        "values" => {
                                            return Ok(Expr::ArrayValues(Box::new(extern_ref)));
                                        }
                                        _ => {} // Fall through for other methods
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check for array methods on inline array literals (e.g., ['a', 'b'].join('-'))
            if let ast::Callee::Expr(expr) = &call.callee {
                if let ast::Expr::Member(member) = expr.as_ref() {
                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                        let method_name = method_ident.sym.as_ref();
                        if let ast::Expr::Array(_arr_lit) = member.obj.as_ref() {
                            // Lower the array literal
                            let array_expr = lower_expr(ctx, &member.obj)?;
                            match method_name {
                                "join" => {
                                    // ['a', 'b'].join(separator?) -> string
                                    let separator = args.into_iter().next().map(Box::new);
                                    return Ok(Expr::ArrayJoin {
                                        array: Box::new(array_expr),
                                        separator,
                                    });
                                }
                                "map" => {
                                    if !args.is_empty() {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        return Ok(Expr::ArrayMap {
                                            array: Box::new(array_expr),
                                            callback: Box::new(cb),
                                        });
                                    }
                                }
                                "filter" => {
                                    if !args.is_empty() {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        return Ok(Expr::ArrayFilter {
                                            array: Box::new(array_expr),
                                            callback: Box::new(cb),
                                        });
                                    }
                                }
                                "forEach" => {
                                    if !args.is_empty() {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        return Ok(Expr::ArrayForEach {
                                            array: Box::new(array_expr),
                                            callback: Box::new(cb),
                                        });
                                    }
                                }
                                "find" => {
                                    if !args.is_empty() {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        return Ok(Expr::ArrayFind {
                                            array: Box::new(array_expr),
                                            callback: Box::new(cb),
                                        });
                                    }
                                }
                                "sort" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::ArraySort {
                                            array: Box::new(array_expr),
                                            comparator: Box::new(args.into_iter().next().unwrap()),
                                        });
                                    }
                                }
                                "indexOf" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::ArrayIndexOf {
                                            array: Box::new(array_expr),
                                            value: Box::new(args.into_iter().next().unwrap()),
                                        });
                                    }
                                }
                                "includes" => {
                                    if !args.is_empty() {
                                        return Ok(Expr::ArrayIncludes {
                                            array: Box::new(array_expr),
                                            value: Box::new(args.into_iter().next().unwrap()),
                                        });
                                    }
                                }
                                "slice" => {
                                    if !args.is_empty() {
                                        let mut args_iter = args.into_iter();
                                        let start = args_iter.next().unwrap();
                                        let end = args_iter.next();
                                        return Ok(Expr::ArraySlice {
                                            array: Box::new(array_expr),
                                            start: Box::new(start),
                                            end: end.map(Box::new),
                                        });
                                    }
                                }
                                "reduce" => {
                                    if !args.is_empty() {
                                        let mut args_iter = args.into_iter();
                                        let callback = args_iter.next().unwrap();
                                        let initial = args_iter.next().map(Box::new);
                                        return Ok(Expr::ArrayReduce {
                                            array: Box::new(array_expr),
                                            callback: Box::new(callback),
                                            initial,
                                        });
                                    }
                                }
                                "flat" => {
                                    // depth-aware calls fall through.
                                    if args.is_empty() {
                                        return Ok(Expr::ArrayFlat {
                                            array: Box::new(array_expr),
                                        });
                                    }
                                }
                                "reduceRight" => {
                                    if !args.is_empty() {
                                        let mut args_iter = args.into_iter();
                                        let callback = args_iter.next().unwrap();
                                        let initial = args_iter.next().map(Box::new);
                                        return Ok(Expr::ArrayReduceRight {
                                            array: Box::new(array_expr),
                                            callback: Box::new(callback),
                                            initial,
                                        });
                                    }
                                }
                                "toReversed" => {
                                    return Ok(Expr::ArrayToReversed {
                                        array: Box::new(array_expr),
                                    });
                                }
                                "toSorted" => {
                                    let comparator = args.into_iter().next().map(Box::new);
                                    return Ok(Expr::ArrayToSorted {
                                        array: Box::new(array_expr),
                                        comparator,
                                    });
                                }
                                "toSpliced" => {
                                    if args.len() >= 2 {
                                        let mut args_iter = args.into_iter();
                                        let start = args_iter.next().unwrap();
                                        let delete_count = args_iter.next().unwrap();
                                        let items: Vec<Expr> = args_iter.collect();
                                        return Ok(Expr::ArrayToSpliced {
                                            array: Box::new(array_expr),
                                            start: Box::new(start),
                                            delete_count: Box::new(delete_count),
                                            items,
                                        });
                                    }
                                }
                                "with" => {
                                    if args.len() >= 2 {
                                        let mut args_iter = args.into_iter();
                                        let index = args_iter.next().unwrap();
                                        let value = args_iter.next().unwrap();
                                        return Ok(Expr::ArrayWith {
                                            array: Box::new(array_expr),
                                            index: Box::new(index),
                                            value: Box::new(value),
                                        });
                                    }
                                }
                                "entries" => {
                                    return Ok(Expr::ArrayEntries(Box::new(array_expr)));
                                }
                                "keys" => {
                                    return Ok(Expr::ArrayKeys(Box::new(array_expr)));
                                }
                                "values" => {
                                    return Ok(Expr::ArrayValues(Box::new(array_expr)));
                                }
                                _ => {} // Fall through for other methods
                            }
                        }
                    }
                }
            }

            // TextEncoder.encode() / TextDecoder.decode() on inline expressions
            // e.g., new TextEncoder().encode("hello"), new TextDecoder().decode(buf)
            if let ast::Callee::Expr(expr) = &call.callee {
                if let ast::Expr::Member(member) = expr.as_ref() {
                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                        let method_name = method_ident.sym.as_ref();
                        // Check if the receiver is new TextEncoder() or new TextDecoder()
                        if let ast::Expr::New(new_expr) = member.obj.as_ref() {
                            if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                                let class_name = class_ident.sym.as_ref();
                                if class_name == "TextEncoder" && method_name == "encode" {
                                    let str_arg = if !args.is_empty() {
                                        args.into_iter().next().unwrap()
                                    } else {
                                        Expr::String(String::new())
                                    };
                                    return Ok(Expr::TextEncoderEncode(Box::new(str_arg)));
                                }
                                if class_name == "TextDecoder" && method_name == "decode" {
                                    if !args.is_empty() {
                                        return Ok(Expr::TextDecoderDecode(Box::new(
                                            args.into_iter().next().unwrap(),
                                        )));
                                    } else {
                                        return Ok(Expr::String(String::new()));
                                    }
                                }
                            }
                        }
                        // Also check for local variable typed as TextEncoder/TextDecoder
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            let obj_name = obj_ident.sym.to_string();
                            let is_text_encoder = ctx
                                .lookup_local_type(&obj_name)
                                .map(|ty| matches!(ty, Type::Named(name) if name == "TextEncoder"))
                                .unwrap_or(false);
                            if is_text_encoder && method_name == "encode" {
                                let str_arg = if !args.is_empty() {
                                    args.into_iter().next().unwrap()
                                } else {
                                    Expr::String(String::new())
                                };
                                return Ok(Expr::TextEncoderEncode(Box::new(str_arg)));
                            }
                            let is_text_decoder = ctx
                                .lookup_local_type(&obj_name)
                                .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                                .unwrap_or(false);
                            if is_text_decoder && method_name == "decode" {
                                if !args.is_empty() {
                                    return Ok(Expr::TextDecoderDecode(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                } else {
                                    return Ok(Expr::String(String::new()));
                                }
                            }
                        }
                    }
                }
            }

            // Check for array-only methods on any expression (e.g., Object.entries(x).reduce(...))
            // ONLY match methods that are unique to arrays (not shared with strings)
            // "includes", "indexOf", "slice", "join" also exist on strings, so skip those
            if let ast::Callee::Expr(expr) = &call.callee {
                if let ast::Expr::Member(member) = expr.as_ref() {
                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                        let method_name = method_ident.sym.as_ref();
                        // Helper: skip array-method dispatch when the receiver is a
                        // known class instance (e.g. mongo `Collection.find`,
                        // `Stack<T>.map`). Without this guard the lowering blindly
                        // emits `Expr::Array<Method>` and the compiled binary calls
                        // `js_array_<method>` on a class handle.
                        let recv_is_class = match member.obj.as_ref() {
                            ast::Expr::Ident(ident) => {
                                let n = ident.sym.to_string();
                                let ty = ctx.lookup_local_type(&n);
                                let class_typed = ty
                                    .as_ref()
                                    .map(|t| {
                                        matches!(t, Type::Named(_) | Type::Generic { .. })
                                            && !matches!(t, Type::Array(_))
                                    })
                                    .unwrap_or(false);
                                let unknown_recv =
                                    matches!(ty, None | Some(Type::Any) | Some(Type::Unknown));
                                let is_overlapping = matches!(
                                    method_name,
                                    "find"
                                        | "findIndex"
                                        | "findLast"
                                        | "findLastIndex"
                                        | "map"
                                        | "filter"
                                        | "some"
                                        | "every"
                                        | "forEach"
                                        | "reduce"
                                        | "reduceRight"
                                        | "join"
                                );
                                class_typed || (unknown_recv && is_overlapping)
                            }
                            ast::Expr::New(_) => true,
                            // `this.<method>(...)` inside a class method.
                            // The receiver is the class instance; treat it
                            // like a class-typed local. Without this, calls
                            // with names that overlap an Array method
                            // (e.g. a user-defined `this.forEach(...)` in
                            // an ECS Archetype, `this.map(...)` on a Stack)
                            // got folded into Expr::ArrayForEach /
                            // Expr::ArrayMap and the compiled binary
                            // dispatched `js_array_*` on a class handle.
                            // For non-overlapping names there's nothing to
                            // hijack, so we don't need to check the
                            // current_class field list — the array-method
                            // arms simply don't fire.
                            ast::Expr::This(_) => true,
                            // `this.<field>.<method>(...)` — when the field
                            // is statically typed as a non-Array container
                            // (Map / Set / a user class), the array-method
                            // fold would silently rewrite the call to
                            // `Expr::Array<Method>` and codegen would
                            // dispatch `js_array_*` on the wrong header.
                            // Concretely: an ECS Archetype's
                            // `private componentData: Map<EntityId, any[]>`
                            // ends up with `this.componentData.forEach(cb)`
                            // hitting `js_array_forEach`, which reads
                            // "length" out of a Map header and returns the
                            // map's `size` field as a fake array length,
                            // then iterates with bogus index-based reads
                            // (which is why `forEach` reported keys
                            // `[0,1,2]` while the real Map keys were
                            // 1024-range EntityIds).
                            //
                            // Resolve the field's type via the class field
                            // registry (populated by `lower_class_decl`'s
                            // first pass) and treat anything that isn't
                            // declared `Array<T>` / `T[]` as a class
                            // receiver so the fold bails. Unknown / Any
                            // stays in the prior `false` arm — no info to
                            // act on, fall through to the existing fast
                            // path.
                            ast::Expr::Member(inner) => {
                                if let (ast::Expr::This(_), ast::MemberProp::Ident(p)) =
                                    (inner.obj.as_ref(), &inner.prop)
                                {
                                    if let Some(cls) = ctx.current_class.clone() {
                                        match ctx.lookup_class_field_type(&cls, p.sym.as_ref()) {
                                            Some(Type::Array(_)) | Some(Type::Tuple(_)) => false,
                                            Some(_) => true,
                                            None => false,
                                        }
                                    } else {
                                        false
                                    }
                                } else if let (
                                    ast::Expr::Ident(obj_ident),
                                    ast::MemberProp::Ident(prop_ident),
                                ) = (inner.obj.as_ref(), &inner.prop)
                                {
                                    // Closes #589 (runtime path): `r.headers.forEach(cb)`
                                    // where `r` is a Response/Request (or returned Headers
                                    // already). Without this guard, the array-method fold
                                    // catch-all rewrites the call to `Expr::ArrayForEach`
                                    // and codegen dispatches `js_array_forEach` against a
                                    // Headers handle — Headers iteration silently no-ops.
                                    // Bail to the chained-Web-Fetch dispatch in
                                    // `crates/perry-codegen/src/lower_call.rs:1313`, which
                                    // routes `.forEach` / `.get` / `.has` / `.keys` /
                                    // `.values` / `.entries` through the Headers FFI.
                                    let is_fetch_headers = prop_ident.sym.as_ref() == "headers"
                                        && matches!(
                                            ctx.lookup_native_instance(obj_ident.sym.as_ref()),
                                            Some(("fetch", _))
                                                | Some(("Request", _))
                                                | Some(("Headers", _))
                                        );
                                    if is_fetch_headers {
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            }
                            // Issue #528: chained `inner_call(...).<overlapping>(...)`.
                            // The previous catch-all `_ => false` let the array-method
                            // fold fire on ANY chained call, including ones whose inner
                            // call returns a user object — `this.col().find({})` got
                            // rewritten as `Expr::ArrayFind(this.col(), {})` and at
                            // runtime `js_array_find` read garbage out of the object's
                            // header (no fallback path: only `map`/`filter`/`forEach`/
                            // `slice`/`with` have arms in `js_native_call_method`'s
                            // array dispatch tower; `find`/`findIndex`/`reduce` do not).
                            //
                            // For overlapping methods (the same set the Ident arm
                            // gates on), bail unless the inner call's method name is
                            // one of the known array-producing builtins. The AST-level
                            // check is conservative: it doesn't catch every chained-
                            // array shape (e.g. `Array.from(x).find(p)` — `Array` is
                            // an Ident, not a method on a Member), but it DOES catch
                            // the common `arr.filter(p).find(q)` chain whose inner
                            // call IS a Member-on-something. Non-overlapping methods
                            // (slice/indexOf/includes/etc.) keep their existing
                            // positive inner-HIR-shape pattern at lines ~3950+.
                            ast::Expr::Call(inner_call) => {
                                let is_overlapping = matches!(
                                    method_name,
                                    "find"
                                        | "findIndex"
                                        | "findLast"
                                        | "findLastIndex"
                                        | "map"
                                        | "filter"
                                        | "some"
                                        | "every"
                                        | "forEach"
                                        | "reduce"
                                        | "reduceRight"
                                        | "join"
                                );
                                if !is_overlapping {
                                    false
                                } else {
                                    // Look up the inner call's method name. If it's
                                    // one of the known array-producing builtins, the
                                    // chained fold IS safe — keep the ident-receiver
                                    // optimistic behaviour for `arr.filter(p).find(q)`
                                    // shapes.
                                    let inner_method: Option<&str> = match &inner_call.callee {
                                        ast::Callee::Expr(e) => match e.as_ref() {
                                            ast::Expr::Member(m) => match &m.prop {
                                                ast::MemberProp::Ident(i) => Some(i.sym.as_ref()),
                                                _ => None,
                                            },
                                            _ => None,
                                        },
                                        _ => None,
                                    };
                                    let inner_returns_array = inner_method
                                        .map(|m| {
                                            matches!(
                                                m,
                                                "map"
                                                    | "filter"
                                                    | "slice"
                                                    | "concat"
                                                    | "flat"
                                                    | "flatMap"
                                                    | "splice"
                                                    | "sort"
                                                    | "reverse"
                                                    | "fill"
                                                    | "copyWithin"
                                                    | "toReversed"
                                                    | "toSorted"
                                                    | "toSpliced"
                                                    | "with"
                                            )
                                        })
                                        .unwrap_or(false);
                                    // recv_is_class = true means BAIL. Bail when the
                                    // inner call is NOT a known array-producing method.
                                    !inner_returns_array
                                }
                            }
                            _ => false,
                        };
                        match method_name {
                            "reduce" if !args.is_empty() && !recv_is_class => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                let mut args_iter = args.into_iter();
                                let callback = args_iter.next().unwrap();
                                let initial = args_iter.next().map(Box::new);
                                return Ok(Expr::ArrayReduce {
                                    array: Box::new(array_expr),
                                    callback: Box::new(callback),
                                    initial,
                                });
                            }
                            "map" if !args.is_empty() && !recv_is_class => {
                                let cb = args.into_iter().next().unwrap();
                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::ArrayMap {
                                    array: Box::new(array_expr),
                                    callback: Box::new(cb),
                                });
                            }
                            "filter" if !args.is_empty() && !recv_is_class => {
                                let cb = args.into_iter().next().unwrap();
                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::ArrayFilter {
                                    array: Box::new(array_expr),
                                    callback: Box::new(cb),
                                });
                            }
                            "forEach" if !args.is_empty() && !recv_is_class => {
                                // Check if the receiver is a Map or Set - if so, don't use ArrayForEach
                                let is_map_or_set = if let ast::Expr::Ident(ident) =
                                    member.obj.as_ref()
                                {
                                    ctx.lookup_local_type(ident.sym.as_ref())
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set"))
                                                .unwrap_or(false)
                                } else {
                                    false
                                };
                                if !is_map_or_set {
                                    let cb = args.into_iter().next().unwrap();
                                    let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                    let array_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::ArrayForEach {
                                        array: Box::new(array_expr),
                                        callback: Box::new(cb),
                                    });
                                }
                            }
                            "find" if !args.is_empty() && !recv_is_class => {
                                let cb = args.into_iter().next().unwrap();
                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::ArrayFind {
                                    array: Box::new(array_expr),
                                    callback: Box::new(cb),
                                });
                            }
                            "findIndex" if !args.is_empty() && !recv_is_class => {
                                let cb = args.into_iter().next().unwrap();
                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::ArrayFindIndex {
                                    array: Box::new(array_expr),
                                    callback: Box::new(cb),
                                });
                            }
                            "some" if !args.is_empty() && !recv_is_class => {
                                let cb = args.into_iter().next().unwrap();
                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::ArraySome {
                                    array: Box::new(array_expr),
                                    callback: Box::new(cb),
                                });
                            }
                            "every" if !args.is_empty() && !recv_is_class => {
                                let cb = args.into_iter().next().unwrap();
                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::ArrayEvery {
                                    array: Box::new(array_expr),
                                    callback: Box::new(cb),
                                });
                            }
                            // #597: arr.entries() / .keys() / .values() on
                            // any-typed receivers (`function f(arr: any) { for (const [i,v] of arr.entries()) ... }`).
                            // Pre-fix this fell through to a generic
                            // `js_native_call_method` dispatch that returned
                            // an iterator-shaped object whose `.length` was
                            // 0 / undefined, so the index-based for-of loop
                            // (the index lowering at lower_decl.rs:4445)
                            // saw `__arr_N.length === 0` and ran 0 times.
                            // The static-Array path already folds at
                            // line 3966 above; this catch-all extends the
                            // same fold to dynamic-receiver shapes —
                            // `js_array_entries` / `_keys` / `_values`
                            // tolerates non-array receivers (returns empty)
                            // so the lowered loop's behavior on non-array
                            // values matches Node's empty-iterator semantics.
                            // recv_is_class gating preserves user classes
                            // that happen to expose an `entries` method.
                            // Drizzle's `dialect.buildInsertQuery` uses
                            // `for (const [valueIndex, value] of values.entries())`
                            // where `values` arrives via destructuring of an
                            // any-typed function param.
                            "entries" if args.is_empty() && !recv_is_class => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::ArrayEntries(Box::new(array_expr)));
                            }
                            "keys" if args.is_empty() && !recv_is_class => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::ArrayKeys(Box::new(array_expr)));
                            }
                            "values" if args.is_empty() && !recv_is_class => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::ArrayValues(Box::new(array_expr)));
                            }
                            "sort" if !args.is_empty() => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::ArraySort {
                                    array: Box::new(array_expr),
                                    comparator: Box::new(args.into_iter().next().unwrap()),
                                });
                            }
                            // .slice() exists on both Array and String, so we can only safely
                            // lower to ArraySlice when the receiver is definitely an
                            // array-producing expression (matches the indexOf/includes pattern
                            // below). Without this, `arr.sort(cb).slice(0, 5)` falls through to
                            // generic dynamic dispatch which corrupts the result — the inner
                            // ArraySort returns a real array pointer but the outer .slice goes
                            // through `js_native_call_method` which can't unwrap it properly,
                            // producing an "object" with the right .length but Array.isArray
                            // returns false and JSON.stringify segfaults.
                            "slice" if !args.is_empty() => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                if matches!(
                                    &array_expr,
                                    Expr::ArrayMap { .. } | Expr::ArrayFilter { .. } | Expr::ArraySort { .. } |
                                            Expr::ArraySlice { .. } | Expr::Array(_) | Expr::ArraySpread(_) |
                                            Expr::ArrayFrom(_) | Expr::ArrayFromMapped { .. } |
                                            Expr::ArrayFlat { .. } | Expr::StringSplit(_, _) |
                                            Expr::ArrayToReversed { .. } | Expr::ArrayToSorted { .. } |
                                            Expr::ArrayToSpliced { .. } | Expr::ArrayWith { .. } |
                                            Expr::ArrayEntries(_) | Expr::ArrayKeys(_) | Expr::ArrayValues(_) |
                                            Expr::ObjectKeys(_) | Expr::ObjectValues(_) | Expr::ObjectEntries(_) |
                                            // `process.argv` is a `string[]`. Without this arm the
                                            // fallthrough picked String.slice semantics — so
                                            // `process.argv.slice(2)` returned a "string" whose
                                            // length was the argv count and whose elements were
                                            // NaN-box bits of string pointers read as doubles
                                            // (closes #41).
                                            Expr::ProcessArgv
                                ) {
                                    let mut args_iter = args.into_iter();
                                    let start = args_iter.next().unwrap();
                                    let end = args_iter.next();
                                    return Ok(Expr::ArraySlice {
                                        array: Box::new(array_expr),
                                        start: Box::new(start),
                                        end: end.map(Box::new),
                                    });
                                }
                                // Fall through to generic Call handling (could be a String.slice).
                            }
                            // .join() folds to Array.join only when the receiver is
                            // statically array-producing. Pre-fix the unconditional fold
                            // misrouted drizzle's `sql.join(arr)` (the `sql` template tag
                            // function has its own `.join` static method via
                            // `sql2.join = join`) to `js_array_join(sql, arr)`, which
                            // returned an empty string. Refs #420.
                            //
                            // Mirrors the "slice" arm above: fold only for array-producing
                            // shapes (literals, .map/.filter/.split chains, etc.). Other
                            // receivers fall through to generic dispatch which respects
                            // user-defined `.join` methods.
                            "join" if args.len() <= 1 && !recv_is_class => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                let is_array_producing = matches!(
                                    &array_expr,
                                    Expr::Array(_)
                                        | Expr::ArrayMap { .. }
                                        | Expr::ArrayFilter { .. }
                                        | Expr::ArraySort { .. }
                                        | Expr::ArraySlice { .. }
                                        | Expr::ArraySpread(_)
                                        | Expr::ArrayFrom(_)
                                        | Expr::ArrayFromMapped { .. }
                                        | Expr::ArrayFlat { .. }
                                        | Expr::StringSplit(_, _)
                                        | Expr::ArrayToReversed { .. }
                                        | Expr::ArrayToSorted { .. }
                                        | Expr::ArrayToSpliced { .. }
                                        | Expr::ArrayWith { .. }
                                        | Expr::ArrayEntries(_)
                                        | Expr::ArrayKeys(_)
                                        | Expr::ArrayValues(_)
                                        | Expr::ObjectKeys(_)
                                        | Expr::ObjectValues(_)
                                        | Expr::ObjectEntries(_)
                                        | Expr::ProcessArgv
                                );
                                // Also fold when the receiver is statically Array-typed
                                // via `lookup_local_type` (e.g. `const arr: string[] = ...`).
                                let is_array_local =
                                    if let ast::Expr::Ident(ident) = member.obj.as_ref() {
                                        matches!(
                                            ctx.lookup_local_type(ident.sym.as_ref()),
                                            Some(Type::Array(_))
                                        )
                                    } else {
                                        false
                                    };
                                if is_array_producing || is_array_local {
                                    let separator = if args.is_empty() {
                                        None
                                    } else {
                                        Some(Box::new(args.into_iter().next().unwrap()))
                                    };
                                    return Ok(Expr::ArrayJoin {
                                        array: Box::new(array_expr),
                                        separator,
                                    });
                                }
                                // Fall through to generic dispatch — could be a user
                                // object's `.join` method (drizzle's sql.join, etc.).
                            }
                            "indexOf" if !args.is_empty() => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                if matches!(
                                    &array_expr,
                                    Expr::ArrayMap { .. }
                                        | Expr::ArrayFilter { .. }
                                        | Expr::ArraySort { .. }
                                        | Expr::ArraySlice { .. }
                                        | Expr::Array(_)
                                        | Expr::ArrayFrom(_)
                                        | Expr::StringSplit(_, _)
                                        | Expr::ObjectKeys(_)
                                        | Expr::ObjectValues(_)
                                        | Expr::PropertyGet { .. }
                                ) {
                                    let value_expr = args.into_iter().next().unwrap();
                                    return Ok(Expr::ArrayIndexOf {
                                        array: Box::new(array_expr),
                                        value: Box::new(value_expr),
                                    });
                                }
                            }
                            "includes" if !args.is_empty() => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                // Don't treat error string properties as arrays
                                let is_error_string_prop = matches!(&array_expr,
                                    Expr::PropertyGet { property, .. }
                                    if matches!(property.as_str(), "stack" | "message" | "name")
                                );
                                if !is_error_string_prop
                                    && matches!(
                                        &array_expr,
                                        Expr::ArrayMap { .. }
                                            | Expr::ArrayFilter { .. }
                                            | Expr::ArraySort { .. }
                                            | Expr::ArraySlice { .. }
                                            | Expr::Array(_)
                                            | Expr::ArrayFrom(_)
                                            | Expr::StringSplit(_, _)
                                            | Expr::ObjectKeys(_)
                                            | Expr::ObjectValues(_)
                                            | Expr::PropertyGet { .. }
                                    )
                                {
                                    let value_expr = args.into_iter().next().unwrap();
                                    return Ok(Expr::ArrayIncludes {
                                        array: Box::new(array_expr),
                                        value: Box::new(value_expr),
                                    });
                                }
                            }
                            "flat" if args.is_empty() => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::ArrayFlat {
                                    array: Box::new(array_expr),
                                });
                            }
                            "reduceRight" if !args.is_empty() => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                let mut args_iter = args.into_iter();
                                let callback = args_iter.next().unwrap();
                                let initial = args_iter.next().map(Box::new);
                                return Ok(Expr::ArrayReduceRight {
                                    array: Box::new(array_expr),
                                    callback: Box::new(callback),
                                    initial,
                                });
                            }
                            "toReversed" => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::ArrayToReversed {
                                    array: Box::new(array_expr),
                                });
                            }
                            "toSorted" => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                let comparator = args.into_iter().next().map(Box::new);
                                return Ok(Expr::ArrayToSorted {
                                    array: Box::new(array_expr),
                                    comparator,
                                });
                            }
                            "toSpliced" if args.len() >= 2 => {
                                let array_expr = lower_expr(ctx, &member.obj)?;
                                let mut args_iter = args.into_iter();
                                let start = args_iter.next().unwrap();
                                let delete_count = args_iter.next().unwrap();
                                let items: Vec<Expr> = args_iter.collect();
                                return Ok(Expr::ArrayToSpliced {
                                    array: Box::new(array_expr),
                                    start: Box::new(start),
                                    delete_count: Box::new(delete_count),
                                    items,
                                });
                            }
                            "with" if args.len() >= 2 => {
                                // Array.prototype.with(idx, value) — only fold when the
                                // receiver is statically known to be array-like. The
                                // `with` method name is heavily overloaded with user-defined
                                // builder methods (`class Builder { with(a, b): this { … } }`,
                                // `const obj = { with(a, b) { … } }`); aggressively folding
                                // non-array receivers silently rewrites the call to a
                                // typed-array index-replace and breaks user code.
                                //
                                // The sibling array arms (`map`/`filter`/`reduce`) fold
                                // optimistically because the runtime's `js_native_call_method`
                                // has dispatch arms for them — `with` has no such arm, but
                                // that's fine: bailing here falls through to the general
                                // method-call dispatch which finds the user's `with` method.
                                // The known-array fold paths for bare-ident receivers
                                // (line ~2897), imported-var array receivers (line ~3432),
                                // and inline array literals (line ~3610) handle the
                                // legitimate `Array.prototype.with` cases. (#515)
                                let recv_is_array_like = match member.obj.as_ref() {
                                    ast::Expr::Ident(ident) => {
                                        let n = ident.sym.to_string();
                                        ctx.lookup_local_type(&n)
                                            .map(|ty| matches!(ty, Type::Array(_) | Type::Tuple(_)))
                                            .unwrap_or(false)
                                    }
                                    ast::Expr::Array(_) => true,
                                    _ => false,
                                };
                                if recv_is_array_like {
                                    let array_expr = lower_expr(ctx, &member.obj)?;
                                    let mut args_iter = args.into_iter();
                                    let index = args_iter.next().unwrap();
                                    let value = args_iter.next().unwrap();
                                    return Ok(Expr::ArrayWith {
                                        array: Box::new(array_expr),
                                        index: Box::new(index),
                                        value: Box::new(value),
                                    });
                                }
                                // Fall through to general method-call dispatch
                            }
                            "push" if !args.is_empty() => {
                                // Generic expr.push(value) or expr.push(...spread)
                                // GUARD: Skip if the receiver is a user-defined class instance
                                // (e.g. Stack<T>.push()), or an object type literal (e.g.
                                // { push: (v) => void, ... }), so its method dispatches correctly.
                                let is_user_class_receiver = match member.obj.as_ref() {
                                    ast::Expr::Ident(ident) => {
                                        ctx.lookup_local_type(ident.sym.as_ref())
                                            .map(|ty| {
                                                match ty {
                                                    Type::Named(name) => {
                                                        ctx.lookup_class(name).is_some()
                                                    }
                                                    Type::Generic { base, .. } => {
                                                        let builtin = [
                                                            "Map", "Set", "WeakMap", "WeakSet",
                                                            "Promise",
                                                        ];
                                                        !builtin.contains(&base.as_str())
                                                            && ctx.lookup_class(base).is_some()
                                                    }
                                                    Type::Object(_) => true, // object type literal with push property
                                                    _ => false,
                                                }
                                            })
                                            .unwrap_or(false)
                                    }
                                    ast::Expr::New(_) => true, // new ClassName().push()
                                    _ => false,
                                };
                                if !is_user_class_receiver {
                                    let array_expr = lower_expr(ctx, &member.obj)?;
                                    if !call.args.is_empty() && call.args[0].spread.is_some() {
                                        return Ok(Expr::NativeMethodCall {
                                            module: "array".to_string(),
                                            method: "push_spread".to_string(),
                                            class_name: None,
                                            object: Some(Box::new(array_expr)),
                                            args,
                                        });
                                    } else {
                                        return Ok(Expr::NativeMethodCall {
                                            module: "array".to_string(),
                                            method: "push_single".to_string(),
                                            class_name: None,
                                            object: Some(Box::new(array_expr)),
                                            args,
                                        });
                                    }
                                }
                            }
                            _ => {} // Fall through - ambiguous methods on non-array expressions use generic dispatch
                        }
                    }
                }
            }

            // Check for regex .test() / .exec() method call on any expression
            if let ast::Callee::Expr(callee_expr) = &call.callee {
                if let ast::Expr::Member(member) = callee_expr.as_ref() {
                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                        let m = method_ident.sym.as_ref();
                        if (m == "test" || m == "exec") && args.len() == 1 {
                            // Check if the object is a regex literal or a local assigned to a regex
                            let is_regex_obj = match member.obj.as_ref() {
                                ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                                ast::Expr::Ident(ident) => ctx
                                    .lookup_local_type(ident.sym.as_ref())
                                    .map(|ty| {
                                        matches!(ty, Type::Any | Type::Unknown)
                                            || matches!(ty, Type::Named(n) if n == "RegExp")
                                    })
                                    .unwrap_or(true),
                                _ => false,
                            };
                            if is_regex_obj {
                                let regex_expr = lower_expr(ctx, &member.obj)?;
                                // Only emit RegExp method calls if the object is actually a regex
                                if matches!(&regex_expr, Expr::RegExp { .. })
                                    || matches!(&regex_expr, Expr::LocalGet(_))
                                {
                                    let string_expr = args.into_iter().next().unwrap();
                                    if m == "test" {
                                        return Ok(Expr::RegExpTest {
                                            regex: Box::new(regex_expr),
                                            string: Box::new(string_expr),
                                        });
                                    } else {
                                        return Ok(Expr::RegExpExec {
                                            regex: Box::new(regex_expr),
                                            string: Box::new(string_expr),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check for string .match(regex) method call
            if let ast::Callee::Expr(callee_expr) = &call.callee {
                if let ast::Expr::Member(member) = callee_expr.as_ref() {
                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                        if (method_ident.sym.as_ref() == "match"
                            || method_ident.sym.as_ref() == "matchAll")
                            && args.len() == 1
                        {
                            let is_match_all = method_ident.sym.as_ref() == "matchAll";
                            // Check if the argument is a regex literal or a local holding a regex
                            let arg_is_regex = match call.args.first().map(|a| a.expr.as_ref()) {
                                Some(ast::Expr::Lit(ast::Lit::Regex(_))) => true,
                                Some(ast::Expr::Ident(ident)) => {
                                    match ctx.lookup_local_type(ident.sym.as_ref()) {
                                        // Known regex local
                                        Some(Type::Named(n)) if n == "RegExp" => true,
                                        // Unknown type — assume could be regex
                                        Some(Type::Any) | Some(Type::Unknown) | None => true,
                                        _ => false,
                                    }
                                }
                                _ => false,
                            };
                            if arg_is_regex {
                                let string_expr = lower_expr(ctx, &member.obj)?;
                                let regex_expr = args.remove(0);
                                if matches!(&regex_expr, Expr::RegExp { .. })
                                    || matches!(&regex_expr, Expr::LocalGet(_))
                                {
                                    return Ok(if is_match_all {
                                        Expr::StringMatchAll {
                                            string: Box::new(string_expr),
                                            regex: Box::new(regex_expr),
                                        }
                                    } else {
                                        Expr::StringMatch {
                                            string: Box::new(string_expr),
                                            regex: Box::new(regex_expr),
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
            }

            // Check for global built-in function calls (parseInt, parseFloat, Number, String, isNaN, isFinite)
            if let ast::Expr::Ident(ident) = expr.as_ref() {
                let func_name = ident.sym.as_ref();
                match func_name {
                    "parseInt" => {
                        let string_arg = if !args.is_empty() {
                            Box::new(args.remove(0))
                        } else {
                            return Err(anyhow!("parseInt requires at least one argument"));
                        };
                        let radix_arg = if !args.is_empty() {
                            Some(Box::new(args.remove(0)))
                        } else {
                            None
                        };
                        return Ok(Expr::ParseInt {
                            string: string_arg,
                            radix: radix_arg,
                        });
                    }
                    "parseFloat" => {
                        if !args.is_empty() {
                            return Ok(Expr::ParseFloat(Box::new(args.remove(0))));
                        } else {
                            return Err(anyhow!("parseFloat requires one argument"));
                        }
                    }
                    "Number" => {
                        if !args.is_empty() {
                            return Ok(Expr::NumberCoerce(Box::new(args.remove(0))));
                        } else {
                            // Number() with no args returns 0
                            return Ok(Expr::Number(0.0));
                        }
                    }
                    "BigInt" => {
                        if !args.is_empty() {
                            return Ok(Expr::BigIntCoerce(Box::new(args.remove(0))));
                        } else {
                            // BigInt() with no args returns 0n
                            return Ok(Expr::BigInt("0".to_string()));
                        }
                    }
                    "String" => {
                        if !args.is_empty() {
                            return Ok(Expr::StringCoerce(Box::new(args.remove(0))));
                        } else {
                            // String() with no args returns ""
                            return Ok(Expr::String(String::new()));
                        }
                    }
                    "Boolean" => {
                        if !args.is_empty() {
                            return Ok(Expr::BooleanCoerce(Box::new(args.remove(0))));
                        } else {
                            // Boolean() with no args returns false
                            return Ok(Expr::Bool(false));
                        }
                    }
                    "Array"
                        if ctx.lookup_local("Array").is_none()
                            && ctx.lookup_func("Array").is_none()
                            && ctx.lookup_imported_func("Array").is_none() =>
                    {
                        // Issue #904: `Array(n)` (bare call, no `new`) must
                        // behave identically to `new Array(n)` per spec
                        // (ES2015 §22.1.1.1 / §22.1.1.2). Pre-fix the call
                        // fell through to the unknown-ident sentinel path,
                        // which lowers to `Call { callee: GlobalGet(0), ... }`
                        // and explodes at runtime via `js_closure_callN`'s
                        // `throw_not_callable` → `TypeError: value is not a
                        // function`. dayjs's `format()` hits this through
                        // `padStart`'s `Array(length + 1 - s.length).join(pad)`
                        // every time it formats a sub-10 number (e.g. month
                        // "07" in "YYYY-MM"). Route through `Expr::New` so the
                        // existing Array-constructor codegen in
                        // `crates/perry-codegen/src/lower_call/builtin.rs`
                        // handles it. Shadow-safe: only fires when no local
                        // / user fn / imported fn named `Array` is in scope.
                        return Ok(Expr::New {
                            class_name: "Array".to_string(),
                            args,
                            type_args: Vec::new(),
                        });
                    }
                    "isNaN" => {
                        if !args.is_empty() {
                            return Ok(Expr::IsNaN(Box::new(args.remove(0))));
                        } else {
                            return Err(anyhow!("isNaN requires one argument"));
                        }
                    }
                    "isFinite" => {
                        if !args.is_empty() {
                            return Ok(Expr::IsFinite(Box::new(args.remove(0))));
                        } else {
                            return Err(anyhow!("isFinite requires one argument"));
                        }
                    }
                    "atob" => {
                        if !args.is_empty() {
                            return Ok(Expr::Atob(Box::new(args.remove(0))));
                        } else {
                            return Err(anyhow!("atob requires one argument"));
                        }
                    }
                    "btoa" => {
                        if !args.is_empty() {
                            return Ok(Expr::Btoa(Box::new(args.remove(0))));
                        } else {
                            return Err(anyhow!("btoa requires one argument"));
                        }
                    }
                    "encodeURI" => {
                        if !args.is_empty() {
                            return Ok(Expr::EncodeURI(Box::new(args.remove(0))));
                        } else {
                            return Err(anyhow!("encodeURI requires one argument"));
                        }
                    }
                    "decodeURI" => {
                        if !args.is_empty() {
                            return Ok(Expr::DecodeURI(Box::new(args.remove(0))));
                        } else {
                            return Err(anyhow!("decodeURI requires one argument"));
                        }
                    }
                    "encodeURIComponent" => {
                        if !args.is_empty() {
                            return Ok(Expr::EncodeURIComponent(Box::new(args.remove(0))));
                        } else {
                            return Err(anyhow!("encodeURIComponent requires one argument"));
                        }
                    }
                    "decodeURIComponent" => {
                        if !args.is_empty() {
                            return Ok(Expr::DecodeURIComponent(Box::new(args.remove(0))));
                        } else {
                            return Err(anyhow!("decodeURIComponent requires one argument"));
                        }
                    }
                    "structuredClone" => {
                        if !args.is_empty() {
                            return Ok(Expr::StructuredClone(Box::new(args.remove(0))));
                        } else {
                            return Err(anyhow!("structuredClone requires one argument"));
                        }
                    }
                    "queueMicrotask" => {
                        if !args.is_empty() {
                            return Ok(Expr::QueueMicrotask(Box::new(args.remove(0))));
                        } else {
                            return Err(anyhow!("queueMicrotask requires one argument"));
                        }
                    }
                    "Symbol" => {
                        // Symbol() / Symbol(description)
                        if args.is_empty() {
                            return Ok(Expr::SymbolNew(None));
                        } else {
                            return Ok(Expr::SymbolNew(Some(Box::new(args.remove(0)))));
                        }
                    }
                    "perryResolveStaticPlugin" => {
                        if !args.is_empty() {
                            return Ok(Expr::StaticPluginResolve(Box::new(args.remove(0))));
                        } else {
                            return Err(anyhow!("perryResolveStaticPlugin requires one argument"));
                        }
                    }
                    "fetchWithAuth" => {
                        // fetchWithAuth(url, authHeader) -> Promise<Response>
                        // Calls js_fetch_get_with_auth(url, auth_header)
                        if args.len() >= 2 {
                            let url = args.remove(0);
                            let auth_header = args.remove(0);
                            ctx.uses_fetch = true;
                            return Ok(Expr::FetchGetWithAuth {
                                url: Box::new(url),
                                auth_header: Box::new(auth_header),
                            });
                        } else {
                            return Err(anyhow!(
                                "fetchWithAuth requires url and authHeader arguments"
                            ));
                        }
                    }
                    "fetchPostWithAuth" => {
                        // fetchPostWithAuth(url, authHeader, body) -> Promise<Response>
                        // Calls js_fetch_post_with_auth(url, auth_header, body)
                        if args.len() >= 3 {
                            let url = args.remove(0);
                            let auth_header = args.remove(0);
                            let body = args.remove(0);
                            ctx.uses_fetch = true;
                            return Ok(Expr::FetchPostWithAuth {
                                url: Box::new(url),
                                auth_header: Box::new(auth_header),
                                body: Box::new(body),
                            });
                        } else {
                            return Err(anyhow!(
                                "fetchPostWithAuth requires url, authHeader, and body arguments"
                            ));
                        }
                    }
                    "fetch" => {
                        // Handle fetch(url) and fetch(url, options)
                        // Extract URL (first argument)
                        let url = if !args.is_empty() {
                            args.remove(0)
                        } else {
                            return Err(anyhow!("fetch requires at least a URL argument"));
                        };

                        // Check if there's an options object (second argument)
                        if !args.is_empty() {
                            // Extract options from the object literal
                            // We need to get the original AST to extract the object properties
                            if let Some(options_arg) = call.args.get(1) {
                                if let ast::Expr::Object(obj) = &*options_arg.expr {
                                    // Extract method, body, and headers from options
                                    let mut method = Expr::String("GET".to_string());
                                    let mut body = Expr::Undefined;
                                    let mut headers_obj: Vec<(String, Expr)> = Vec::new();

                                    for prop in &obj.props {
                                        if let ast::PropOrSpread::Prop(prop) = prop {
                                            match prop.as_ref() {
                                                ast::Prop::KeyValue(kv) => {
                                                    let key = match &kv.key {
                                                        ast::PropName::Ident(ident) => {
                                                            ident.sym.to_string()
                                                        }
                                                        ast::PropName::Str(s) => s
                                                            .value
                                                            .as_str()
                                                            .unwrap_or("")
                                                            .to_string(),
                                                        _ => continue,
                                                    };
                                                    match key.as_str() {
                                                        "method" => {
                                                            method = lower_expr(ctx, &kv.value)?;
                                                        }
                                                        "body" => {
                                                            body = lower_expr(ctx, &kv.value)?;
                                                        }
                                                        "headers" => {
                                                            // Extract headers object
                                                            if let ast::Expr::Object(headers_ast) =
                                                                &*kv.value
                                                            {
                                                                for hprop in &headers_ast.props {
                                                                    if let ast::PropOrSpread::Prop(
                                                                        hprop,
                                                                    ) = hprop
                                                                    {
                                                                        if let ast::Prop::KeyValue(
                                                                            hkv,
                                                                        ) = hprop.as_ref()
                                                                        {
                                                                            let hkey = match &hkv.key {
                                                                                        ast::PropName::Ident(ident) => ident.sym.to_string(),
                                                                                        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                                                                        _ => continue,
                                                                                    };
                                                                            let hval = lower_expr(
                                                                                ctx, &hkv.value,
                                                                            )?;
                                                                            headers_obj
                                                                                .push((hkey, hval));
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                                ast::Prop::Shorthand(ident) => {
                                                    // Handle shorthand properties like { body } which means { body: body }
                                                    let key = ident.sym.to_string();
                                                    let value = if let Some(local_id) =
                                                        ctx.lookup_local(&key)
                                                    {
                                                        Expr::LocalGet(local_id)
                                                    } else {
                                                        continue;
                                                    };
                                                    match key.as_str() {
                                                        "method" => method = value,
                                                        "body" => body = value,
                                                        _ => {}
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }

                                    // Create a FetchWithOptions expression
                                    ctx.uses_fetch = true;
                                    return Ok(Expr::FetchWithOptions {
                                        url: Box::new(url),
                                        method: Box::new(method),
                                        body: Box::new(body),
                                        headers: headers_obj,
                                    });
                                }
                            }
                        }

                        // Simple fetch(url) with no options - use GET
                        ctx.uses_fetch = true;
                        return Ok(Expr::FetchWithOptions {
                            url: Box::new(url),
                            method: Box::new(Expr::String("GET".to_string())),
                            body: Box::new(Expr::Undefined),
                            headers: Vec::new(),
                        });
                    }
                    _ => {} // Fall through to generic handling
                }

                // Check if this is a named import from child_process (e.g., execSync, spawnSync)
                if let Some((module_name, _method)) = ctx.lookup_native_module(func_name) {
                    if module_name == "child_process" {
                        match func_name {
                            "execSync" => {
                                if !args.is_empty() {
                                    let mut args_iter = args.into_iter();
                                    let command = args_iter.next().unwrap();
                                    let options = args_iter.next().map(Box::new);
                                    return Ok(Expr::ChildProcessExecSync {
                                        command: Box::new(command),
                                        options,
                                    });
                                }
                            }
                            "spawnSync" => {
                                if !args.is_empty() {
                                    let mut args_iter = args.into_iter();
                                    let command = args_iter.next().unwrap();
                                    let spawn_args = args_iter.next().map(Box::new);
                                    let options = args_iter.next().map(Box::new);
                                    return Ok(Expr::ChildProcessSpawnSync {
                                        command: Box::new(command),
                                        args: spawn_args,
                                        options,
                                    });
                                }
                            }
                            "spawn" => {
                                if !args.is_empty() {
                                    let mut args_iter = args.into_iter();
                                    let command = args_iter.next().unwrap();
                                    let spawn_args = args_iter.next().map(Box::new);
                                    let options = args_iter.next().map(Box::new);
                                    return Ok(Expr::ChildProcessSpawn {
                                        command: Box::new(command),
                                        args: spawn_args,
                                        options,
                                    });
                                }
                            }
                            "exec" => {
                                if !args.is_empty() {
                                    let mut args_iter = args.into_iter();
                                    let command = args_iter.next().unwrap();
                                    let options = args_iter.next().map(Box::new);
                                    let callback = args_iter.next().map(Box::new);
                                    return Ok(Expr::ChildProcessExec {
                                        command: Box::new(command),
                                        options,
                                        callback,
                                    });
                                }
                            }
                            "spawnBackground" => {
                                if args.len() >= 3 {
                                    let mut args_iter = args.into_iter();
                                    let command = args_iter.next().unwrap();
                                    let spawn_args = args_iter.next().map(Box::new);
                                    let log_file = args_iter.next().unwrap();
                                    let env_json = args_iter.next().map(Box::new);
                                    return Ok(Expr::ChildProcessSpawnBackground {
                                        command: Box::new(command),
                                        args: spawn_args,
                                        log_file: Box::new(log_file),
                                        env_json,
                                    });
                                }
                            }
                            "getProcessStatus" => {
                                if !args.is_empty() {
                                    return Ok(Expr::ChildProcessGetProcessStatus(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "killProcess" => {
                                if !args.is_empty() {
                                    return Ok(Expr::ChildProcessKillProcess(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            _ => {} // Fall through
                        }
                    }

                    // Check if this is a named import from path (e.g., join, dirname, basename)
                    if module_name == "path" {
                        match func_name {
                            "join" => {
                                if args.is_empty() {
                                    return Ok(Expr::String(".".to_string()));
                                }
                                if args.len() == 1 {
                                    return Ok(Expr::PathNormalize(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                                let mut iter = args.into_iter();
                                let mut result = iter.next().unwrap();
                                for next_arg in iter {
                                    result = Expr::PathJoin(Box::new(result), Box::new(next_arg));
                                }
                                return Ok(result);
                            }
                            "dirname" => {
                                if !args.is_empty() {
                                    return Ok(Expr::PathDirname(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "basename" => {
                                if args.len() >= 2 {
                                    let mut iter = args.into_iter();
                                    let path_arg = iter.next().unwrap();
                                    let ext_arg = iter.next().unwrap();
                                    return Ok(Expr::PathBasenameExt(
                                        Box::new(path_arg),
                                        Box::new(ext_arg),
                                    ));
                                }
                                if !args.is_empty() {
                                    return Ok(Expr::PathBasename(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "extname" => {
                                if !args.is_empty() {
                                    return Ok(Expr::PathExtname(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "resolve" => {
                                if !args.is_empty() {
                                    let mut iter = args.into_iter();
                                    let first = iter.next().unwrap();
                                    let mut joined = first;
                                    for next_arg in iter {
                                        joined = Expr::PathResolveJoin(
                                            Box::new(joined),
                                            Box::new(next_arg),
                                        );
                                    }
                                    return Ok(Expr::PathResolve(Box::new(joined)));
                                }
                            }
                            "isAbsolute" => {
                                if !args.is_empty() {
                                    return Ok(Expr::PathIsAbsolute(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "relative" => {
                                if args.len() >= 2 {
                                    let mut iter = args.into_iter();
                                    let from = iter.next().unwrap();
                                    let to = iter.next().unwrap();
                                    return Ok(Expr::PathRelative(Box::new(from), Box::new(to)));
                                }
                            }
                            "normalize" => {
                                if !args.is_empty() {
                                    return Ok(Expr::PathNormalize(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "parse" => {
                                if !args.is_empty() {
                                    return Ok(Expr::PathParse(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "format" => {
                                if !args.is_empty() {
                                    return Ok(Expr::PathFormat(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "toNamespacedPath" => {
                                if !args.is_empty() {
                                    return Ok(Expr::PathToNamespacedPath(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "matchesGlob" => {
                                if args.len() >= 2 {
                                    let mut iter = args.into_iter();
                                    let path_arg = iter.next().unwrap();
                                    let pattern = iter.next().unwrap();
                                    return Ok(Expr::PathMatchesGlob(
                                        Box::new(path_arg),
                                        Box::new(pattern),
                                    ));
                                }
                            }
                            _ => {} // Fall through
                        }
                    }

                    // Check if this is a named import from url (e.g., fileURLToPath)
                    if module_name == "url" {
                        match func_name {
                            "fileURLToPath" => {
                                if !args.is_empty() {
                                    return Ok(Expr::FileURLToPath(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            _ => {} // Fall through
                        }
                    }

                    // Check if this is a named import from fs (e.g., existsSync, mkdirSync, etc.)
                    if module_name == "fs" {
                        match func_name {
                            "readFileSync" => {
                                if args.len() >= 2 {
                                    // readFileSync(path, encoding) — returns string
                                    return Ok(Expr::FsReadFileSync(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                } else if args.len() == 1 {
                                    // readFileSync(path) without encoding — returns Buffer (Node parity)
                                    return Ok(Expr::FsReadFileBinary(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "writeFileSync" => {
                                if args.len() >= 2 {
                                    let mut iter = args.into_iter();
                                    let path = iter.next().unwrap();
                                    let content = iter.next().unwrap();
                                    return Ok(Expr::FsWriteFileSync(
                                        Box::new(path),
                                        Box::new(content),
                                    ));
                                }
                            }
                            "existsSync" => {
                                if !args.is_empty() {
                                    return Ok(Expr::FsExistsSync(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "mkdirSync" => {
                                if !args.is_empty() {
                                    return Ok(Expr::FsMkdirSync(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "unlinkSync" => {
                                if !args.is_empty() {
                                    return Ok(Expr::FsUnlinkSync(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "appendFileSync" => {
                                if args.len() >= 2 {
                                    let mut iter = args.into_iter();
                                    let path = iter.next().unwrap();
                                    let content = iter.next().unwrap();
                                    return Ok(Expr::FsAppendFileSync(
                                        Box::new(path),
                                        Box::new(content),
                                    ));
                                }
                            }
                            "readFileBuffer" => {
                                if !args.is_empty() {
                                    return Ok(Expr::FsReadFileBinary(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            "rmRecursive" => {
                                if !args.is_empty() {
                                    return Ok(Expr::FsRmRecursive(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            // Issue #648 fallout: see twin arm above.
                            "rmSync" => {
                                if !args.is_empty() {
                                    return Ok(Expr::FsRmRecursive(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            _ => {} // Fall through
                        }
                    }

                    // Named imports from `node:crypto` (e.g.
                    // `import { randomFillSync, randomUUID, randomBytes }
                    //  from 'node:crypto'`). Without these arms the bare
                    // call `randomFillSync(buf)` falls into the generic
                    // `NativeMethodCall` path, which has no `crypto`
                    // dispatcher and returns `undefined` — so the buffer
                    // never gets filled (jose's signing flow silently
                    // produces all-zero IVs / nonces). Route each call
                    // straight to the dedicated `Expr::CryptoRandom*`
                    // variant that the `crypto.xxx(...)` (object-method)
                    // arm above (line ~3060) uses, so both call shapes
                    // share one codegen path.
                    if module_name == "crypto" {
                        match func_name {
                            "randomFillSync" => {
                                if !args.is_empty() {
                                    let mut iter = args.into_iter();
                                    let buffer = iter.next().unwrap();
                                    let offset = iter.next().unwrap_or(Expr::Undefined);
                                    let size = iter.next().unwrap_or(Expr::Undefined);
                                    return Ok(Expr::CryptoRandomFillSync {
                                        buffer: Box::new(buffer),
                                        offset: Box::new(offset),
                                        size: Box::new(size),
                                    });
                                }
                            }
                            "randomUUID" => {
                                return Ok(Expr::CryptoRandomUUID);
                            }
                            "randomBytes" => {
                                if !args.is_empty() {
                                    return Ok(Expr::CryptoRandomBytes(Box::new(
                                        args.into_iter().next().unwrap(),
                                    )));
                                }
                            }
                            // `createSecretKey(key, encoding?)` from a named
                            // import. Without this arm the call lowered to a
                            // generic NativeMethodCall with no dispatcher for
                            // `crypto.createSecretKey`, so the call returned
                            // undefined and `jose.sign(undefined)` threw
                            // "Received undefined". Reuse the same PropertyGet
                            // shape that the `crypto.createSecretKey(...)`
                            // call-site form already exercises (handled in
                            // `expr.rs` near the createHash/createHmac block)
                            // so both shapes share one codegen path.
                            "createSecretKey" => {
                                if !args.is_empty() {
                                    let mut iter = args.into_iter();
                                    let key_arg = iter.next().unwrap();
                                    let mut new_args = vec![key_arg];
                                    if let Some(enc) = iter.next() {
                                        new_args.push(enc);
                                    }
                                    return Ok(Expr::Call {
                                        callee: Box::new(Expr::PropertyGet {
                                            object: Box::new(Expr::NativeModuleRef(
                                                "crypto".to_string(),
                                            )),
                                            property: "createSecretKey".to_string(),
                                        }),
                                        args: new_args,
                                        type_args: vec![],
                                    });
                                }
                            }
                            _ => {} // Fall through
                        }
                    }
                }

                // Check if this is a direct call on an aliased named import
                // e.g., uuid() where import { v4 as uuid } from 'uuid'
                if let Some((module_name, Some(method_name))) = ctx.lookup_native_module(func_name)
                {
                    return Ok(Expr::NativeMethodCall {
                        module: module_name.to_string(),
                        class_name: None,
                        object: None,
                        method: method_name.to_string(),
                        args,
                    });
                }

                // Check if this is a call on a default import from a native module
                // e.g., Fastify() where import Fastify from 'fastify'
                if let Some((module_name, None)) = ctx.lookup_native_module(func_name) {
                    return Ok(Expr::NativeMethodCall {
                        module: module_name.to_string(),
                        class_name: None,
                        object: None,
                        method: "default".to_string(), // Use "default" for default export calls
                        args,
                    });
                }
            }

            let callee_expr = lower_expr(ctx, expr)?;

            // Fill in default arguments if callee is a known function
            let mut args = args;
            if let Expr::FuncRef(func_id) = &callee_expr {
                if let Some((defaults, param_ids, rest_idx)) = ctx.lookup_func_defaults(*func_id) {
                    let defaults = defaults.to_vec();
                    let param_ids = param_ids.to_vec();
                    let num_provided = args.len();
                    // Refs #653 followup to v0.5.789's #645 fix: stop the
                    // default-fill loop BEFORE the rest param's slot. Pushing
                    // `Expr::Undefined` for a rest param turns
                    // `function f(name, ...args)` called as `f('a')` into
                    // `f('a', undefined)`, which the runtime then spreads into
                    // `args[0] = undefined`. Real semantics: trailing positional
                    // args get bundled into the rest array at runtime; missing
                    // ones produce an empty array, NOT a single-undefined array.
                    let fill_end = match rest_idx {
                        Some(i) => i,
                        None => defaults.len(),
                    };
                    // Build substitution map: callee param LocalId -> actual arg expression
                    // For provided args, map to the caller's arg expression
                    // For defaulted args, map to the expanded default (built incrementally)
                    let mut param_map: Vec<(LocalId, Expr)> = Vec::new();
                    for i in 0..param_ids.len().min(num_provided) {
                        param_map.push((param_ids[i], args[i].clone()));
                    }
                    // Fill in missing arguments with their defaults, substituting
                    // any parameter references to use the caller's scope.
                    //
                    // Refs #645 deeper followup / #488 drizzle-sqlite: push
                    // something for EVERY slot from num_provided..defaults.len(),
                    // even when defaults[i] is None — otherwise a later Some
                    // default lands at the wrong positional slot. Drizzle's
                    // tableBase(name, columns, extraConfig, schema?, baseName=name)
                    // is the load-bearing repro: 3-arg call, slot 3 (schema)
                    // has None default and slot 4 (baseName) has Some(name).
                    // The pre-fix loop skipped slot 3 and pushed baseName's
                    // default into slot 3 — so schema got the table name and
                    // rendered SQL became `"users"."users"` instead of `"users"`.
                    for i in num_provided..fill_end {
                        let substituted = if let Some(default_expr) = &defaults[i] {
                            LoweringContext::substitute_param_refs_in_default(
                                default_expr,
                                &param_map,
                            )
                        } else {
                            Expr::Undefined
                        };
                        // Add this expanded default to the map so later defaults
                        // can reference it (e.g., c = b where b was also defaulted)
                        if i < param_ids.len() {
                            param_map.push((param_ids[i], substituted.clone()));
                        }
                        args.push(substituted);
                    }
                }
            }

            // If inside a namespace, convert calls to namespace functions into StaticMethodCall
            if let Expr::FuncRef(func_id) = &callee_expr {
                if let Some(ref ns_name) = ctx.current_namespace {
                    if let Some(func_name) = ctx.lookup_func_name(*func_id) {
                        if ctx.has_static_method(ns_name, func_name) {
                            let method_name = func_name.to_string();
                            let class_name = ns_name.clone();
                            return Ok(Expr::StaticMethodCall {
                                class_name,
                                method_name,
                                args,
                            });
                        }
                    }
                }
            }

            let callee = Box::new(callee_expr);
            // Extract explicit type arguments if present (e.g., identity<number>(x))
            let type_args = call
                .type_args
                .as_ref()
                .map(|ta| {
                    ta.params
                        .iter()
                        .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                        .collect()
                })
                .unwrap_or_default();

            // Use CallSpread if any argument has spread
            if let Some(spread_args) = spread_args {
                Ok(Expr::CallSpread {
                    callee,
                    args: spread_args,
                    type_args,
                })
            } else {
                Ok(Expr::Call {
                    callee,
                    args,
                    type_args,
                })
            }
        }
        ast::Callee::Import(_) => {
            // Issue #100: dynamic import() — lower to Expr::DynamicImport with
            // an empty `paths` list. `collect_modules` runs the path-const
            // folder and populates `paths`. If the argument can't be folded
            // to a finite set, that pass raises a compile error before codegen
            // sees the empty node.
            let arg = args
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("dynamic import() requires a path argument"))?;
            Ok(Expr::DynamicImport {
                paths: Vec::new(),
                arg: Box::new(arg),
            })
        }
    }
}

/// Issue #562 — does `method` name a stream-API method on the given
/// stream module? Used to gate the native-instance method rerouting so
/// user-declared subclass methods fall through to regular class method
/// dispatch. Mirrors the methods table in
/// `crates/perry-codegen/src/lower_call.rs`'s `module == "<stream>"`
/// arms — see also the parallel `is_stream_api_member` in
/// `expr_member.rs`.
fn is_stream_api_method(module: &str, method: &str) -> bool {
    match module {
        "readable_stream" => matches!(
            method,
            "getReader"
                | "cancel"
                | "tee"
                | "pipeTo"
                | "pipeThrough"
                | "locked"
                | "enqueue"
                | "close"
                | "error"
                | "desiredSize"
        ),
        "readable_stream_reader" => {
            matches!(method, "read" | "releaseLock" | "cancel" | "closed")
        }
        "writable_stream" => matches!(method, "getWriter" | "abort" | "close" | "locked"),
        "writable_stream_writer" => matches!(
            method,
            "write" | "close" | "abort" | "releaseLock" | "closed" | "ready" | "desiredSize"
        ),
        "transform_stream" => matches!(method, "readable" | "writable"),
        _ => false,
    }
}

/// Issue #562 — `class X extends ReadableStream/TransformStream`
/// constructor: register the controller param of each callback in
/// `super({...})` as a `readable_stream` native instance so
/// `controller.enqueue(...)` inside those bodies dispatches through the
/// streams arms in `lower_call.rs`.
///
/// Mirrors the field-spec table in `expr_new.rs::lower_new` for the
/// `new ReadableStream/TransformStream` form. WritableStream's
/// `write`/`close`/`abort` callbacks don't take a controller —
/// no pre-registration is required for that parent.
fn register_super_stream_controller_params(
    ctx: &mut LoweringContext,
    parent_ident: &str,
    call: &ast::CallExpr,
) {
    let field_specs: &[(&'static str, usize, &'static str, &'static str)] = match parent_ident {
        "ReadableStream" => &[
            ("start", 0, "readable_stream", "ReadableStream"),
            ("pull", 0, "readable_stream", "ReadableStream"),
        ],
        "TransformStream" => &[
            ("transform", 1, "readable_stream", "ReadableStream"),
            ("flush", 0, "readable_stream", "ReadableStream"),
        ],
        _ => return,
    };

    let Some(first) = call.args.first() else {
        return;
    };
    let ast::Expr::Object(obj_lit) = first.expr.as_ref() else {
        return;
    };
    for prop in &obj_lit.props {
        let ast::PropOrSpread::Prop(boxed_prop) = prop else {
            continue;
        };
        match boxed_prop.as_ref() {
            ast::Prop::KeyValue(kv) => {
                let name = match &kv.key {
                    ast::PropName::Ident(i) => Some(i.sym.as_ref()),
                    ast::PropName::Str(s) => s.value.as_str(),
                    _ => None,
                };
                let Some(name) = name else { continue };
                let Some((_, idx, mod_name, class_name)) =
                    field_specs.iter().find(|(f, _, _, _)| *f == name)
                else {
                    continue;
                };
                let pat: Option<&ast::Pat> = match kv.value.as_ref() {
                    ast::Expr::Arrow(arrow) => arrow.params.get(*idx),
                    ast::Expr::Fn(fn_expr) => fn_expr.function.params.get(*idx).map(|p| &p.pat),
                    _ => None,
                };
                if let Some(ast::Pat::Ident(pid)) = pat {
                    ctx.register_native_instance(
                        pid.id.sym.to_string(),
                        mod_name.to_string(),
                        class_name.to_string(),
                    );
                }
            }
            ast::Prop::Method(m) => {
                let name = match &m.key {
                    ast::PropName::Ident(i) => Some(i.sym.as_ref()),
                    ast::PropName::Str(s) => s.value.as_str(),
                    _ => None,
                };
                let Some(name) = name else { continue };
                let Some((_, idx, mod_name, class_name)) =
                    field_specs.iter().find(|(f, _, _, _)| *f == name)
                else {
                    continue;
                };
                if let Some(param) = m.function.params.get(*idx) {
                    if let ast::Pat::Ident(pid) = &param.pat {
                        ctx.register_native_instance(
                            pid.id.sym.to_string(),
                            mod_name.to_string(),
                            class_name.to_string(),
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

/// (key, value, target, propertyKey?) — `Reflect.defineMetadata`'s 3-or-4 arg
/// shape. Defaults missing leading args to `undefined`; `property_key` stays
/// `None` when omitted so the runtime can distinguish class-level metadata
/// from a property-level one.
fn take_reflect_kvtp_args(args: Vec<Expr>) -> (Expr, Expr, Expr, Option<Box<Expr>>) {
    let mut it = args.into_iter();
    let key = it.next().unwrap_or(Expr::Undefined);
    let value = it.next().unwrap_or(Expr::Undefined);
    let target = it.next().unwrap_or(Expr::Undefined);
    let property_key = it.next().map(Box::new);
    (key, value, target, property_key)
}

/// (key, target, propertyKey?) — `Reflect.{get,getOwn,has,hasOwn,delete}Metadata`.
fn take_reflect_ktp_args(args: Vec<Expr>) -> (Expr, Expr, Option<Box<Expr>>) {
    let mut it = args.into_iter();
    let key = it.next().unwrap_or(Expr::Undefined);
    let target = it.next().unwrap_or(Expr::Undefined);
    let property_key = it.next().map(Box::new);
    (key, target, property_key)
}

/// (target, propertyKey?) — `Reflect.{get,getOwn}MetadataKeys`.
fn take_reflect_tp_args(args: Vec<Expr>) -> (Expr, Option<Box<Expr>>) {
    let mut it = args.into_iter();
    let target = it.next().unwrap_or(Expr::Undefined);
    let property_key = it.next().map(Box::new);
    (target, property_key)
}

/// Issue #886: synthesize the dedicated HIR variant for an indirect call
/// through an `Object.<staticMethod>` alias. Mirrors the literal-callee
/// recogniser in `lower_call` (the `obj_name == "Object"` arm) but skips
/// the AST-shape sub-cases that depend on argument-AST inspection
/// (`Object.assign({}, …)` fresh-target ObjectSpread fold,
/// `Object.defineProperties` static-descriptor sequence fold). The aliased
/// shape can't benefit from those AST-time folds anyway — the literal
/// recogniser has the original arg expressions, this path only has the
/// already-lowered HIR — so we always emit the general HIR variant which
/// preserves spec semantics at runtime, just without the constant-fold
/// optimisations.
///
/// The whitelist here MUST stay in sync with the `is_supported` filter in
/// `destructuring.rs::lower_var_decl_with_destructuring` — methods missing
/// from the dispatch below will be tagged as aliases and then fall through
/// to the original generic call path that throws `TypeError: value is not
/// a function`, regressing the very pattern this fix is supposed to fix.
fn build_object_static_method_call(method: &str, args: Vec<Expr>) -> Expr {
    let mut iter = args.into_iter();
    match method {
        "defineProperty" => {
            let obj = iter.next().unwrap_or(Expr::Undefined);
            let key = iter.next().unwrap_or(Expr::Undefined);
            let descriptor = iter.next().unwrap_or(Expr::Undefined);
            Expr::ObjectDefineProperty(Box::new(obj), Box::new(key), Box::new(descriptor))
        }
        "defineProperties" => {
            let target = iter.next().unwrap_or(Expr::Undefined);
            let descs = iter.next().unwrap_or(Expr::Undefined);
            Expr::ObjectDefineProperties(Box::new(target), Box::new(descs))
        }
        "setPrototypeOf" => {
            let obj = iter.next().unwrap_or(Expr::Undefined);
            let proto = iter.next().unwrap_or(Expr::Undefined);
            Expr::ObjectSetPrototypeOf(Box::new(obj), Box::new(proto))
        }
        "getPrototypeOf" => {
            Expr::ObjectGetPrototypeOf(Box::new(iter.next().unwrap_or(Expr::Undefined)))
        }
        "getOwnPropertyDescriptor" => {
            let obj = iter.next().unwrap_or(Expr::Undefined);
            let key = iter.next().unwrap_or(Expr::Undefined);
            Expr::ObjectGetOwnPropertyDescriptor(Box::new(obj), Box::new(key))
        }
        "getOwnPropertyNames" => {
            Expr::ObjectGetOwnPropertyNames(Box::new(iter.next().unwrap_or(Expr::Undefined)))
        }
        "getOwnPropertySymbols" => {
            Expr::ObjectGetOwnPropertySymbols(Box::new(iter.next().unwrap_or(Expr::Undefined)))
        }
        "keys" => Expr::ObjectKeys(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "values" => Expr::ObjectValues(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "entries" => Expr::ObjectEntries(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "assign" => {
            let target = iter.next().unwrap_or(Expr::Object(Vec::new()));
            let sources: Vec<Expr> = iter.collect();
            Expr::ObjectAssign {
                target: Box::new(target),
                sources,
            }
        }
        "fromEntries" => Expr::ObjectFromEntries(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "create" => Expr::ObjectCreate(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "freeze" => Expr::ObjectFreeze(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "seal" => Expr::ObjectSeal(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "preventExtensions" => {
            Expr::ObjectPreventExtensions(Box::new(iter.next().unwrap_or(Expr::Undefined)))
        }
        "isFrozen" => Expr::ObjectIsFrozen(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "isSealed" => Expr::ObjectIsSealed(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "isExtensible" => {
            Expr::ObjectIsExtensible(Box::new(iter.next().unwrap_or(Expr::Undefined)))
        }
        "hasOwn" => {
            let obj = iter.next().unwrap_or(Expr::Undefined);
            let key = iter.next().unwrap_or(Expr::Undefined);
            Expr::ObjectHasOwn(Box::new(obj), Box::new(key))
        }
        "is" => {
            let a = iter.next().unwrap_or(Expr::Undefined);
            let b = iter.next().unwrap_or(Expr::Undefined);
            Expr::ObjectIs(Box::new(a), Box::new(b))
        }
        // Unreachable in practice — the whitelist in destructuring.rs
        // gates which methods reach this dispatch. The fall-through is
        // defensive: an unrecognised method shouldn't have been tagged
        // as an alias, but if it slips through we emit Undefined rather
        // than panic-ing the compiler.
        _ => Expr::Undefined,
    }
}
