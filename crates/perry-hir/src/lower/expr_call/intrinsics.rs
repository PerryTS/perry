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
