//! Function-expression lowering: `ast::Expr::Arrow` + `ast::Expr::Fn`.
//!
//! Tier 2.3 follow-up (v0.5.338) — second extraction round from the
//! 6,508-LOC `lower::lower_expr` function. Both arrow functions and
//! `function () {...}` expressions lower to the same `Expr::Closure`
//! HIR node; the only differences are (a) arrows capture `this` from
//! the enclosing scope while function expressions don't, (b) arrows
//! can have a single-expression body shorthand, (c) function
//! expressions have a separate `function.params` indirection. The two
//! helpers below share the same closure-capture analysis (collect
//! local refs in body, intersect with outer locals, identify
//! mutable captures) so they live together.
//!
//! Pattern matches `expr_misc.rs`: free `pub(super) fn` helpers,
//! recursion through `super::lower_expr`, all `LoweringContext`
//! mutation goes through public methods + `pub(crate)` fields.

use anyhow::Result;
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::analysis::{
    closure_uses_new_target, closure_uses_this, collect_assigned_locals_stmt,
    collect_local_refs_stmt,
};
use crate::ir::{Expr, Param, Stmt};
use crate::lower_patterns::{
    generate_param_destructuring_stmts, get_param_default, get_pat_name, get_pat_type,
    is_destructuring_pattern, is_rest_param,
};

use super::{lower_expr, LoweringContext};

/// #4101: retain a function's original source text keyed by FuncId so
/// `Function.prototype.toString` can reconstruct it. Slices the installed
/// module source against `span`; when `is_async` is set but the slice doesn't
/// already begin with `async` (SWC's `Function.span`/`ArrowExpr.span` start at
/// the params/`function` keyword, excluding the leading `async`), the modifier
/// is prepended so the result matches Node. A no-op when no module source is
/// installed (unit tests / `check`).
pub(crate) fn capture_function_source(
    ctx: &mut LoweringContext,
    func_id: perry_types::FuncId,
    span: &swc_common::Span,
    is_async: bool,
) {
    let Some(mut src) = crate::ir::current_module_source_slice(span.lo.0, span.hi.0) else {
        return;
    };
    if is_async && !src.trim_start().starts_with("async") {
        src = format!("async {src}");
    }
    ctx.closure_source_text.insert(func_id, src);
}

fn block_has_use_strict(block: Option<&ast::BlockStmt>) -> bool {
    let Some(block) = block else {
        return false;
    };
    for stmt in &block.stmts {
        let Some(directive) = super::string_directive_stmt_lit(stmt) else {
            break;
        };
        if super::is_raw_use_strict_directive(directive) {
            return true;
        }
    }
    false
}

fn arrow_body_has_use_strict(body: &ast::BlockStmtOrExpr) -> bool {
    match body {
        ast::BlockStmtOrExpr::BlockStmt(block) => block_has_use_strict(Some(block)),
        ast::BlockStmtOrExpr::Expr(_) => false,
    }
}

fn collect_direct_eval_var_names_from_pat(pat: &ast::Pat, out: &mut Vec<String>) {
    match pat {
        ast::Pat::Assign(assign) => {
            collect_direct_eval_var_names_from_pat(&assign.left, out);
            collect_direct_eval_var_names_from_expr(&assign.right, out);
        }
        ast::Pat::Array(arr) => {
            for elem in arr.elems.iter().flatten() {
                collect_direct_eval_var_names_from_pat(elem, out);
            }
        }
        ast::Pat::Object(obj) => {
            for prop in &obj.props {
                match prop {
                    ast::ObjectPatProp::Assign(assign) => {
                        if let Some(default) = &assign.value {
                            collect_direct_eval_var_names_from_expr(default, out);
                        }
                    }
                    ast::ObjectPatProp::KeyValue(kv) => {
                        collect_direct_eval_var_names_from_pat(&kv.value, out);
                    }
                    ast::ObjectPatProp::Rest(rest) => {
                        collect_direct_eval_var_names_from_pat(&rest.arg, out);
                    }
                }
            }
        }
        ast::Pat::Rest(rest) => collect_direct_eval_var_names_from_pat(&rest.arg, out),
        _ => {}
    }
}

