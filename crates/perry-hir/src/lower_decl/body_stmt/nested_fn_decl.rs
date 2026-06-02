use super::*;

fn stmt_is_string_directive(stmt: &ast::Stmt) -> Option<&str> {
    let ast::Stmt::Expr(expr_stmt) = stmt else {
        return None;
    };
    let mut expr = expr_stmt.expr.as_ref();
    while let ast::Expr::Paren(paren) = expr {
        expr = paren.expr.as_ref();
    }
    let ast::Expr::Lit(ast::Lit::Str(s)) = expr else {
        return None;
    };
    s.value.as_str()
}

fn function_has_use_strict(func: &ast::Function) -> bool {
    let Some(block) = func.body.as_ref() else {
        return false;
    };
    for stmt in &block.stmts {
        let Some(directive) = stmt_is_string_directive(stmt) else {
            break;
        };
        if directive == "use strict" {
            return true;
        }
    }
    false
}

pub(super) fn lower_nested_fn_decl(
    ctx: &mut LoweringContext,
    fn_decl: &ast::FnDecl,
    result: &mut Vec<Stmt>,
) -> Result<()> {
    let func_name = fn_decl.ident.sym.to_string();
    let func_id = ctx.fresh_func();

    // Register the function name temporarily so self-recursive calls
    // inside the body resolve to FuncRef(func_id).
    ctx.register_func(func_name.clone(), func_id);

    // Define the local for the function name BEFORE lowering the body,
    // so self-recursive references inside the body resolve to
    // LocalGet(local_id) rather than FuncRef(func_id). This ensures
    // the LLVM backend's boxed-var analysis sees the same LocalId at
    // both the declaration and self-reference sites.
    let local_id = ctx
        .lookup_local(&func_name)
        .unwrap_or_else(|| ctx.define_local(func_name.clone(), Type::Any));

    let scope_mark = ctx.enter_scope();

    // Track outer locals for capture detection
    let outer_locals: Vec<(String, LocalId)> = ctx
        .locals
        .iter()
        .map(|(name, id, _)| (name.clone(), *id))
        .collect();

    // Lower parameters. Skip the TypeScript `this:` annotation —
    // it has no runtime existence (see the sibling site above for
    // the full rationale).
    let mut params = Vec::new();
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &fn_decl.function.params {
        let param_name = get_pat_name(&param.pat)?;
        if param_name == "this" {
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
        if is_destructuring_pattern(&param.pat) {
            destructuring_params.push((param_id, param.pat.clone()));
        }
    }

    // #677: synthesize `arguments` for nested function decls.
    let user_has_arguments_param = fn_decl
        .function
        .params
        .iter()
        .any(|p| get_pat_name(&p.pat).ok().as_deref() == Some("arguments"));
    let user_has_rest = fn_decl
        .function
        .params
        .iter()
        .any(|p| is_rest_param(&p.pat));
    let needs_arguments_synth = !user_has_arguments_param
        && !user_has_rest
        && fn_decl
            .function
            .body
            .as_ref()
            .map(|b| body_uses_arguments(&b.stmts))
            .unwrap_or(false);
    if needs_arguments_synth {
        append_synthetic_arguments_param(ctx, &mut params);
    }

    // Generate destructuring stmts
    let mut destructuring_stmts = Vec::new();
    for (param_id, pat) in &destructuring_params {
        let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
        destructuring_stmts.extend(stmts);
    }

    let outer_strict = ctx.current_strict;
    let is_strict = outer_strict || function_has_use_strict(&fn_decl.function);
    ctx.current_strict = is_strict;

    // Lower body — see issue #569; hoist nested function-decl
    // statements within this inner fn body to the top so
    // forward refs and sibling captures work end-to-end.
    let mut body = if let Some(ref block) = fn_decl.function.body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };
    ctx.current_strict = outer_strict;

    if !destructuring_stmts.is_empty() {
        let mut new_body = destructuring_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

    ctx.exit_scope(scope_mark);

    // Detect captured variables
    let mut all_refs = Vec::new();
    let mut visited_closures = std::collections::HashSet::new();
    for stmt in &body {
        collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
    }

    let outer_local_ids: std::collections::HashSet<LocalId> =
        outer_locals.iter().map(|(_, id)| *id).collect();
    let param_ids: std::collections::HashSet<LocalId> = params.iter().map(|p| p.id).collect();

    // dayjs (issue: format() returned `292278994-08`): local
    // IDs are scope-local — see expr_function.rs
    // compute_closure_captures for the long explanation.
    // Strip locally-declared ids from the capture set so an
    // inner `var i = ...` doesn't collide with a same-id
    // outer constant.
    let inner_decls: std::collections::HashSet<LocalId> = {
        let mut s = std::collections::HashSet::new();
        for stmt in &body {
            collect_let_decls_in_stmt(stmt, &mut s);
        }
        s
    };

    let mut captures: Vec<LocalId> = all_refs
        .into_iter()
        .filter(|id| {
            outer_local_ids.contains(id) && !param_ids.contains(id) && !inner_decls.contains(id)
        })
        .collect();
    captures.sort();
    captures.dedup();
    captures = ctx.filter_module_level_captures(captures);

    // Detect mutable captures
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

    // Issue #838 followup (b): tag the function-decl's
    // local id as function-valued so the assignment
    // recogniser routes `M.prototype.x = fn` (and the
    // `var m = M.prototype` aliased form) through the
    // function-classic prototype-method path. Babel's
    // class-from-function emit pattern and dayjs's
    // minified bundle both lower `function M(){}` inside
    // an IIFE to exactly this `Stmt::Let { init:
    // Some(Closure{…}) }` shape — the destructuring.rs
    // path only fires for `var/let/const` lets, so the
    // tag has to be applied here too.
    ctx.function_valued_locals.insert(local_id);

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
        is_async: fn_decl.function.is_async,
        is_generator: false,
        is_strict,
    };
    result.push(Stmt::Let {
        id: local_id,
        name: func_name,
        ty: Type::Any,
        init: Some(closure),
        mutable: false,
    });

    Ok(())
}
