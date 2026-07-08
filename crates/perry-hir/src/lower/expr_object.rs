//! Object literal lowering: `ast::Expr::Object`.
//!
//! Tier 2.3 follow-up (v0.5.338) — extracts the 477-LOC `Object` arm
//! from `lower_expr` into a focused module. This is the largest single
//! arm extraction so far. The lowered shape depends on whether the
//! literal is a "closed shape" (no spreads, all fixed string keys) —
//! such literals lower to `new __AnonShape_N()` so downstream property
//! access hits the codegen direct-GEP fast path. Open-shape literals
//! (spreads, computed keys, getters/setters) fall through to a generic
//! `Object` / `ObjectSpread` HIR node that the runtime resolves
//! dynamically.
//!
//! Pattern matches `expr_misc.rs` and `expr_function.rs`: free
//! `pub(super) fn` helpers, recursion through `super::lower_expr`.

use anyhow::Result;
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::analysis::{
    closure_uses_this, collect_assigned_locals_stmt, collect_local_refs_stmt, uses_this_stmt,
};
use crate::ir::{EnumValue, Expr, Function, Param, Stmt};
use crate::lower_decl::{
    append_synthetic_arguments_param, body_uses_arguments, lower_block_stmt,
    lower_fn_body_block_stmt,
};
use crate::lower_patterns::{
    generate_param_destructuring_stmts, get_param_default, get_pat_name, is_destructuring_pattern,
    is_rest_param,
};
use crate::lower_types::{extract_param_type_with_ctx, extract_ts_type_with_ctx};

use super::{lower_expr, LoweringContext};

/// Lower an object-literal property *value* with NamedEvaluation: when the
/// value is an anonymous function / arrow / class expression and the property
/// has a static string key, the resulting function's `.name` is the key
/// (spec PropertyDefinitionEvaluation → SetFunctionName). Mirrors the
/// assignment / destructuring-default paths via `ctx.assignment_inferred_name`;
/// a *named* function expression ignores the hint and keeps its own name.
fn lower_prop_value_named(ctx: &mut LoweringContext, key: &str, value: &ast::Expr) -> Result<Expr> {
    if crate::lower::expr_assign::rhs_accepts_assignment_name(value) {
        let old = ctx.assignment_inferred_name.replace(key.to_string());
        let lowered = lower_expr(ctx, value);
        ctx.assignment_inferred_name = old;
        lowered
    } else {
        lower_expr(ctx, value)
    }
}

fn is_fetch_global_value_name(name: &str) -> bool {
    matches!(
        name,
        "fetch" | "Blob" | "File" | "FormData" | "Headers" | "Request" | "Response"
    )
}

fn builtin_global_value_expr(ctx: &mut LoweringContext, name: &str) -> Option<Expr> {
    if !crate::analysis::is_builtin_global_value_name(name) {
        return None;
    }
    if is_fetch_global_value_name(name) {
        ctx.uses_fetch = true;
    }
    Some(Expr::PropertyGet {
        object: Box::new(Expr::GlobalGet(0)),
        property: name.to_string(),
    })
}

/// Resolution of an object-literal `KeyValue` property key.
enum KeyResolution {
    /// A statically-known string key (`x:`, `"x":`, `1:`, `[Enum.M]:`,
    /// `["lit"]:`, `[1]:`).
    Static(String),
    /// A computed key whose value is only known at runtime
    /// (`[symLocal]:`, `[TypeId]:`, `[expr]:`).
    Dynamic(Expr),
    /// Key shape we don't model — skip the property.
    Skip,
}

/// Resolve a `KeyValue` property name to a static string, a dynamic key
/// expression, or skip. Extracted (verbatim) from the legacy `lower_object`
/// loop so the spread path can reuse the identical resolution rules.
fn resolve_keyvalue_key(ctx: &mut LoweringContext, key: &ast::PropName) -> KeyResolution {
    match key {
        ast::PropName::Ident(ident) => KeyResolution::Static(ident.sym.to_string()),
        ast::PropName::Str(s) => KeyResolution::Static(s.value.as_str().unwrap_or("").to_string()),
        ast::PropName::Num(n) => KeyResolution::Static(super::number_to_js_key(n.value)),
        ast::PropName::Computed(computed) => {
            // Handle computed property keys like [ChainName.ETHEREUM]
            // Try to resolve enum member access to string keys first.
            match computed.expr.as_ref() {
                ast::Expr::Member(member) => {
                    if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) =
                        (member.obj.as_ref(), &member.prop)
                    {
                        let enum_name = obj.sym.to_string();
                        let member_name = prop.sym.to_string();
                        if let Some(value) = ctx.lookup_enum_member(&enum_name, &member_name) {
                            match value {
                                EnumValue::String(s) => KeyResolution::Static(s.clone()),
                                EnumValue::Number(n) => KeyResolution::Static(n.to_string()),
                            }
                        } else {
                            // Non-enum member access: lower as a dynamic expression.
                            match lower_expr(ctx, computed.expr.as_ref()) {
                                Ok(e) => KeyResolution::Dynamic(e),
                                Err(_) => KeyResolution::Skip,
                            }
                        }
                    } else {
                        match lower_expr(ctx, computed.expr.as_ref()) {
                            Ok(e) => KeyResolution::Dynamic(e),
                            Err(_) => KeyResolution::Skip,
                        }
                    }
                }
                // Even literal computed keys must flow through ToPropertyKey:
                // `[1e55]` is `"1e+55"` in JS, not Rust's default decimal
                // spelling, and symbols must survive without stringification.
                _ => match lower_expr(ctx, computed.expr.as_ref()) {
                    Ok(e) => KeyResolution::Dynamic(e),
                    Err(_) => KeyResolution::Skip,
                },
            }
        }
        _ => KeyResolution::Skip,
    }
}

fn is_noncomputed_proto_key(key: &ast::PropName) -> bool {
    match key {
        ast::PropName::Ident(ident) => ident.sym == *"__proto__",
        ast::PropName::Str(s) => s.value.as_str().unwrap_or("") == "__proto__",
        _ => false,
    }
}

/// Resolution of an object-literal `Method` property key.
enum MethodKeyKind {
    Static(String),
    Computed(Expr),
}

