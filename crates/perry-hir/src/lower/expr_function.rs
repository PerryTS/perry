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
    // #6604: truncate mark for capturing class expressions recorded while
    // lowering THIS arrow — placed at scope entry so default-param
    // expressions are covered too; see the truncate below the body match.
    let body_class_expr_captures_mark = ctx.body_class_expr_captures.len();
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

    // Lower parameters and collect destructuring info
    let mut params = Vec::new();
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &arrow.params {
        let param_name = get_pat_name(param)?;
        let is_rest = is_rest_param(param);
        let param_ty = get_pat_type(param, ctx);
        let param_id = ctx.define_local(param_name.clone(), param_ty.clone());
        ctx.shadow_native_instance_if_present(&param_name);
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
            // #6003: a param typed with a USER class that shares a native
            // name (`class Headers { ... }; (h: Headers) => ...`) must
            // dispatch through the user class, not the native FFI.
            if ctx.lookup_class(type_name).is_some() {
                continue;
            }
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
    // #6604: a capturing class expression in an EXPRESSION-bodied arrow
    // (`x => new (class { … })(x)`) records a body-class-expr entry that no
    // body twin will drain (the block-bodied arm drains its own inside
    // `lower_fn_body_block_stmt`; default-param entries are self-truncated by
    // `get_param_default`). Truncate on exit so the entry — whose ids are
    // only meaningful in the arrow's own local numbering — never leaks into
    // the ENCLOSING body's refresh statements. Nothing is lost: a
    // single-expression body has no later statements that could reassign the
    // class's captured locals.
    ctx.body_class_expr_captures
        .truncate(body_class_expr_captures_mark);
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

    // The closure's own scope has been popped, so `ctx.locals.id_set()` is now
    // exactly the enclosing scope's live locals — the membership view capture
    // analysis needs. (Previously rebuilt per closure from a cloned snapshot.)
    let (captures, mutable_captures) =
        compute_closure_captures(ctx, &body, ctx.locals.id_set(), &params);

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

/// #5126: a named function expression binds its own name as a read-only
/// local *inside its own body* (the FunctionExpression name scope per
/// spec) — `const fact = function f(n){ return n<=1?1:n*f(n-1); }` must
/// see `f` from within the body even though the outer binding is `fact`.
///
/// We model this without a new HIR node by wrapping the (otherwise
/// anonymous) function in an immediately-invoked arrow that binds the
/// name to the function value:
///   (() => { let f = <function expr>; return f; })()
/// The `let f = <closure that references f>` shape is exactly the
/// self-recursive-`const` pattern that `collect_boxed_vars` already
/// boxes (step 5: a `Stmt::Let` whose `Closure` init references the
/// Let's own id). So `f` resolves to the function through its heap box,
/// reusing the proven recursion machinery.
pub(crate) fn lower_fn_expr(ctx: &mut LoweringContext, fn_expr: &ast::FnExpr) -> Result<Expr> {
    // A named function expression with a non-empty ident may reference its
    // own name from within its body. Lower it through the self-binding path,
    // which keeps the IIFE wrapper only when the body actually captures the
    // name (so plain `function f(){...}` and the synthetic `Function(...)`
    // body stay a bare `Closure`).
    if let Some(ident) = &fn_expr.ident {
        let own_name = ident.sym.to_string();
        if !own_name.is_empty() {
            return lower_named_fn_expr(ctx, fn_expr, own_name);
        }
    }
    lower_fn_expr_anon(ctx, fn_expr)
}

/// Lower a *named* function expression, binding its own name inside the
/// body. We put the name in scope, lower the function anonymously, and
/// inspect whether the lowered closure actually captured the name. If it
/// didn't (the common case — no recursive self-reference), we discard the
/// scaffolding and return the bare closure unchanged. If it did, we wrap
/// it in an immediately-invoked arrow that binds the name to the function
/// value:
///   (() => { let f = <function expr>; return f; })()
/// The `let f = <closure that references f>` shape is exactly the
/// self-recursive-`const` pattern that `collect_boxed_vars` already boxes
/// (a `Stmt::Let` whose `Closure` init references the Let's own id), so the
/// name resolves to the function through its heap box — reusing the proven
/// recursion machinery without a dedicated HIR node.
fn lower_named_fn_expr(
    ctx: &mut LoweringContext,
    fn_expr: &ast::FnExpr,
    own_name: String,
) -> Result<Expr> {
    // Wrapper scope: holds just the self-binding local. Collect the
    // enclosing scope's locals first so they (not the self-binding) are
    // what the wrapper itself captures and threads through to the inner
    // function.
    let wrapper_scope = ctx.enter_scope();
    // Snapshot the enclosing locals *before* the self-binding is added, so the
    // wrapper captures them (not the self-binding). Unlike the arrow/fn-expr
    // paths, capture analysis runs here while the wrapper scope is still open
    // (the self-binding lives in it), so we can't use `ctx.locals.id_set()` —
    // it would wrongly include `self_id`. This is a rare path (named function
    // expressions that recursively self-reference), so an explicit snapshot is
    // fine.
    let outer_local_ids: std::collections::HashSet<LocalId> =
        ctx.locals.iter().map(|(_, id, _)| *id).collect();
    let self_id = ctx.define_local(own_name.clone(), Type::Any);

    // Lower the function itself as an anonymous closure. With `self_id`
    // already in scope, any reference to the name inside the body resolves
    // to it (correctly shadowing any outer binding of the same name) and is
    // captured.
    let inner = lower_fn_expr_anon(ctx, fn_expr)?;

    let self_referenced =
        matches!(&inner, Expr::Closure { captures, .. } if captures.contains(&self_id));
    if !self_referenced {
        // No recursive self-reference — drop the scaffolding.
        ctx.exit_scope(wrapper_scope);
        return Ok(inner);
    }

    let wrapper_func_id = ctx.fresh_func();
    let body = vec![
        Stmt::Let {
            id: self_id,
            name: own_name,
            ty: Type::Any,
            mutable: false,
            init: Some(inner),
        },
        Stmt::Return(Some(Expr::LocalGet(self_id))),
    ];
    let (captures, mutable_captures) = compute_closure_captures(ctx, &body, &outer_local_ids, &[]);
    ctx.exit_scope(wrapper_scope);

    Ok(Expr::Call {
        callee: Box::new(Expr::Closure {
            func_id: wrapper_func_id,
            params: Vec::new(),
            return_type: Type::Any,
            body,
            captures,
            mutable_captures,
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: ctx.current_strict,
        }),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    })
}

fn lower_fn_expr_anon(ctx: &mut LoweringContext, fn_expr: &ast::FnExpr) -> Result<Expr> {
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
    // #6604: capturing class EXPRESSIONS lowered in THIS function register
    // from here for the end-of-body refresh (twin of
    // `lower_fn_body_block_stmt`); the mark sits at scope entry so nothing
    // recorded for this function can leak into the enclosing body.
    // (Default-param entries never reach the drain — `get_param_default`
    // self-truncates.)
    let body_class_expr_captures_mark = ctx.body_class_expr_captures.len();
    // A plain function has its own `arguments` object, so a direct `eval`
    // inside its body may reference `arguments` even when the function sits
    // in a class field initializer. Cleared here, restored at the end.
    let saved_field_init = ctx.in_class_field_init;
    ctx.in_class_field_init = false;
    let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
    ctx.in_nonarrow_fn = true;

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
        ctx.shadow_native_instance_if_present(&param_name);
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

    // Annex B B.3.3 (#5297): this function-expression body owns its own
    // block-nested-function `var` map; take the enclosing one aside and restore
    // it on exit (mirrors `lower_fn_body_block_stmt`). The nested-var pass below
    // repopulates it for this body.
    let saved_annexb_block_fn_var_ids = std::mem::take(&mut ctx.annexb_block_fn_var_ids);
    let saved_annexb_block_fn_names_all = std::mem::take(&mut ctx.annexb_block_fn_names_all);
    // Nested `function*` declarations forward-referenced by an earlier sibling
    // in this function-expression body (the cjs_wrap IIFE: `pathToRegexp` calls
    // `flatten`, declared below it) must use the closure-lowering path. Scope
    // the set to this body and restore on exit.
    let saved_nested_gen_fwd = std::mem::take(&mut ctx.nested_generator_forward_referenced);
    ctx.nested_generator_forward_referenced = fn_expr
        .function
        .body
        .as_ref()
        .map(|b| {
            crate::lower_decl::forward_referenced_nested_generators(&b.stmts)
                .into_iter()
                .collect()
        })
        .unwrap_or_default();

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
    // Forward-captured `let`/`const` boxes pre-registered for THIS fn-expr body
    // (the cjs `const _cjs = (function(){…})()` wrapper) — see
    // `pre_register_forward_captured_lets`. Kept out of `hoisted_id_set` and
    // preallocated directly at the assembly below.
    let mut forward_boxed_ids: Vec<LocalId> = Vec::new();
    // #4950: undefined-initialised `Stmt::Let`s for `var`s found nested in
    // compound statements — prepended to the lowered body below.
    let mut nested_var_prologue: Vec<Stmt> = Vec::new();
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
                        // Collect every binding name introduced by this
                        // declarator. Plain `var x` is a single `Pat::Ident`;
                        // a destructuring `var { t: dSq } = re()` (the esbuild
                        // semver shape `var {safeRe:QSq,t:dSq}=bV6()`) binds
                        // `dSq` via a `Pat::Object` — which the old
                        // `Pat::Ident`-only arm skipped, so the destructured
                        // binding was never pre-registered/hoisted. A class
                        // method or sibling function created BEFORE the
                        // destructuring decl then snapshot-captured the
                        // not-yet-defined slot and read `undefined`
                        // (`Cannot read properties of undefined`). Walk the
                        // whole pattern so destructured `var` bindings get the
                        // same forward-capture box as plain-ident ones.
                        let mut names = Vec::new();
                        crate::lower_decl::collect_var_binding_names_from_pat(
                            &decl.name, &mut names,
                        );
                        for name in names {
                            let already_in_scope = ctx
                                .locals
                                .lookup_index_in_scope(&name, outer_locals_len)
                                .is_some();
                            if !already_in_scope {
                                let id = ctx.define_local(name.clone(), Type::Any);
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
                                // Emit an undefined-initialised entry slot,
                                // exactly like the nested-`var` pre-pass
                                // below (and `predefine_var_bindings_in_
                                // function_body` on the fn-decl/arrow path).
                                // The `hoisted_id_set` box above only covers
                                // the forward-CAPTURE case (a closure created
                                // before the decl reads the box). A `var`
                                // merely READ/WRITTEN before its own textual
                                // declaration but NOT captured by any closure
                                // gets no prealloc box, so without this Let
                                // its slot first materialises at the late
                                // `var <name> = …` and every earlier read/
                                // write folds to `undefined`. React's
                                // cloneElement/createElement do exactly this:
                                //   for (propName in o) …;  var propName = …;
                                // one hoisted `propName` used by the for-in
                                // loop, then redeclared as a number — so the
                                // loop body's `hasOwnProperty.call(o, propName)`
                                // saw `undefined` and dropped every prop.
                                nested_var_prologue.push(Stmt::Let {
                                    id,
                                    name,
                                    ty: Type::Any,
                                    mutable: true,
                                    init: Some(Expr::Undefined),
                                });
                            }
                        }
                    }
                }
            }
        }
        for stmt in &block.stmts {
            if let ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) = stmt {
                let name = fn_decl.ident.sym.to_string();
                // Non-generator fn-decls hoist as before. GENERATOR fn-decls
                // only need a pre-defined+hoisted local when they take the
                // closure-lowering path AND are forward-referenced by an earlier
                // sibling (path-to-regexp's `pathToRegexp` → `flatten`): the
                // closure path emits `let <name> = Closure`, which must be
                // pre-defined so the earlier reference resolves to the local and
                // hoisted (in `hoisted_id_set`) so the binding runs before that
                // reference. A generator that takes the TOP-LEVEL path (the
                // common, non-forward-referenced case) must NOT be pre-defined
                // here — boxing/hoisting its `FuncRef` binding would make its own
                // recursive self-call read a boxed slot instead of the callable
                // `FuncRef` (`TypeError: value is not a function`).
                let take = if fn_decl.function.body.is_none() {
                    false
                } else if fn_decl.function.is_generator {
                    ctx.nested_generator_forward_referenced.contains(&name)
                } else {
                    true
                };
                if take {
                    let existing_in_scope = ctx
                        .locals
                        .lookup_index_in_scope(&name, outer_locals_len)
                        .map(|pos| ctx.locals[pos].1);
                    let local_id = if let Some(existing) = existing_in_scope {
                        existing
                    } else {
                        ctx.define_local(name, Type::Any)
                    };
                    hoisted_id_set.insert(local_id);
                }
            }
        }
        // #4973: pre-register top-level `let`/`const` Ident bindings of this
        // function body so a hoisted sibling FUNCTION that references them
        // before their lexical position binds the (boxed) function-scope
        // local instead of falling through to a global read. The classic
        // Node test shape (test-http-upgrade-server):
        //   function t() { … server.close(); }
        //   const server = createTestServer();
        //   server.listen(0, () => t());
        // JS hoists the *binding* (with a TDZ Perry is lax about); pre-fix,
        // `server` inside `t` lowered to a globalThis read → undefined.
        // Gated on the body containing at least one hoisted function
        // declaration — the only consumers that can legally observe the
        // binding before its source position — to bound the blast radius.
        // Keyed by declarator-ident span: the Let site in var_decl.rs reuses
        // the id only for the *exact* declarator (`lexical_forward_decls`),
        // so a shadowing `const` in an inner block still gets a fresh
        // binding.
        let body_has_fn_decl = block
            .stmts
            .iter()
            .any(|s| matches!(s, ast::Stmt::Decl(ast::Decl::Fn(_))));
        if body_has_fn_decl {
            for stmt in &block.stmts {
                if let ast::Stmt::Decl(ast::Decl::Var(var_decl)) = stmt {
                    if matches!(
                        var_decl.kind,
                        ast::VarDeclKind::Let | ast::VarDeclKind::Const
                    ) {
                        for decl in &var_decl.decls {
                            if let ast::Pat::Ident(ident) = &decl.name {
                                let name = ident.id.sym.to_string();
                                let already_in_scope = ctx
                                    .locals
                                    .lookup_index_in_scope(&name, outer_locals_len)
                                    .is_some();
                                if !already_in_scope {
                                    let id = ctx.define_local(name, Type::Any);
                                    // Boxed-capture semantics: a closure
                                    // created before the init must see the
                                    // post-init value through the box, not a
                                    // snapshot of the empty slot.
                                    ctx.var_hoisted_ids.insert(id);
                                    hoisted_id_set.insert(id);
                                    // Lexical let/const forward-decl: TDZ-eligible.
                                    // A read before the declaration runs throws.
                                    ctx.tdz_forward_ids.insert(id);
                                    ctx.lexical_forward_decls.insert(ident.id.span.lo.0, id);
                                }
                            }
                        }
                    }
                }
            }
        }
        // #4950: pre-register `var` bindings nested inside compound
        // statements (if/else arms, loops, try/catch, switch) of this
        // function body. `var` is function-scoped, so react's
        //   if (cond) { var getCurrentTime = function () {…}; }
        //   else { getCurrentTime = function () {…}; }
        // (react-reconciler's time source, factory shape) must resolve the
        // else-arm write AND sibling-closure reads to ONE hoisted local.
        // The top-level pre-pass above only saw direct `Stmt::Decl(Var)`
        // children, so the if-arm declaration never registered, the
        // else-arm assignment fell through to an implicit-global write,
        // and `getCurrentTime()` inside prepareFreshStack threw
        // `getCurrentTime is not defined` — silently swallowed by the
        // scheduler pump, killing every React render. Mirrors the
        // module-level nested-var hoisting in lower_module_fn.rs.
        for stmt in &block.stmts {
            // Direct top-level var decls are handled by the pre-pass above;
            // only walk into compound statements for nested `var`s here.
            if matches!(stmt, ast::Stmt::Decl(_)) {
                continue;
            }
            let mut names = Vec::new();
            crate::lower_decl::collect_var_binding_names_from_stmt(stmt, &mut names);
            names.sort();
            names.dedup();
            for name in names {
                let already_in_scope = ctx
                    .locals
                    .lookup_index_in_scope(&name, outer_locals_len)
                    .is_some();
                if !already_in_scope {
                    let id = ctx.define_local(name.clone(), Type::Any);
                    ctx.var_hoisted_ids.insert(id);
                    hoisted_id_set.insert(id);
                    // Emit an explicit undefined-initialised slot at body
                    // entry (same rationale as the module-level pass: a
                    // read or LocalSet compiled before the nested decl
                    // needs storage to exist; the nested `Stmt::Let`
                    // later reuses the slot via the var-redeclaration
                    // path).
                    nested_var_prologue.push(Stmt::Let {
                        id,
                        name,
                        ty: Type::Any,
                        mutable: true,
                        init: Some(Expr::Undefined),
                    });
                }
            }
        }
        // Annex B B.3.3 (#5297): a block-nested `function f(){}` in this sloppy
        // function-expression body also gets an enclosing-scope `var f`
        // (undefined until the declaration runs). Register one hoisted slot per
        // such name and record name -> slot so the block-nested declaration
        // writes the closure into it while keeping its block-local binding
        // independent. The IIFE wrapper `(function(){ { function f(){} } f(); }())`
        // is exactly the test262 `annexB/.../function-code` shape. The legacy
        // `var` is skipped when the name collides with a parameter or a lexical
        // binding (it would make `var f` an early error) — `forbidden` carries
        // the parameter names, the body's top-level lexical names, and
        // `arguments`; nested blocks add their own lexical names as we descend.
        if !ctx.current_strict {
            let mut forbidden: std::collections::HashSet<String> =
                params.iter().map(|p| p.name.clone()).collect();
            crate::lower_decl::collect_lexical_decl_names(&block.stmts, &mut forbidden);
            forbidden.insert("arguments".to_string());

            let mut all_names = Vec::new();
            let mut names = Vec::new();
            crate::lower_decl::collect_annexb_block_fn_decl_names(
                &block.stmts,
                &forbidden,
                &mut all_names,
                &mut names,
            );
            ctx.annexb_block_fn_names_all.extend(all_names);
            names.sort();
            names.dedup();
            for name in names {
                // Reuse an existing in-scope `var` (parameters are excluded by
                // `forbidden`, so any same-name binding here is a hoisted
                // `var`); otherwise mint a fresh hoisted slot. Either way emit
                // an undefined-init entry slot: a direct top-level `var f = …`
                // in a function expression has no entry `Let` (only its source-
                // position one), but the block's B.3.3 write runs BEFORE that
                // position, so the slot must already exist (#5297
                // `existing-var-update`).
                let id =
                    if let Some(pos) = ctx.locals.lookup_index_in_scope(&name, outer_locals_len) {
                        ctx.locals[pos].1
                    } else {
                        ctx.define_local(name.clone(), Type::Any)
                    };
                nested_var_prologue.push(Stmt::Let {
                    id,
                    name: name.clone(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Undefined),
                });
                ctx.var_hoisted_ids.insert(id);
                ctx.annexb_block_fn_var_ids.insert(name, id);
            }
        }
        // Forward-captured `let`/`const` (incl. destructuring) referenced by an
        // EARLIER closure than their declaration. The `#4973` pass above only
        // covers `Pat::Ident` and only when the body has a `function`
        // declaration; the cjs IIFE's `_export(exports, { SpanKind: () =>
        // SpanKind })` getter forward-captures the later `const { SpanKind } =
        // api` (Next.js tracer), which it misses. Shared with arrow / fn-decl
        // bodies (`lower_fn_body_block_stmt`). Bindings already pre-registered
        // above are skipped (the `already_in_scope` guard inside).
        forward_boxed_ids =
            crate::lower_decl::pre_register_forward_captured_lets(ctx, block, outer_locals_len);
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
    // Pre-register sibling class DECLARATION names so forward references in
    // earlier statements (and nested closures lowered before the class) resolve
    // to `ClassRef` rather than the unknown-global sentinel — the same Phase 1.5
    // that `lower_fn_body_block_stmt` (arrow / fn-decl bodies) performs. Plain
    // function expressions previously skipped it: the cjs_wrap IIFE is exactly
    // such an expression, and a class it can't hoist out (one whose body
    // references an IIFE-local, e.g. `class X extends imp.Base { constructor(){
    // super(imp2.CONST) } }`) stays inside the IIFE with its export getter
    // `() => X` lowered ABOVE it — that forward read fell through to
    // `js_global_get_or_throw_unresolved("X")` → `ReferenceError: X is not
    // defined` (Next.js RSCPathnameNormalizer). Scoped: restored after the body.
    let saved_forward_class_names = ctx.forward_class_names.clone();
    let saved_forward_class_decl_depth = ctx.forward_class_decl_depth.clone();
    let saved_class_renames = ctx.class_renames.clone();
    let fn_body_scope_depth = ctx.scope_depth;
    if let Some(ref block) = fn_expr.function.body {
        for stmt in &block.stmts {
            if let ast::Stmt::Decl(ast::Decl::Class(class_decl)) = stmt {
                // Disambiguate a distinct same-named class (the cjs/ncc IIFE
                // shape `(function(e){…class s{…}…})(t)` declares superstruct's
                // `Struct` = `class s`, which collided with other `class s` in
                // the bundle and was dedup-skipped). See `class_renames`.
                ctx.maybe_rename_colliding_class(class_decl.ident.sym.as_str());
                let cname = class_decl.ident.sym.to_string();
                ctx.forward_class_decl_depth
                    .entry(cname.clone())
                    .and_modify(|d| *d = (*d).min(fn_body_scope_depth))
                    .or_insert(fn_body_scope_depth);
                ctx.forward_class_names.insert(cname);
            }
        }
    }
    let mut body = if let Some(ref block) = fn_expr.function.body {
        // #4795: a `using` / `await using` declaration in a function-expression
        // body must be desugared (scope-exit disposal + declaration-time
        // disposability check). The hand-rolled per-statement loop below routes
        // through `lower_body_stmt`, which lowers `using` as a plain `const`
        // with no disposal — so route the whole body through the using-aware
        // lowering (the same path arrow/fn-decl bodies use) when any top-level
        // `using` is present. Forward references stay resolvable via the
        // FnDecl/var pre-registration done above.
        let has_using = block
            .stmts
            .iter()
            .any(|s| matches!(s, ast::Stmt::Decl(ast::Decl::Using(_))));
        if has_using {
            crate::lower_decl::lower_stmts_using_aware(ctx, &block.stmts)?
        } else {
            let mut func_decls = Vec::new();
            let mut exec_stmts = Vec::new();
            for stmt in &block.stmts {
                let lowered = crate::lower_decl::lower_body_stmt(ctx, stmt)?;
                match stmt {
                    ast::Stmt::Decl(ast::Decl::Fn(_)) => func_decls.extend(lowered),
                    _ => exec_stmts.extend(lowered),
                }
            }
            let mut combined: Vec<Stmt> =
                Vec::with_capacity(nested_var_prologue.len() + func_decls.len() + exec_stmts.len());
            // Nested-var undefined slots first so every later read/write —
            // including from hoisted function-declaration closures — sees
            // initialised storage (#4950).
            combined.extend(std::mem::take(&mut nested_var_prologue));
            combined.extend(func_decls);
            combined.extend(exec_stmts);
            // Issue #633: prealloc-box for sibling/forward captures. Merge the
            // forward-captured `let`/`const` boxes (kept out of `hoisted_id_set`
            // to avoid hoist-reordering their non-hoistable declarations) so
            // their boxes exist before the earlier capturing closure literal.
            if !hoisted_id_set.is_empty() || !forward_boxed_ids.is_empty() {
                let mut prealloc = crate::lower_decl::compute_prealloc_for_hoisted_closures(
                    &combined,
                    &hoisted_id_set,
                );
                for id in &forward_boxed_ids {
                    if !prealloc.contains(id) {
                        prealloc.push(*id);
                    }
                }
                prealloc.sort();
                // Split TDZ-seeded lexical `let`/`const` boxes from ordinary
                // boxes (see block.rs Phase 5 for the rationale).
                let mut tdz_prealloc: Vec<LocalId> = Vec::new();
                let mut plain_prealloc: Vec<LocalId> = Vec::new();
                for id in prealloc {
                    if ctx.tdz_forward_ids.contains(&id) {
                        tdz_prealloc.push(id);
                    } else {
                        plain_prealloc.push(id);
                    }
                }
                if !plain_prealloc.is_empty() || !tdz_prealloc.is_empty() {
                    let mut with_prealloc: Vec<Stmt> = Vec::with_capacity(combined.len() + 2);
                    if !plain_prealloc.is_empty() {
                        with_prealloc.push(Stmt::PreallocateBoxes(plain_prealloc));
                    }
                    if !tdz_prealloc.is_empty() {
                        with_prealloc.push(Stmt::PreallocateTdzBoxes(tdz_prealloc));
                    }
                    with_prealloc.extend(combined);
                    combined = with_prealloc;
                }
            }
            combined
        }
    } else {
        Vec::new()
    };

    // Mirror `lower_fn_body_block_stmt`'s (block.rs) end-of-body class-capture
    // re-registration for FUNCTION-EXPRESSION bodies — which previously skipped
    // it (arrow / fn-decl bodies already do this; function expressions, incl.
    // the cjs_wrap IIFE `(function() { … })()`, did not). The decl-site
    // `RegisterClassCaptures` snapshot is taken at the class's declaration
    // position, which runs BEFORE later statements assign captured vars: the
    // ubiquitous tsc computed-member emit `var _a; class C { [_a]=… }; _a =
    // Symbol.for(…)` assigns `_a` AFTER the class, so the decl-site snapshot
    // recorded `undefined`. A statically-resolved `new C(…)` appends the live
    // value and is fine, but a DYNAMICALLY-resolved construct (`new ns.C(…)`
    // cross-module — how NestJS builds `InstanceWrapper`) appends no cap arg and
    // falls back to that stale snapshot → captured field reads `undefined`.
    // Refresh the snapshot with the FINAL values at body end (inserted before a
    // trailing `return`).
    if let Some(ref block) = fn_expr.function.body {
        let mut re_regs: Vec<Stmt> = Vec::new();
        let mut re_reg_capsets: Vec<(Stmt, std::collections::HashSet<LocalId>)> = Vec::new();
        for stmt in &block.stmts {
            if let ast::Stmt::Decl(ast::Decl::Class(class_decl)) = stmt {
                // A colliding `class X` may have been renamed during body
                // lowering; captures + `new` are registered under the resolved
                // name, so use it here too (the raw AST name would miss).
                let cname = ctx.resolve_class_name(class_decl.ident.sym.as_str());
                if let Some(captured) = ctx.lookup_class_captures(&cname) {
                    if !captured.is_empty() {
                        let captures: Vec<Expr> =
                            captured.iter().map(|id| Expr::LocalGet(*id)).collect();
                        let cap_args: Vec<(LocalId, LocalId)> =
                            captured.iter().map(|id| (*id, *id)).collect();
                        for s in body.iter_mut() {
                            crate::lower_decl::append_new_args_stmt(s, &cname, &cap_args, true);
                        }
                        let re_reg = Stmt::Expr(Expr::RegisterClassCaptures {
                            class_name: cname,
                            captures,
                        });
                        re_reg_capsets.push((re_reg.clone(), captured.iter().copied().collect()));
                        re_regs.push(re_reg);
                    }
                }
            }
        }
        // #6604: capturing class EXPRESSIONS lowered directly in this body —
        // the semver/esbuild `__commonJS` wrapper shape `var Comparator =
        // class _Comparator { … }; …; var parseOptions = require_…()` — join
        // the same refresh machinery as class declarations, so the snapshot
        // tracks captured vars assigned AFTER the class. Recorded by
        // `lower_class_expr` under the RESOLVED registration name; see the
        // block-body twin (`lower_fn_body_block_stmt`) for why no
        // `append_new_args_stmt` pass runs for expressions.
        for (cname, ids) in ctx
            .body_class_expr_captures
            .split_off(body_class_expr_captures_mark)
        {
            let captures: Vec<Expr> = ids.iter().map(|id| Expr::LocalGet(*id)).collect();
            let re_reg = Stmt::Expr(Expr::RegisterClassCaptures {
                class_name: cname,
                captures,
            });
            re_reg_capsets.push((re_reg.clone(), ids.iter().copied().collect()));
            re_regs.push(re_reg);
        }
        if !re_regs.is_empty() {
            // Audit P0-B twin of the block-body path: refresh after every
            // same-body assignment to a captured local so mid-body constructs
            // read the live value through the authoritative snapshot.
            insert_class_capture_refresh_after_assignments(&mut body, &re_reg_capsets);
            // Refresh the snapshot before EVERY reachable `return` in the body
            // (not only a trailing one): an EARLY `return <class>` after the
            // captured locals are assigned would otherwise return a class with
            // the stale declaration-time snapshot. The walk descends statement
            // children (if/loops/try/switch/labeled) but NOT into nested
            // closures — their `return`s belong to a different function.
            insert_class_capture_refresh_before_returns(&mut body, &re_regs);
            // Fallthrough (implicit return at body end). When the body already
            // ends in a `return`, the walk above handled it; otherwise append so
            // a no-early-return fallthrough path also records the final values.
            if !matches!(body.last(), Some(Stmt::Return(_))) {
                body.extend(re_regs.iter().cloned());
            }
        }
    }
    ctx.current_strict = outer_strict;
    ctx.annexb_block_fn_var_ids = saved_annexb_block_fn_var_ids;
    ctx.annexb_block_fn_names_all = saved_annexb_block_fn_names_all;
    ctx.forward_class_names = saved_forward_class_names;
    ctx.forward_class_decl_depth = saved_forward_class_decl_depth;
    ctx.class_renames = saved_class_renames;
    ctx.nested_generator_forward_referenced = saved_nested_gen_fwd;

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
    ctx.in_class_field_init = saved_field_init;
    ctx.in_nonarrow_fn = saved_in_nonarrow_fn;

    // Scope popped: `ctx.locals.id_set()` is now the enclosing scope's locals.
    let (captures, mutable_captures) =
        compute_closure_captures(ctx, &body, ctx.locals.id_set(), &params);

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

