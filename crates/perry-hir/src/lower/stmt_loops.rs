//! `for...of` and `for...in` statement lowering.
//!
//! Extracted from `lower/stmt.rs` so that file stays under the 2,000-LOC
//! soft cap. Both arms produce significant generated HIR — the for-of arm
//! covers the generator iterator-protocol path, the
//! `*[Symbol.iterator]()`-based class path, the `for await (... of ...)`
//! async path, and the regular indexed-array path. The for-in arm
//! desugars to a for-of over `Object.keys(...)`.
//!
//! The match arms inside `lower_stmt` collapse to one-line delegations
//! to `lower_stmt_for_of` / `lower_stmt_for_in`.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

fn unwrap_stream_expr(mut expr: &ast::Expr) -> &ast::Expr {
    loop {
        expr = match expr {
            ast::Expr::TsAs(ts_as) => &ts_as.expr,
            ast::Expr::TsNonNull(non_null) => &non_null.expr,
            ast::Expr::TsConstAssertion(assertion) => &assertion.expr,
            ast::Expr::TsTypeAssertion(assertion) => &assertion.expr,
            ast::Expr::Paren(paren) => &paren.expr,
            _ => break,
        };
    }
    expr
}

fn web_readable_stream_values_receiver(expr: &ast::Expr) -> Option<&ast::Expr> {
    let ast::Expr::Call(call) = unwrap_stream_expr(expr) else {
        return None;
    };
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return None;
    };
    let ast::Expr::Member(member) = callee_expr.as_ref() else {
        return None;
    };
    if !matches!(&member.prop, ast::MemberProp::Ident(prop) if prop.sym.as_ref() == "values") {
        return None;
    }
    Some(member.obj.as_ref())
}

fn is_web_readable_stream_expr(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    match unwrap_stream_expr(expr) {
        ast::Expr::Ident(ident) => {
            let name = ident.sym.as_ref();
            matches!(
                ctx.lookup_native_instance(name),
                Some((_, "ReadableStream"))
            ) || matches!(
                ctx.lookup_local_type(name),
                Some(Type::Named(n)) if n == "ReadableStream"
            )
        }
        ast::Expr::New(new_expr) => matches!(
            new_expr.callee.as_ref(),
            ast::Expr::Ident(callee) if callee.sym.as_ref() == "ReadableStream"
        ),
        _ => false,
    }
}

fn strip_for_of_expr_wrappers(mut expr: &ast::Expr) -> &ast::Expr {
    loop {
        expr = match expr {
            ast::Expr::TsAs(x) => &x.expr,
            ast::Expr::TsNonNull(x) => &x.expr,
            ast::Expr::TsConstAssertion(x) => &x.expr,
            ast::Expr::Paren(x) => &x.expr,
            _ => return expr,
        };
    }
}

fn is_node_readable_class_ref(expr: &ast::Expr) -> bool {
    match strip_for_of_expr_wrappers(expr) {
        ast::Expr::Ident(ident) => ident.sym.as_ref() == "Readable",
        ast::Expr::Member(member) => {
            matches!(&member.prop, ast::MemberProp::Ident(prop) if prop.sym.as_ref() == "Readable")
        }
        _ => false,
    }
}

fn is_node_readable_static_factory(expr: &ast::Expr) -> bool {
    let ast::Expr::Call(call) = strip_for_of_expr_wrappers(expr) else {
        return false;
    };
    let ast::Callee::Expr(callee) = &call.callee else {
        return false;
    };
    let ast::Expr::Member(member) = strip_for_of_expr_wrappers(callee.as_ref()) else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return false;
    };
    matches!(prop.sym.as_ref(), "from" | "of") && is_node_readable_class_ref(&member.obj)
}

fn is_node_readable_expr(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    is_node_readable_static_factory(expr)
        || is_node_readable_helper_chain(ctx, expr)
        || matches!(
            crate::lower_types::infer_type_from_expr(strip_for_of_expr_wrappers(expr), ctx),
            Type::Named(name) if name == "Readable"
        )
}

fn is_node_readable_helper_chain(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    let ast::Expr::Call(call) = strip_for_of_expr_wrappers(expr) else {
        return false;
    };
    let ast::Callee::Expr(callee) = &call.callee else {
        return false;
    };
    let ast::Expr::Member(member) = strip_for_of_expr_wrappers(callee.as_ref()) else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return false;
    };
    match prop.sym.as_ref() {
        "from" | "of" => is_node_readable_class_ref(&member.obj),
        "map" | "filter" | "flatMap" | "take" | "drop" | "compose" => {
            is_node_readable_expr(ctx, &member.obj)
        }
        _ => false,
    }
}

fn is_node_readable_for_await_target(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    is_node_readable_expr(ctx, expr)
}

fn is_filehandle_readlines_for_await_target(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    matches!(
        crate::lower_types::infer_type_from_expr(strip_for_of_expr_wrappers(expr), ctx),
        Type::Named(name) if name == crate::lower_types::FILEHANDLE_READLINES_ITERATOR_TYPE
    )
}

fn is_fs_dir_type(ty: Type) -> bool {
    matches!(ty, Type::Named(name) if name == "Dir" || name == "fs.Dir")
}

fn is_fs_dir_for_await_target(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    let expr = strip_for_of_expr_wrappers(expr);
    if is_fs_dir_type(crate::lower_types::infer_type_from_expr(expr, ctx)) {
        return true;
    }

    let ast::Expr::Call(call) = expr else {
        return false;
    };
    let ast::Callee::Expr(callee) = &call.callee else {
        return false;
    };
    let ast::Expr::Member(member) = strip_for_of_expr_wrappers(callee.as_ref()) else {
        return false;
    };
    if !matches!(&member.prop, ast::MemberProp::Ident(prop) if prop.sym.as_ref() == "entries") {
        return false;
    }
    is_fs_dir_type(crate::lower_types::infer_type_from_expr(
        strip_for_of_expr_wrappers(&member.obj),
        ctx,
    ))
}

fn is_fs_promises_glob_for_await_target(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    let ast::Expr::Call(call) = strip_for_of_expr_wrappers(expr) else {
        return false;
    };
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return false;
    };
    match strip_for_of_expr_wrappers(callee_expr.as_ref()) {
        ast::Expr::Ident(ident) => {
            ctx.lookup_native_module(ident.sym.as_ref())
                .is_some_and(|(module, method)| {
                    module.strip_prefix("node:").unwrap_or(module) == "fs/promises"
                        && method == Some("glob")
                })
                || ctx.lookup_imported_func(ident.sym.as_ref()) == Some("glob")
        }
        ast::Expr::Member(member) => {
            let ast::MemberProp::Ident(prop) = &member.prop else {
                return false;
            };
            if prop.sym.as_ref() != "glob" {
                return false;
            }
            match strip_for_of_expr_wrappers(&member.obj) {
                ast::Expr::Ident(obj) => {
                    ctx.lookup_native_module(obj.sym.as_ref())
                        .is_some_and(|(module, method)| {
                            method.is_none()
                                && module.strip_prefix("node:").unwrap_or(module) == "fs/promises"
                        })
                        || ctx
                            .lookup_builtin_module_alias(obj.sym.as_ref())
                            .is_some_and(|module| {
                                module.strip_prefix("node:").unwrap_or(module) == "fs/promises"
                            })
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// `for await (const line of rl)` where `rl = readline.createInterface(...)`.
/// The interface is registered as a `("readline", "Interface")` native
/// instance, so its method calls (`.on`, `.close`, and now `.iterator`)
/// dispatch to the readline runtime. Mirrors the node:stream Readable arm.
fn is_readline_interface_for_await_target(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    matches!(
        strip_for_of_expr_wrappers(expr),
        ast::Expr::Ident(ident)
            if matches!(
                ctx.lookup_native_instance(ident.sym.as_ref()),
                Some(("readline", "Interface"))
            )
    )
}

fn async_iterator_method_call(iterable: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::IndexGet {
            object: Box::new(iterable),
            index: Box::new(Expr::SymbolFor(Box::new(Expr::String(
                "@@__perry_wk_asyncIterator".to_string(),
            )))),
        }),
        args: vec![],
        type_args: vec![],
    }
}

fn iterator_return_call(iter_id: LocalId, needs_await: bool) -> Expr {
    let call = Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(iter_id)),
            property: "return".to_string(),
        }),
        args: vec![],
        type_args: vec![],
    };
    if needs_await {
        Expr::Await(Box::new(call))
    } else {
        call
    }
}

