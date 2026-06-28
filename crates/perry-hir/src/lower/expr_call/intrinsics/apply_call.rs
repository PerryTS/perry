use super::*;

use anyhow::Result;
use perry_types::Type;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

use super::super::super::{is_known_namespace_static_function, lower_expr, LoweringContext};

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
pub(crate) fn try_iife_call_rewrite(
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
                                is_arrow,
                                body,
                                ..
                            } = &lowered_callee
                            {
                                // Dropping the `.call` thisArg is only sound
                                // when the body never observes `this`. An arrow
                                // (captures_this == false) has no own `this`. A
                                // regular function expression ALSO reports
                                // captures_this == false (it has its own dynamic
                                // `this`, not a captured one — expr_function.rs),
                                // so its body may still read `this`; folding
                                // `(function(){ "use strict"; return this })
                                // .call(null)` to `fn()` would lose the bound
                                // receiver (the body would see undefined, not
                                // null). Require a this-free body there. #3576.
                                let drops_this_safely =
                                    *is_arrow || !crate::analysis::closure_uses_this(body);
                                if drops_this_safely {
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
                                        byte_offset: 0,
                                    }));
                                }
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
pub(crate) fn try_native_module_method_apply_call(
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

    // #4973: `http.Server.call(this, handler)` — the util.inherits-era
    // subclass pattern. For native CLASS exports the thisArg is NOT
    // irrelevant: Node initializes `this` as the server. Route to the
    // construct-with-this extern (which constructs the server AND aliases
    // `this` → handle) instead of dropping the receiver below.
    if !is_apply && !call.args.is_empty() {
        let module = ctx
            .lookup_builtin_module_alias(ns_name)
            .map(str::to_string)
            .or_else(|| {
                ctx.lookup_native_module(ns_name)
                    .map(|(m, _)| m.to_string())
            });
        if let (Some(module), ast::MemberProp::Ident(method_ident)) = (module, &inner.prop) {
            let normalized = module.strip_prefix("node:").unwrap_or(&module);
            if matches!(normalized, "http" | "https") && method_ident.sym.as_ref() == "Server" {
                let mut lowered: Vec<Expr> = call
                    .args
                    .iter()
                    .map(|a| lower_expr(ctx, &a.expr))
                    .collect::<Result<Vec<_>>>()?;
                // (this, options?, listener?) — fixed 3-arg extern ABI.
                lowered.resize(3, Expr::Undefined);
                let extern_name = if normalized == "https" {
                    "js_https_server_construct_with_this"
                } else {
                    "js_http_server_construct_with_this"
                };
                return Ok(Some(Expr::Call {
                    callee: Box::new(Expr::ExternFuncRef {
                        name: extern_name.to_string(),
                        param_types: Vec::new(),
                        return_type: Type::Any,
                    }),
                    args: lowered,
                    type_args: Vec::new(),
                    byte_offset: 0,
                }));
            }
        }
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
    Ok(Some(super::super::lower_call(ctx, &synth_call)?))
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
/// #4101: is `expr` the member expression `Function.prototype`? Used to keep
/// `Function.prototype.toString.call(x)` from folding into `x.toString()` so
/// the runtime brand check (throw on non-function `this`) still fires.
fn is_function_prototype_member(expr: &ast::Expr) -> bool {
    let ast::Expr::Member(member) = expr else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return false;
    };
    if prop.sym.as_ref() != "prototype" {
        return false;
    }
    matches!(member.obj.as_ref(), ast::Expr::Ident(id) if id.sym.as_ref() == "Function")
}

/// #4100: true when `recv.<method>` is a primitive-wrapper prototype method that
/// performs a spec `this` brand check at runtime (throws `TypeError` on an
/// incompatible receiver). Folding `<recv>.<method>.call(x)` into `x.<method>()`
/// would route through the lenient codegen fast-path / `Object.prototype`
/// fallback (returns `"[object Object]"`, no throw). Keeping it reflective lets
/// the installed brand-check thunk run. `Number.prototype.toFixed`/
/// `toExponential`/`toPrecision` are deliberately excluded — the fold is the
/// *correct* path for those (their reflective dispatch over-throws on a valid
/// receiver), and only the brand-checked `valueOf`/`toString`/`toLocaleString`
/// methods are affected. Symbol/BigInt have no codegen fold path, so they need
/// no guard here.
fn is_primitive_wrapper_brand_method(recv: &ast::Expr, method: &str) -> bool {
    let ast::Expr::Member(member) = recv else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return false;
    };
    if prop.sym.as_ref() != "prototype" {
        return false;
    }
    let ast::Expr::Ident(base) = member.obj.as_ref() else {
        return false;
    };
    match base.sym.as_ref() {
        "Number" => matches!(method, "valueOf" | "toString" | "toLocaleString"),
        "Boolean" => matches!(method, "valueOf" | "toString"),
        _ => false,
    }
}