/// Insert a copy of `re_regs` (class-capture refresh statements) immediately
/// before EVERY reachable `Stmt::Return` in `stmts`, descending into nested
/// statement bodies (if/loops/try/switch/labeled) but NOT into nested closures
/// — a closure's `return` exits a different function and must keep its own
/// snapshot. Each return path then records the live capture values at that
/// point. See the call site in `lower_fn_expr_anon` (CodeRabbit #5739).
pub(crate) fn insert_class_capture_refresh_before_returns(stmts: &mut Vec<Stmt>, re_regs: &[Stmt]) {
    let mut i = 0;
    while i < stmts.len() {
        insert_class_capture_refresh_into_stmt(&mut stmts[i], re_regs);
        if matches!(&stmts[i], Stmt::Return(_)) {
            for (j, s) in re_regs.iter().cloned().enumerate() {
                stmts.insert(i + j, s);
            }
            i += re_regs.len();
        }
        i += 1;
    }
}

/// Recurse into a single statement's child statement lists for
/// [`insert_class_capture_refresh_before_returns`].
fn insert_class_capture_refresh_into_stmt(stmt: &mut Stmt, re_regs: &[Stmt]) {
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            insert_class_capture_refresh_before_returns(then_branch, re_regs);
            if let Some(eb) = else_branch {
                insert_class_capture_refresh_before_returns(eb, re_regs);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            insert_class_capture_refresh_before_returns(body, re_regs);
        }
        Stmt::For { init, body, .. } => {
            if let Some(init) = init {
                insert_class_capture_refresh_into_stmt(init, re_regs);
            }
            insert_class_capture_refresh_before_returns(body, re_regs);
        }
        Stmt::Labeled { body, .. } => insert_class_capture_refresh_into_stmt(body, re_regs),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            insert_class_capture_refresh_before_returns(body, re_regs);
            if let Some(c) = catch {
                insert_class_capture_refresh_before_returns(&mut c.body, re_regs);
            }
            if let Some(f) = finally {
                insert_class_capture_refresh_before_returns(f, re_regs);
            }
        }
        Stmt::Switch { cases, .. } => {
            for c in cases {
                insert_class_capture_refresh_before_returns(&mut c.body, re_regs);
            }
        }
        _ => {}
    }
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
    outer_local_ids: &std::collections::HashSet<LocalId>,
    params: &[Param],
) -> (Vec<LocalId>, Vec<LocalId>) {
    // Detect captured variables: locals referenced in the body that
    // were defined in outer scope.
    let mut all_refs = Vec::new();
    let mut visited_closures = std::collections::HashSet::new();
    for stmt in body {
        collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
    }

    // `outer_local_ids` is the membership view of the enclosing scope's
    // locals, supplied by the caller (the live `ctx.locals.id_set()` once the
    // closure's own scope has been popped). Previously this was rebuilt into a
    // fresh `HashSet` from an `&[(String, LocalId)]` snapshot on *every*
    // closure — O(scope) per closure, i.e. O(n²) over n sibling closures in a
    // large scope. We only ever need membership tests here, so the caller's
    // incrementally-maintained set is reused directly.
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

/// Insert a class-capture refresh immediately AFTER every statement that
/// assigns one of the class's captured locals (2026-07-02 audit capture
/// P0-B). The decl-site snapshot is AUTHORITATIVE at construct time (the
/// appended cap arg can be a mis-boxed multi-level capture — the W6 class),
/// so a same-body assignment after the class declaration must update the
/// snapshot or every later construct reads the stale decl-time value and
/// `emit_class_capture_writeback` then resets the outer local to it:
///
///   let x = 1;
///   class C { m() { return x; } }
///   x = 2;                    // ← refresh inserted after this statement
///   new C().m()               // reads 2 (was: 1, and x reset to 1)
///
/// Descends into nested statement bodies (assignments inside if/loop arms
/// get their refresh at that level) but not into closures — a closure's
/// assignment happens at ITS call time, not lexically here. Refreshes
/// inserted before the class's own declaration are harmless: the decl-site
/// registration overwrites them in program order.
pub(crate) fn insert_class_capture_refresh_after_assignments(
    stmts: &mut Vec<Stmt>,
    regs: &[(Stmt, std::collections::HashSet<perry_types::LocalId>)],
) {
    let mut i = 0;
    while i < stmts.len() {
        // Recurse first so nested bodies get their own refreshes.
        match &mut stmts[i] {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                insert_class_capture_refresh_after_assignments(then_branch, regs);
                if let Some(eb) = else_branch {
                    insert_class_capture_refresh_after_assignments(eb, regs);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                insert_class_capture_refresh_after_assignments(body, regs);
            }
            Stmt::For { body, .. } => {
                insert_class_capture_refresh_after_assignments(body, regs);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                insert_class_capture_refresh_after_assignments(body, regs);
                if let Some(c) = catch {
                    insert_class_capture_refresh_after_assignments(&mut c.body, regs);
                }
                if let Some(f) = finally {
                    insert_class_capture_refresh_after_assignments(f, regs);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    insert_class_capture_refresh_after_assignments(&mut case.body, regs);
                }
            }
            _ => {}
        }
        // Does this statement (at THIS level, closures excluded by the
        // collector) assign any watched capture?
        let mut assigned = Vec::new();
        collect_assigned_locals_stmt(&stmts[i], &mut assigned);
        // #6037: a forward-captured `var`/`let` re-bound later in the same body
        // — including a DESTRUCTURING leaf (`var { t: dSq } = f()`) — lowers to
        // a `Stmt::Let` that REUSES the pre-registered forward-decl id (see the
        // `lexical_forward_decls` / `PreallocateBoxes` machinery). Such a Let
        // IS an assignment of the captured value, but `collect_assigned_locals_
        // stmt` classifies every `Let` as a fresh binding and skips it, so the
        // decl-site snapshot (taken while the forward-decl still held
        // `undefined`) was never refreshed and the class method read the
        // capture as undefined (semver's `Comparator` reading `dSq.COMPARATOR`).
        // A non-`undefined` init re-binding a watched id triggers the refresh;
        // the `undefined` forward-decl init itself is skipped (it snapshots the
        // same pre-assignment value the decl-site `RegisterClassCaptures`
        // already recorded).
        if let Stmt::Let {
            id, init: Some(v), ..
        } = &stmts[i]
        {
            if !matches!(v, Expr::Undefined) {
                assigned.push(*id);
            }
        }
        if !assigned.is_empty() {
            let mut inserts: Vec<Stmt> = Vec::new();
            for (re_reg, capset) in regs {
                if matches!(&stmts[i], Stmt::Expr(Expr::RegisterClassCaptures { .. })) {
                    continue;
                }
                if assigned.iter().any(|id| capset.contains(id)) {
                    inserts.push(re_reg.clone());
                }
            }
            for (j, s) in inserts.iter().cloned().enumerate() {
                stmts.insert(i + 1 + j, s);
            }
            i += inserts.len();
        }
        i += 1;
    }
}