fn insert_iterator_return_before_abrupts(
    stmts: &mut Vec<Stmt>,
    iter_id: LocalId,
    needs_await: bool,
) {
    let mut rewritten = Vec::with_capacity(stmts.len());
    for stmt in stmts.drain(..) {
        match stmt {
            Stmt::Break => {
                rewritten.push(Stmt::Expr(iterator_return_call(iter_id, needs_await)));
                rewritten.push(Stmt::Break);
            }
            Stmt::LabeledBreak(label) => {
                rewritten.push(Stmt::Expr(iterator_return_call(iter_id, needs_await)));
                rewritten.push(Stmt::LabeledBreak(label));
            }
            Stmt::Return(value) => {
                rewritten.push(Stmt::Expr(iterator_return_call(iter_id, needs_await)));
                rewritten.push(Stmt::Return(value));
            }
            Stmt::Throw(expr) => {
                rewritten.push(Stmt::Expr(iterator_return_call(iter_id, needs_await)));
                rewritten.push(Stmt::Throw(expr));
            }
            Stmt::If {
                condition,
                mut then_branch,
                mut else_branch,
            } => {
                insert_iterator_return_before_abrupts(&mut then_branch, iter_id, needs_await);
                if let Some(else_stmts) = else_branch.as_mut() {
                    insert_iterator_return_before_abrupts(else_stmts, iter_id, needs_await);
                }
                rewritten.push(Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                });
            }
            other => rewritten.push(other),
        }
    }
    *stmts = rewritten;
}

/// Element source for a `for...of` binding: `__result.value` on the lazy
/// iterator path, `__arr[__idx]` on the materialized-array path.
pub(crate) fn lazy_or_index_elem(
    use_lazy_iter: bool,
    arr_id: LocalId,
    idx_id: LocalId,
    result_id: LocalId,
) -> Expr {
    if use_lazy_iter {
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(result_id)),
            property: "value".to_string(),
        }
    } else {
        Expr::IndexGet {
            object: Box::new(Expr::LocalGet(arr_id)),
            index: Box::new(Expr::LocalGet(idx_id)),
        }
    }
}

/// `__iter.next()`.
pub(crate) fn iterator_next_call(iter_id: LocalId) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(iter_id)),
            property: "next".to_string(),
        }),
        args: vec![],
        type_args: vec![],
    }
}

/// The lazy `for...of` driver loop, modeled as a `for` so `continue` re-pulls
/// the iterator via the update clause (a `while` with the advance at the body
/// tail would skip it on `continue` and spin):
///   for (let __result = __iter.next();
///        !__result.done;
///        __result = __iter.next()) { <loop_body> }
/// `iter_id` holds the iterator, `result_id` the latest `{ value, done }`.
pub(crate) fn lazy_iter_for_stmt(
    iter_id: LocalId,
    result_id: LocalId,
    loop_body: Vec<Stmt>,
) -> Stmt {
    Stmt::For {
        init: Some(Box::new(Stmt::Let {
            id: result_id,
            name: format!("__result_{}", result_id),
            ty: Type::Any,
            mutable: true,
            init: Some(iterator_next_call(iter_id)),
        })),
        condition: Some(Expr::Unary {
            op: UnaryOp::Not,
            operand: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(result_id)),
                property: "done".to_string(),
            }),
        }),
        update: Some(Expr::LocalSet(
            result_id,
            Box::new(iterator_next_call(iter_id)),
        )),
        body: loop_body,
    }
}

/// Spec IteratorClose, guarded: `if (__iter.return != null) __iter.return();`.
/// Array iterators have no `return` method, so the guard makes close a no-op
/// for them (closing an array iterator is a spec no-op); generators / custom
/// iterators run their `return` (which executes pending `finally` blocks).
pub(crate) fn iterator_close_guarded_stmt(iter_id: LocalId) -> Stmt {
    Stmt::If {
        condition: Expr::Compare {
            op: CompareOp::LooseNe,
            left: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(iter_id)),
                property: "return".to_string(),
            }),
            right: Box::new(Expr::Null),
        },
        then_branch: vec![Stmt::Expr(iterator_return_call(iter_id, false))],
        else_branch: None,
    }
}

/// Rewrite a synchronous `for...of` body so every abrupt completion that
/// escapes the loop runs IteratorClose first. Per spec ForIn/OfBodyEvaluation:
/// an unlabeled `break` that targets this loop, a labeled `break`/`continue`
/// that targets an enclosing construct, and a `return` all close the iterator.
/// Unlabeled `continue` (next iteration) and `break`/`continue` captured by a
/// loop/switch nested *within* the body do not. `throw` is intentionally not
/// rewritten here: a `throw` caught by an in-body `try/catch` must not close,
/// and the uncaught case is handled separately.
///
/// `break_capture_depth` counts enclosing loops/switches inside the body (which
/// capture an unlabeled `break`); `inner_labels` are labels declared within the
/// body (which capture a matching labeled `break`/`continue`).
pub(crate) fn insert_iterator_close_on_abrupt(
    stmts: &mut Vec<Stmt>,
    iter_id: LocalId,
    break_capture_depth: usize,
    inner_labels: &[String],
) {
    let mut rewritten = Vec::with_capacity(stmts.len());
    for stmt in stmts.drain(..) {
        match stmt {
            Stmt::Break if break_capture_depth == 0 => {
                rewritten.push(iterator_close_guarded_stmt(iter_id));
                rewritten.push(Stmt::Break);
            }
            Stmt::LabeledBreak(label) if !inner_labels.contains(&label) => {
                rewritten.push(iterator_close_guarded_stmt(iter_id));
                rewritten.push(Stmt::LabeledBreak(label));
            }
            Stmt::LabeledContinue(label) if !inner_labels.contains(&label) => {
                rewritten.push(iterator_close_guarded_stmt(iter_id));
                rewritten.push(Stmt::LabeledContinue(label));
            }
            Stmt::Return(value) => {
                rewritten.push(iterator_close_guarded_stmt(iter_id));
                rewritten.push(Stmt::Return(value));
            }
            Stmt::If {
                condition,
                mut then_branch,
                mut else_branch,
            } => {
                insert_iterator_close_on_abrupt(
                    &mut then_branch,
                    iter_id,
                    break_capture_depth,
                    inner_labels,
                );
                if let Some(else_stmts) = else_branch.as_mut() {
                    insert_iterator_close_on_abrupt(
                        else_stmts,
                        iter_id,
                        break_capture_depth,
                        inner_labels,
                    );
                }
                rewritten.push(Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                });
            }
            Stmt::Try {
                mut body,
                mut catch,
                mut finally,
            } => {
                insert_iterator_close_on_abrupt(
                    &mut body,
                    iter_id,
                    break_capture_depth,
                    inner_labels,
                );
                if let Some(c) = catch.as_mut() {
                    insert_iterator_close_on_abrupt(
                        &mut c.body,
                        iter_id,
                        break_capture_depth,
                        inner_labels,
                    );
                }
                if let Some(f) = finally.as_mut() {
                    insert_iterator_close_on_abrupt(f, iter_id, break_capture_depth, inner_labels);
                }
                rewritten.push(Stmt::Try {
                    body,
                    catch,
                    finally,
                });
            }
            Stmt::While {
                condition,
                mut body,
            } => {
                insert_iterator_close_on_abrupt(
                    &mut body,
                    iter_id,
                    break_capture_depth + 1,
                    inner_labels,
                );
                rewritten.push(Stmt::While { condition, body });
            }
            Stmt::DoWhile {
                mut body,
                condition,
            } => {
                insert_iterator_close_on_abrupt(
                    &mut body,
                    iter_id,
                    break_capture_depth + 1,
                    inner_labels,
                );
                rewritten.push(Stmt::DoWhile { body, condition });
            }
            Stmt::For {
                init,
                condition,
                update,
                mut body,
            } => {
                insert_iterator_close_on_abrupt(
                    &mut body,
                    iter_id,
                    break_capture_depth + 1,
                    inner_labels,
                );
                rewritten.push(Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                });
            }
            Stmt::Switch {
                discriminant,
                mut cases,
            } => {
                for case in cases.iter_mut() {
                    insert_iterator_close_on_abrupt(
                        &mut case.body,
                        iter_id,
                        break_capture_depth + 1,
                        inner_labels,
                    );
                }
                rewritten.push(Stmt::Switch {
                    discriminant,
                    cases,
                });
            }
            Stmt::Labeled { label, mut body } => {
                let mut labels = inner_labels.to_vec();
                labels.push(label.clone());
                let mut body_vec = vec![*body];
                insert_iterator_close_on_abrupt(
                    &mut body_vec,
                    iter_id,
                    break_capture_depth,
                    &labels,
                );
                body = Box::new(body_vec.into_iter().next().unwrap());
                rewritten.push(Stmt::Labeled { label, body });
            }
            other => rewritten.push(other),
        }
    }
    *stmts = rewritten;
}

