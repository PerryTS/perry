//! Whole-module scans that gate the producer set: detect callers that
//! use the producer's return value in unsupported expression positions,
//! and detect non-callee FuncRef references (closures over the
//! producer, callback arguments, etc.).

use super::*;

/// Records `FuncId`s whose calls appear in unsupported expression
/// positions. Supported positions:
/// 1. `Stmt::Let { init: Some(Expr::Call { callee: FuncRef(id), .. }) }` — let-bind producer call
/// 2. `Stmt::Expr(Expr::Call { callee: FuncRef(id), .. })` — bare call (return ignored)
///
/// Anywhere else (e.g. `f(args).join()`, `return f(args)`,
/// `someFn(f(args))`) is unsafe because the rewritten producer
/// returns `undefined`. Any caller relying on the array as a value
/// in expression context would break.
pub fn scan_unsafe_call_sites(
    stmts: &[Stmt],
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    for s in stmts {
        scan_stmt_call_sites(s, candidates, out);
    }
}

fn scan_stmt_call_sites(
    stmt: &Stmt,
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                // Allowed shape: top-level Call { callee: FuncRef(prod) }
                if let Expr::Call { callee, args, .. } = e {
                    if matches!(callee.as_ref(), Expr::FuncRef(id) if candidates.contains_key(id)) {
                        // The CALL ITSELF is fine. But its args may
                        // themselves contain producer calls in unsafe
                        // positions; recurse into args only.
                        for a in args {
                            scan_expr_call_sites(a, candidates, out);
                        }
                        return;
                    }
                }
                scan_expr_call_sites(e, candidates, out);
            }
        }
        Stmt::Expr(e) => {
            // Allowed shape: top-level Stmt::Expr(Call { callee: FuncRef(prod) })
            if let Expr::Call { callee, args, .. } = e {
                if matches!(callee.as_ref(), Expr::FuncRef(id) if candidates.contains_key(id)) {
                    for a in args {
                        scan_expr_call_sites(a, candidates, out);
                    }
                    return;
                }
            }
            scan_expr_call_sites(e, candidates, out);
        }
        Stmt::Throw(e) => scan_expr_call_sites(e, candidates, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                scan_expr_call_sites(e, candidates, out);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr_call_sites(condition, candidates, out);
            scan_unsafe_call_sites(then_branch, candidates, out);
            if let Some(eb) = else_branch {
                scan_unsafe_call_sites(eb, candidates, out);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            scan_expr_call_sites(condition, candidates, out);
            scan_unsafe_call_sites(body, candidates, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_call_sites(i, candidates, out);
            }
            if let Some(c) = condition {
                scan_expr_call_sites(c, candidates, out);
            }
            if let Some(u) = update {
                scan_expr_call_sites(u, candidates, out);
            }
            scan_unsafe_call_sites(body, candidates, out);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            scan_unsafe_call_sites(body, candidates, out);
            if let Some(c) = catch {
                scan_unsafe_call_sites(&c.body, candidates, out);
            }
            if let Some(f) = finally {
                scan_unsafe_call_sites(f, candidates, out);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_call_sites(discriminant, candidates, out);
            for c in cases {
                if let Some(t) = &c.test {
                    scan_expr_call_sites(t, candidates, out);
                }
                scan_unsafe_call_sites(&c.body, candidates, out);
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt_call_sites(body, candidates, out),
        _ => {}
    }
}

/// Walk an expression. Any `Expr::Call { callee: FuncRef(id) }` here
/// (where `id` is in `candidates`) is in expression position (a
/// nested context, not a top-level Stmt::Let or Stmt::Expr) — record
/// the producer as unsafe.
fn scan_expr_call_sites(
    e: &Expr,
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    if let Expr::Call { callee, .. } = e {
        if let Expr::FuncRef(id) = callee.as_ref() {
            if candidates.contains_key(id) {
                out.insert(*id);
            }
        }
    }
    walk_expr_children(e, &mut |child| scan_expr_call_sites(child, candidates, out));
}

/// Records `FuncId`s whose `Expr::FuncRef(id)` is observed in a
/// non-callee position (function value, callback arg, stored to a
/// local, etc.). The set of "misused" producers is then subtracted
/// from the candidate set so the rewrite only fires on functions
/// whose every use is a direct call.
pub fn scan_funcref_misuses(
    stmts: &[Stmt],
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    for s in stmts {
        scan_stmt_funcrefs(s, candidates, out);
    }
}

fn scan_stmt_funcrefs(
    stmt: &Stmt,
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                scan_expr_funcrefs(e, candidates, out);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => scan_expr_funcrefs(e, candidates, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                scan_expr_funcrefs(e, candidates, out);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr_funcrefs(condition, candidates, out);
            scan_funcref_misuses(then_branch, candidates, out);
            if let Some(eb) = else_branch {
                scan_funcref_misuses(eb, candidates, out);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            scan_expr_funcrefs(condition, candidates, out);
            scan_funcref_misuses(body, candidates, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_funcrefs(i, candidates, out);
            }
            if let Some(c) = condition {
                scan_expr_funcrefs(c, candidates, out);
            }
            if let Some(u) = update {
                scan_expr_funcrefs(u, candidates, out);
            }
            scan_funcref_misuses(body, candidates, out);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            scan_funcref_misuses(body, candidates, out);
            if let Some(c) = catch {
                scan_funcref_misuses(&c.body, candidates, out);
            }
            if let Some(f) = finally {
                scan_funcref_misuses(f, candidates, out);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_funcrefs(discriminant, candidates, out);
            for c in cases {
                if let Some(t) = &c.test {
                    scan_expr_funcrefs(t, candidates, out);
                }
                scan_funcref_misuses(&c.body, candidates, out);
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt_funcrefs(body, candidates, out),
        _ => {}
    }
}

fn scan_expr_funcrefs(
    e: &Expr,
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    // Direct callee FuncRefs are SAFE (they're being called). Visit
    // only the args. Anywhere else (a bare FuncRef in argument
    // position, a let-init, etc.) is a "misuse" and we record it.
    match e {
        Expr::Call { callee, args, .. } => {
            // Don't recurse into the FuncRef callee, but DO recurse
            // into anything else.
            if !matches!(callee.as_ref(), Expr::FuncRef(id) if candidates.contains_key(id)) {
                scan_expr_funcrefs(callee, candidates, out);
            }
            for a in args {
                scan_expr_funcrefs(a, candidates, out);
            }
            return;
        }
        Expr::CallSpread { callee, args, .. } => {
            if !matches!(callee.as_ref(), Expr::FuncRef(id) if candidates.contains_key(id)) {
                scan_expr_funcrefs(callee, candidates, out);
            }
            for a in args {
                match a {
                    perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => {
                        scan_expr_funcrefs(e, candidates, out);
                    }
                }
            }
            return;
        }
        Expr::FuncRef(id) if candidates.contains_key(id) => {
            // Bare FuncRef in non-callee position → misuse.
            out.insert(*id);
            return;
        }
        _ => {}
    }
    walk_expr_children(e, &mut |child| scan_expr_funcrefs(child, candidates, out));
}
