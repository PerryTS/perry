//! Compile-time intrinsics + bare-callee CJS/UMD legacy shapes.
//!
//! Handles, in order: `require(literal)` bail, `embedWasm(literal)`,
//! the `(function(){...}).call(this, ...)` IIFE rewrite, the
//! `Function('return this')()` globalThis fold, and the
//! `RegExp(pattern, flags?)` bare-call fold.
//!
//! Each helper returns `Result<Option<Expr>>` — `Some` if it matched
//! and the caller should return that expression; `None` to fall
//! through. Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;

use super::super::{lower_expr, LoweringContext};

/// Issue #668: AOT `require(stringLiteral)` from a user TypeScript file
/// currently lowers to `Call { callee: GlobalGet(0), ... }` (the unknown-ident
/// sentinel) and explodes at runtime as `TypeError: value is not a function`.
/// Until we wire up synthetic namespace-imports for `require(literal)`, fail
/// at compile time with a fix-it pointing at `import ...` so the user finds the
/// problem on the first build instead of the first prod request.
pub(super) fn try_require_literal_bail(ctx: &LoweringContext, call: &ast::CallExpr) -> Result<()> {
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
                    let hint = super::super::unimpl_hints::require_module_hint(spec)
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
    Ok(())
}

/// #1678 (Phase 0 of #1677) — classify a bare `Function(...)` /
/// `eval(...)` call. The `Function('return this')()` globalThis fold runs
/// before this (in `lower_call_inner`) and short-circuits, so its inner
/// `Function('return this')` never reaches here.
///
/// Returns `Err` (span-tagged) only for the runtime-unknown bucket —
/// const-foldable (string-literal body) and known-codegen-library sites
/// log under `PERRY_EVAL_DIAG` and fall through to the existing lowering
/// (a bare `Function`/`eval` ident → `GlobalGet(0)` sentinel) unchanged,
/// to be picked up by later phases. `Ok(())` means proceed.
pub(super) fn check_eval_function_call(ctx: &LoweringContext, call: &ast::CallExpr) -> Result<()> {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(());
    };
    let mut callee = callee_expr.as_ref();
    while let ast::Expr::Paren(p) = callee {
        callee = p.expr.as_ref();
    }
    let ast::Expr::Ident(ident) = callee else {
        return Ok(());
    };
    let name = ident.sym.as_ref();
    let surface = match name {
        "eval" => crate::eval_classifier::EvalSurface::Eval,
        "Function" => crate::eval_classifier::EvalSurface::FunctionCall,
        _ => return Ok(()),
    };
    // A local/func/imported binding named `eval`/`Function` shadows the
    // builtin — leave those alone.
    if ctx.lookup_local(name).is_some()
        || ctx.lookup_func(name).is_some()
        || ctx.lookup_imported_func(name).is_some()
    {
        return Ok(());
    }
    // Body argument: the only arg for `eval(code)`, the last arg for
    // `Function(p1, p2, body)`. A spread in the body position yields a
    // non-constant inner expr → the classifier buckets it runtime-unknown.
    let body_arg = match surface {
        crate::eval_classifier::EvalSurface::Eval => call.args.first(),
        _ => call.args.last(),
    }
    .map(|a| a.expr.as_ref());
    crate::eval_classifier::check_site(surface, body_arg, &ctx.source_file_path, call.span)
}