fn lower_runtime_for_await_iterator(
    ctx: &mut LoweringContext,
    module: &mut Module,
    for_of_stmt: &ast::ForOfStmt,
    source_expr: Expr,
) -> Result<()> {
    let for_scope_mark = ctx.push_block_scope();
    let iter_id = ctx.fresh_local();
    ctx.locals
        .push((format!("__iter_{}", iter_id), iter_id, Type::Any));
    module.init.push(Stmt::Let {
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
    };
    let next_call = Expr::Await(Box::new(raw_next_call));
    module.init.push(Stmt::Let {
        id: result_id,
        name: format!("__result_{}", result_id),
        ty: Type::Any,
        mutable: true,
        init: Some(next_call.clone()),
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
    let init_before = module.init.len();
    if let ast::Stmt::Block(block) = &*for_of_stmt.body {
        for s in &block.stmts {
            lower_stmt(ctx, module, s)?;
        }
    } else {
        lower_stmt(ctx, module, &for_of_stmt.body)?;
    }
    let mut user_body: Vec<Stmt> = module.init.drain(init_before..).collect();
    insert_iterator_return_before_abrupts(&mut user_body, iter_id, true);
    body_stmts.append(&mut user_body);
    body_stmts.push(Stmt::Expr(Expr::LocalSet(result_id, Box::new(next_call))));

    module.init.push(Stmt::While {
        condition: Expr::Unary {
            op: UnaryOp::Not,
            operand: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(result_id)),
                property: "done".to_string(),
            }),
        },
        body: body_stmts,
    });

    ctx.pop_block_scope(for_scope_mark);
    Ok(())
}