fn collect_direct_eval_var_names_from_expr(expr: &ast::Expr, out: &mut Vec<String>) {
    match expr {
        ast::Expr::Call(call) => {
            if let ast::Callee::Expr(callee) = &call.callee {
                let mut callee_expr = callee.as_ref();
                while let ast::Expr::Paren(paren) = callee_expr {
                    callee_expr = paren.expr.as_ref();
                }
                if matches!(callee_expr, ast::Expr::Ident(id) if id.sym.as_ref() == "eval")
                    && call.args.len() == 1
                    && call.args[0].spread.is_none()
                {
                    if let Some(body) = crate::eval_classifier::const_string_of(&call.args[0].expr)
                    {
                        if let Some(name) = super::const_fold_fn::direct_eval_var_decl_name(&body) {
                            out.push(name);
                        }
                    }
                }
                collect_direct_eval_var_names_from_expr(callee, out);
            }
            for arg in &call.args {
                collect_direct_eval_var_names_from_expr(&arg.expr, out);
            }
        }
        ast::Expr::Paren(paren) => collect_direct_eval_var_names_from_expr(&paren.expr, out),
        ast::Expr::Seq(seq) => {
            for expr in &seq.exprs {
                collect_direct_eval_var_names_from_expr(expr, out);
            }
        }
        ast::Expr::Assign(assign) => {
            collect_direct_eval_var_names_from_expr(&assign.right, out);
        }
        ast::Expr::Cond(cond) => {
            collect_direct_eval_var_names_from_expr(&cond.test, out);
            collect_direct_eval_var_names_from_expr(&cond.cons, out);
            collect_direct_eval_var_names_from_expr(&cond.alt, out);
        }
        ast::Expr::Bin(bin) => {
            collect_direct_eval_var_names_from_expr(&bin.left, out);
            collect_direct_eval_var_names_from_expr(&bin.right, out);
        }
        ast::Expr::Unary(unary) => collect_direct_eval_var_names_from_expr(&unary.arg, out),
        ast::Expr::Update(update) => collect_direct_eval_var_names_from_expr(&update.arg, out),
        ast::Expr::Member(member) => {
            collect_direct_eval_var_names_from_expr(&member.obj, out);
            if let ast::MemberProp::Computed(computed) = &member.prop {
                collect_direct_eval_var_names_from_expr(&computed.expr, out);
            }
        }
        ast::Expr::Array(arr) => {
            for elem in arr.elems.iter().flatten() {
                collect_direct_eval_var_names_from_expr(&elem.expr, out);
            }
        }
        ast::Expr::Object(obj) => {
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(prop) = prop {
                    match prop.as_ref() {
                        ast::Prop::KeyValue(kv) => {
                            collect_direct_eval_var_names_from_expr(&kv.value, out)
                        }
                        ast::Prop::Assign(assign) => {
                            collect_direct_eval_var_names_from_expr(&assign.value, out)
                        }
                        ast::Prop::Getter(_)
                        | ast::Prop::Setter(_)
                        | ast::Prop::Method(_)
                        | ast::Prop::Shorthand(_) => {}
                    }
                }
            }
        }
        ast::Expr::Fn(_) | ast::Expr::Arrow(_) | ast::Expr::Class(_) => {}
        _ => {}
    }
}