/// Lower an object-literal method (`m() {}`, `[Symbol.x]() {}`) into its
/// value expression (a `FuncRef` for capture-free non-`this` methods, else a
/// `Closure`) plus its key and whether the body uses `this`.
///
/// Returns `Ok(None)` for key shapes the legacy loop skipped (a `Num`/other
/// non-computed PropName, or a computed key that failed to lower). Extracted
/// verbatim from the legacy `lower_object` loop so both the spread and
/// non-spread paths share one implementation (no behavioral drift).
fn lower_method_prop(
    ctx: &mut LoweringContext,
    method: &ast::MethodProp,
) -> Result<Option<(MethodKeyKind, Expr, bool)>> {
    let method_key = match &method.key {
        ast::PropName::Ident(ident) => MethodKeyKind::Static(ident.sym.to_string()),
        ast::PropName::Str(s) => MethodKeyKind::Static(s.value.as_str().unwrap_or("").to_string()),
        // A numeric-keyed method shorthand (`{ 900(e,t,r){} }`) — its key is the
        // stringified number per spec (`{900(){}}` has own key "900"). Without
        // this arm it fell through to `_ => Ok(None)` and the method was DROPPED
        // entirely (invisible to `obj[900]` and `Object.keys`), so a webpack
        // bundle's numeric-keyed module-factory table (`{900(e,t,r){…}}`,
        // `t[900].call(…)`) lost its factories — Next.js's
        // `app-page-turbo.runtime.prod.js` entry `a(900)`. Mirrors the KeyValue
        // and closed-shape numeric-key handling (`number_to_js_key`).
        ast::PropName::Num(n) => MethodKeyKind::Static(super::number_to_js_key(n.value)),
        ast::PropName::Computed(computed) => match lower_expr(ctx, computed.expr.as_ref()) {
            Ok(e) => MethodKeyKind::Computed(e),
            Err(_) => return Ok(None),
        },
        _ => return Ok(None),
    };
    let key_label: String = match &method_key {
        MethodKeyKind::Static(s) => s.clone(),
        MethodKeyKind::Computed(_) => format!("computed_{}", ctx.next_func_id),
    };
    let key: String = key_label.clone();
    let func_id = ctx.fresh_func();
    // #2076: an object-literal shorthand method's `fn.name` is the
    // property key per spec (`{m(){}}.m.name === "m"`). The synthetic
    // function name we mint below (`__obj_method_<key>_<id>`) starts
    // with `_`, so the artifacts.rs registration loop would skip it
    // without this override.
    if let MethodKeyKind::Static(s) = &method_key {
        if !s.is_empty() {
            ctx.closure_display_names.insert(func_id, s.clone());
        }
    }
    // Use a unique synthetic name to avoid collisions
    let func_name = format!("__obj_method_{}_{}", key, func_id);

    // Snapshot outer locals for capture analysis
    let outer_locals: Vec<(String, LocalId)> = ctx
        .locals
        .iter()
        .map(|(name, id, _)| (name.clone(), *id))
        .collect();

    let scope_mark = ctx.enter_scope();
    let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
    ctx.in_nonarrow_fn = true;
    // Object-literal methods are NOT implicitly strict (unlike class bodies):
    // strictness is inherited from the enclosing code or introduced by the
    // method's own directive prologue. A blanket `true` here made every
    // direct `eval` inside a sloppy object method apply strict-mode early
    // errors (test262 language/eval-code direct/*meth*-declare-arguments,
    // super-prop-method).
    let method_strict = ctx.current_strict_mode()
        || method
            .function
            .body
            .as_ref()
            .map(|b| crate::lower_decl::body_has_use_strict(&b.stmts))
            .unwrap_or(false);
    ctx.enter_strict_mode(method_strict);
    let mut params = Vec::new();
    let mut default_param_pats: Vec<ast::Pat> = Vec::new();
    // Destructuring method params (`method([x, y]) {}` / `m({a, b}) {}`) need
    // extraction statements prepended to the body, mirroring `fn_decl` /
    // `expr_function`. Without this the bound names (`x`, `y`, `a`, `b`) never
    // get a `Let`, so the body throws `ReferenceError: identifier is not
    // defined` — every `dstr/{,gen-,async-gen-}meth-*` test262 case.
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in method.function.params.iter() {
        let param_name = get_pat_name(&param.pat)?;
        // TypeScript's `this: T` is a TYPE-only marker (SWC emits it as a
        // regular `Param { pat: Ident("this") }`), so skip it — it must not
        // become a runtime parameter. Mirrors the `fn_decl.rs` /
        // `expr_function.rs` sites. Without this skip, an object-literal
        // method `m(this: T, fin) {}` is lowered as a 2-arg function, so
        // when it dispatches through the `Object.create(proto)` chain (which
        // binds `this` separately) the real `fin` arg lands in the `this`
        // slot and the declared `fin` reads undefined (effect's
        // `ScopeImplProto.addFinalizer(this, fin)` Layer/Scope blocker, #321).
        if param_name == "this" {
            continue;
        }
        let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
        let param_id = ctx.define_local(param_name.clone(), param_type.clone());
        ctx.shadow_native_instance_if_present(&param_name);
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_type,
            default: None,
            decorators: Vec::new(),
            is_rest: is_rest_param(&param.pat),
            arguments_object: None,
        });
        default_param_pats.push(param.pat.clone());
        // Unwrap `Pat::Assign` (`[x, y] = [1, 2]`) to the inner array/object
        // pattern — the default value is applied separately via
        // `build_default_param_stmts`, and the destructuring extracts from the
        // (possibly defaulted) param. Mirrors `lower_decl/class_members.rs`.
        let inner_pat = if let ast::Pat::Assign(assign) = &param.pat {
            assign.left.as_ref()
        } else {
            &param.pat
        };
        if is_destructuring_pattern(inner_pat) {
            destructuring_params.push((param_id, inner_pat.clone()));
        }
    }
    for (param, pat) in params.iter_mut().zip(default_param_pats.iter()) {
        param.default = get_param_default(ctx, pat)?;
    }
    // Generate extraction `Let`s BEFORE lowering the body so the destructured
    // bindings are in scope when the body references them.
    let mut destructuring_stmts = Vec::new();
    for (param_id, pat) in &destructuring_params {
        let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
        destructuring_stmts.extend(stmts);
    }
    let destructuring_prologue_len = destructuring_stmts.len();
    let return_type = method
        .function
        .return_type
        .as_ref()
        .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
        .unwrap_or(Type::Any);

    // #321 / #64 / #65: synthesize legacy `arguments` for object-literal methods
    // whose body references it. Without this, effect's Pipeable prototype
    // methods (`pipe() { return pipeArguments(this, arguments) }` on
    // `TypeMatcherProto`/`ValueMatcherProto` and friends) see an unbound
    // `arguments` identifier, and `.pipe(...)` quietly drops all of its
    // operands. Mirrors the synthesis in `class_members.rs` / `fn_decl.rs` /
    // `expr_function.rs` — the only call site that was missing this hook.
    let user_has_arguments_param = method
        .function
        .params
        .iter()
        .any(|p| get_pat_name(&p.pat).ok().as_deref() == Some("arguments"));
    let needs_arguments_synth = !user_has_arguments_param
        && method
            .function
            .body
            .as_ref()
            .map(|b| body_uses_arguments(&b.stmts))
            .unwrap_or(false);
    if needs_arguments_synth {
        append_synthetic_arguments_param(ctx, &mut params, true, false, true, Vec::new());
    }

    let mut body = if let Some(ref block) = method.function.body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };
    // Prepend destructuring extraction statements (mirrors expr_function).
    if !destructuring_stmts.is_empty() {
        let mut new_body = destructuring_stmts;
        new_body.append(&mut body);
        body = new_body;
    }
    // Prepend default-parameter fill statements. Object-literal methods never
    // dispatch through the `__perry_wrap_<name>` prologue that applies a
    // top-level function's defaults, so the body must carry the
    // `if (p === undefined) p = <default>` checks itself — otherwise
    // `{ m(a = 23) {} }; obj.m(undefined)` reads `a === undefined`. Defaults
    // must run BEFORE destructuring (`m([x] = [1]) {}`), so prepend them last.
    let default_stmts = crate::lower_decl::build_default_param_stmts(&params);
    // Record the param-prologue length for generator methods so the generator
    // transform lifts param binding (default guards + destructuring) into the
    // outer wrapper and runs it synchronously at call time (spec
    // FunctionDeclarationInstantiation). Without this, `{ *m({}) {} }.m(null)`
    // and async-generator equivalents defer the destructuring TypeError into
    // the state machine instead of throwing at the call. Mirrors fn_decl.rs.
    if method.function.is_generator {
        let prologue_len = default_stmts.len() + destructuring_prologue_len;
        if prologue_len > 0 {
            ctx.gen_param_prologue_len.insert(func_id, prologue_len);
        }
    }
    if !default_stmts.is_empty() {
        let mut new_body = default_stmts;
        new_body.append(&mut body);
        body = new_body;
    }
    ctx.exit_strict_mode();
    ctx.exit_scope(scope_mark);
    ctx.in_nonarrow_fn = saved_in_nonarrow_fn;

    // #5846: retain the method's own source for `Function.prototype.toString`.
    // Per spec a concise/object-literal method's toString is
    // `PropertyName ( params ) { body }` — no "function" keyword — so slice
    // from the key's start (the `[` for a computed key) through the function
    // body's closing brace, mirroring `capture_function_source`'s use for
    // declarations/expressions/arrows. Without this, `{ a(){} }.a` fell back
    // to the synthesized `function a() { [native code] }` form, which is
    // wrong when that source is itself used (e.g. as a ToPropertyKey
    // coercion target for a computed key elsewhere) — test262
    // toString/method-computed-property-name.
    //
    // Scoped to plain (non-async, non-generator) methods only: an
    // `async`/`*` modifier sits BEFORE the key in source (`async *[k](){}`),
    // so `method_key_span.lo` would truncate it off the front — and
    // toString/{async,generator}-method* were already passing via the
    // synthesized-native-form fallback (which conforms to the loose
    // NativeFunction-syntax check `assertToStringOrNativeFunction` falls
    // back to on an exact-match miss); capturing a truncated real slice here
    // would regress them to neither an exact match nor valid NativeFunction
    // syntax. TODO: extend to async/generator methods once the modifier
    // (and any comments before the key) can be included in the span.
    if !method.function.is_async && !method.function.is_generator {
        let method_key_span = match &method.key {
            ast::PropName::Ident(i) => i.span,
            ast::PropName::Str(s) => s.span,
            ast::PropName::Num(n) => n.span,
            ast::PropName::Computed(c) => c.span,
            ast::PropName::BigInt(b) => b.span,
        };
        let method_src_span = swc_common::Span::new(method_key_span.lo, method.function.span.hi);
        super::capture_function_source(ctx, func_id, &method_src_span, false);
    }

    // Capture analysis (same pattern as arrow/function expressions)
    let mut all_refs = Vec::new();
    let mut visited_closures = std::collections::HashSet::new();
    for stmt in &body {
        collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
    }
    let outer_local_ids: std::collections::HashSet<LocalId> =
        outer_locals.iter().map(|(_, id)| *id).collect();
    let method_param_ids: std::collections::HashSet<LocalId> =
        params.iter().map(|p| p.id).collect();
    let mut captures: Vec<LocalId> = all_refs
        .into_iter()
        .filter(|id| outer_local_ids.contains(id) && !method_param_ids.contains(id))
        .collect();
    captures.sort();
    captures.dedup();
    captures = ctx.filter_module_level_captures(captures);

    // Check if the method body uses `this` — even with no outer-scope
    // captures we must emit a Closure so the object-literal creation code
    // can patch capture slot 0 with the object pointer.
    let uses_this = closure_uses_this(&body);

    let value_expr: Expr = if captures.is_empty() && !uses_this {
        // No captures and no `this`: keep as standalone Function + FuncRef
        ctx.register_func(func_name.clone(), func_id);
        let defaults: Vec<Option<Expr>> = params.iter().map(|p| p.default.clone()).collect();
        let param_ids: Vec<LocalId> = params.iter().map(|p| p.id).collect();
        let rest_idx = params.iter().position(|p| p.is_rest);
        let has_synth_args = params.last().is_some_and(|p| p.arguments_object.is_some());
        ctx.func_defaults
            .push((func_id, defaults, param_ids, rest_idx, has_synth_args));
        ctx.pending_functions.push(Function {
            id: func_id,
            name: func_name,
            type_params: Vec::new(),
            params,
            return_type,
            body,
            is_async: method.function.is_async,
            is_generator: method.function.is_generator,
            is_strict: ctx.current_strict,
            was_plain_async: false,
            was_unrolled: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
        });
        Expr::FuncRef(func_id)
    } else {
        // Has captures: emit as Closure
        let mut all_assigned = Vec::new();
        for stmt in &body {
            collect_assigned_locals_stmt(stmt, &mut all_assigned);
        }
        let assigned_set: std::collections::HashSet<LocalId> = all_assigned.into_iter().collect();
        let mutable_captures: Vec<LocalId> = captures
            .iter()
            .filter(|id| assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
            .copied()
            .collect();
        let captures_this = uses_this;
        let enclosing_class = if captures_this {
            ctx.current_class.clone()
        } else {
            None
        };
        Expr::Closure {
            func_id,
            params,
            return_type,
            body,
            captures,
            mutable_captures,
            captures_this,
            captures_new_target: false,
            enclosing_class,
            is_arrow: false,
            is_async: method.function.is_async,
            is_generator: method.function.is_generator,
            is_strict: ctx.current_strict,
        }
    };
    Ok(Some((method_key, value_expr, uses_this)))
}