pub(crate) fn lower_stmt_for_of(
    ctx: &mut LoweringContext,
    module: &mut Module,
    for_of_stmt: &ast::ForOfStmt,
) -> Result<()> {
    // --- Iterator protocol path for generators ---
    // Detect: for (const x of genFunc(...)) where genFunc is function*
    let is_generator_call = if let ast::Expr::Call(call) = &*for_of_stmt.right {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Ident(ident) = &**callee_expr {
                ctx.generator_func_names.contains(ident.sym.as_ref())
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    // Detect whether the called generator was an `async function*`.
    // Async generators always return `Promise<{value, done}>` from
    // `.next()`, so the iterator-protocol loop must `await` each
    // call before reading `.value` / `.done`. Either the user
    // wrote `for await (...)` (SWC `is_await`) or the callee was
    // declared async — both must trigger awaiting.
    let callee_is_async_gen = if let ast::Expr::Call(call) = &*for_of_stmt.right {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Ident(ident) = &**callee_expr {
                ctx.async_generator_func_names.contains(ident.sym.as_ref())
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };
    let needs_await = for_of_stmt.is_await || callee_is_async_gen;

    let is_timer_promises_interval_call = for_of_stmt.is_await
        && if let ast::Expr::Call(call) = &*for_of_stmt.right {
            if let ast::Callee::Expr(callee_expr) = &call.callee {
                match &**callee_expr {
                    ast::Expr::Ident(ident) => {
                        ctx.lookup_native_module(ident.sym.as_ref()).is_some_and(
                            |(module, method)| {
                                module.strip_prefix("node:").unwrap_or(module) == "timers/promises"
                                    && method == Some("setInterval")
                            },
                        ) || ctx
                            .lookup_imported_func(ident.sym.as_ref())
                            .is_some_and(|imported| imported == "setInterval")
                    }
                    ast::Expr::Member(member) => {
                        if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) =
                            (&*member.obj, &member.prop)
                        {
                            prop.sym.as_ref() == "setInterval"
                                && ctx.lookup_local(obj.sym.as_ref()).is_none()
                        } else {
                            false
                        }
                    }
                    _ => false,
                }
            } else {
                false
            }
        } else {
            false
        };

    // Also detect: for (const x of new Range(...)) where Range
    // defines `*[Symbol.iterator]()`. We lowered that method as
    // a synthesized top-level generator function taking `this`
    // as its first parameter; the for-of here dispatches by
    // calling that function with the lowered receiver.
    let iter_from_class: Option<perry_types::FuncId> =
        if let ast::Expr::New(new_expr) = &*for_of_stmt.right {
            if let ast::Expr::Ident(ident) = new_expr.callee.as_ref() {
                let class_name = ident.sym.to_string();
                ctx.iterator_func_for_class.get(&class_name).copied()
            } else {
                None
            }
        } else {
            None
        };

    let is_node_readable_for_await =
        for_of_stmt.is_await && is_node_readable_for_await_target(ctx, &for_of_stmt.right);
    let is_filehandle_readlines_for_await =
        for_of_stmt.is_await && is_filehandle_readlines_for_await_target(ctx, &for_of_stmt.right);
    let is_fs_dir_for_await =
        for_of_stmt.is_await && is_fs_dir_for_await_target(ctx, &for_of_stmt.right);
    let is_fs_promises_glob_for_await =
        for_of_stmt.is_await && is_fs_promises_glob_for_await_target(ctx, &for_of_stmt.right);
    let is_readline_interface_for_await =
        for_of_stmt.is_await && is_readline_interface_for_await_target(ctx, &for_of_stmt.right);

    if is_generator_call
        || iter_from_class.is_some()
        || is_timer_promises_interval_call
        || is_node_readable_for_await
        || is_filehandle_readlines_for_await
        || is_fs_dir_for_await
        || is_fs_promises_glob_for_await
        || is_readline_interface_for_await
    {
        // Lower to iterator protocol:
        //   let __iter = genFunc(...);                     // generator-fn path
        //   let __iter = __perry_iter_Range(new Range(...));  // class path
        //   let __iter = readable.iterator();              // node:stream path
        //   let __result = __iter.next();
        //   while (!__result.done) { const x = __result.value; body; __result = __iter.next(); }
        let for_scope_mark = ctx.push_block_scope();
        let iter_expr = lower_expr(ctx, &for_of_stmt.right)?;
        // For the class path we wrap the lowered `new Range(..)`
        // in a direct FuncRef call to the synthesized iterator
        // function (which has `this` as its first parameter).
        let iter_expr = if let Some(iter_fn_id) = iter_from_class {
            Expr::Call {
                callee: Box::new(Expr::FuncRef(iter_fn_id)),
                args: vec![iter_expr],
                type_args: vec![],
            }
        } else if is_filehandle_readlines_for_await || is_fs_dir_for_await {
            async_iterator_method_call(iter_expr)
        } else if is_node_readable_for_await {
            Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(iter_expr),
                    property: "iterator".to_string(),
                }),
                args: vec![],
                type_args: vec![],
            }
        } else if is_readline_interface_for_await {
            // rl.iterator() -> readline async-iterator object; .next() then
            // awaits each line. Dispatched explicitly to js_readline_iterator.
            Expr::NativeMethodCall {
                module: "readline".to_string(),
                class_name: Some("Interface".to_string()),
                object: Some(Box::new(iter_expr)),
                method: "iterator".to_string(),
                args: vec![],
            }
        } else {
            iter_expr
        };
        let iter_id = ctx.fresh_local();
        ctx.locals
            .push((format!("__iter_{}", iter_id), iter_id, Type::Any));
        module.init.push(Stmt::Let {
            id: iter_id,
            name: format!("__iter_{}", iter_id),
            ty: Type::Any,
            mutable: false,
            init: Some(iter_expr),
        });

        let result_id = ctx.fresh_local();
        ctx.locals
            .push((format!("__result_{}", result_id), result_id, Type::Any));
        // __result = __iter.next()
        // For async generators / `for await ... of`, wrap the
        // call in `Expr::Await` so the resolved iter-result
        // (`{value, done}`) is what's stored, not the Promise.
        let raw_next_call = Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(iter_id)),
                property: "next".to_string(),
            }),
            args: vec![],
            type_args: vec![],
        };
        let next_call = if needs_await {
            Expr::Await(Box::new(raw_next_call))
        } else {
            raw_next_call
        };
        module.init.push(Stmt::Let {
            id: result_id,
            name: format!("__result_{}", result_id),
            ty: Type::Any,
            mutable: true,
            init: Some(next_call.clone()),
        });

        // Extract the loop variable binding
        let item_name = if let ast::ForHead::VarDecl(var_decl) = &for_of_stmt.left {
            if let Some(decl) = var_decl.decls.first() {
                if let ast::Pat::Ident(ident) = &decl.name {
                    ident.id.sym.to_string()
                } else {
                    "__gen_item".to_string()
                }
            } else {
                "__gen_item".to_string()
            }
        } else {
            "__gen_item".to_string()
        };
        let item_id = ctx.define_local(item_name.clone(), Type::Any);

        // Lower loop body
        let mut body_stmts = Vec::new();
        // const x = __result.value
        body_stmts.push(Stmt::Let {
            id: item_id,
            name: item_name,
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(result_id)),
                property: "value".to_string(),
            }),
        });
        // Lower user body statements. lower_stmt appends to module.init,
        // so we snapshot and drain to capture the body stmts.
        // Handle both Block bodies (`for (...) { ... }`) AND single-statement
        // bodies (`for (...) console.log(v);`). Pre-fix the brace-less
        // form was silently dropped — `for (const v of gen()) doThing(v);`
        // produced no output at all.
        let init_before = module.init.len();
        if let ast::Stmt::Block(block) = &*for_of_stmt.body {
            for s in &block.stmts {
                lower_stmt(ctx, module, s)?;
            }
        } else {
            lower_stmt(ctx, module, &for_of_stmt.body)?;
        }
        let mut user_body: Vec<Stmt> = module.init.drain(init_before..).collect();
        if is_node_readable_for_await
            || is_filehandle_readlines_for_await
            || is_fs_dir_for_await
            || is_readline_interface_for_await
        {
            insert_iterator_return_before_abrupts(&mut user_body, iter_id, needs_await);
        }
        body_stmts.append(&mut user_body);
        // __result = __iter.next()
        body_stmts.push(Stmt::Expr(Expr::LocalSet(result_id, Box::new(next_call))));

        // while (!__result.done) { body }
        module.init.push(Stmt::While {
            condition: Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(result_id)),
                    property: "done".to_string(),
                }),
            },
            body: body_stmts,
        });

        ctx.pop_block_scope(for_scope_mark);
        return Ok(());
    }

    // --- #1646: `for await (const c of <Web ReadableStream>)` ---
    // The WHATWG ReadableStream async-iterator (Node 17+) drains via
    // getReader()/read(). The DOM lib types don't declare it, so user code
    // writes `for await (const v of rs as any)`; peel `as T` / `!` / parens
    // and recognise a Web stream by its native-instance registration OR its
    // inferred `Named("ReadableStream")` type (a directly-constructed
    // `new ReadableStream(...)` local carries only the latter). Without this
    // the loop falls through to the array-index desugar below, reads
    // `.length` on the numeric stream handle (0) and silently iterates zero
    // times. Mirrors the function-body path in `lower_decl/body_stmt.rs`.
    if for_of_stmt.is_await {
        let stream_source =
            web_readable_stream_values_receiver(&for_of_stmt.right).unwrap_or(&for_of_stmt.right);
        let mut iter_inner: &ast::Expr = stream_source;
        loop {
            iter_inner = match iter_inner {
                ast::Expr::TsAs(x) => &x.expr,
                ast::Expr::TsNonNull(x) => &x.expr,
                ast::Expr::TsConstAssertion(x) => &x.expr,
                ast::Expr::Paren(x) => &x.expr,
                _ => break,
            };
        }
        let is_readable_stream = match iter_inner {
            ast::Expr::Ident(_) | ast::Expr::New(_) => is_web_readable_stream_expr(ctx, iter_inner),
            // #1670: `for await (const c of res.body)` — `res.body` is a
            // `ReadableStream` but arrives as a bare `Member` (Any-typed), so
            // the Ident arm above misses it. Recognise `<obj>.body` on a
            // Response/Request and `<ts>.readable` on a TransformStream, the
            // same native-instance property mapping `var_decl` uses when those
            // reads are bound to a typed local. Without this the loop falls
            // through to the array-index desugar and iterates zero times.
            ast::Expr::Member(member) => {
                if let (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(prop_ident)) =
                    (member.obj.as_ref(), &member.prop)
                {
                    let prop = prop_ident.sym.as_ref();
                    let class = ctx
                        .lookup_native_instance(obj_ident.sym.as_ref())
                        .map(|(_, c)| c);
                    matches!(
                        (prop, class),
                        ("body", Some("Response"))
                            | ("body", Some("Request"))
                            | ("readable", Some("TransformStream"))
                    )
                } else {
                    false
                }
            }
            _ => false,
        };

        if is_readable_stream {
            let for_scope_mark = ctx.push_block_scope();
            // `as T` etc. are erased by lower_expr; for `rs.values()` lower
            // the underlying stream receiver because this branch drives the
            // reader loop directly.
            let stream_expr = lower_expr(ctx, stream_source)?;

            // const __reader = stream.getReader();
            let reader_id = ctx.fresh_local();
            ctx.locals
                .push((format!("__reader_{}", reader_id), reader_id, Type::Any));
            ctx.register_native_instance(
                format!("__reader_{}", reader_id),
                "readable_stream_reader".to_string(),
                "ReadableStreamDefaultReader".to_string(),
            );
            module.init.push(Stmt::Let {
                id: reader_id,
                name: format!("__reader_{}", reader_id),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::NativeMethodCall {
                    module: "readable_stream".to_string(),
                    class_name: Some("ReadableStream".to_string()),
                    object: Some(Box::new(stream_expr)),
                    method: "getReader".to_string(),
                    args: vec![],
                }),
            });

            // let __res = await __reader.read();
            let read_call = |reader_id: u32| {
                Expr::Await(Box::new(Expr::NativeMethodCall {
                    module: "readable_stream_reader".to_string(),
                    class_name: Some("ReadableStreamDefaultReader".to_string()),
                    object: Some(Box::new(Expr::LocalGet(reader_id))),
                    method: "read".to_string(),
                    args: vec![],
                }))
            };
            let res_id = ctx.fresh_local();
            ctx.locals
                .push((format!("__res_{}", res_id), res_id, Type::Any));
            module.init.push(Stmt::Let {
                id: res_id,
                name: format!("__res_{}", res_id),
                ty: Type::Any,
                mutable: true,
                init: Some(read_call(reader_id)),
            });

            // Loop variable: const <name> = __res.value;
            let item_name = if let ast::ForHead::VarDecl(var_decl) = &for_of_stmt.left {
                var_decl
                    .decls
                    .first()
                    .and_then(|decl| match &decl.name {
                        ast::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "__chunk".to_string())
            } else {
                "__chunk".to_string()
            };
            let item_id = ctx.define_local(item_name.clone(), Type::Any);

            let mut body_stmts: Vec<Stmt> = Vec::new();
            body_stmts.push(Stmt::Let {
                id: item_id,
                name: item_name,
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(res_id)),
                    property: "value".to_string(),
                }),
            });
            // Lower user body (lower_stmt appends to module.init; drain it).
            let init_before = module.init.len();
            if let ast::Stmt::Block(block) = &*for_of_stmt.body {
                for s in &block.stmts {
                    lower_stmt(ctx, module, s)?;
                }
            } else {
                lower_stmt(ctx, module, &for_of_stmt.body)?;
            }
            let mut user_body: Vec<Stmt> = module.init.drain(init_before..).collect();
            body_stmts.append(&mut user_body);
            // __res = await __reader.read();
            body_stmts.push(Stmt::Expr(Expr::LocalSet(
                res_id,
                Box::new(read_call(reader_id)),
            )));

            // while (!__res.done) { body }
            module.init.push(Stmt::While {
                condition: Expr::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(res_id)),
                        property: "done".to_string(),
                    }),
                },
                body: body_stmts,
            });

            // reader.releaseLock(); — best-effort cleanup.
            module.init.push(Stmt::Expr(Expr::NativeMethodCall {
                module: "readable_stream_reader".to_string(),
                class_name: Some("ReadableStreamDefaultReader".to_string()),
                object: Some(Box::new(Expr::LocalGet(reader_id))),
                method: "releaseLock".to_string(),
                args: vec![],
            }));

            ctx.pop_block_scope(for_scope_mark);
            return Ok(());
        }
    }

    // --- Standard array-based for-of path ---
    // Desugar for-of to a regular for loop:
    // for (const x of arr) { body }
    // becomes:
    // { let __arr = arr; for (let __i = 0; __i < __arr.length; __i++) { const x = __arr[__i]; body } }
    // Push a block scope so loop variables and internal temporaries don't leak.
    let for_scope_mark = ctx.push_block_scope();

    // Detect string iteration BEFORE lowering (so we can use the AST-level type info).
    // for (const ch of "hello") — each iteration yields a 1-char string via str[i].
    let is_string_iter = is_ast_string_expr(ctx, &for_of_stmt.right);

    // `for (const [k, v] of h)` where h is a Headers handle: WHATWG
    // Fetch spec says iteration of a Headers object yields `[key,
    // value]` pairs sorted by key. Without this rewrite, for-of falls
    // through to the generic array path and reads `.length` on the
    // raw handle (returns 0 → silent empty loop). Refs #576.
    let is_headers_iter = match &*for_of_stmt.right {
        ast::Expr::Ident(ident) => matches!(
            ctx.lookup_native_instance(ident.sym.as_ref()),
            Some((_, "Headers"))
        ),
        _ => false,
    };

    // `for (const [k, v] of params)` where `params` is a
    // URLSearchParams local. Same shape as the Headers case but
    // tracked via `lookup_local_type` (Type::Named) instead of the
    // native-instance registry. Refs #575.
    let is_urlsp_iter = match &*for_of_stmt.right {
        ast::Expr::Ident(ident) => matches!(
            ctx.lookup_local_type(ident.sym.as_ref()),
            Some(Type::Named(n)) if n == "URLSearchParams"
        ),
        ast::Expr::New(new_expr) => matches!(
            new_expr.callee.as_ref(),
            ast::Expr::Ident(c) if c.sym.as_ref() == "URLSearchParams"
        ),
        _ => false,
    };

    // Lower the iterable expression (the array)
    let arr_expr = lower_expr(ctx, &for_of_stmt.right)?;
    let arr_expr = if is_headers_iter {
        Expr::NativeMethodCall {
            module: "Headers".to_string(),
            class_name: Some("Headers".to_string()),
            object: Some(Box::new(arr_expr)),
            method: "entries".to_string(),
            args: vec![],
        }
    } else if is_urlsp_iter {
        Expr::UrlSearchParamsEntries(Box::new(arr_expr))
    } else {
        arr_expr
    };

    // Issue #302: resolve iterable type from either local var or
    // class instance field (`this.someMap`). Was limited to
    // `Ident` only. Issue #311 extends to plain object property
    // access (`obj.m` where `obj` is a local with an inferred
    // `Type::Object` shape) — without this arm `for (const x of
    // obj.m)` fell through to `None`, the loop read `.length` on
    // a raw Map handle (returns 0), and silently iterated zero
    // times.
    let iterable_type: Option<Type> = match &*for_of_stmt.right {
        ast::Expr::Ident(ident) => ctx.lookup_local_type(ident.sym.as_ref()).cloned(),
        ast::Expr::Member(m) => {
            if matches!(m.obj.as_ref(), ast::Expr::This(_)) {
                if let (Some(cls), ast::MemberProp::Ident(p)) = (ctx.current_class.clone(), &m.prop)
                {
                    ctx.lookup_class_field_type(&cls, p.sym.as_ref()).cloned()
                } else {
                    None
                }
            } else if let ast::MemberProp::Ident(p) = &m.prop {
                let obj_ty = crate::lower_types::infer_type_from_expr(&m.obj, ctx);
                match obj_ty {
                    Type::Object(ot) => ot.properties.get(p.sym.as_ref()).map(|pi| pi.ty.clone()),
                    // Class instance: receiver is `new Example()` or
                    // a local typed `Example`. Consult the same
                    // class_field_types registry the `this.<field>`
                    // arm uses (populated for #302).
                    Type::Named(cls) => ctx.lookup_class_field_type(&cls, p.sym.as_ref()).cloned(),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    };

    // If the iterable is a Map, wrap in MapEntries to convert to array
    // This handles: for (const [k, v] of myMap) { ... } AND
    // for (const [k, v] of this.classMap) { ... } per #302.
    let mut map_key_type: Option<Type> = None;
    let mut map_val_type: Option<Type> = None;
    // Issue #542/#543: also accept Type::Union containing Map (the
    // shape produced by `Map<K, V> | undefined` parameters/returns).
    let type_contains_map =
        |ty: &Type| -> bool { matches!(ty, Type::Generic { base, .. } if base == "Map") };
    let is_iterable_map = match &iterable_type {
        Some(Type::Generic { base, .. }) if base == "Map" => true,
        Some(Type::Union(variants)) => variants.iter().any(type_contains_map),
        _ => false,
    };
    // Fast path: `for (const [k, v] of mapExpr)` with an exact two-element
    // identifier destructure can iterate the Map's flat entries buffer
    // directly via `MapEntryKeyAt` / `MapEntryValueAt`, skipping the N+1
    // small Array allocations that `MapEntries` would do per iteration.
    // Detected here so we can keep the iterable expression unwrapped
    // and emit a different binding/bound shape below.
    // Map fast path also fires for the single-binding shapes
    //   for (const [k] of map)        — only key
    //   for (const [, v] of map)      — only value
    // Each non-empty slot must be a plain Ident (no nested patterns).
    // Anything else falls through to the MapEntries materialization
    // path so destructuring semantics for objects / nested arrays
    // / defaults stay correct.
    let map_kv_fastpath = is_iterable_map
        && match &for_of_stmt.left {
            ast::ForHead::VarDecl(var_decl) => match var_decl.decls.first() {
                Some(decl) => match &decl.name {
                    ast::Pat::Array(arr_pat) => {
                        let len = arr_pat.elems.len();
                        (len == 1 || len == 2)
                            && arr_pat
                                .elems
                                .iter()
                                .all(|e| e.is_none() || matches!(e, Some(ast::Pat::Ident(_))))
                    }
                    _ => false,
                },
                None => false,
            },
            _ => false,
        };
    // Fast path: `for (const x of setExpr)` with a single-Ident
    // binding. Reads elements directly via `SetValueAt` (→
    // `js_set_value_at`) instead of materializing the buffer with
    // `js_set_to_array`. ECS hot paths (changeset.removes, etc.)
    // iterate Sets repeatedly; this saves an Array alloc per loop.
    // Issue #542/#543: also accept Type::Union containing Set.
    let type_contains_set =
        |ty: &Type| -> bool { matches!(ty, Type::Generic { base, .. } if base == "Set") };
    let is_iterable_set = match &iterable_type {
        Some(Type::Generic { base, .. }) if base == "Set" => true,
        Some(Type::Union(variants)) => variants.iter().any(type_contains_set),
        _ => false,
    };
    let set_fastpath = is_iterable_set
        && match &for_of_stmt.left {
            ast::ForHead::VarDecl(var_decl) => match var_decl.decls.first() {
                Some(decl) => matches!(&decl.name, ast::Pat::Ident(_)),
                None => false,
            },
            _ => false,
        };
    // Issue #542/#543: dispatch on `is_iterable_map` / `is_iterable_set`
    // so the Union-with-Map / Union-with-Set shapes also wrap correctly
    // (matches the same fix applied to `lower_decl.rs`'s for-of arm).
    // Extract the Map's K/V type args from whichever variant carries
    // them (direct Generic or the Union's Map arm).
    let map_type_args: Option<Vec<Type>> = if is_iterable_map {
        match &iterable_type {
            Some(Type::Generic { base, type_args }) if base == "Map" => Some(type_args.clone()),
            Some(Type::Union(variants)) => variants.iter().find_map(|v| match v {
                Type::Generic { base, type_args } if base == "Map" => Some(type_args.clone()),
                _ => None,
            }),
            _ => None,
        }
    } else {
        None
    };
    // Issue #578: typed-array iterables. Wrap in `Expr::ArrayFrom`
    // so the holder is a regular Array of materialized element values.
    // Without this, the generated `for (let i=0; i<__arr.length; ++i)
    // __item = __arr[i]` loop reads f64s straight off the typed
    // array's byte-packed storage and yields raw bit reinterpretations.
    // `js_array_clone` (the runtime backing of `ArrayFrom`) detects the
    // typed-array tag and materializes through the per-kind accessor.
    let is_iterable_typed_array = matches!(
        &iterable_type,
        Some(Type::Named(name)) if matches!(name.as_str(),
            "Uint8Array" | "Int8Array" | "Uint8ClampedArray"
            | "Uint16Array" | "Int16Array"
            | "Uint32Array" | "Int32Array"
            | "Float16Array" | "Float32Array" | "Float64Array"
        )
    );
    // #321: the for-of desugar reads `__arr.length` / `__arr[i]` and so
    // assumes the iterable is a plain Array. When the receiver's static
    // type can NOT be proven to be an Array — an `any`-typed Map/Set
    // (effect's `for (const [tag, s] of self.unsafeMap)`), an untyped
    // JS-source value, a `Type::Object` / class instance carrying a
    // custom `[Symbol.iterator]`, etc. — that assumption silently reads
    // `.length` off the wrong handle (Map/Set → 0) and iterates zero
    // times. Detect "the type proves a plain Array" so everything else
    // routes through the runtime default-iterator (`js_for_of_to_array`).
    //
    // We deliberately DON'T wrap the statically-resolved kinds handled
    // above (Map/Set/typed-array via their own materializers, strings via
    // the string index-loop, Headers/URLSearchParams via their entries
    // rewrite) nor proven arrays — those keep their existing fast paths.
    let proven_array = match &iterable_type {
        Some(Type::Array(_)) => true,
        Some(Type::Generic { base, .. }) => base == "Array",
        _ => false,
    };
    let needs_runtime_iterator = !is_string_iter
        && !is_headers_iter
        && !is_urlsp_iter
        && !is_iterable_map
        && !is_iterable_set
        && !is_iterable_typed_array
        && !proven_array;
    if for_of_stmt.is_await && needs_runtime_iterator {
        return lower_runtime_for_await_iterator(ctx, module, for_of_stmt, arr_expr);
    }
    // #for-of lazy iterator protocol: a generic/untyped iterable (custom
    // iterator, generator object, any-typed value) must be driven lazily —
    // pull one element via `__iter.next()` per iteration and run IteratorClose
    // (`__iter.return()`) on an abrupt completion (break / labeled break /
    // labeled continue escaping the loop / return). The previous
    // `ForOfToArray` materialization eagerly drained the iterator up front,
    // which (a) runs a generator past the point a `break` should have closed
    // it and (b) made IteratorClose impossible. `is_await` is already handled
    // by the early return above, so this is always the synchronous path.
    let use_lazy_iter = needs_runtime_iterator;
    let arr_expr = if is_iterable_map {
        if let Some(args) = map_type_args.as_ref() {
            if args.len() >= 2 {
                map_key_type = Some(args[0].clone());
                map_val_type = Some(args[1].clone());
            }
        }
        if map_kv_fastpath {
            arr_expr
        } else {
            Expr::MapEntries(Box::new(arr_expr))
        }
    } else if is_iterable_set {
        if set_fastpath {
            arr_expr
        } else {
            Expr::SetValues(Box::new(arr_expr))
        }
    } else if is_iterable_typed_array {
        Expr::ArrayFrom(Box::new(arr_expr))
    } else if use_lazy_iter {
        // GetIterator(obj): obj[Symbol.iterator](). Drives the lazy loop below.
        Expr::GetIterator(Box::new(arr_expr))
    } else {
        arr_expr
    };

    // Determine the array element type: String for strings, Tuple(K, V) for Maps, Any otherwise.
    // For an identifier iterable like `for (const word of words)` where
    // `words: string[]`, extract the element type from the local's
    // declared Array<T> so the synthesized iteration variable gets
    // the right type (was always Any, breaking `word.length` etc.).
    // #302: also draws Set + class-field Array element types
    // from the resolved `iterable_type` above instead of
    // re-doing the Ident lookup here.
    let elem_type = if is_string_iter {
        Type::String
    } else if let (Some(ref k), Some(ref v)) = (&map_key_type, &map_val_type) {
        Type::Tuple(vec![k.clone(), v.clone()])
    } else if is_iterable_typed_array {
        // Issue #578: typed-array element values are always Number.
        Type::Number
    } else {
        match &iterable_type {
            Some(Type::Array(elem)) => (**elem).clone(),
            Some(Type::Generic { base, type_args }) if base == "Array" && type_args.len() == 1 => {
                type_args[0].clone()
            }
            Some(Type::Generic { base, type_args }) if base == "Set" && !type_args.is_empty() => {
                type_args[0].clone()
            }
            _ => Type::Any,
        }
    };
    // The __arr holder's type: String for string iteration, Map for
    // the Map-fast-path so `__m.size` resolves through `is_map_expr`,
    // Array otherwise.
    let arr_type = if is_string_iter {
        Type::String
    } else if map_kv_fastpath {
        Type::Generic {
            base: "Map".to_string(),
            type_args: vec![
                map_key_type.clone().unwrap_or(Type::Any),
                map_val_type.clone().unwrap_or(Type::Any),
            ],
        }
    } else if set_fastpath {
        Type::Generic {
            base: "Set".to_string(),
            type_args: vec![elem_type.clone()],
        }
    } else if use_lazy_iter {
        // Holds the iterator object, not an array.
        Type::Any
    } else {
        Type::Array(Box::new(elem_type.clone()))
    };

    // Create internal variables for the array and index
    let arr_id = ctx.fresh_local();
    let idx_id = ctx.fresh_local();
    // Register these in the context so they can be looked up
    ctx.locals
        .push((format!("__arr_{}", arr_id), arr_id, arr_type.clone()));
    ctx.locals
        .push((format!("__idx_{}", idx_id), idx_id, Type::Number));

    // For the lazy iterator path `arr_id` holds the iterator and `result_id`
    // holds the most recent `__iter.next()` result `{ value, done }`.
    let result_id = ctx.fresh_local();
    if use_lazy_iter {
        ctx.locals
            .push((format!("__result_{}", result_id), result_id, Type::Any));
    }

    // Store array reference: let __arr = arr (or `let __iter = GetIterator(..)`).
    module.init.push(Stmt::Let {
        id: arr_id,
        name: format!("__arr_{}", arr_id),
        ty: arr_type,
        mutable: false,
        init: Some(arr_expr),
    });

    // IMPORTANT: Define iteration variables BEFORE lowering the body
    // so the body can reference them
    let item_id = ctx.fresh_local();
    ctx.locals
        .push((format!("__item_{}", item_id), item_id, elem_type.clone()));

    // Pre-define all variables from the pattern so body can reference them
    let var_ids: Vec<(String, u32)> = match &for_of_stmt.left {
        ast::ForHead::VarDecl(var_decl) => {
            if let Some(decl) = var_decl.decls.first() {
                match &decl.name {
                    ast::Pat::Ident(ident) => {
                        let name = ident.id.sym.to_string();
                        let id = ctx.define_local(name.clone(), elem_type.clone());
                        vec![(name, id)]
                    }
                    ast::Pat::Array(arr_pat) => {
                        let mut ids = Vec::new();
                        for (idx, elem) in arr_pat.elems.iter().enumerate() {
                            if let Some(elem_pat) = elem {
                                if let ast::Pat::Ident(ident) = elem_pat {
                                    let name = ident.id.sym.to_string();
                                    // For Map destructuring [k, v], use key type for idx 0, value type for idx 1
                                    let var_type = if let Type::Tuple(ref types) = elem_type {
                                        types.get(idx).cloned().unwrap_or(Type::Any)
                                    } else {
                                        Type::Any
                                    };
                                    let id = ctx.define_local(name.clone(), var_type);
                                    ids.push((name, id));
                                }
                            }
                        }
                        ids
                    }
                    ast::Pat::Object(obj_pat) => {
                        let mut ids = Vec::new();
                        for prop in &obj_pat.props {
                            match prop {
                                ast::ObjectPatProp::Assign(assign) => {
                                    let name = assign.key.sym.to_string();
                                    let id = ctx.define_local(name.clone(), Type::Any);
                                    ids.push((name, id));
                                }
                                ast::ObjectPatProp::KeyValue(kv) => {
                                    if let ast::Pat::Ident(ident) = &*kv.value {
                                        let name = ident.id.sym.to_string();
                                        let id = ctx.define_local(name.clone(), Type::Any);
                                        ids.push((name, id));
                                    } else {
                                        // Nested pattern (e.g. `key: [a, b]`).
                                        // Recurse so leaves get pre-defined and
                                        // the body can reference them. Issue #554.
                                        collect_for_of_pattern_leaves(ctx, &kv.value, &mut ids);
                                    }
                                }
                                _ => {}
                            }
                        }
                        ids
                    }
                    _ => {
                        let name = get_binding_name(&decl.name)?;
                        let id = ctx.define_local(name.clone(), Type::Any);
                        vec![(name, id)]
                    }
                }
            } else {
                return Err(anyhow!("for-of requires a variable declaration"));
            }
        }
        ast::ForHead::Pat(pat) => {
            let name = get_pat_name(pat)?;
            let id = ctx.define_local(name.clone(), Type::Any);
            vec![(name, id)]
        }
        _ => return Err(anyhow!("Unsupported for-of left-hand side")),
    };

    // NOW lower the body - variables are defined so body can reference them
    let mut loop_body = lower_body_stmt(ctx, &for_of_stmt.body)?;

    // Build binding statements using the pre-defined variable IDs
    let binding_stmts = match &for_of_stmt.left {
        ast::ForHead::VarDecl(var_decl) => {
            if let Some(decl) = var_decl.decls.first() {
                // `for await (const x of arr)`: spec ECMA-262 §14.7.5.10
                // says each iteration must Await the value yielded by
                // the iterator. For a plain-array iterable that means
                // `await arr[i]` — unwraps a Promise element into its
                // resolved value before binding. Without this, `for
                // await (const x of [Promise.resolve(1), …])` would
                // bind `x = <Promise object>` and any numeric op would
                // see NaN. The iterator-protocol path above already
                // wraps the `__iter.next()` call in `Expr::Await` for
                // async generators; this brings the array-iteration
                // path to parity.
                let item_expr = if use_lazy_iter {
                    // Lazy path: the element is `__result.value`.
                    Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(result_id)),
                        property: "value".to_string(),
                    }
                } else {
                    let raw_item_expr = Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(arr_id)),
                        index: Box::new(Expr::LocalGet(idx_id)),
                    };
                    if for_of_stmt.is_await {
                        Expr::Await(Box::new(raw_item_expr))
                    } else {
                        raw_item_expr
                    }
                };

                match &decl.name {
                    ast::Pat::Ident(_) => {
                        // Simple binding: for (const x of arr)
                        let (name, id) = var_ids[0].clone();
                        let init = if set_fastpath {
                            Expr::SetValueAt {
                                set: Box::new(Expr::LocalGet(arr_id)),
                                idx: Box::new(Expr::LocalGet(idx_id)),
                            }
                        } else {
                            item_expr
                        };
                        vec![Stmt::Let {
                            id,
                            name,
                            ty: elem_type.clone(),
                            mutable: false,
                            init: Some(init),
                        }]
                    }
                    ast::Pat::Array(arr_pat) => {
                        if map_kv_fastpath {
                            // Map [k, v] / [k] / [, v] fast path: read
                            // each requested entry slot directly from
                            // the Map's flat buffer at the loop index.
                            // No `__item` Array materialization. Skipped
                            // slots ([,v] etc.) emit no binding.
                            let key_ty = map_key_type.clone().unwrap_or(Type::Any);
                            let val_ty = map_val_type.clone().unwrap_or(Type::Any);
                            let mut stmts: Vec<Stmt> = Vec::new();
                            let mut var_idx = 0;
                            for (slot, elem) in arr_pat.elems.iter().enumerate() {
                                let Some(ast::Pat::Ident(_)) = elem else {
                                    continue;
                                };
                                let (name, id) = var_ids[var_idx].clone();
                                var_idx += 1;
                                let (ty, init) = if slot == 0 {
                                    (
                                        key_ty.clone(),
                                        Expr::MapEntryKeyAt {
                                            map: Box::new(Expr::LocalGet(arr_id)),
                                            idx: Box::new(Expr::LocalGet(idx_id)),
                                        },
                                    )
                                } else {
                                    (
                                        val_ty.clone(),
                                        Expr::MapEntryValueAt {
                                            map: Box::new(Expr::LocalGet(arr_id)),
                                            idx: Box::new(Expr::LocalGet(idx_id)),
                                        },
                                    )
                                };
                                stmts.push(Stmt::Let {
                                    id,
                                    name,
                                    ty,
                                    mutable: false,
                                    init: Some(init),
                                });
                            }
                            stmts
                        } else {
                            // Array destructuring: for (const [a, b] of arr)
                            let mut stmts = vec![Stmt::Let {
                                id: item_id,
                                name: format!("__item_{}", item_id),
                                ty: elem_type.clone(),
                                mutable: false,
                                init: Some(item_expr),
                            }];

                            // Extract each element using pre-defined IDs
                            let mut var_idx = 0;
                            for (idx, elem) in arr_pat.elems.iter().enumerate() {
                                if let Some(elem_pat) = elem {
                                    if let ast::Pat::Ident(_) = elem_pat {
                                        let (name, id) = var_ids[var_idx].clone();
                                        var_idx += 1;
                                        // For Map destructuring, use the Tuple element type
                                        let var_type = if let Type::Tuple(ref types) = elem_type {
                                            types.get(idx).cloned().unwrap_or(Type::Any)
                                        } else {
                                            Type::Any
                                        };
                                        stmts.push(Stmt::Let {
                                            id,
                                            name,
                                            ty: var_type,
                                            mutable: false,
                                            init: Some(Expr::IndexGet {
                                                object: Box::new(Expr::LocalGet(item_id)),
                                                index: Box::new(Expr::Number(idx as f64)),
                                            }),
                                        });
                                    }
                                }
                            }
                            stmts
                        }
                    }
                    ast::Pat::Object(obj_pat) => {
                        // Object destructuring: for (const { a, b } of arr)
                        let mut stmts = vec![Stmt::Let {
                            id: item_id,
                            name: format!("__item_{}", item_id),
                            ty: Type::Any,
                            mutable: false,
                            init: Some(item_expr),
                        }];

                        // Extract each property using pre-defined IDs
                        let mut var_idx = 0;
                        for prop in &obj_pat.props {
                            match prop {
                                ast::ObjectPatProp::Assign(assign) => {
                                    let prop_name = assign.key.sym.to_string();
                                    let (name, id) = var_ids[var_idx].clone();
                                    var_idx += 1;
                                    let init_value = if let Some(default_expr) = &assign.value {
                                        let prop_access = Expr::PropertyGet {
                                            object: Box::new(Expr::LocalGet(item_id)),
                                            property: prop_name,
                                        };
                                        let default_val = lower_expr(ctx, default_expr)?;
                                        let condition = Expr::Compare {
                                            op: CompareOp::Ne,
                                            left: Box::new(prop_access.clone()),
                                            right: Box::new(Expr::Undefined),
                                        };
                                        Expr::Conditional {
                                            condition: Box::new(condition),
                                            then_expr: Box::new(prop_access),
                                            else_expr: Box::new(default_val),
                                        }
                                    } else {
                                        Expr::PropertyGet {
                                            object: Box::new(Expr::LocalGet(item_id)),
                                            property: prop_name,
                                        }
                                    };
                                    stmts.push(Stmt::Let {
                                        id,
                                        name,
                                        ty: Type::Any,
                                        mutable: false,
                                        init: Some(init_value),
                                    });
                                }
                                ast::ObjectPatProp::KeyValue(kv) => {
                                    let key = match &kv.key {
                                        ast::PropName::Ident(ident) => ident.sym.to_string(),
                                        ast::PropName::Str(s) => {
                                            s.value.as_str().unwrap_or("").to_string()
                                        }
                                        _ => continue,
                                    };
                                    let key_source = Expr::PropertyGet {
                                        object: Box::new(Expr::LocalGet(item_id)),
                                        property: key,
                                    };
                                    if let ast::Pat::Ident(_) = &*kv.value {
                                        let (name, id) = var_ids[var_idx].clone();
                                        var_idx += 1;
                                        stmts.push(Stmt::Let {
                                            id,
                                            name,
                                            ty: Type::Any,
                                            mutable: false,
                                            init: Some(key_source),
                                        });
                                    } else {
                                        // Nested pattern (e.g. `key: [a, b]`).
                                        // Issue #554.
                                        emit_for_of_pattern_binding(
                                            ctx,
                                            &kv.value,
                                            key_source,
                                            &var_ids,
                                            &mut var_idx,
                                            &mut stmts,
                                        )?;
                                    }
                                }
                                _ => {}
                            }
                        }
                        stmts
                    }
                    _ => {
                        let (name, id) = var_ids[0].clone();
                        vec![Stmt::Let {
                            id,
                            name,
                            ty: Type::Any,
                            mutable: false,
                            init: Some(lazy_or_index_elem(
                                use_lazy_iter,
                                arr_id,
                                idx_id,
                                result_id,
                            )),
                        }]
                    }
                }
            } else {
                return Err(anyhow!("for-of requires a variable declaration"));
            }
        }
        ast::ForHead::Pat(_) => {
            let (name, id) = var_ids[0].clone();
            vec![Stmt::Let {
                id,
                name,
                ty: Type::Any,
                mutable: false,
                init: Some(lazy_or_index_elem(use_lazy_iter, arr_id, idx_id, result_id)),
            }]
        }
        _ => return Err(anyhow!("Unsupported for-of left-hand side")),
    };

    // Lazy iterator path: rewrite the user body so every abrupt completion
    // escaping the loop runs IteratorClose (`__iter.return()`) first.
    if use_lazy_iter {
        insert_iterator_close_on_abrupt(&mut loop_body, arr_id, 0, &[]);
    }

    // Prepend the binding statements to the loop body
    for (i, stmt) in binding_stmts.into_iter().enumerate() {
        loop_body.insert(i, stmt);
    }

    if use_lazy_iter {
        module
            .init
            .push(lazy_iter_for_stmt(arr_id, result_id, loop_body));
        ctx.pop_block_scope(for_scope_mark);
        return Ok(());
    }

    // Loop bound. Map/Set fast paths read `.size` (lowered by
    // codegen to `js_map_size` / `js_set_size`); regular path uses
    // `__arr.length` against the materialized iterable.
    let bound_expr = if map_kv_fastpath || set_fastpath {
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(arr_id)),
            property: "size".to_string(),
        }
    } else {
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(arr_id)),
            property: "length".to_string(),
        }
    };
    // Create the for loop:
    // for (let __i = 0; __i < __arr.length; __i++) { ... }
    module.init.push(Stmt::For {
        init: Some(Box::new(Stmt::Let {
            id: idx_id,
            name: format!("__idx_{}", idx_id),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(0.0)),
        })),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(idx_id)),
            right: Box::new(bound_expr),
        }),
        update: Some(Expr::Update {
            id: idx_id,
            op: UpdateOp::Increment,
            prefix: true,
        }),
        body: loop_body,
    });
    ctx.pop_block_scope(for_scope_mark);
    Ok(())
}