/// #1681 (Phase 3 of #1677) — `precompile(EXPR)` build-time intrinsic.
///
/// `precompile` marks a build-time-evaluable codegen expression: `EXPR` is
/// run **at build time** (by Perry compiling and running its own output —
/// no node, no embedded engine) and must produce a *function-source
/// string*; that source is then compiled natively and substituted for the
/// call. This is the self-hosted "evaporate dynamism at build time" path:
/// the generated function ships native, with no `new Function`/engine in
/// the binary.
///
/// Two lowering modes (set by the driver via `set_precompile_capture` /
/// `set_precompile_results`):
///   - **Capture stage** (the Stage-1 subprocess): lower to
///     `console.log("<marker>…" + JSON.stringify(EXPR))` so running the
///     produced binary emits `EXPR`'s build-time value, keyed by this call
///     site's `(source_file, span.lo)`.
///   - **Main compile**: look up the captured source for this `(file, lo)`,
///     parse it as a function expression, and lower it in place. A missing
///     result (the capture run never reached this site) is a hard error —
///     no silent fallback (acceptance criterion of #1681).
pub(super) fn try_precompile(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
) -> Result<Option<Expr>> {
    // Bare unshadowed `precompile(<one non-spread arg>)`.
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Ident(ident) = callee_expr.as_ref() else {
        return Ok(None);
    };
    if ident.sym.as_ref() != "precompile"
        || ctx.lookup_local("precompile").is_some()
        || ctx.lookup_func("precompile").is_some()
        || ctx.lookup_imported_func("precompile").is_some()
        || call.args.len() != 1
        || call.args[0].spread.is_some()
    {
        return Ok(None);
    }
    let span = call.span;
    let site_lo = span.lo.0;
    let file = ctx.source_file_path.clone();

    if crate::ir::precompile_capture_enabled() {
        // Stage 1: emit `console.log("<marker>" + JSON.stringify(EXPR))`.
        // Synthesize the AST and re-dispatch through `lower_call` so the
        // normal console.log / JSON.stringify / string-concat lowerings do
        // the work. The marker carries the site key so the driver can route
        // the captured source back without depending on lowering order.
        let marker = format!("\u{1}PERRY_PRECOMPILE\u{1}{file}\u{1}{site_lo}\u{1}");
        let sctx = swc_common::SyntaxContext::empty();
        let member = |obj: &str, prop: &str| {
            ast::Expr::Member(ast::MemberExpr {
                span,
                obj: Box::new(ast::Expr::Ident(ast::Ident::new(obj.into(), span, sctx))),
                prop: ast::MemberProp::Ident(ast::IdentName {
                    span,
                    sym: prop.into(),
                }),
            })
        };
        // JSON.stringify(EXPR)
        let mut json_call = call.clone();
        json_call.callee = ast::Callee::Expr(Box::new(member("JSON", "stringify")));
        json_call.args = vec![call.args[0].clone()];
        // "<marker>" + JSON.stringify(EXPR)
        let concat = ast::Expr::Bin(ast::BinExpr {
            span,
            op: ast::BinaryOp::Add,
            left: Box::new(ast::Expr::Lit(ast::Lit::Str(ast::Str {
                span,
                value: marker.into(),
                raw: None,
            }))),
            right: Box::new(ast::Expr::Call(json_call)),
        });
        // console.log(<concat>)
        let mut log_call = call.clone();
        log_call.callee = ast::Callee::Expr(Box::new(member("console", "log")));
        log_call.args = vec![ast::ExprOrSpread {
            spread: None,
            expr: Box::new(concat),
        }];
        return Ok(Some(super::lower_call(ctx, &log_call)?));
    }

    // Main compile: substitute the captured generated function.
    match crate::ir::precompile_result_at(&file, site_lo) {
        Some(src) => Ok(Some(lower_precompiled_source(ctx, &src, span)?)),
        None => {
            crate::lower_bail!(
                span,
                "`precompile(...)` produced no build-time result for this call site \
                 ({}:{}). The build-time capture run did not reach it — its argument \
                 must be evaluable at build time and produce a function-source string. \
                 (#1681)",
                file,
                site_lo,
            );
        }
    }
}

/// Parse a build-time-captured function-source string (e.g. `"(a) => a + 3"`
/// or `"function (a) { return a }"`) and lower it as an ordinary function
/// expression — the same path the Phase 1 const-fold uses.
fn lower_precompiled_source(
    ctx: &mut LoweringContext,
    src: &str,
    span: swc_common::Span,
) -> Result<Expr> {
    let wrapped = format!("({src});\n");
    let module = perry_parser::parse_typescript(&wrapped, "<precompiled>").map_err(|e| {
        anyhow::Error::new(crate::error::LowerError::new(
            format!(
                "build-time `precompile` result is not a valid function expression: {e} \
                 (#1681)\n  source: {src:?}"
            ),
            span,
        ))
    })?;
    let fn_expr = module
        .body
        .first()
        .and_then(|item| match item {
            ast::ModuleItem::Stmt(ast::Stmt::Expr(es)) => Some(es.expr.as_ref()),
            _ => None,
        })
        .map(|mut e| {
            while let ast::Expr::Paren(p) = e {
                e = p.expr.as_ref();
            }
            e
        });
    match fn_expr {
        Some(e @ (ast::Expr::Fn(_) | ast::Expr::Arrow(_))) => lower_expr(ctx, e),
        _ => crate::lower_bail!(
            span,
            "build-time `precompile` result must be a function expression (#1681)\n  source: {src:?}"
        ),
    }
}