/// True when `recv.<method>` is a `String.prototype` generic-`this` method backed
/// by a real reflective runtime thunk (RequireObjectCoercible + ToString(this)).
/// Folding `String.prototype.charAt.call(x)` into `x.charAt()` would re-dispatch
/// `charAt` *by name on `x`'s own type* — a boolean/number/object has no
/// `charAt`, so it throws `(boolean).charAt is not a function`. Keeping it
/// reflective lets the installed thunk coerce `this` to a string. Only the
/// `String.prototype.<m>` receiver shape is guarded (string-literal receivers
/// like `"".charAt.call(x)` are vanishingly rare); kept in lock-step with
/// `string_proto_thunks::install_string_proto_methods`.
fn is_string_prototype_generic_method(recv: &ast::Expr, method: &str) -> bool {
    let ast::Expr::Member(member) = recv else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return false;
    };
    if prop.sym.as_ref() != "prototype" {
        return false;
    }
    let ast::Expr::Ident(base) = member.obj.as_ref() else {
        return false;
    };
    base.sym.as_ref() == "String"
        && matches!(
            method,
            // Char-access (dedicated thunks) + every coercing method installed
            // as the generic `string_proto_generic_thunk`. Keep in lock-step with
            // `string_proto_thunks::GENERIC_STRING_PROTO_METHODS`. Excluded:
            // `toString`/`valueOf` (brand-checked, not ToString-coercing).
            // Annex B §B.2.2 HTML wrappers.
            "anchor"
                | "big"
                | "blink"
                | "bold"
                | "fixed"
                | "fontcolor"
                | "fontsize"
                | "italics"
                | "link"
                | "small"
                | "strike"
                | "sub"
                | "sup"
                | "at"
                | "charAt"
                | "charCodeAt"
                | "codePointAt"
                | "concat"
                | "endsWith"
                | "includes"
                | "indexOf"
                | "isWellFormed"
                | "lastIndexOf"
                | "localeCompare"
                | "match"
                | "matchAll"
                | "normalize"
                | "padEnd"
                | "padStart"
                | "repeat"
                | "replace"
                | "replaceAll"
                | "search"
                | "slice"
                | "split"
                | "startsWith"
                | "substr"
                | "substring"
                | "toLocaleLowerCase"
                | "toLocaleUpperCase"
                | "toLowerCase"
                | "toUpperCase"
                | "toWellFormed"
                | "trim"
                | "trimEnd"
                | "trimStart"
        )
}

