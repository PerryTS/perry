use std::collections::HashSet;

pub fn collect_closures_in_stmts(
    stmts: &[perry_hir::Stmt],
    seen: &mut HashSet<perry_hir::types::FuncId>,
    out: &mut Vec<(perry_hir::types::FuncId, perry_hir::Expr)>,
) {
    for s in stmts {
        match s {
            perry_hir::Stmt::Expr(e) | perry_hir::Stmt::Throw(e) => {
                collect_closures_in_expr(e, seen, out);
            }
            perry_hir::Stmt::Return(opt) => {
                if let Some(e) = opt {
                    collect_closures_in_expr(e, seen, out);
                }
            }
            perry_hir::Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    collect_closures_in_expr(e, seen, out);
                }
            }
            perry_hir::Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_closures_in_expr(condition, seen, out);
                collect_closures_in_stmts(then_branch, seen, out);
                if let Some(eb) = else_branch {
                    collect_closures_in_stmts(eb, seen, out);
                }
            }
            perry_hir::Stmt::While { condition, body } => {
                collect_closures_in_expr(condition, seen, out);
                collect_closures_in_stmts(body, seen, out);
            }
            perry_hir::Stmt::DoWhile { body, condition } => {
                collect_closures_in_stmts(body, seen, out);
                collect_closures_in_expr(condition, seen, out);
            }
            perry_hir::Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    collect_closures_in_stmts(std::slice::from_ref(init_stmt), seen, out);
                }
                if let Some(cond) = condition {
                    collect_closures_in_expr(cond, seen, out);
                }
                if let Some(upd) = update {
                    collect_closures_in_expr(upd, seen, out);
                }
                collect_closures_in_stmts(body, seen, out);
            }
            perry_hir::Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_closures_in_expr(discriminant, seen, out);
                for case in cases {
                    if let Some(test) = &case.test {
                        collect_closures_in_expr(test, seen, out);
                    }
                    collect_closures_in_stmts(&case.body, seen, out);
                }
            }
            perry_hir::Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_closures_in_stmts(body, seen, out);
                if let Some(c) = catch {
                    collect_closures_in_stmts(&c.body, seen, out);
                }
                if let Some(f) = finally {
                    collect_closures_in_stmts(f, seen, out);
                }
            }
            perry_hir::Stmt::Labeled { body, .. } => {
                collect_closures_in_stmts(std::slice::from_ref(body.as_ref()), seen, out);
            }
            _ => {}
        }
    }
}

pub fn collect_closures_in_expr(
    e: &perry_hir::Expr,
    seen: &mut HashSet<perry_hir::types::FuncId>,
    out: &mut Vec<(perry_hir::types::FuncId, perry_hir::Expr)>,
) {
    use perry_hir::Expr;

    // Step 1 — register `e` itself when it's a closure, and descend into
    // its body. The centralized walker (Step 2) only visits direct `Expr`
    // children of a `Closure` (its `Param.default` exprs); the body is a
    // `Vec<Stmt>` and is intentionally outside its responsibility per
    // `perry_hir::walker` module docs, so we descend manually here.
    if let Expr::Closure { func_id, body, .. } = e {
        if seen.insert(*func_id) {
            out.push((*func_id, e.clone()));
        }
        collect_closures_in_stmts(body, seen, out);
    }

    // Step 2 — recurse into every direct sub-expression by delegating to
    // the centralized exhaustive walker.
    //
    // This replaces a long ad-hoc match (with a `_ => {}` catch-all) that
    // historically dropped closures hidden inside variants like
    // `Expr::RegExpReplaceFn { callback }`,
    // `Expr::NetCreateServer { connection_listener }`,
    // `Expr::ProxyNew { handler }` / `Expr::ProxyApply { args }`, the
    // Reflect.* family, and many others — producing
    // "use of undefined value @perry_closure_*" link errors at clang time
    // (issue #318, recurrence of the v0.5.323 / v0.5.388 / v0.5.396 /
    // v0.5.405 walker bug class).
    //
    // Delegating to `walk_expr_children` gives us compile-time
    // enforcement: any new HIR variant added to `ir.rs` becomes a
    // `non-exhaustive match` error in `walker.rs` until it's listed
    // there, exactly the property the v0.5.329 Tier 1.1 fix introduced
    // for the four other walker consumers (`substitute_locals`,
    // `find_max_local_id`, `collect_local_refs_expr`,
    // `remap_local_ids_in_expr`).
    perry_hir::walker::walk_expr_children(e, &mut |sub| {
        collect_closures_in_expr(sub, seen, out);
    });
}

// NOTE: `collect_extern_func_refs_in_*` previously lived here as a
// pre-walker that scanned the HIR for cross-module Call sites and
// added a `declare` for each one to the LLVM module. It missed any
// Expr::ExternFuncRef hidden inside an Expr variant the walker didn't
// recurse into (Closure body, ArrayMap callback, Stmt::Try, etc.),
// which produced clang "use of undefined value @perry_fn_*" errors.
// Replaced by lazy declares emitted from `lower_call.rs` directly via
// `FnCtx.pending_declares`, drained back into the module after each
// compile_function/method/closure/static call returns.
