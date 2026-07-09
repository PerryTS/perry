//! Closure-body local-set / outer-write analysis helpers used by the
//! perry/thread thread-safety check inside `lower_native_method_call`.

/// Walk a statement to collect LocalIds declared inside a closure body —
/// `Stmt::Let` and `Stmt::For` init `let`s. Used by the perry/thread
/// thread-safety check to distinguish inner locals (safe to write) from
/// captures (unsafe). Recurses into nested control-flow but deliberately
/// NOT into nested closures: those have their own inner-id set.
pub fn collect_closure_introduced_ids(
    stmt: &perry_hir::Stmt,
    out: &mut std::collections::HashSet<perry_types::LocalId>,
) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { id, .. } => {
            out.insert(*id);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                collect_closure_introduced_ids(s, out);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    collect_closure_introduced_ids(s, out);
                }
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for s in body {
                collect_closure_introduced_ids(s, out);
            }
        }
        Stmt::For { init, body, .. } => {
            if let Some(init_stmt) = init.as_ref() {
                collect_closure_introduced_ids(init_stmt, out);
            }
            for s in body {
                collect_closure_introduced_ids(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_closure_introduced_ids(s, out);
            }
            if let Some(cc) = catch {
                if let Some((id, _)) = &cc.param {
                    out.insert(*id);
                }
                for s in &cc.body {
                    collect_closure_introduced_ids(s, out);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    collect_closure_introduced_ids(s, out);
                }
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                for s in &case.body {
                    collect_closure_introduced_ids(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => collect_closure_introduced_ids(body, out),
        _ => {} // Expr, Return, Throw, Break, Continue, LabeledBreak/Continue — don't declare locals
    }
}

/// Walk a statement looking for LocalSet / Update whose target LocalId is
/// NOT in `inner_ids` — i.e. the closure is writing to a captured or
/// module-level variable. Does NOT recurse into nested Closure expressions
/// (those are a separate scope with their own check when they're passed to
/// a threading primitive).
pub fn find_outer_writes_stmt(
    stmt: &perry_hir::Stmt,
    inner_ids: &std::collections::HashSet<perry_types::LocalId>,
    out: &mut Vec<perry_types::LocalId>,
) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init {
                find_outer_writes_expr(expr, inner_ids, out);
            }
        }
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => {
            find_outer_writes_expr(e, inner_ids, out);
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            find_outer_writes_expr(condition, inner_ids, out);
            for s in then_branch {
                find_outer_writes_stmt(s, inner_ids, out);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    find_outer_writes_stmt(s, inner_ids, out);
                }
            }
        }
        Stmt::While { condition, body } => {
            find_outer_writes_expr(condition, inner_ids, out);
            for s in body {
                find_outer_writes_stmt(s, inner_ids, out);
            }
        }
        Stmt::DoWhile { condition, body } => {
            for s in body {
                find_outer_writes_stmt(s, inner_ids, out);
            }
            find_outer_writes_expr(condition, inner_ids, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init.as_ref() {
                find_outer_writes_stmt(init_stmt, inner_ids, out);
            }
            if let Some(c) = condition {
                find_outer_writes_expr(c, inner_ids, out);
            }
            if let Some(u) = update {
                find_outer_writes_expr(u, inner_ids, out);
            }
            for s in body {
                find_outer_writes_stmt(s, inner_ids, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                find_outer_writes_stmt(s, inner_ids, out);
            }
            if let Some(cc) = catch {
                for s in &cc.body {
                    find_outer_writes_stmt(s, inner_ids, out);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    find_outer_writes_stmt(s, inner_ids, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            find_outer_writes_expr(discriminant, inner_ids, out);
            for case in cases {
                if let Some(val) = &case.test {
                    find_outer_writes_expr(val, inner_ids, out);
                }
                for s in &case.body {
                    find_outer_writes_stmt(s, inner_ids, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => find_outer_writes_stmt(body, inner_ids, out),
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

fn find_outer_writes_expr(
    expr: &perry_hir::Expr,
    inner_ids: &std::collections::HashSet<perry_types::LocalId>,
    out: &mut Vec<perry_types::LocalId>,
) {
    use perry_hir::Expr;
    match expr {
        Expr::LocalSet(id, val) => {
            if !inner_ids.contains(id) {
                out.push(*id);
            }
            find_outer_writes_expr(val, inner_ids, out);
        }
        Expr::Update { id, .. } if !inner_ids.contains(id) => {
            out.push(*id);
        }
        Expr::Closure { .. } => {
            // Stop at nested closure boundary — it has its own scope and
            // will be checked separately if it's the one being passed to
            // a threading primitive.
        }
        Expr::Binary { left, right, .. } => {
            find_outer_writes_expr(left, inner_ids, out);
            find_outer_writes_expr(right, inner_ids, out);
        }
        Expr::Call { callee, args, .. } => {
            find_outer_writes_expr(callee, inner_ids, out);
            for a in args {
                find_outer_writes_expr(a, inner_ids, out);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                find_outer_writes_expr(o, inner_ids, out);
            }
            for a in args {
                find_outer_writes_expr(a, inner_ids, out);
            }
        }
        Expr::PropertyGet { object, .. } => {
            find_outer_writes_expr(object, inner_ids, out);
        }
        Expr::IndexGet { object, index } => {
            find_outer_writes_expr(object, inner_ids, out);
            find_outer_writes_expr(index, inner_ids, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                find_outer_writes_expr(e, inner_ids, out);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            find_outer_writes_expr(condition, inner_ids, out);
            find_outer_writes_expr(then_expr, inner_ids, out);
            find_outer_writes_expr(else_expr, inner_ids, out);
        }
        _ => {} // Literals, LocalGet, GlobalGet, etc. — no writes
    }
}