/// Lower an object-literal accessor (`get k() {}` / `set k(v) {}`) into its
/// key plus a `Closure` value. Getters take no params; setters take exactly
/// one. The closure is always emitted with `captures_this` reflecting whether
/// the body reads `this`, so the runtime installer (`js_object_define_accessor`)
/// can rebind `this` to the receiver object — mirroring how the
/// `Object.defineProperty(obj, k, { get(){...} })` path binds accessors (#450).
///
/// Returns `Ok(None)` for key/param shapes we don't model (a destructuring
/// setter param, or a computed key that failed to lower) so the caller skips
/// the entry rather than aborting the whole literal. (#2442)
fn lower_accessor_prop(
    ctx: &mut LoweringContext,
    key: &ast::PropName,
    setter_param: Option<&ast::Pat>,
    body: Option<&ast::BlockStmt>,
) -> Result<Option<(MethodKeyKind, Expr)>> {
    let accessor_key = match key {
        ast::PropName::Ident(ident) => MethodKeyKind::Static(ident.sym.to_string()),
        ast::PropName::Str(s) => MethodKeyKind::Static(s.value.as_str().unwrap_or("").to_string()),
        ast::PropName::Num(n) => MethodKeyKind::Static(super::number_to_js_key(n.value)),
        ast::PropName::Computed(computed) => match lower_expr(ctx, computed.expr.as_ref()) {
            Ok(e) => MethodKeyKind::Computed(e),
            Err(_) => return Ok(None),
        },
        _ => return Ok(None),
    };

    let func_id = ctx.fresh_func();
    let outer_locals: Vec<(String, LocalId)> = ctx
        .locals
        .iter()
        .map(|(name, id, _)| (name.clone(), *id))
        .collect();

    let scope_mark = ctx.enter_scope();
    let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
    ctx.in_nonarrow_fn = true;
    // Accessors in object literals inherit strictness (see lower_method_prop).
    let accessor_strict = ctx.current_strict_mode()
        || body
            .map(|b| crate::lower_decl::body_has_use_strict(&b.stmts))
            .unwrap_or(false);
    ctx.enter_strict_mode(accessor_strict);
    let mut params = Vec::new();
    if let Some(pat) = setter_param {
        // Setters take a single param. Skip the TS `this:` type-only marker
        // (mirrors `lower_method_prop`). Destructuring setter params aren't
        // modeled here — bail to `None` rather than crash.
        let param_name = match get_pat_name(pat) {
            Ok(n) => n,
            Err(_) => {
                ctx.exit_strict_mode();
                ctx.exit_scope(scope_mark);
                return Ok(None);
            }
        };
        if param_name != "this" {
            let param_type = extract_param_type_with_ctx(pat, Some(ctx));
            let param_default = get_param_default(ctx, pat)?;
            let param_id = ctx.define_local(param_name.clone(), param_type.clone());
            ctx.shadow_native_instance_if_present(&param_name);
            params.push(Param {
                id: param_id,
                name: param_name,
                ty: param_type,
                default: param_default,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            });
        }
    }

    let body = if let Some(block) = body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };
    ctx.exit_strict_mode();
    ctx.exit_scope(scope_mark);
    ctx.in_nonarrow_fn = saved_in_nonarrow_fn;

    // Capture analysis — identical pattern to `lower_method_prop`.
    let mut all_refs = Vec::new();
    let mut visited_closures = std::collections::HashSet::new();
    for stmt in &body {
        collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
    }
    let outer_local_ids: std::collections::HashSet<LocalId> =
        outer_locals.iter().map(|(_, id)| *id).collect();
    let param_ids: std::collections::HashSet<LocalId> = params.iter().map(|p| p.id).collect();
    let mut captures: Vec<LocalId> = all_refs
        .into_iter()
        .filter(|id| outer_local_ids.contains(id) && !param_ids.contains(id))
        .collect();
    captures.sort();
    captures.dedup();
    captures = ctx.filter_module_level_captures(captures);

    let mut all_assigned = Vec::new();
    for stmt in &body {
        collect_assigned_locals_stmt(stmt, &mut all_assigned);
    }
    let assigned_set: std::collections::HashSet<LocalId> = all_assigned.into_iter().collect();
    let mutable_captures: Vec<LocalId> = captures
        .iter()
        .filter(|id| assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
        .copied()
        .collect();
    // An object-literal accessor (`get k() {}`) is a REGULAR (non-arrow)
    // function: `this` binds dynamically to the receiver at call time, NOT to
    // the object the accessor is defined on. Emitting `captures_this: true`
    // (mirroring `uses_this`) captured `this` at object-construction time, so an
    // inherited read (`Object.create(proto).k`, where the getter lives on
    // `proto`) saw `this === proto` instead of the instance — @hono/node-server's
    // request prototype `get method() { return this[incomingKey].method }`
    // crashed because `this[incomingKey]` was undefined on the prototype. The
    // runtime accessor-invocation path binds `this` via IMPLICIT_THIS (the same
    // way `Object.defineProperty(obj, k, { get(){…} })` already worked), so the
    // closure must NOT capture it.
    let closure = Expr::Closure {
        func_id,
        params,
        return_type: Type::Any,
        body,
        captures,
        mutable_captures,
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: ctx.current_strict,
    };
    Ok(Some((accessor_key, closure)))
}