pub(crate) fn try_builtin_prototype_method_apply_call(
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
    // Resolve the builtin prototype method name from the thing we're calling
    // `.call`/`.apply` ON. Two shapes are supported:
    //   * `<recv>.<method>.call(...)` — a member whose object is a builtin
    //     prototype receiver (array/string literal or `<Ctor>.prototype`).
    //   * `local.call(...)` — an identifier previously bound to such a method
    //     ref, e.g. `const m = [].map` (#3144).
    // `method_prop` is the `IdentName` for the resolved method; we reuse it as
    // the synthesized member's `.prop`.
    let method_prop: ast::IdentName = match outer.obj.as_ref() {
        ast::Expr::Member(inner) => {
            let ast::MemberProp::Ident(method_ident) = &inner.prop else {
                return Ok(None);
            };
            if !is_builtin_prototype_receiver(ctx, inner.obj.as_ref()) {
                return Ok(None);
            }
            // #4101: keep `Function.prototype.toString.call(x)` reflective so
            // the runtime thunk runs its brand check (throw a TypeError on a
            // non-function `this`) and reconstructs source. Folding it to
            // `x.toString()` would erase the Function brand and route through
            // the lenient universal `toString` (returns "[object Object]", no
            // throw). `Object.prototype.toString.call(x)` is unaffected — it
            // keeps folding (ramda relies on it).
            if method_ident.sym.as_ref() == "toString"
                && is_function_prototype_member(inner.obj.as_ref())
            {
                return Ok(None);
            }
            // #4100: keep `Number.prototype.valueOf.call(x)` /
            // `Boolean.prototype.toString.call(x)` reflective so the installed
            // brand-check thunk runs (throws a `TypeError` on an incompatible
            // `this`). Folding to `x.<method>()` routes through the lenient
            // `Object.prototype` fallback (`"[object Object]"`, no throw).
            if is_primitive_wrapper_brand_method(inner.obj.as_ref(), method_ident.sym.as_ref()) {
                return Ok(None);
            }
            // Generic-`this` String.prototype char-access methods must stay
            // reflective so the runtime thunk coerces `this` to a string (see
            // `is_string_prototype_generic_method`). Folding to `x.<m>()` would
            // dispatch on `x`'s own type and throw.
            if is_string_prototype_generic_method(inner.obj.as_ref(), method_ident.sym.as_ref()) {
                return Ok(None);
            }
            method_ident.clone()
        }
        ast::Expr::Ident(id) => match ctx.builtin_proto_method_locals.get(id.sym.as_ref()) {
            Some(name) => {
                // Build the method `.prop` IdentName by cloning the outer
                // `.call`/`.apply` IdentName and overwriting its `sym`
                // (avoids needing a synthetic span).
                let mut prop = outer_prop.clone();
                prop.sym = name.as_str().into();
                prop
            }
            // Not a tracked builtin-method local: leave unrelated
            // `someFn.call(...)` untouched.
            None => return Ok(None),
        },
        _ => return Ok(None),
    };

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

    // `Array.prototype.<m>.call(arrayLike, ...)` — when `<m>` is a supported
    // generic Array method, this is an explicit, unambiguous request to run
    // the Array algorithm on a *generic array-like* receiver (a plain object
    // with `length` + indexed keys; ECMA-262 §23.1.3). The default synthesized
    // `(thisArg).<m>(...)` member call below only routes to the Array runtime
    // helper when the receiver is statically array-typed; for an Any/object
    // receiver it lowers to a dynamic method lookup that finds no `map`/`reduce`
    // field and throws "value is not a function". Build the dedicated
    // `Expr::Array*` variant directly so the receiver flows to `js_array_*`
    // regardless of its static type — the runtime materializes the array-like
    // (see `normalize_array_receiver`). Most handled methods are
    // read-only/returning; the mutators `fill` / `copyWithin` / `reverse` use
    // dedicated generic helpers because they must write back to the original
    // receiver rather than a materialized clone. Unsupported mutators fall
    // through to the member call below (unchanged behavior).
    if let Some(folded) =
        try_arraylike_receiver_method(ctx, method_prop.sym.as_ref(), &this_arg.expr, &rest_args)?
    {
        return Ok(Some(folded));
    }

    // Synthesize `(thisArg).<method>(rest_args)`: use the resolved method
    // name, make the receiver the real `thisArg`, drop the `.apply`/`.call`
    // wrapper, and re-dispatch.
    let synth_member = ast::MemberExpr {
        span: outer.span,
        obj: this_arg.expr.clone(),
        prop: ast::MemberProp::Ident(method_prop),
    };
    let mut synth_call = call.clone();
    synth_call.callee = ast::Callee::Expr(Box::new(ast::Expr::Member(synth_member)));
    synth_call.args = rest_args;
    Ok(Some(super::super::lower_call(ctx, &synth_call)?))
}

