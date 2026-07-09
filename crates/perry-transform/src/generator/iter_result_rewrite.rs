//! Inline IterResultGet / IterResultSet rewriting over the final HIR.

use super::*;

pub fn rewrite_expr(expr: &mut Expr) {
    // Match the exact `{value: V, done: bool}` shape and replace
    // BEFORE recursing — once we've rewritten, the children are
    // already visited inside the new IterResultSet's child expression.
    if let Expr::Object(props) = expr {
        if props.len() == 2 {
            let has_value = props.iter().any(|(k, _)| k == "value");
            let has_done = props.iter().any(|(k, _)| k == "done");
            if has_value && has_done {
                // Resolve done to a literal bool.
                let done_expr = props
                    .iter()
                    .find(|(k, _)| k == "done")
                    .map(|(_, v)| v.clone())
                    .unwrap();
                let done_bool = match &done_expr {
                    Expr::Bool(b) => Some(*b),
                    _ => None,
                };
                if let Some(done_b) = done_bool {
                    let mut value_expr = props
                        .iter()
                        .find(|(k, _)| k == "value")
                        .map(|(_, v)| v.clone())
                        .unwrap();
                    // Recurse INTO the value expression first so nested
                    // iter-result allocs (rare but possible inside
                    // user-side returns) are also rewritten.
                    rewrite_expr(&mut value_expr);
                    *expr = Expr::IterResultSet(Box::new(value_expr), done_b);
                    return;
                }
            }
        }
    }
    rewrite_expr_children(expr);
}

pub fn rewrite_expr_children(expr: &mut Expr) {
    // Recurse into all child Exprs. For Closure, ALSO recurse into
    // body statements (the centralised walker explicitly excludes
    // closure bodies, but we need them — the generator transform's
    // synthesized iter-result returns live inside next/return/throw
    // closures' bodies).
    match expr {
        Expr::Closure { body, params, .. } => {
            for stmt in body.iter_mut() {
                rewrite_stmt(stmt);
            }
            for p in params.iter_mut() {
                if let Some(d) = p.default.as_mut() {
                    rewrite_expr(d);
                }
            }
        }
        _ => {
            perry_hir::walker::walk_expr_children_mut(expr, &mut rewrite_expr);
        }
    }
}

pub fn rewrite_stmt(stmt: &mut Stmt) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => rewrite_expr(e),
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => {}
        Stmt::Let { init, .. } => {
            if let Some(e) = init.as_mut() {
                rewrite_expr(e);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_expr(condition);
            for s in then_branch.iter_mut() {
                rewrite_stmt(s);
            }
            if let Some(eb) = else_branch.as_mut() {
                for s in eb.iter_mut() {
                    rewrite_stmt(s);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            rewrite_expr(condition);
            for s in body.iter_mut() {
                rewrite_stmt(s);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init.as_mut() {
                rewrite_stmt(s);
            }
            if let Some(e) = condition.as_mut() {
                rewrite_expr(e);
            }
            if let Some(e) = update.as_mut() {
                rewrite_expr(e);
            }
            for s in body.iter_mut() {
                rewrite_stmt(s);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body.iter_mut() {
                rewrite_stmt(s);
            }
            if let Some(c) = catch.as_mut() {
                for s in c.body.iter_mut() {
                    rewrite_stmt(s);
                }
            }
            if let Some(f) = finally.as_mut() {
                for s in f.iter_mut() {
                    rewrite_stmt(s);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            rewrite_expr(discriminant);
            for c in cases.iter_mut() {
                if let Some(e) = c.test.as_mut() {
                    rewrite_expr(e);
                }
                for s in c.body.iter_mut() {
                    rewrite_stmt(s);
                }
            }
        }
        Stmt::Labeled { body, .. } => rewrite_stmt(body),
    }
}