pub(super) fn lower_arrow(ctx: &mut LoweringContext, arrow: &ast::ArrowExpr) -> Result<Expr> {
    // Lower arrow function to a closure
    let func_id = ctx.fresh_func();
    // #4101: retain source text for `fn.toString()`.
    capture_function_source(ctx, func_id, &arrow.span, arrow.is_async);
    let scope_mark = ctx.enter_scope();
    let strict = ctx.current_strict_mode()
        || match &*arrow.body {
            ast::BlockStmtOrExpr::BlockStmt(block) => {
                crate::lower_decl::body_has_use_strict(&block.stmts)
            }
            ast::BlockStmtOrExpr::Expr(_) => false,
        };
    ctx.enter_strict_mode(strict);

    // Enter a type-parameter scope for arrow generics — `<T extends string>
    // (self: T) => ...`. Without this scope the `T` reference in `self: T`
    // never matches `is_type_param` and stays as `Type::Named("T")`, so
    // the constraint-substitution path in `extract_ts_type_with_ctx` can't
    // resolve it. Arrows and function expressions aren't monomorphized
    // (only `FuncRef`-targeted calls go through that pass), so the
    // un-narrowed param type would be the one codegen lowers — and the
    // IndexGet fast path keys off the param's static type. Mirrors the
    // existing `lower_fn_decl` scope-entry. (#321: effect's
    // `Str.capitalize` / `Capitalize<T>` arrow utilities.)
    let arrow_type_params = arrow
        .type_params
        .as_ref()
        .map(|tp| crate::lower_types::extract_type_params(tp))
        .unwrap_or_default();
    ctx.enter_type_param_scope(&arrow_type_params);

    // Track which locals exist before entering the closure scope
    let outer_locals: Vec<(String, LocalId)> = ctx
        .locals
        .iter()
        .map(|(name, id, _)| (name.clone(), *id))
        .collect();

    // Lower parameters and collect destructuring info
    let mut params = Vec::new();
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &arrow.params {
        let param_name = get_pat_name(param)?;
        let is_rest = is_rest_param(param);
        let param_ty = get_pat_type(param, ctx);
        let param_id = ctx.define_local(param_name.clone(), param_ty.clone());
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_ty,
            default: None,
            decorators: Vec::new(),
            is_rest,
            arguments_object: None,
        });
        // Track destructuring patterns to generate extraction statements. A
        // `([x, y] = [1, 2]) =>` param is a `Pat::Assign` wrapping the array/
        // object pattern; unwrap it so the destructuring binding is still
        // emitted (the `= [1,2]` default is handled separately via
        // `get_param_default`). Mirrors `lower_fn_decl`.
        let inner_pat = if let ast::Pat::Assign(assign) = param {
            assign.left.as_ref()
        } else {
            param
        };
        if is_destructuring_pattern(inner_pat) {
            destructuring_params.push((param_id, inner_pat.clone()));
        }
    }

    let mut eval_var_names = Vec::new();
    for param in &arrow.params {
        collect_direct_eval_var_names_from_pat(param, &mut eval_var_names);
    }
    eval_var_names.sort();
    eval_var_names.dedup();
    let mut param_eval_var_stmts = Vec::new();
    for name in eval_var_names {
        let existing_current_scope = ctx
            .locals
            .iter()
            .enumerate()
            .rev()
            .any(|(idx, (n, _, _))| n == &name && idx >= scope_mark.0);
        if !existing_current_scope {
            let id = ctx.define_local(name.clone(), Type::Any);
            ctx.var_hoisted_ids.insert(id);
            param_eval_var_stmts.push(Stmt::Let {
                id,
                name,
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Undefined),
            });
        }
    }

    for (idx, param) in arrow.params.iter().enumerate() {
        params[idx].default = get_param_default(ctx, param)?;
    }

    // Register arrow function parameters with known native types as native instances
    for param in &params {
        if let Type::Named(type_name) = &param.ty {
            let native_info = match type_name.as_str() {
                "PluginApi" => Some(("perry/plugin", "PluginApi")),
                "WebSocket" | "WebSocketServer" => Some(("ws", type_name.as_str())),
                "Redis" => Some(("ioredis", "Redis")),
                "EventEmitter" => Some(("events", "EventEmitter")),
                "EventEmitterAsyncResource" => Some(("events", "EventEmitterAsyncResource")),
                // Web Fetch API: Request / Response / Headers passed as
                // function parameters need the same native-instance
                // registration the `new Request()`/`new Response()`/
                // `new Headers()` paths get from destructuring.rs:1457+,
                // otherwise codegen's `Request.url` / `Response.status` /
                // `Headers.get` static dispatches don't fire and the
                // generic-object-property-get fallback hands `request.url`
                // a raw integer handle as if it were an object pointer
                // (handle IDs aren't NaN-boxed pointers — `js_request_new`
                // returns `id as f64`). Hono's `app.fetch(request)` reads
                // `request.url` inside cross-module compiled code; without
                // this registration the read returned undefined and the
                // downstream `url.indexOf("/")` threw "Cannot read
                // properties of undefined (reading 'indexOf')".
                "Request" => Some(("Request", "Request")),
                "Response" => Some(("fetch", "Response")),
                "Headers" => Some(("Headers", "Headers")),
                // Fastify types
                "FastifyInstance" => Some(("fastify", "App")),
                "FastifyRequest" => Some(("fastify", "Request")),
                "FastifyReply" => Some(("fastify", "Reply")),
                // HTTP/HTTPS types
                "IncomingMessage" => Some(("http", "IncomingMessage")),
                "ClientRequest" => Some(("http", "ClientRequest")),
                "ServerResponse" => Some(("http", "ServerResponse")),
                _ => None,
            };
            if let Some((module, class)) = native_info {
                ctx.register_native_instance(
                    param.name.clone(),
                    module.to_string(),
                    class.to_string(),
                );
            }
        }
    }

    // #1483: perry/ui widget arrow-params (`(canvas: Canvas) => ...` or, via a
    // type-only import alias, `(canvas: CanvasType) => ...`) dispatch instance
    // methods through perry/ui `NativeMethodCall` like a local `const canvas =
    // Canvas(...)`. Mirrors the fn-decl registration; resolution requires a
    // real perry/ui import so user classes sharing a widget name aren't tagged.
    for param in &params {
        if let Type::Named(type_name) = &param.ty {
            if let Some(widget) = ctx.resolve_perry_ui_widget_type(type_name) {
                ctx.register_native_instance(param.name.clone(), "perry/ui".to_string(), widget);
            }
        }
    }

    // Generate Let statements for destructuring patterns BEFORE lowering body
    // This ensures the destructured variable names are defined when the body references them
    let mut destructuring_stmts = Vec::new();
    for (param_id, pat) in &destructuring_params {
        let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
        destructuring_stmts.extend(stmts);
    }

    let outer_strict = ctx.current_strict;
    let is_strict = outer_strict || arrow_body_has_use_strict(&arrow.body);
    ctx.current_strict = is_strict;

    // Lower body with JS function hoisting.
    // Only `var` declarations and function declarations are hoisted
    // to the top per JS semantics — `let`/`const` MUST remain at their
    // lexical position because they have block-scoped temporal dead
    // zone semantics and, critically, their init expressions are only
    // evaluated when control flow reaches them. Hoisting a `const x =
    // someCall()` above a conditional that should skip it would
    // eagerly invoke the call and break user code.
    let mut body = match &*arrow.body {
        ast::BlockStmtOrExpr::BlockStmt(block) => {
            crate::lower_decl::lower_fn_body_block_stmt(ctx, block)?
        }
        ast::BlockStmtOrExpr::Expr(expr) => {
            let return_expr = lower_expr(ctx, expr)?;
            vec![Stmt::Return(Some(return_expr))]
        }
    };
    ctx.current_strict = outer_strict;

    // Prepend destructuring statements to body
    if !destructuring_stmts.is_empty() {
        let mut new_body = destructuring_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

    // Refs #486: prepend default-parameter `if (p === undefined) p = <default>`
    // checks. Without this, arrow functions with `(fn = console.log) =>
    // fn(out)` (the canonical hono `logger()` middleware shape) lower to
    // a closure whose body invokes `LocalGet(fn_id)` directly — but the
    // call-site doesn't pass `fn`, the call-site arg-padding writes
    // TAG_UNDEFINED, and the body never sees the default. Mirror the
    // identical desugar on `lower_fn_decl` / constructor / class method
    // bodies (lower_decl.rs:406 / :2156 / :2465).
    let default_stmts = crate::lower_decl::build_default_param_stmts(&params);
    if !default_stmts.is_empty() {
        let mut new_body = default_stmts;
        new_body.append(&mut body);
        body = new_body;
    }
    if !param_eval_var_stmts.is_empty() {
        param_eval_var_stmts.append(&mut body);
        body = param_eval_var_stmts;
    }

    ctx.exit_strict_mode();
    ctx.exit_scope(scope_mark);

    // Exit the type-parameter scope opened at the top of `lower_arrow`.
    // Paired with `enter_type_param_scope` above so nested generic
    // arrows don't leak outer T/U bindings into sibling code.
    ctx.exit_type_param_scope();

    let (captures, mutable_captures) = compute_closure_captures(ctx, &body, &outer_locals, &params);

    // Check if this arrow function uses `this` (needs to capture it from enclosing scope)
    let captures_this = closure_uses_this(&body);
    let captures_new_target = closure_uses_new_target(&body);

    // Store enclosing class name for arrow functions that capture `this`
    let enclosing_class = if captures_this {
        ctx.current_class.clone()
    } else {
        None
    };

    if let Some(name) = ctx.assignment_inferred_name.as_ref() {
        if !name.is_empty() {
            ctx.closure_display_names.insert(func_id, name.clone());
        }
    }

    Ok(Expr::Closure {
        func_id,
        params,
        return_type: Type::Any,
        body,
        captures,
        mutable_captures,
        captures_this,
        captures_new_target,
        enclosing_class,
        is_arrow: true,
        is_async: arrow.is_async,
        is_generator: false,
        is_strict,
    })
}