/// Build a dedicated `Expr::Array*` HIR variant for `Array.prototype.<m>.call`
/// / `.apply` on a *generic array-like* receiver, bypassing the receiver-type
/// gate that the normal member-call fast path applies. `receiver` is the
/// `thisArg`; `rest_args` are the post-`thisArg` positional arguments (already
/// expanded from the `.apply` array if applicable).
///
/// Returns `Some(expr)` for a supported read-only/returning method, plus
/// dedicated generic `fill` / `copyWithin` / `reverse` mutator paths, or `None`
/// for other mutators / unsupported methods (caller falls back to the
/// synthesized member call). The read-only set mirrors the runtime methods that
/// route through `normalize_array_receiver`.
fn try_arraylike_receiver_method(
    ctx: &mut LoweringContext,
    method: &str,
    receiver: &ast::Expr,
    rest_args: &[ast::ExprOrSpread],
) -> Result<Option<Expr>> {
    // Any spread in the positional args defeats static argument expansion.
    if rest_args.iter().any(|a| a.spread.is_some()) {
        return Ok(None);
    }
    // `fill` mutates in place but is generic over an array-like receiver; route
    // to the dedicated generic mutator helper (`js_array_fill_generic`), which
    // writes back to the original receiver rather than a materialized clone.
    if method == "fill" {
        let object = Box::new(lower_expr(ctx, receiver)?);
        let mut args = Vec::with_capacity(rest_args.len());
        for a in rest_args {
            args.push(lower_expr(ctx, &a.expr)?);
        }
        return Ok(Some(Expr::NativeMethodCall {
            module: "array".to_string(),
            class_name: None,
            object: Some(object),
            method: "fill_generic".to_string(),
            args,
        }));
    }
    // `copyWithin` mutates in place but is generic over an array-like receiver;
    // keep the dedicated value-receiver lowering.
    if method == "copyWithin" {
        let receiver = Box::new(lower_expr(ctx, receiver)?);
        let arg = |ctx: &mut LoweringContext, i: usize| -> Result<Option<Box<Expr>>> {
            match rest_args.get(i) {
                Some(a) => Ok(Some(Box::new(lower_expr(ctx, &a.expr)?))),
                None => Ok(None),
            }
        };
        let target = match arg(ctx, 0)? {
            Some(t) => t,
            None => Box::new(Expr::Undefined),
        };
        let start = match arg(ctx, 1)? {
            Some(s) => s,
            None => Box::new(Expr::Undefined),
        };
        let end = arg(ctx, 2)?;
        return Ok(Some(Expr::ArrayCopyWithinValue {
            receiver,
            target,
            start,
            end,
        }));
    }
    // `reverse` mutates in place and returns the same receiver; route to the
    // dedicated `js_array_reverse_value` helper (no positional args allowed).
    if method == "reverse" {
        if !rest_args.is_empty() {
            return Ok(None);
        }
        return Ok(Some(Expr::ArrayReverseValue {
            receiver: Box::new(lower_expr(ctx, receiver)?),
        }));
    }
    // The read-only/returning methods the runtime generic engine implements
    // directly over an array-like receiver (`js_arraylike_*`, #4597). Unlike
    // the old materialize-then-call fold, these preserve the original receiver
    // identity (passed as the callback's 3rd argument) and read live via
    // `Get(O, k)` / `HasProperty(O, k)` — so they also work on plain objects,
    // functions (`obj.length`/expando indices), strings, and bare primitives,
    // and pass the receiver-identity test262 cases that a materialised clone
    // fails. The hot `arr.<m>(…)` member-call paths are untouched — only the
    // explicit `.call`/`.apply`/bound-local forms route here.
    let generic = matches!(
        method,
        "map"
            | "filter"
            | "forEach"
            | "find"
            | "findIndex"
            | "findLast"
            | "findLastIndex"
            | "some"
            | "every"
            | "reduce"
            | "reduceRight"
            | "indexOf"
            | "lastIndexOf"
            | "includes"
            | "slice"
            | "at"
            | "join"
            // Generic mutators with dedicated runtime engines (#4597
            // extension): `sort` sorts the receiver in place via
            // Get/HasProperty/Set/Delete; `splice`/`concat` apply the spec
            // algorithms over the array-like (test262 sort/call-with-primitive,
            // splice/set_length_no_args, concat/call-with-boolean).
            | "sort"
            | "splice"
            | "concat"
            // Generic stack/queue mutators over a value receiver: route the
            // `.call`/`.apply` form to the spec-generic engine so a primitive
            // (`Array.prototype.pop.call(true)`) returns `undefined` instead of
            // a synthesized `(true).pop()` member call throwing "not a function"
            // (test262 pop|shift|unshift/call-with-boolean), and a plain
            // array-like object mutates via live Get/Set/Delete.
            | "pop"
            | "shift"
            | "push"
            | "unshift"
    );
    if generic {
        // Receiver lowers before the positional args, matching source order.
        let receiver = Box::new(lower_expr(ctx, receiver)?);
        let mut args = Vec::with_capacity(rest_args.len());
        for a in rest_args {
            args.push(lower_expr(ctx, &a.expr)?);
        }
        return Ok(Some(Expr::ArrayLikeMethod {
            method: method.to_string(),
            receiver,
            args,
        }));
    }

    // `flatMap` has no generic runtime entry yet; keep the holey
    // materialize-then-call behavior. `Expr::ArrayFromArrayLikeHoley` keeps
    // absent indexed keys as holes (vs `Array.from({ length })` creating
    // present undefined slots), so the flatMap callback doesn't visit holes.
    // Everything else (mutators, flat, etc.) bails BEFORE lowering the receiver
    // so unrelated shapes keep the existing member-call behavior.
    if method != "flatMap" {
        return Ok(None);
    }
    let Some(cb) = rest_args.first() else {
        return Ok(None);
    };
    let array = Box::new(Expr::ArrayFromArrayLikeHoley(Box::new(lower_expr(
        ctx, receiver,
    )?)));
    let callback = Box::new(lower_expr(ctx, &cb.expr)?);
    Ok(Some(Expr::ArrayFlatMap { array, callback }))
}