/// Map a resolved accessor key to the `js_object_define_accessor` key argument.
fn accessor_key_expr(key: MethodKeyKind) -> Expr {
    match key {
        MethodKeyKind::Static(s) => Expr::String(s),
        MethodKeyKind::Computed(e) => e,
    }
}

pub(super) fn lower_object(ctx: &mut LoweringContext, obj: &ast::ObjectLit) -> Result<Expr> {
    // Phase 3: closed-shape object literals lower to `new __AnonShape_N()`
    // so downstream field access hits the direct-GEP fast path. The
    // anon class is synthesized as a shape-only class with constructor
    // parameters for each field. The literal's lowered values move into
    // `Expr::New { args }`, and the constructor assigns them via direct-GEP
    // `PropertySet` because `this` resolves to the anon class via class_stack.
    //
    // Runtime parity for Object.* introspection APIs on anon-shape
    // classes is handled runtime-side in perry-runtime's object module
    // — see that crate's handling of `class_id`-tagged objects on
    // getOwnPropertyDescriptor / Object.keys / JSON.stringify / etc.
    fn is_closed_shape(obj: &ast::ObjectLit) -> bool {
        if obj.props.is_empty() {
            return false;
        }
        for prop in &obj.props {
            let ast::PropOrSpread::Prop(p) = prop else {
                return false;
            };
            match p.as_ref() {
                ast::Prop::KeyValue(kv) => {
                    if is_noncomputed_proto_key(&kv.key) {
                        return false;
                    }
                    match &kv.key {
                        ast::PropName::Ident(_) | ast::PropName::Str(_) | ast::PropName::Num(_) => {
                        }
                        _ => return false,
                    }
                }
                ast::Prop::Shorthand(_) => {}
                _ => return false,
            }
        }
        true
    }
    if is_closed_shape(obj) {
        let mut fields: Vec<(String, Type, Expr)> = Vec::new();
        let mut bail = false;
        let mut seen = std::collections::HashSet::new();
        for prop in &obj.props {
            let ast::PropOrSpread::Prop(p) = prop else {
                unreachable!()
            };
            match p.as_ref() {
                ast::Prop::KeyValue(kv) => {
                    let key = match &kv.key {
                        ast::PropName::Ident(ident) => ident.sym.to_string(),
                        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                        ast::PropName::Num(n) => super::number_to_js_key(n.value),
                        _ => unreachable!(),
                    };
                    if !seen.insert(key.clone()) {
                        bail = true;
                        break;
                    }
                    let ty = crate::lower_types::infer_type_from_expr(&kv.value, ctx);
                    let value = lower_prop_value_named(ctx, &key, &kv.value)?;
                    fields.push((key, ty, value));
                }
                ast::Prop::Shorthand(ident) => {
                    let name = ident.sym.to_string();
                    if !seen.insert(name.clone()) {
                        bail = true;
                        break;
                    }
                    // Issue #624: prefer LocalGet over FuncRef when both resolve.
                    // A function declaration inside a closure (e.g. an IIFE)
                    // is registered both as a function (functions_index) AND
                    // as a local Let (because the function body lowers to a
                    // Closure assigned to a let-bound name). The FuncRef path
                    // assumes the function id is a top-level user function
                    // (one with a `__perry_wrap_<name>` wrapper emitted in
                    // codegen.rs); inner closures don't get a wrapper, so the
                    // FuncRef-as-value singleton-alloc resolves to the
                    // `__perry_wrap_perry_unknown_func` fallback (returns
                    // TAG_UNDEFINED on every call). LocalGet of the let-bound
                    // closure value produces a real callable NaN-boxed
                    // pointer, matching JS spec where local bindings shadow
                    // outer-scope function names.
                    let (value, ty) = if let Some(local_id) = ctx.lookup_local(&name) {
                        let ty = ctx.lookup_local_type(&name).cloned().unwrap_or(Type::Any);
                        (Expr::LocalGet(local_id), ty)
                    } else if let Some(func_id) = ctx.lookup_func(&name) {
                        (Expr::FuncRef(func_id), Type::Any)
                    } else if let Some(orig_name) = ctx.lookup_imported_func(&name) {
                        // An imported binding (`import { db } from "./client.js"`)
                        // used as a shorthand property (`{ db }`). Resolve it to
                        // the same `ExternFuncRef` the explicit form `{ db: db }`
                        // produces — otherwise the property was silently dropped
                        // (`{ db }` lowered to an EMPTY object), so e.g.
                        // `getContext()` returned a context with `db === undefined`.
                        let (param_types, return_type) = ctx
                            .lookup_extern_func_types(orig_name)
                            .map(|(p, r)| (p.clone(), r.clone()))
                            .unwrap_or_else(|| (Vec::new(), Type::Any));
                        (
                            Expr::ExternFuncRef {
                                name: orig_name.to_string(),
                                param_types,
                                return_type,
                            },
                            Type::Any,
                        )
                    } else if ctx.lookup_class(&name).is_some() {
                        (Expr::ClassRef(name.clone()), Type::Any)
                    } else if ctx.lookup_native_module(&name).is_some() {
                        // #5242: a native-module-bound name (`import { relative }
                        // from 'path'`) used as a shorthand property. Resolve it to
                        // the same value the bare identifier produces, so the
                        // property is the callable builtin instead of being dropped
                        // (which left it `undefined` — yargs shim.path.relative).
                        (
                            super::lower_expr::native_module_binding_value(ctx, &name),
                            Type::Any,
                        )
                    } else if let Some(value) = builtin_global_value_expr(ctx, &name) {
                        (value, Type::Any)
                    } else {
                        bail = true;
                        break;
                    };
                    fields.push((name, ty, value));
                }
                _ => unreachable!(),
            }
        }
        if !bail {
            let field_shapes: Vec<(String, Type)> = fields
                .iter()
                .map(|(name, ty, _)| (name.clone(), ty.clone()))
                .collect();
            let class_name = ctx.synthesize_anon_shape_class(&field_shapes);
            let args: Vec<Expr> = fields.into_iter().map(|(_, _, value)| value).collect();
            return Ok(Expr::New {
                class_name,
                args,
                type_args: Vec::new(),
                byte_offset: 0,
            });
        }
    }
    // Legacy path — spread, methods/getters/setters, computed keys,
    // dup keys, or unresolvable shorthand.
    let has_spread = obj
        .props
        .iter()
        .any(|p| matches!(p, ast::PropOrSpread::Spread(_)));
    let has_computed = obj.props.iter().any(|p| {
        matches!(
            p,
            ast::PropOrSpread::Prop(prop)
                if match prop.as_ref() {
                    ast::Prop::KeyValue(kv) => matches!(kv.key, ast::PropName::Computed(_)),
                    ast::Prop::Method(method) => matches!(method.key, ast::PropName::Computed(_)),
                    ast::Prop::Getter(getter) => matches!(getter.key, ast::PropName::Computed(_)),
                    ast::Prop::Setter(setter) => matches!(setter.key, ast::PropName::Computed(_)),
                    _ => false,
                }
        )
    });
    let has_proto_setter = obj.props.iter().any(|p| {
        matches!(
            p,
            ast::PropOrSpread::Prop(prop)
                if match prop.as_ref() {
                    ast::Prop::KeyValue(kv) => is_noncomputed_proto_key(&kv.key),
                    _ => false,
                }
        )
    });
    // #2442: object literals containing getters/setters also go through the
    // fully source-ordered IIFE so the accessor key lands in its source
    // position (`{a, get x(){}, b}` → keys `[a, x, b]`, not `[a, b, x]`).
    // The post-init path used for computed keys appends after all static
    // props, which would mis-order the accessor key.
    let has_accessor = obj.props.iter().any(|p| {
        matches!(
            p,
            ast::PropOrSpread::Prop(prop)
                if matches!(prop.as_ref(), ast::Prop::Getter(_) | ast::Prop::Setter(_))
        )
    });
    let has_method = obj.props.iter().any(|p| {
        matches!(
            p,
            ast::PropOrSpread::Prop(prop) if matches!(prop.as_ref(), ast::Prop::Method(_))
        )
    });
    if has_spread || has_accessor || has_computed || has_method || has_proto_setter {
        // #809: an object literal that mixes a `...spread` with computed
        // keys, methods, and `this`-binding methods. The old code lowered
        // this to `Expr::ObjectSpread { parts }`, whose `parts` list can
        // only express static-string `KeyValue` and spreads — it silently
        // DROPPED every `Prop::Method` and every computed `KeyValue` (the
        // `_ => continue` / `_ => {}` arms). For Effect's `HashRing.ts`
        // `Proto` this dropped `[Symbol.iterator]()`, `pipe()`, the
        // `[TypeId]` computed string key, and the trailing `toJSON()`,
        // leaving only the spread — hence `keys: 2` and the
        // `value is not a function` crash on the first method dispatch.
        //
        // Lower instead to a fully SOURCE-ORDERED IIFE so spreads
        // interleave correctly with the other entries (a later property
        // or spread overrides an earlier same key, per JS semantics — the
        // non-spread fast paths can't be used because they apply every
        // static prop before any post-init, which would let a trailing
        // `...src` clobber the literal's own `toJSON()`):
        //
        //   ((__o) => {
        //       __o["k"] = v;                                  // static / computed
        //       js_object_set_method_by_name(__o, "m", clo);   // static this-method
        //       js_object_set_symbol_method(__o, sym, clo);    // computed this-method
        //       js_object_assign_one(__o, src);                // ...src
        //       return __o;
        //   })({})
        enum SpreadOp {
            /// `__o[key] = value` (key = `String(..)` or a dynamic expr).
            Set {
                key: Expr,
                value: Expr,
                infer_name: bool,
            },
            /// Static-string method whose body uses `this`.
            MethodByName { key: String, closure: Expr },
            /// Computed-key method whose body uses `this`.
            SymbolMethod { key: Expr, closure: Expr },
            /// `get k(){}` / `set k(v){}` accessor (#2442). `getter`/`setter`
            /// is either the lowered closure or `Expr::Undefined`; the runtime
            /// merges a separate get/set for the same key.
            DefineAccessor {
                key: Expr,
                getter: Expr,
                setter: Expr,
            },
            /// Non-computed `__proto__: value` special form.
            SetPrototype { value: Expr },
            /// `...src` — copy src's own enumerable string+symbol props.
            Assign { src: Expr },
        }

        let iife_func_id = ctx.fresh_func();
        let scope_mark = ctx.enter_scope();
        let param_id = ctx.define_local("__perry_obj_iife".to_string(), Type::Any);
        let param = Param {
            id: param_id,
            name: "__perry_obj_iife".to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        };

        // Pass 1: lower every entry's value while the IIFE parameter is in
        // scope. Object-literal `super` in a method body captures this hidden
        // home object; the method call's dynamic `this` remains separate.
        let mut ops: Vec<SpreadOp> = Vec::new();
        for prop in &obj.props {
            match prop {
                ast::PropOrSpread::Spread(spread) => {
                    let src = lower_expr(ctx, &spread.expr)?;
                    ops.push(SpreadOp::Assign { src });
                }
                ast::PropOrSpread::Prop(prop) => match prop.as_ref() {
                    ast::Prop::KeyValue(kv) => match resolve_keyvalue_key(ctx, &kv.key) {
                        KeyResolution::Skip => {}
                        KeyResolution::Static(key) => {
                            if is_noncomputed_proto_key(&kv.key) {
                                let value = lower_expr(ctx, &kv.value)?;
                                ops.push(SpreadOp::SetPrototype { value });
                            } else {
                                let value = lower_prop_value_named(ctx, &key, &kv.value)?;
                                ops.push(SpreadOp::Set {
                                    key: Expr::String(key),
                                    value,
                                    infer_name: false,
                                });
                            }
                        }
                        KeyResolution::Dynamic(key_expr) => {
                            let value = lower_expr(ctx, &kv.value)?;
                            ops.push(SpreadOp::Set {
                                key: key_expr,
                                value,
                                infer_name: true,
                            });
                        }
                    },
                    ast::Prop::Shorthand(ident) => {
                        let name = ident.sym.to_string();
                        // Issue #624: same prefer-local order as the
                        // closed-shape shorthand fold above.
                        let value = if let Some(local_id) = ctx.lookup_local(&name) {
                            Expr::LocalGet(local_id)
                        } else if let Some(func_id) = ctx.lookup_func(&name) {
                            Expr::FuncRef(func_id)
                        } else if let Some(orig_name) = ctx.lookup_imported_func(&name) {
                            // Imported binding used as shorthand in a spread/method
                            // object literal (`{ db, ...rest }` — getContext's exact
                            // shape). Resolve to the `ExternFuncRef` value rather
                            // than dropping the property (which left `db` undefined).
                            let (param_types, return_type) = ctx
                                .lookup_extern_func_types(orig_name)
                                .map(|(p, r)| (p.clone(), r.clone()))
                                .unwrap_or_else(|| (Vec::new(), Type::Any));
                            Expr::ExternFuncRef {
                                name: orig_name.to_string(),
                                param_types,
                                return_type,
                            }
                        } else if ctx.lookup_class(&name).is_some() {
                            Expr::ClassRef(name.clone())
                        } else if let Some(value) = builtin_global_value_expr(ctx, &name) {
                            value
                        } else {
                            // #6143: an unresolvable shorthand is a ReferenceError,
                            // not a silently-dropped property (see the non-spread
                            // path above). Lower a normal identifier read.
                            lower_expr(ctx, &ast::Expr::Ident(ident.clone()))?
                        };
                        ops.push(SpreadOp::Set {
                            key: Expr::String(name),
                            value,
                            infer_name: false,
                        });
                    }
                    ast::Prop::Method(method) => {
                        ctx.object_super_home_stack.push(param_id);
                        let lowered_method = lower_method_prop(ctx, method);
                        ctx.object_super_home_stack.pop();
                        let Some((mkey, value_expr, uses_this)) = lowered_method? else {
                            continue;
                        };
                        match mkey {
                            MethodKeyKind::Static(k) => {
                                if uses_this {
                                    ops.push(SpreadOp::MethodByName {
                                        key: k,
                                        closure: value_expr,
                                    });
                                } else {
                                    ops.push(SpreadOp::Set {
                                        key: Expr::String(k),
                                        value: value_expr,
                                        infer_name: false,
                                    });
                                }
                            }
                            MethodKeyKind::Computed(ke) => {
                                if uses_this {
                                    ops.push(SpreadOp::SymbolMethod {
                                        key: ke,
                                        closure: value_expr,
                                    });
                                } else {
                                    ops.push(SpreadOp::Set {
                                        key: ke,
                                        value: value_expr,
                                        infer_name: true,
                                    });
                                }
                            }
                        }
                    }
                    // Object-literal getters/setters (#2442): lower to a
                    // `js_object_define_accessor` op at this source position.
                    ast::Prop::Getter(getter) => {
                        ctx.object_super_home_stack.push(param_id);
                        let lowered_getter =
                            lower_accessor_prop(ctx, &getter.key, None, getter.body.as_ref());
                        ctx.object_super_home_stack.pop();
                        if let Some((gkey, closure)) = lowered_getter? {
                            ops.push(SpreadOp::DefineAccessor {
                                key: accessor_key_expr(gkey),
                                getter: closure,
                                setter: Expr::Undefined,
                            });
                        }
                    }
                    ast::Prop::Setter(setter) => {
                        ctx.object_super_home_stack.push(param_id);
                        let lowered_setter = lower_accessor_prop(
                            ctx,
                            &setter.key,
                            Some(setter.param.as_ref()),
                            setter.body.as_ref(),
                        );
                        ctx.object_super_home_stack.pop();
                        if let Some((skey, closure)) = lowered_setter? {
                            ops.push(SpreadOp::DefineAccessor {
                                key: accessor_key_expr(skey),
                                getter: Expr::Undefined,
                                setter: closure,
                            });
                        }
                    }
                    _ => {}
                },
            }
        }

        // Pass 2: build the IIFE wrapper. `__o` starts as an empty object
        // and each op mutates it in source order.
        let extern_call = |name: &str, args: Vec<Expr>| Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: name.to_string(),
                param_types: Vec::new(),
                return_type: Type::Any,
            }),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
        };
        let mut body: Vec<Stmt> = Vec::with_capacity(ops.len() * 4 + 1);
        let mut inner_local_ids = vec![param_id];
        for op in ops {
            match op {
                SpreadOp::Set {
                    key,
                    value,
                    infer_name,
                } => {
                    if infer_name {
                        let key_name = format!("__perry_obj_iife_key_{}", body.len());
                        let key_id = ctx.define_local(key_name.clone(), Type::Any);
                        inner_local_ids.push(key_id);
                        body.push(Stmt::Let {
                            id: key_id,
                            name: key_name,
                            ty: Type::Any,
                            mutable: false,
                            init: Some(key),
                        });
                        let prop_key_name = format!("__perry_obj_iife_prop_key_{}", body.len());
                        let prop_key_id = ctx.define_local(prop_key_name.clone(), Type::Any);
                        inner_local_ids.push(prop_key_id);
                        body.push(Stmt::Let {
                            id: prop_key_id,
                            name: prop_key_name,
                            ty: Type::Any,
                            mutable: false,
                            init: Some(extern_call(
                                "js_object_literal_to_property_key",
                                vec![Expr::LocalGet(key_id)],
                            )),
                        });
                        let value_name = format!("__perry_obj_iife_value_{}", body.len());
                        let value_id = ctx.define_local(value_name.clone(), Type::Any);
                        inner_local_ids.push(value_id);
                        body.push(Stmt::Let {
                            id: value_id,
                            name: value_name,
                            ty: Type::Any,
                            mutable: false,
                            init: Some(value),
                        });
                        body.push(Stmt::Expr(extern_call(
                            "js_object_literal_set_computed",
                            vec![
                                Expr::LocalGet(param_id),
                                Expr::LocalGet(prop_key_id),
                                Expr::LocalGet(value_id),
                            ],
                        )));
                        body.push(Stmt::Expr(extern_call(
                            "js_object_literal_infer_computed_function_name",
                            vec![Expr::LocalGet(prop_key_id), Expr::LocalGet(value_id)],
                        )));
                    } else {
                        body.push(Stmt::Expr(Expr::IndexSet {
                            object: Box::new(Expr::LocalGet(param_id)),
                            index: Box::new(key),
                            value: Box::new(value),
                        }));
                    }
                }
                SpreadOp::MethodByName { key, closure } => {
                    body.push(Stmt::Expr(extern_call(
                        "js_object_set_method_by_name",
                        vec![Expr::LocalGet(param_id), Expr::String(key), closure],
                    )));
                }
                SpreadOp::SymbolMethod { key, closure } => {
                    body.push(Stmt::Expr(extern_call(
                        "js_object_set_property_key_method",
                        vec![Expr::LocalGet(param_id), key, closure],
                    )));
                }
                SpreadOp::DefineAccessor {
                    key,
                    getter,
                    setter,
                } => {
                    body.push(Stmt::Expr(extern_call(
                        "js_object_define_accessor",
                        vec![Expr::LocalGet(param_id), key, getter, setter],
                    )));
                }
                SpreadOp::SetPrototype { value } => {
                    body.push(Stmt::Expr(extern_call(
                        "js_object_literal_set_prototype",
                        vec![Expr::LocalGet(param_id), value],
                    )));
                }
                SpreadOp::Assign { src } => {
                    body.push(Stmt::Expr(extern_call(
                        "js_object_assign_one",
                        vec![Expr::LocalGet(param_id), src],
                    )));
                }
            }
        }
        body.push(Stmt::Return(Some(Expr::LocalGet(param_id))));
        ctx.exit_scope(scope_mark);

        // Capture analysis — identical to the computed-key IIFE below.
        let mut all_refs = Vec::new();
        let mut visited_closures = std::collections::HashSet::new();
        for stmt in &body {
            collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
        }
        let mut captures: Vec<LocalId> = all_refs
            .into_iter()
            .filter(|id| !inner_local_ids.contains(id))
            .collect();
        captures.sort();
        captures.dedup();
        captures = ctx.filter_module_level_captures(captures);
        let body_uses_this = body.iter().any(uses_this_stmt);
        let closure = Expr::Closure {
            func_id: iife_func_id,
            params: vec![param],
            return_type: Type::Any,
            body,
            captures,
            mutable_captures: Vec::new(),
            captures_this: body_uses_this,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: ctx.current_strict,
        };
        return Ok(Expr::Call {
            callee: Box::new(closure),
            args: vec![Expr::Object(Vec::new())],
            type_args: vec![],
            byte_offset: 0,
        });
    }
    let mut props = Vec::new();
    // Computed keys whose value can't be folded to a string at HIR time
    // (typically symbol-typed locals like `{ [symProp]: 42 }`). Deferred
    // and emitted as statements inside an IIFE wrapper after the
    // static-key Object literal is built.
    //
    // For `Prop::Method` with a computed key whose body uses `this`
    // (e.g. `{ [Symbol.toPrimitive](hint) { return this.value; } }`),
    // we emit a dedicated `js_object_set_symbol_method` runtime call
    // that BOTH stores the closure in the symbol side-table AND
    // patches the closure's reserved `this` slot with the object.
    enum PostInit {
        SetValue { key: Expr, value: Expr },
        SetMethodWithThis { key: Expr, closure: Expr },
    }
    let mut computed_post_init: Vec<PostInit> = Vec::new();
    for prop in &obj.props {
        if let ast::PropOrSpread::Prop(prop) = prop {
            match prop.as_ref() {
                ast::Prop::KeyValue(kv) => match resolve_keyvalue_key(ctx, &kv.key) {
                    KeyResolution::Skip => continue,
                    KeyResolution::Static(key) => {
                        let value = lower_expr(ctx, &kv.value)?;
                        props.push((key, value));
                    }
                    KeyResolution::Dynamic(key_expr) => {
                        let value = lower_expr(ctx, &kv.value)?;
                        computed_post_init.push(PostInit::SetValue {
                            key: key_expr,
                            value,
                        });
                    }
                },
                ast::Prop::Shorthand(ident) => {
                    // Shorthand property: { help } → { help: help }
                    let name = ident.sym.to_string();
                    let value = if let Some(func_id) = ctx.lookup_func(&name) {
                        Expr::FuncRef(func_id)
                    } else if let Some(local_id) = ctx.lookup_local(&name) {
                        Expr::LocalGet(local_id)
                    } else if let Some(orig_name) = ctx.lookup_imported_func(&name) {
                        // Imported binding used as a shorthand property — resolve
                        // to its `ExternFuncRef` value instead of dropping it.
                        let (param_types, return_type) = ctx
                            .lookup_extern_func_types(orig_name)
                            .map(|(p, r)| (p.clone(), r.clone()))
                            .unwrap_or_else(|| (Vec::new(), Type::Any));
                        Expr::ExternFuncRef {
                            name: orig_name.to_string(),
                            param_types,
                            return_type,
                        }
                    } else if ctx.lookup_class(&name).is_some() {
                        Expr::ClassRef(name.clone())
                    } else if ctx.lookup_native_module(&name).is_some() {
                        // #5242: native-module-bound shorthand (`{ relative }` from
                        // `import { relative } from 'path'`). Resolve to the builtin
                        // value instead of dropping the property.
                        super::lower_expr::native_module_binding_value(ctx, &name)
                    } else if let Some(value) = builtin_global_value_expr(ctx, &name) {
                        value
                    } else {
                        // #6143: an unresolvable shorthand (`{ undeclaredName }`) is
                        // a ReferenceError, not a silently-dropped property. Lower a
                        // normal identifier read, which throws for a truly-undeclared
                        // name (and reads a sloppy implicit global if one exists).
                        lower_expr(ctx, &ast::Expr::Ident(ident.clone()))?
                    };
                    props.push((name, value));
                }
                ast::Prop::Method(method) => {
                    // Inline method: `{ help(): string { ... } }`. Computed
                    // keys (e.g. `[Symbol.toPrimitive](hint) {}`) get routed
                    // through the IIFE wrapper's SetMethodWithThis post-init,
                    // which emits a `js_object_set_symbol_method` call that
                    // also patches the closure's reserved `this` slot. Shared
                    // with the spread path via `lower_method_prop`.
                    let Some((method_key, value_expr, uses_this)) = lower_method_prop(ctx, method)?
                    else {
                        continue;
                    };
                    match method_key {
                        MethodKeyKind::Static(key_str) => {
                            props.push((key_str, value_expr));
                        }
                        MethodKeyKind::Computed(key_expr) => {
                            if uses_this {
                                computed_post_init.push(PostInit::SetMethodWithThis {
                                    key: key_expr,
                                    closure: value_expr,
                                });
                            } else {
                                computed_post_init.push(PostInit::SetValue {
                                    key: key_expr,
                                    value: value_expr,
                                });
                            }
                        }
                    }
                }
                // Getters/setters are handled by the source-ordered IIFE
                // path above (`has_accessor`), so they never reach here.
                _ => {}
            }
        }
    }
    // No computed-key post-init: emit a plain object literal.
    if computed_post_init.is_empty() {
        return Ok(Expr::Object(props));
    }
    // Has computed keys: synthesize an IIFE wrapper that builds the
    // object with static props, then runs IndexSet for each computed
    // key, then applies literal-specific symbol-function name inference,
    // then returns the object. Keeping storage on IndexSet preserves the
    // existing string/number/symbol dispatch while avoiding name inference
    // for later `obj[sym] = fn` assignments.
    //
    // Lowered shape:
    //   ((__o) => {
    //       __o[k1] = v1;
    //       __o[k2] = v2;
    //       return __o;
    //   })({ static_props })
    let iife_func_id = ctx.fresh_func();
    let scope_mark = ctx.enter_scope();
    let param_id = ctx.define_local("__perry_obj_iife".to_string(), Type::Any);
    let param = Param {
        id: param_id,
        name: "__perry_obj_iife".to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    };
    let mut body: Vec<Stmt> = Vec::with_capacity(computed_post_init.len() * 4 + 1);
    let mut inner_local_ids = vec![param_id];
    let extern_call = |name: &str, args: Vec<Expr>| Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: name.to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    };
    for init in computed_post_init {
        match init {
            PostInit::SetValue { key, value } => {
                let key_name = format!("__perry_obj_iife_key_{}", body.len());
                let key_id = ctx.define_local(key_name.clone(), Type::Any);
                inner_local_ids.push(key_id);
                body.push(Stmt::Let {
                    id: key_id,
                    name: key_name,
                    ty: Type::Any,
                    mutable: false,
                    init: Some(key),
                });
                let prop_key_name = format!("__perry_obj_iife_prop_key_{}", body.len());
                let prop_key_id = ctx.define_local(prop_key_name.clone(), Type::Any);
                inner_local_ids.push(prop_key_id);
                body.push(Stmt::Let {
                    id: prop_key_id,
                    name: prop_key_name,
                    ty: Type::Any,
                    mutable: false,
                    init: Some(extern_call(
                        "js_object_literal_to_property_key",
                        vec![Expr::LocalGet(key_id)],
                    )),
                });
                let value_name = format!("__perry_obj_iife_value_{}", body.len());
                let value_id = ctx.define_local(value_name.clone(), Type::Any);
                inner_local_ids.push(value_id);
                body.push(Stmt::Let {
                    id: value_id,
                    name: value_name,
                    ty: Type::Any,
                    mutable: false,
                    init: Some(value),
                });
                body.push(Stmt::Expr(extern_call(
                    "js_object_literal_set_computed",
                    vec![
                        Expr::LocalGet(param_id),
                        Expr::LocalGet(prop_key_id),
                        Expr::LocalGet(value_id),
                    ],
                )));
                body.push(Stmt::Expr(extern_call(
                    "js_object_literal_infer_computed_function_name",
                    vec![Expr::LocalGet(prop_key_id), Expr::LocalGet(value_id)],
                )));
            }
            PostInit::SetMethodWithThis { key, closure } => {
                // Emit a direct call to the runtime helper that
                // stores the closure in the symbol side-table AND
                // patches its reserved `this` slot with __o.
                body.push(Stmt::Expr(Expr::Call {
                    callee: Box::new(Expr::ExternFuncRef {
                        name: "js_object_set_symbol_method".to_string(),
                        param_types: Vec::new(),
                        return_type: Type::Any,
                    }),
                    args: vec![Expr::LocalGet(param_id), key, closure],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }));
            }
        }
    }
    body.push(Stmt::Return(Some(Expr::LocalGet(param_id))));
    ctx.exit_scope(scope_mark);
    // Capture analysis: any LocalIds referenced inside the body that
    // weren't defined here (i.e. the symbol locals from the outer scope).
    let mut all_refs = Vec::new();
    let mut visited_closures = std::collections::HashSet::new();
    for stmt in &body {
        collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
    }
    let mut captures: Vec<LocalId> = all_refs
        .into_iter()
        .filter(|id| !inner_local_ids.contains(id))
        .collect();
    captures.sort();
    captures.dedup();
    captures = ctx.filter_module_level_captures(captures);
    // Refs #488 drizzle-sqlite: when a computed-key object literal
    // contains `[this.X]: ...` (drizzle's `{ [this.tableName]: true }`
    // shape from SQLiteSelectQueryBuilderBase ctor), the IIFE wrapper's
    // body must lexically capture `this` from the enclosing scope —
    // otherwise `this` inside the body resolves to undefined and the
    // key expression throws `Cannot read properties of undefined`.
    // Conservatively scan the deferred body for any `this` references
    // and set `captures_this` accordingly. Setting unconditionally is
    // also correctness-safe but adds a small per-call cost for the
    // common non-this-key cases; the scan keeps the existing fast path.
    let body_uses_this = body.iter().any(uses_this_stmt);
    let static_obj = Expr::Object(props);
    let closure = Expr::Closure {
        func_id: iife_func_id,
        params: vec![param],
        return_type: Type::Any,
        body,
        captures,
        mutable_captures: Vec::new(),
        captures_this: body_uses_this,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: ctx.current_strict,
    };
    Ok(Expr::Call {
        callee: Box::new(closure),
        args: vec![static_obj],
        type_args: vec![],
        byte_offset: 0,
    })
}