pub(crate) fn lower_stmt_for_in(
    ctx: &mut LoweringContext,
    module: &mut Module,
    for_in_stmt: &ast::ForInStmt,
) -> Result<()> {
    // Desugar for-in to a for-of over Object.keys(obj):
    // for (const key in obj) { body }
    // becomes:
    // { let __keys = Object.keys(obj); for (let __i = 0; __i < __keys.length; __i++) { const key = __keys[__i]; body } }
    // Push a block scope so the loop key and internal temporaries don't leak.
    let for_scope_mark = ctx.push_block_scope();

    // Get the iteration variable name
    let key_name = match &for_in_stmt.left {
        ast::ForHead::VarDecl(var_decl) => {
            if let Some(decl) = var_decl.decls.first() {
                get_binding_name(&decl.name)?
            } else {
                return Err(anyhow!("for-in requires a variable declaration"));
            }
        }
        ast::ForHead::Pat(pat) => get_pat_name(pat)?,
        _ => return Err(anyhow!("Unsupported for-in left-hand side")),
    };

    // Lower the object expression
    let obj_expr = lower_expr(ctx, &for_in_stmt.right)?;

    // for-in enumerates the receiver's own AND inherited enumerable string
    // keys (deduplicated), and is a no-op — not a throw — on null/undefined.
    // `ForInKeys` carries those semantics; `ObjectKeys` (Object.keys) would
    // throw on nullish and miss inherited keys. Refs language/statements/for-in
    // S12.6.4_A1/A2 (nullish) and A6/A6.1 (prototype chain).
    let keys_expr = Expr::ForInKeys(Box::new(obj_expr));

    // Create internal variables for the keys array and index
    let keys_id = ctx.fresh_local();
    let idx_id = ctx.fresh_local();
    let key_id = ctx.define_local(key_name.clone(), Type::String);

    // Store keys array reference: let __keys = Object.keys(obj)
    module.init.push(Stmt::Let {
        id: keys_id,
        name: format!("__keys_{}", keys_id),
        ty: Type::Array(Box::new(Type::String)),
        mutable: false,
        init: Some(keys_expr),
    });

    // Lower the body
    let mut loop_body = lower_body_stmt(ctx, &for_in_stmt.body)?;

    // Prepend: const key = __keys[__i]
    loop_body.insert(
        0,
        Stmt::Let {
            id: key_id,
            name: key_name,
            ty: Type::String,
            mutable: false,
            init: Some(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(keys_id)),
                index: Box::new(Expr::LocalGet(idx_id)),
            }),
        },
    );

    // Create the for loop:
    // for (let __i = 0; __i < __keys.length; __i++) { ... }
    module.init.push(Stmt::For {
        init: Some(Box::new(Stmt::Let {
            id: idx_id,
            name: format!("__idx_{}", idx_id),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(0.0)),
        })),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(idx_id)),
            right: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(keys_id)),
                property: "length".to_string(),
            }),
        }),
        update: Some(Expr::Update {
            id: idx_id,
            op: UpdateOp::Increment,
            prefix: true,
        }),
        body: loop_body,
    });
    ctx.pop_block_scope(for_scope_mark);
    Ok(())
}