/// Issue #76 — `embedWasm("./file.wasm")` from `perry/build` is a
/// compile-time intrinsic that bakes the file's bytes directly into the
/// produced binary. Resolves the path relative to the current source
/// file (matches the maintainer's preferred MVP shape vs. the in-flight
/// import-attributes proposal). The argument MUST be a string literal —
/// dynamic paths defeat the whole purpose. Unknown failure (file not
/// found, etc.) bails the compile with a clear error.
pub(super) fn try_embed_wasm(ctx: &LoweringContext, call: &ast::CallExpr) -> Result<Option<Expr>> {
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
                    return Ok(Some(Expr::Uint8ArrayNew(Some(Box::new(Expr::Array(
                        elems,
                    ))))));
                }
                crate::lower_bail!(
                    call.span,
                    "embedWasm(...) requires a string-literal path argument so the bytes can be embedded at compile time"
                );
            }
        }
    }
    Ok(None)
}

/// Issue #957 — `(function(...) { ... }.call(<thisArg>, ...args))` IIFE
/// pattern used at the top of older CJS packages (lodash, underscore, and
/// every package that copies their UMD prelude). Pre-fix the inner
/// function expression lowers to a Closure, then `.call(thisArg, ...args)`
/// falls through to `js_native_call_method` on the closure handle which
/// doesn't recognize Function.prototype.call — the body never runs and
/// mutations to outer captures (e.g. `module.exports = _` inside the
/// wrap) are silently dropped, so `import _ from "lodash"` resolves to
/// `undefined` and `_.add` throws. Rewrite the AST shape directly to a
/// plain Call on the inner function expression, dropping the thisArg.
///
/// Conservative scope: only fires when the callee's receiver is a
/// FunctionExpression or ArrowExpression literal AND the inner function
/// does NOT reference `this` (`captures_this == false` after lowering).
/// Method dispatch like `obj.fn.call(otherObj, args)` keeps its existing
/// semantics — those go through the generic property-call path. We can
/// safely drop the thisArg because `captures_this == false` means the
/// body has no `this` references that depend on the bound value.
pub(super) fn try_iife_call_rewrite(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
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
                                return Ok(Some(Expr::Call {
                                    callee: Box::new(lowered_callee),
                                    args: rest_args,
                                    type_args: Vec::new(),
                                }));
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Issue #1722 — `<stdlibNamespace>.<method>.apply(thisArg, args)` /
/// `<stdlibNamespace>.<method>.call(thisArg, ...args)`.
///
/// Stdlib namespace methods (`path.join`, `fs.existsSync`, `os.platform`,
/// …) are dispatched by dedicated HIR lowerings keyed on the
/// `<namespace>.<method>(...)` *direct-call* shape — `path.join(a, b)`
/// folds to `Expr::PathJoin`, etc. The bare value `path.join` lowers to a
/// runtime namespace-property read that returns `undefined` for methods
/// not on the callable-export whitelist, so invoking it *indirectly* via
/// `Function.prototype.apply` / `.call` never reaches the native impl and
/// silently evaluates to `undefined` (Node returns the real result).
/// Surfaced by the #800 node-core radar (`test-path-join.js` uses
/// `path.join.apply(...)`).
///
/// Fix: when the callee is exactly `<ns>.<method>.{apply,call}` and `<ns>`
/// is a known native-module namespace binding (so `this` is irrelevant —
/// these are plain free functions), rewrite the AST to the equivalent
/// direct call and re-dispatch through `lower_call`, reusing every
/// existing per-method lowering. `thisArg` is dropped (correct for
/// namespace functions, which ignore `this`).
///
/// Conservative scope:
///   - `.call(thisArg, a, b, …)`         → `ns.method(a, b, …)`
///   - `.apply(thisArg)` / `.apply()`    → `ns.method()`
///   - `.apply(thisArg, [a, b, …])`      → `ns.method(a, b, …)` — only for
///     a clean array *literal* (no holes, no element spreads).
/// A non-literal apply-args array (a variable / call result) can't be
/// statically expanded into positional args, so it falls through
/// unchanged (the runtime spread path `ns.method(...arr)` is a separate
/// gap). The namespace-binding guard keeps this away from `obj.fn.call(…)`
/// method dispatch, function-literal IIFEs (`try_iife_call_rewrite`), and
/// `Object.prototype.<m>.call(…)` (`try_object_prototype_call`).
pub(super) fn try_native_module_method_apply_call(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    if has_spread {
        return Ok(None);
    }
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    // Outer member: `<inner>.apply` / `<inner>.call`.
    let ast::Expr::Member(outer) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(outer_prop) = &outer.prop else {
        return Ok(None);
    };
    let is_apply = match outer_prop.sym.as_ref() {
        "apply" => true,
        "call" => false,
        _ => return Ok(None),
    };
    // Inner member: `<ns>.<method>` where `<ns>` is a native-module
    // namespace ident and `<method>` is a plain (non-computed) name.
    let ast::Expr::Member(inner) = outer.obj.as_ref() else {
        return Ok(None);
    };
    if !matches!(&inner.prop, ast::MemberProp::Ident(_)) {
        return Ok(None);
    }
    let ast::Expr::Ident(ns_id) = inner.obj.as_ref() else {
        return Ok(None);
    };
    let ns_name = ns_id.sym.as_ref();
    // Namespace bindings register both an alias (require / `import * as`)
    // and a `(module, None)` native-module entry; named imports register
    // `(module, Some(symbol))` and must NOT match here.
    let is_module_ns = ctx.lookup_builtin_module_alias(ns_name).is_some()
        || matches!(ctx.lookup_native_module(ns_name), Some((_, None)));
    if !is_module_ns {
        return Ok(None);
    }

    // Build the synthesized direct-call argument list at the AST level.
    let synth_args: Vec<ast::ExprOrSpread> = if is_apply {
        match call.args.get(1) {
            // `.apply(thisArg)` / `.apply()` → no positional args.
            None => Vec::new(),
            Some(arr_arg) => match arr_arg.expr.as_ref() {
                ast::Expr::Array(arr) => {
                    // Only a clean literal (no holes, no element spreads)
                    // can be expanded into positional args statically.
                    let clean = arr
                        .elems
                        .iter()
                        .all(|e| matches!(e, Some(eos) if eos.spread.is_none()));
                    if !clean {
                        return Ok(None);
                    }
                    arr.elems.iter().filter_map(|e| e.clone()).collect()
                }
                // Non-literal args array — can't statically expand.
                _ => return Ok(None),
            },
        }
    } else {
        // `.call(thisArg, a, b, …)` → drop thisArg, keep the rest.
        call.args.iter().skip(1).cloned().collect()
    };

    // Synthesize `<ns>.<method>(synth_args)` and re-dispatch. The new
    // callee carries no `.apply`/`.call`, so this hook can't re-match it.
    let mut synth_call = call.clone();
    synth_call.callee = ast::Callee::Expr(Box::new(ast::Expr::Member(inner.clone())));
    synth_call.args = synth_args;
    Ok(Some(super::lower_call(ctx, &synth_call)?))
}

/// Issue #1777 — `<builtinProto>.<method>.{call,apply}(thisArg, …)` where the
/// receiver is a **builtin prototype** (`Array.prototype`, `String.prototype`,
/// …) or an array/string literal (`[].slice.call(…)`, `"".charAt.call(…)`).
///
/// This is the general case of #1722. A builtin prototype method read as a
/// *value* — `Array.prototype.slice`, `[].slice` — lowers to `undefined`, so
/// `Array.prototype.slice.call(arguments, 1)` / `[].slice.call(arguments)`
/// throws `TypeError: Cannot read properties of undefined (reading 'call')`.
/// The arguments-to-array idiom (`[].slice.call(arguments)`) and prototype
/// borrowing (`Array.prototype.map.call(arrayLike, fn)`) are pervasive in
/// real-world JS and in the node-core test harness (`mustCall`/`mustSucceed`),
/// the single largest runtime-fail cluster in the #800 radar.
///
/// Unlike the namespace case (#1722, where `this` is irrelevant), here the
/// first argument **is** the receiver: `Proto.method.call(thisArg, ...rest)`
/// is semantically `thisArg.method(...rest)`. We rewrite to that direct
/// member call and re-dispatch through `lower_call`, so the normal
/// type-directed method dispatch picks the right native impl based on
/// `thisArg`'s runtime value (perry materializes `arguments` as a real
/// array, so `arguments.slice(1)` dispatches to Array.prototype.slice — the
/// exact behavior the idiom wants).
///
/// Conservative scope:
///   - `.call(thisArg, a, b, …)`        → `thisArg.method(a, b, …)`
///   - `.apply(thisArg)` / `.apply()`   → `thisArg.method()`
///   - `.apply(thisArg, [a, b, …])`     → `thisArg.method(a, b, …)` — only a
///     clean array *literal* (no holes/spreads); a non-literal apply-args
///     array can't be statically expanded, so it falls through unchanged.
///
/// `Object.prototype.{toString,hasOwnProperty}.call(…)` is intentionally NOT
/// matched here — the post-args hooks `try_object_prototype_call` /
/// `try_object_has_own_call` rewrite those to dedicated runtime helpers
/// (`js_object_to_string` / `js_object_has_own`), so `Object.prototype` is
/// excluded from the receiver guard below to preserve that path. This hook
/// only ever fires on a shape that currently *throws* (the method value reads
/// `undefined`), so it cannot regress working code.
pub(super) fn try_builtin_prototype_method_apply_call(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    if has_spread {
        return Ok(None);
    }
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    // Outer member: `<inner>.apply` / `<inner>.call`.
    let ast::Expr::Member(outer) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(outer_prop) = &outer.prop else {
        return Ok(None);
    };
    let is_apply = match outer_prop.sym.as_ref() {
        "apply" => true,
        "call" => false,
        _ => return Ok(None),
    };
    // Inner member: `<recv>.<method>` where `<method>` is a plain name and
    // `<recv>` is a builtin-prototype reference or array/string literal.
    let ast::Expr::Member(inner) = outer.obj.as_ref() else {
        return Ok(None);
    };
    if !matches!(&inner.prop, ast::MemberProp::Ident(_)) {
        return Ok(None);
    }
    if !is_builtin_prototype_receiver(ctx, inner.obj.as_ref()) {
        return Ok(None);
    }

    // `.call`/`.apply` need at least the `thisArg` (the new receiver). A
    // spread in the `thisArg` slot can't be statically resolved to a receiver.
    let Some(this_arg) = call.args.first() else {
        return Ok(None);
    };
    if this_arg.spread.is_some() {
        return Ok(None);
    }
    let this_arg = this_arg.clone();

    // Build the synthesized positional argument list (everything after thisArg).
    let rest_args: Vec<ast::ExprOrSpread> = if is_apply {
        match call.args.get(1) {
            None => Vec::new(),
            Some(arr_arg) => match arr_arg.expr.as_ref() {
                ast::Expr::Array(arr) => {
                    let clean = arr
                        .elems
                        .iter()
                        .all(|e| matches!(e, Some(eos) if eos.spread.is_none()));
                    if !clean {
                        return Ok(None);
                    }
                    arr.elems.iter().filter_map(|e| e.clone()).collect()
                }
                // Non-literal apply-args array — can't statically expand.
                _ => return Ok(None),
            },
        }
    } else {
        call.args.iter().skip(1).cloned().collect()
    };

    // Synthesize `(thisArg).<method>(rest_args)`: keep the original method
    // name, swap the prototype/literal receiver for the real `thisArg`, drop
    // the `.apply`/`.call` wrapper, and re-dispatch.
    let mut synth_member = inner.clone();
    synth_member.obj = this_arg.expr.clone();
    let mut synth_call = call.clone();
    synth_call.callee = ast::Callee::Expr(Box::new(ast::Expr::Member(synth_member)));
    synth_call.args = rest_args;
    Ok(Some(super::lower_call(ctx, &synth_call)?))
}

/// True when `recv` is a builtin constructor's `.prototype` (and that
/// constructor name is not shadowed by a local/function binding) or an
/// array/string literal — the receiver shapes whose prototype-method *values*
/// currently lower to `undefined`. `Object` is deliberately excluded; see
/// `try_builtin_prototype_method_apply_call`.
fn is_builtin_prototype_receiver(ctx: &LoweringContext, recv: &ast::Expr) -> bool {
    match recv {
        // `Array.prototype` / `String.prototype` / … (not `Object`).
        ast::Expr::Member(m) => {
            let ast::MemberProp::Ident(p) = &m.prop else {
                return false;
            };
            if p.sym.as_ref() != "prototype" {
                return false;
            }
            let ast::Expr::Ident(base) = m.obj.as_ref() else {
                return false;
            };
            let name = base.sym.as_ref();
            matches!(name, "Array" | "String" | "Number" | "Boolean" | "Function")
                && ctx.lookup_local(name).is_none()
                && ctx.lookup_func(name).is_none()
        }
        // `[].slice.call(…)` / `[1,2,3].map.call(…)`.
        ast::Expr::Array(_) => true,
        // `"".charAt.call(…)`.
        ast::Expr::Lit(ast::Lit::Str(_)) => true,
        _ => false,
    }
}

/// Followup to #957 / PR #959 — `Function('return this')()`.
///
/// Every CJS/UMD-shaped library (lodash, underscore, Effect, …)
/// computes its "give me whatever the host calls `globalThis` here"
/// root with the double-call idiom:
///   var root = freeGlobal || freeSelf || Function('return this')();
/// Pre-fix the bare `Function` ident lowers to `Expr::GlobalGet(0)`
/// (the no-resolution sentinel), then the inner `Function('return this')`
/// lowers to `Call { callee: GlobalGet(0), args: [String("return this")] }`
/// which codegen treats as "call a non-callable" — the outer `()` then
/// tries to call the returned value and the closure validator throws
/// `TypeError: value is not a function` at module init, leaving the
/// import resolved to undefined.
///
/// PR #959 closed the sibling `.call(this)` IIFE bug and called this
/// one out in its commit message ("the next runtime gap"); fix here.
/// Match the full two-call shape at the AST level (the inner `Function`
/// ident still carries its name, so we can verify it really is the
/// builtin) and fold to `Expr::GlobalThisExpr`, which lowers to the
/// runtime's `js_get_global_this()` singleton — the same object
/// `globalThis[X] = V` already writes to (see #611).
///
/// Conservative: requires the LITERAL "return this" (with optional
/// semicolon / whitespace) AND the outer Call must have no args. Any
/// other `Function(...)` shape (e.g. dynamic body, real `new Function`)
/// falls through to the existing GlobalGet(0) path; arbitrary
/// `new Function(body)` is still not supported (an architectural
/// change — issue #960 / future work).
pub(super) fn try_function_return_this(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Option<Expr> {
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
                                        return Some(Expr::GlobalThisExpr);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Followup to #957 / PR #959 — `RegExp(<args>)` as a bare function call.
///
/// lodash 4 builds half a dozen of these at module init:
///   var reEscapedHtml = /&(?:amp|lt|gt|quot|#39);/g,
///       reHasEscapedHtml = RegExp(reEscapedHtml.source);
/// The bare `RegExp` ident lowers to `Expr::GlobalGet(0)` (no resolved
/// value), so the function-call form dispatches through
/// `js_closure_call1` with a null closure handle and throws
/// `TypeError: value is not a function`. Fold here to
/// `Expr::RegExpDynamic` which lowers to the same `js_regexp_new`
/// runtime entrypoint the static `/foo/g` arm uses.
///
/// Conservative: only `RegExp(pattern)` and `RegExp(pattern, flags)`
/// with no spread. Any local/import named `RegExp` shadows the
/// builtin and falls through to its normal dispatch.
pub(super) fn try_bare_regexp_call(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
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
                    return Ok(Some(Expr::RegExpDynamic {
                        pattern: Box::new(pattern),
                        flags,
                    }));
                }
            }
        }
    }
    Ok(None)
}
