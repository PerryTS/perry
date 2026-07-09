use super::*;

pub(super) fn lower_runtime_for_await_iterator_body(
    ctx: &mut LoweringContext,
    for_of_stmt: &ast::ForOfStmt,
    source_expr: Expr,
) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();
    let scope_mark = ctx.push_block_scope();

    let iter_id = ctx.fresh_local();
    ctx.locals
        .push((format!("__iter_{}", iter_id), iter_id, Type::Any));
    result.push(Stmt::Let {
        id: iter_id,
        name: format!("__iter_{}", iter_id),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::GetAsyncIterator(Box::new(source_expr))),
    });

    let result_id = ctx.fresh_local();
    ctx.locals
        .push((format!("__result_{}", result_id), result_id, Type::Any));
    let raw_next_call = Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(iter_id)),
            property: "next".to_string(),
        }),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    };
    let next_call = Expr::Await(Box::new(raw_next_call));
    result.push(Stmt::Let {
        id: result_id,
        name: format!("__result_{}", result_id),
        ty: Type::Any,
        mutable: true,
        init: Some(Expr::Undefined),
    });

    let binding_pat = match &for_of_stmt.left {
        ast::ForHead::VarDecl(var_decl) => var_decl
            .decls
            .first()
            .map(|decl| &decl.name)
            .ok_or_else(|| anyhow!("for-await-of requires a variable declaration"))?,
        ast::ForHead::Pat(pat) => pat,
        _ => return Err(anyhow!("Unsupported for-await-of left-hand side")),
    };
    let mut var_ids = Vec::new();
    collect_for_of_pattern_leaves(ctx, binding_pat, &mut var_ids);
    if var_ids.is_empty() {
        return Err(anyhow!("Unsupported for-await-of binding pattern"));
    }

    let mut body_stmts = Vec::new();
    let mut var_idx = 0;
    emit_for_of_pattern_binding(
        ctx,
        binding_pat,
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(result_id)),
            property: "value".to_string(),
        },
        &var_ids,
        &mut var_idx,
        &mut body_stmts,
    )?;
    let mut user_body = lower_body_stmt(ctx, &for_of_stmt.body)?;
    insert_iterator_return_before_abrupts(&mut user_body, iter_id, true);
    body_stmts.extend(user_body);

    // Advance-at-top driver: `while (true) { __result = await next();
    // if (__result.done) break; <bind + body> }`. The previous shape put the
    // advance at the body TAIL (`while (!done) { body; result = next() }`),
    // so a `continue` in the user body skipped it and re-processed the SAME
    // result forever — an SSE consumer's `if (ev === "ping") continue;` hung
    // a large esbuild-bundled CLI app on the first real server ping. Mirrors
    // `iter_driver_while_stmt` (lower/stmt_loops.rs); the synthetic
    // `if done break` is appended after the abrupt-close rewrite over the
    // user body, so normal completion never runs a spurious IteratorClose.
    let mut loop_body = vec![
        Stmt::Expr(Expr::LocalSet(result_id, Box::new(next_call))),
        Stmt::If {
            condition: Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(result_id)),
                property: "done".to_string(),
            },
            then_branch: vec![Stmt::Break],
            else_branch: None,
        },
    ];
    loop_body.extend(body_stmts);
    result.push(Stmt::While {
        condition: Expr::Bool(true),
        body: loop_body,
    });

    ctx.pop_block_scope(scope_mark);
    Ok(result)
}
