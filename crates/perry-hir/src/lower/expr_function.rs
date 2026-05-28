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

use crate::analysis::{closure_uses_this, collect_assigned_locals_stmt, collect_local_refs_stmt};
use crate::ir::{Expr, Param, Stmt};
use crate::lower_patterns::{
    generate_param_destructuring_stmts, get_param_default, get_pat_name, get_pat_type,
    is_destructuring_pattern, is_rest_param,
};

use super::{lower_expr, LoweringContext};

pub(super) fn lower_arrow(ctx: &mut LoweringContext, arrow: &ast::ArrowExpr) -> Result<Expr> {
    // Lower arrow function to a closure
    let func_id = ctx.fresh_func();
    let scope_mark = ctx.enter_scope();

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
        let param_default = get_param_default(ctx, param)?;
        let is_rest = is_rest_param(param);
        let param_ty = get_pat_type(param, ctx);
        let param_id = ctx.define_local(param_name.clone(), param_ty.clone());
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_ty,
            default: param_default,
            decorators: Vec::new(),
            is_rest,
        });
        // Track destructuring patterns to generate extraction statements
        if is_destructuring_pattern(param) {
            destructuring_params.push((param_id, param.clone()));
        }
    }

    // Register arrow function parameters with known native types as native instances
    for param in &params {
        if let Type::Named(type_name) = &param.ty {
            let native_info = match type_name.as_str() {
                "PluginApi" => Some(("perry/plugin", "PluginApi")),
                "WebSocket" | "WebSocketServer" => Some(("ws", type_name.as_str())),
                "Redis" => Some(("ioredis", "Redis")),
                "EventEmitter" => Some(("events", "EventEmitter")),
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

    // Hoist function declarations in block body (JS hoisting semantics).
    // Track the hoisted-id set so we can emit `Stmt::PreallocateBoxes`
    // for sibling/forward captures (issue #633).
    //
    // Issue #838 followup (b): see the matching comment in
    // `lower_fn_expr` for the rationale — only reuse an existing local
    // id when the binding is in THIS scope, otherwise dayjs's minified
    // outer-`var M = {…}` / inner-`function M(t){…}` shadow trips the
    // codegen-side global-promotion analysis.
    let outer_locals_len = scope_mark.0;
    let mut hoisted_id_set: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
    if let ast::BlockStmtOrExpr::BlockStmt(block) = &*arrow.body {
        for stmt in &block.stmts {
            if let ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) = stmt {
                // Generator FnDecls go through a different hoist path and
                // aren't closure-bound at the source position.
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
            let mut var_hoisted = Vec::new();
            let mut func_decls = Vec::new();
            let mut exec_stmts = Vec::new();
            for stmt in &block.stmts {
                let lowered = crate::lower_decl::lower_body_stmt(ctx, stmt)?;
                match stmt {
                    ast::Stmt::Decl(ast::Decl::Fn(_)) => func_decls.extend(lowered),
                    ast::Stmt::Decl(ast::Decl::Var(var_decl))
                        if var_decl.kind == ast::VarDeclKind::Var =>
                    {
                        var_hoisted.extend(lowered);
                    }
                    _ => exec_stmts.extend(lowered),
                }
            }
            var_hoisted.extend(func_decls);
            var_hoisted.extend(exec_stmts);
            // Issue #633: if the arrow body has any hoisted FnDecls, run
            // the prealloc-box analysis so sibling-captured FnDecl ids
            // and outer let/const ids referenced from inside the hoisted
            // closure get a pre-allocated box at function entry. Without
            // this, the hoisted closure literal is built before the
            // captured let's `Stmt::Let` runs, and the closure's
            // captures list snapshots the slot's uninitialized value.
            if !hoisted_id_set.is_empty() {
                let prealloc = crate::lower_decl::compute_prealloc_for_hoisted_closures(
                    &var_hoisted,
                    &hoisted_id_set,
                );
                if !prealloc.is_empty() {
                    let mut with_prealloc: Vec<Stmt> = Vec::with_capacity(var_hoisted.len() + 1);
                    with_prealloc.push(Stmt::PreallocateBoxes(prealloc));
                    with_prealloc.extend(var_hoisted);
                    with_prealloc
                } else {
                    var_hoisted
                }
            } else {
                var_hoisted
            }
        }
        ast::BlockStmtOrExpr::Expr(expr) => {
            let return_expr = lower_expr(ctx, expr)?;
            vec![Stmt::Return(Some(return_expr))]
        }
    };

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

    ctx.exit_scope(scope_mark);

    // Exit the type-parameter scope opened at the top of `lower_arrow`.
    // Paired with `enter_type_param_scope` above so nested generic
    // arrows don't leak outer T/U bindings into sibling code.
    ctx.exit_type_param_scope();

    let (captures, mutable_captures) = compute_closure_captures(ctx, &body, &outer_locals, &params);

    // Check if this arrow function uses `this` (needs to capture it from enclosing scope)
    let captures_this = closure_uses_this(&body);

    // Store enclosing class name for arrow functions that capture `this`
    let enclosing_class = if captures_this {
        ctx.current_class.clone()
    } else {
        None
    };

    Ok(Expr::Closure {
        func_id,
        params,
        return_type: Type::Any,
        body,
        captures,
        mutable_captures,
        captures_this,
        enclosing_class,
        is_async: arrow.is_async,
        is_generator: false,
    })
}

pub(crate) fn lower_fn_expr(ctx: &mut LoweringContext, fn_expr: &ast::FnExpr) -> Result<Expr> {
    // Lower function expression to a closure (similar to arrow but
    // without `this` capture — function expressions have their own
    // `this` binding determined by how they're called).
    let func_id = ctx.fresh_func();
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
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &fn_expr.function.params {
        let param_name = get_pat_name(&param.pat)?;
        if param_name == "this" {
            // TS `this:` annotation — skip; it's type-only.
            continue;
        }
        let param_default = get_param_default(ctx, &param.pat)?;
        let is_rest = is_rest_param(&param.pat);
        let param_id = ctx.define_local(param_name.clone(), Type::Any);
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: Type::Any,
            default: param_default,
            decorators: Vec::new(),
            is_rest,
        });
        // Track destructuring patterns to generate extraction statements
        if is_destructuring_pattern(&param.pat) {
            destructuring_params.push((param_id, param.pat.clone()));
        }
    }

    // #677: synthesize `arguments` for non-arrow function expressions when the
    // body references it. Function expressions get their own `arguments`
    // binding per spec — they don't inherit from the enclosing scope.
    let user_has_arguments_param = fn_expr
        .function
        .params
        .iter()
        .any(|p| get_pat_name(&p.pat).ok().as_deref() == Some("arguments"));
    let user_has_rest = fn_expr
        .function
        .params
        .iter()
        .any(|p| is_rest_param(&p.pat));
    let needs_arguments_synth = !user_has_arguments_param
        && !user_has_rest
        && fn_expr
            .function
            .body
            .as_ref()
            .map(|b| crate::lower_decl::body_uses_arguments(&b.stmts))
            .unwrap_or(false);
    if needs_arguments_synth {
        crate::lower_decl::append_synthetic_arguments_param(ctx, &mut params);
    }

    // Generate Let statements for destructuring patterns BEFORE lowering body
    let mut destructuring_stmts = Vec::new();
    for (param_id, pat) in &destructuring_params {
        let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
        destructuring_stmts.extend(stmts);
    }

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

    // Prepend destructuring statements to body
    if !destructuring_stmts.is_empty() {
        let mut new_body = destructuring_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

    // Refs #486: same default-param desugar as lower_arrow above.
    let default_stmts = crate::lower_decl::build_default_param_stmts(&params);
    if !default_stmts.is_empty() {
        let mut new_body = default_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

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
    }

    Ok(Expr::Closure {
        func_id,
        params,
        return_type: Type::Any,
        body,
        captures,
        mutable_captures,
        captures_this: false,
        enclosing_class: None,
        is_async: fn_expr.function.is_async,
        is_generator: fn_expr.function.is_generator,
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
