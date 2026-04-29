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

    // Generate Let statements for destructuring patterns BEFORE lowering body
    // This ensures the destructured variable names are defined when the body references them
    let mut destructuring_stmts = Vec::new();
    for (param_id, pat) in &destructuring_params {
        let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
        destructuring_stmts.extend(stmts);
    }

    // Hoist function declarations in block body (JS hoisting semantics)
    if let ast::BlockStmtOrExpr::BlockStmt(block) = &*arrow.body {
        for stmt in &block.stmts {
            if let ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) = stmt {
                if fn_decl.function.body.is_some() {
                    let name = fn_decl.ident.sym.to_string();
                    if ctx.lookup_local(&name).is_none() {
                        ctx.define_local(name, Type::Any);
                    }
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
            var_hoisted
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

    ctx.exit_scope(scope_mark);

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
    })
}

pub(super) fn lower_fn_expr(ctx: &mut LoweringContext, fn_expr: &ast::FnExpr) -> Result<Expr> {
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

    // Lower parameters and collect destructuring info
    let mut params = Vec::new();
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &fn_expr.function.params {
        let param_name = get_pat_name(&param.pat)?;
        let param_default = get_param_default(ctx, &param.pat)?;
        let is_rest = is_rest_param(&param.pat);
        let param_id = ctx.define_local(param_name.clone(), Type::Any);
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: Type::Any,
            default: param_default,
            is_rest,
        });
        // Track destructuring patterns to generate extraction statements
        if is_destructuring_pattern(&param.pat) {
            destructuring_params.push((param_id, param.pat.clone()));
        }
    }

    // Generate Let statements for destructuring patterns BEFORE lowering body
    let mut destructuring_stmts = Vec::new();
    for (param_id, pat) in &destructuring_params {
        let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
        destructuring_stmts.extend(stmts);
    }

    // Hoist function declarations: pre-register all function declarations in the body
    // so they can be referenced before their lexical position (JS hoisting semantics).
    if let Some(ref block) = fn_expr.function.body {
        for stmt in &block.stmts {
            if let ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) = stmt {
                if fn_decl.function.body.is_some() {
                    let name = fn_decl.ident.sym.to_string();
                    if ctx.lookup_local(&name).is_none() {
                        ctx.define_local(name, Type::Any);
                    }
                }
            }
        }
    }

    // Lower body with JS hoisting: only `var` declarations and function
    // declarations are hoisted per JS semantics. `let`/`const` MUST remain
    // at their lexical position because their init expressions are only
    // evaluated when control flow reaches them — hoisting `const x = fn()`
    // out of a conditional branch would eagerly run the call.
    let mut body = if let Some(ref block) = fn_expr.function.body {
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
        var_hoisted
    } else {
        Vec::new()
    };

    // Prepend destructuring statements to body
    if !destructuring_stmts.is_empty() {
        let mut new_body = destructuring_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

    ctx.exit_scope(scope_mark);

    let (captures, mutable_captures) = compute_closure_captures(ctx, &body, &outer_locals, &params);

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

    // Find unique captures: refs that are in outer_locals but not params.
    let mut captures: Vec<LocalId> = all_refs
        .into_iter()
        .filter(|id| outer_local_ids.contains(id) && !param_ids.contains(id))
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
        .filter(|id| assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
        .copied()
        .collect();

    (captures, mutable_captures)
}