pub(crate) fn lower_fn_expr(ctx: &mut LoweringContext, fn_expr: &ast::FnExpr) -> Result<Expr> {
    // Lower function expression to a closure (similar to arrow but
    // without `this` capture — function expressions have their own
    // `this` binding determined by how they're called).
    let func_id = ctx.fresh_func();
    // #4101: retain source text for `fn.toString()`.
    capture_function_source(
        ctx,
        func_id,
        &fn_expr.function.span,
        fn_expr.function.is_async,
    );
    let scope_mark = ctx.enter_scope();

    // Track which locals exist before entering the closure scope
    let outer_locals: Vec<(String, LocalId)> = ctx
        .locals
        .iter()
        .map(|(name, id, _)| (name.clone(), *id))
        .collect();

    // Lower parameters and collect destructuring info.
    //
    // Refs #915 (gap 1 from #899 — Effect's `dual(arity, body)`): TypeScript's
    // fake `this: T` parameter annotation is a TYPE-only marker and has no
    // runtime existence. SWC emits it as a regular `Param { pat: Ident("this") }`,
    // so a naive iteration would mint a real local for it, shift every
    // subsequent positional arg by one, and break call-site arity matching —
    // `function (this: any, a, b) { ... }` called as `f(3, 4)` would bind
    // `this=3, a=4, b=undefined`. Skip these entries up-front so the
    // remaining params are the real runtime ones. (`fn_decl` already has its
    // own param-lowering site that needs the same fix — handled below.)
    let mut params = Vec::new();
    let mut default_param_pats: Vec<ast::Pat> = Vec::new();
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &fn_expr.function.params {
        let param_name = get_pat_name(&param.pat)?;
        if param_name == "this" {
            // TS `this:` annotation — skip; it's type-only.
            continue;
        }
        let is_rest = is_rest_param(&param.pat);
        let param_id = ctx.define_local(param_name.clone(), Type::Any);
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest,
            arguments_object: None,
        });
        default_param_pats.push(param.pat.clone());
        // Track destructuring patterns to generate extraction statements. A
        // `function*([x, y] = [1, 2]) {}` param is a `Pat::Assign` wrapping the
        // array/object pattern; unwrap it so the destructuring binding is still
        // emitted (the `= [1,2]` default is applied via `get_param_default`).
        // Mirrors `lower_fn_decl`. Without this, an async-generator EXPRESSION
        // with a destructured-default param dropped the binding and `x`/`y`
        // lowered to `js_throw_reference_error_unresolved_get`.
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

    // #677: synthesize `arguments` for non-arrow function expressions when the
    // body references it. Function expressions get their own `arguments`
    // binding per spec — they don't inherit from the enclosing scope.
    let user_has_arguments_param = fn_expr
        .function
        .params
        .iter()
        .any(|p| get_pat_name(&p.pat).ok().as_deref() == Some("arguments"));
    let strict = fn_expr
        .function
        .body
        .as_ref()
        .map(|b| ctx.current_strict_mode() || crate::lower_decl::body_has_use_strict(&b.stmts))
        .unwrap_or(false);
    ctx.enter_strict_mode(strict);
    let simple_parameters =
        crate::lower_decl::params_are_simple_arguments_list(&fn_expr.function.params);
    let needs_arguments_synth = !user_has_arguments_param
        && fn_expr
            .function
            .body
            .as_ref()
            .map(|b| crate::lower_decl::body_uses_arguments(&b.stmts))
            .unwrap_or(false);
    if needs_arguments_synth {
        let mapped = !strict && simple_parameters;
        let mapped_parameter_ids = if mapped {
            crate::lower_decl::mapped_argument_parameter_ids(&params)
        } else {
            Vec::new()
        };
        crate::lower_decl::append_synthetic_arguments_param(
            ctx,
            &mut params,
            strict,
            simple_parameters,
            !mapped,
            mapped_parameter_ids,
        );
    }

    let outer_strict = ctx.current_strict;
    let is_strict = outer_strict || block_has_use_strict(fn_expr.function.body.as_ref());
    ctx.current_strict = is_strict;

    // Generate Let statements for destructuring patterns BEFORE lowering body
    let mut destructuring_stmts = Vec::new();
    for (param_id, pat) in &destructuring_params {
        let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
        destructuring_stmts.extend(stmts);
    }
    let destructuring_prologue_len = destructuring_stmts.len();

    // Hoist function declarations: pre-register all function declarations in the body
    // so they can be referenced before their lexical position (JS hoisting semantics).
    // Track ids for the prealloc-box analysis (issue #633).
    //
    // Issue #838 followup (b): only reuse an existing local id if the
    // binding is in THIS scope. dayjs's minified bundle has `var M = {…}`
    // at the outer IIFE scope AND `function M(t){…}` inside the inner
    // IIFE — `lookup_local("M")` finds the outer M, so without the
    // scope guard the hoist reused the outer's id for the inner
    // function. That id is then "locally defined" inside the inner
    // closure body, which excluded it from `referenced_from_fn` in the
    // codegen-side `scan_body` (refs-minus-defines analysis). The outer
    // M's let-init then never got a module global, so the inner Let's
    // closure-pointer store and the outer prototype-method registration
    // landed in disjoint stack slots and dispatch missed entirely.
    //
    // `scope_mark.0` is `ctx.locals.len()` at scope entry — any local
    // with that index or higher was defined in the current scope.
    let outer_locals_len = scope_mark.0;
    let mut hoisted_id_set: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
    if let Some(ref block) = fn_expr.function.body {
        // Issue #838 followup (b): pre-register top-level `var` decls in
        // this function body BEFORE lowering any statement. dayjs's
        // minified outer IIFE is shaped `function() { var ..., M={…}; var
        // O = function(t){ ...; return new _(n); }; var _ = (function(){
        // ... return M; })(); … }` — `O`'s body references `_` before
        // `_`'s let runs in source order. Without this pre-pass, the
        // recogniser in `lower_new`'s ident arm calls `lookup_local("_")`
        // while lowering O's body and finds nothing, so the assignment
        // falls through to `Expr::New { class_name: "_" }` which codegen
        // then routes to the empty-object placeholder. With the
        // pre-pass, `_` is a known local at the time O's body lowers,
        // and the recogniser routes to `Expr::NewDynamic { callee:
        // LocalGet(_), … }` so `js_new_function_construct` stamps the
        // shared synthetic class id on the instance and dispatch finds
        // the prototype methods. Same shallow-walk policy as the
        // codegen-side `referenced_from_fn` pre-scan.
        for stmt in &block.stmts {
            if let ast::Stmt::Decl(ast::Decl::Var(var_decl)) = stmt {
                if var_decl.kind == ast::VarDeclKind::Var {
                    for decl in &var_decl.decls {
                        if let ast::Pat::Ident(ident) = &decl.name {
                            let name = ident.id.sym.to_string();
                            let already_in_scope = ctx
                                .locals
                                .iter()
                                .enumerate()
                                .rev()
                                .any(|(idx, (n, _, _))| n == &name && idx >= outer_locals_len);
                            if !already_in_scope {
                                let id = ctx.define_local(name, Type::Any);
                                // Mark as hoisted so closures created
                                // before the var's init expression see
                                // it through a box (mutable capture),
                                // not a stale-value snapshot. JS spec:
                                // `var` declarations are hoisted to the
                                // top of the enclosing function and
                                // start as `undefined` until the init
                                // runs.
                                ctx.var_hoisted_ids.insert(id);
                                // Also include the var-hoisted id in
                                // `hoisted_id_set` so the
                                // `compute_prealloc_for_hoisted_closures`
                                // pass (which currently only considers
                                // FnDecl hoists) emits a
                                // `Stmt::PreallocateBoxes` at body entry
                                // when at least one nested closure
                                // captures this id. Without it, the box
                                // is lazily created at the late
                                // `var <name> = …` Let statement —
                                // by which point any inner closure
                                // created before the Let has already
                                // snapshot-captured the slot's zero
                                // value (issue #569's classic
                                // sibling-capture symptom, extended to
                                // forward-var captures).
                                hoisted_id_set.insert(id);
                            }
                        }
                    }
                }
            }
        }
        for stmt in &block.stmts {
            if let ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) = stmt {
                if fn_decl.function.body.is_some() && !fn_decl.function.is_generator {
                    let name = fn_decl.ident.sym.to_string();
                    let existing_in_scope = ctx
                        .locals
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(idx, (n, _, _))| n == &name && *idx >= outer_locals_len)
                        .map(|(_, (_, id, _))| *id);
                    let local_id = if let Some(existing) = existing_in_scope {
                        existing
                    } else {
                        ctx.define_local(name, Type::Any)
                    };
                    hoisted_id_set.insert(local_id);
                }
            }
        }
    }

    // Lower body with JS hoisting: only function declarations are fully
    // hoisted per JS semantics (binding + initialization at function
    // entry). `var` bindings are also hoisted, but their *initializer*
    // expressions run at source position — pre-allocating the slot is
    // already handled by `var_hoisted_ids` + the `PreallocateBoxes` pass
    // below. `let`/`const` MUST remain at their lexical position because
    // their init expressions are only evaluated when control flow reaches
    // them — hoisting `const x = fn()` out of a conditional branch would
    // eagerly run the call.
    //
    // Issue #911: previously this pass split `var` declarations into a
    // separate `var_hoisted` bucket and emitted them BEFORE function
    // declarations, so the express CJS-wrap shape
    //   function require(s) { ... }
    //   var { METHODS } = require('node:http');
    // ran the `require('node:http')` call before `require` was bound and
    // threw `TypeError: value is not a function`. Function declarations
    // must run before any var-init in the body, then var-inits and other
    // executable statements run in source order.
    let mut body = if let Some(ref block) = fn_expr.function.body {
        let mut func_decls = Vec::new();
        let mut exec_stmts = Vec::new();
        for stmt in &block.stmts {
            let lowered = crate::lower_decl::lower_body_stmt(ctx, stmt)?;
            match stmt {
                ast::Stmt::Decl(ast::Decl::Fn(_)) => func_decls.extend(lowered),
                _ => exec_stmts.extend(lowered),
            }
        }
        let mut combined: Vec<Stmt> = Vec::with_capacity(func_decls.len() + exec_stmts.len());
        combined.extend(func_decls);
        combined.extend(exec_stmts);
        // Issue #633: prealloc-box for sibling/forward captures.
        if !hoisted_id_set.is_empty() {
            let prealloc = crate::lower_decl::compute_prealloc_for_hoisted_closures(
                &combined,
                &hoisted_id_set,
            );
            if !prealloc.is_empty() {
                let mut with_prealloc: Vec<Stmt> = Vec::with_capacity(combined.len() + 1);
                with_prealloc.push(Stmt::PreallocateBoxes(prealloc));
                with_prealloc.extend(combined);
                combined = with_prealloc;
            }
        }
        combined
    } else {
        Vec::new()
    };
    ctx.current_strict = outer_strict;

    // Prepend destructuring statements to body
    if !destructuring_stmts.is_empty() {
        let mut new_body = destructuring_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

    // Refs #486: same default-param desugar as lower_arrow above.
    let default_stmts = crate::lower_decl::build_default_param_stmts(&params);
    // Record the param-prologue length for generator function expressions
    // (`async function*([x] = d){}`) so the generator transform runs param
    // binding synchronously at call time (spec FunctionDeclarationInstantiation
    // order). See `Module.gen_param_prologue_len`.
    if fn_expr.function.is_generator {
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

    let (captures, mutable_captures) = compute_closure_captures(ctx, &body, &outer_locals, &params);

    // #2076: a named function expression's own ident is its `fn.name`
    // per spec, regardless of the binding identifier it's later assigned
    // to. `const bar = function namedBar(){}` ⇒ `bar.name === "namedBar"`.
    if let Some(ident) = &fn_expr.ident {
        let own_name = ident.sym.to_string();
        if !own_name.is_empty() {
            ctx.closure_display_names.insert(func_id, own_name);
        }
    } else if let Some(name) = ctx.assignment_inferred_name.as_ref() {
        if !name.is_empty() {
            ctx.closure_display_names.insert(func_id, name.clone());
        }
    }

    Ok(Expr::Closure {
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
        is_async: fn_expr.function.is_async,
        is_generator: fn_expr.function.is_generator,
        is_strict,
    })
}

/// Shared closure-capture analysis used by both `lower_arrow` and
/// `lower_fn_expr`. Walks the lowered body, collects every LocalId
/// referenced anywhere, intersects with the outer-scope locals (minus
/// the closure's own parameters), and separates pure captures from
/// mutable captures (those assigned to inside the body, which need
/// boxing). Pre-Tier-2.3 this code was duplicated verbatim across the
/// Arrow and Fn arms; co-locating them lets one helper serve both.
fn compute_closure_captures(
    ctx: &LoweringContext,
    body: &[Stmt],
    outer_locals: &[(String, LocalId)],
    params: &[Param],
) -> (Vec<LocalId>, Vec<LocalId>) {
    // Detect captured variables: locals referenced in the body that
    // were defined in outer scope.
    let mut all_refs = Vec::new();
    let mut visited_closures = std::collections::HashSet::new();
    for stmt in body {
        collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
    }

    // Filter to only include outer locals (not parameters or locals
    // defined within the closure).
    let outer_local_ids: std::collections::HashSet<LocalId> =
        outer_locals.iter().map(|(_, id)| *id).collect();
    let param_ids: std::collections::HashSet<LocalId> = params.iter().map(|p| p.id).collect();

    // dayjs (issue: format() returned `292278994-08`): local IDs are
    // scope-local — each function's `fresh_local()` counter starts at 0,
    // so an inner closure can legitimately reuse an outer-scope id (e.g.
    // dayjs's minified `parseDate` declares `var i = r[2]-1||0` with id
    // 10, while the surrounding IIFE has a module-level `var i = "second"`
    // also at id 10). Without filtering by *inner-declared* ids, the
    // capture detector misidentifies the inner `i` as a free reference
    // to the outer constant and the closure ends up reading "second"
    // where it expected a month. Strip locally-declared ids from the
    // capture set.
    let inner_decls: std::collections::HashSet<LocalId> = {
        let mut s = std::collections::HashSet::new();
        for stmt in body {
            crate::lower_decl::collect_let_decls_in_stmt(stmt, &mut s);
        }
        s
    };

    // Find unique captures: refs that are in outer_locals but not params
    // and not locally re-declared by an inner `let`/`var`.
    let mut captures: Vec<LocalId> = all_refs
        .into_iter()
        .filter(|id| {
            outer_local_ids.contains(id) && !param_ids.contains(id) && !inner_decls.contains(id)
        })
        .collect();
    captures.sort();
    captures.dedup();
    captures = ctx.filter_module_level_captures(captures);

    // Detect which captures are assigned to inside the closure (need boxing).
    let mut all_assigned = Vec::new();
    for stmt in body {
        collect_assigned_locals_stmt(stmt, &mut all_assigned);
    }
    let assigned_set: std::collections::HashSet<LocalId> = all_assigned.into_iter().collect();
    let mutable_captures: Vec<LocalId> = captures
        .iter()
        .filter(|id| {
            (assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
                && !inner_decls.contains(id)
        })
        .copied()
        .collect();

    (captures, mutable_captures)
}