/// #3144: if `init` is a value-read of a builtin prototype method whose
/// receiver passes [`is_builtin_prototype_receiver`] (e.g. `[].map`,
/// `"".slice`, `Array.prototype.filter`), return the method name. Used to
/// track locals like `const m = [].map` so a later `m.call(arr, ...)` /
/// `m.apply(arr, [...])` can be rewritten to a direct call.
pub(crate) fn as_builtin_proto_method_ref(
    ctx: &LoweringContext,
    init: &ast::Expr,
) -> Option<String> {
    let ast::Expr::Member(member) = init else {
        return None;
    };
    let ast::MemberProp::Ident(method) = &member.prop else {
        return None;
    };
    if !is_builtin_prototype_receiver(ctx, &member.obj) {
        return None;
    }
    // #4100: don't track `const v = Number.prototype.valueOf` for the fold —
    // a later `v.call(x)` must stay reflective so the brand-check thunk runs
    // (see `is_primitive_wrapper_brand_method`). Untracked, the value read goes
    // through the reflective dispatch, which throws correctly.
    if is_primitive_wrapper_brand_method(&member.obj, method.sym.as_ref()) {
        return None;
    }
    // Keep `const m = String.prototype.charAt; m.call(x)` reflective too — the
    // thunk must coerce `this` (see `is_string_prototype_generic_method`).
    if is_string_prototype_generic_method(&member.obj, method.sym.as_ref()) {
        return None;
    }
    // For a `<Ctor>.prototype` receiver, any method ident is accepted (mirrors
    // the existing `.call`/`.apply` rewrite, which doesn't gate on the method
    // name). For an array/string literal receiver, gate on the known
    // array/string prototype-method predicates so we don't track unrelated
    // member reads.
    let is_proto_base = matches!(&*member.obj, ast::Expr::Member(_));
    let known = crate::lower::array_fold::is_known_array_prototype_method(method.sym.as_ref())
        || crate::lower::array_fold::is_known_string_prototype_method(method.sym.as_ref());
    if is_proto_base || known {
        Some(method.sym.to_string())
    } else {
        None
    }
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
            // Number/Boolean primitive methods need to stay reflective so
            // their prototype thunks brand-check `this` (#4100).
            matches!(name, "Array" | "String" | "Function")
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
