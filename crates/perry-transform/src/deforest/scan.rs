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

/// Records `FuncId`s that are referenced ANYWHERE inside a closure
/// body. Refs #5136.
///
/// Both the producer-detection scans above and the phase-3 call-site
/// rewriter stop at closure boundaries: the shared `walk_expr_children`
/// helper visits an `Expr::Closure`'s parameter defaults but never
/// descends into its statement body. So a producer called from inside
/// a closure is invisible to detection (it looks safe to deforest) AND
/// to the rewriter (its call site never gets the synthetic `+1`
/// accumulator argument). The result is a hard arity mismatch — the
/// rewritten producer is defined with the extra out-param while the
/// in-closure call still passes the original arity — which lowers to a
/// garbage accumulator pointer and a SIGSEGV at runtime.
///
/// Rather than teach the rewriter to descend into (and correctly
/// re-scope) closure bodies — the complete-but-risky fix, deferred as
/// a follow-up like the other closure-walking refinements noted in
/// `detect.rs` — conservatively bail on any producer referenced inside
/// a closure. Both inside- and outside-closure call sites then keep the
/// original signature. Every legitimate use of a producer is a direct
/// call whose callee is `Expr::FuncRef(id)`, so flagging any candidate
/// `FuncRef` seen within a closure catches all in-closure call sites.
pub fn scan_producers_used_in_closures(
    stmts: &[Stmt],
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    for s in stmts {
        scan_stmt_for_closures(s, candidates, out);
    }
}

fn scan_stmt_for_closures(
    stmt: &Stmt,
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                scan_expr_for_closures(e, candidates, out);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => scan_expr_for_closures(e, candidates, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                scan_expr_for_closures(e, candidates, out);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr_for_closures(condition, candidates, out);
            scan_producers_used_in_closures(then_branch, candidates, out);
            if let Some(eb) = else_branch {
                scan_producers_used_in_closures(eb, candidates, out);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            scan_expr_for_closures(condition, candidates, out);
            scan_producers_used_in_closures(body, candidates, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_for_closures(i, candidates, out);
            }
            if let Some(c) = condition {
                scan_expr_for_closures(c, candidates, out);
            }
            if let Some(u) = update {
                scan_expr_for_closures(u, candidates, out);
            }
            scan_producers_used_in_closures(body, candidates, out);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            scan_producers_used_in_closures(body, candidates, out);
            if let Some(c) = catch {
                scan_producers_used_in_closures(&c.body, candidates, out);
            }
            if let Some(f) = finally {
                scan_producers_used_in_closures(f, candidates, out);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_for_closures(discriminant, candidates, out);
            for c in cases {
                if let Some(t) = &c.test {
                    scan_expr_for_closures(t, candidates, out);
                }
                scan_producers_used_in_closures(&c.body, candidates, out);
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt_for_closures(body, candidates, out),
        // Leaf statements with no nested expressions or statements —
        // nothing to walk. Listed explicitly (no `_` catch-all) so a
        // future `Stmt` variant that DOES carry a closure forces a
        // compile error here instead of being silently skipped.
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

/// Walk an expression looking for `Expr::Closure`. On finding one,
/// flag every candidate `FuncRef` anywhere in the closure body (and in
/// any nested closures) as unsafe. Parameter-default expressions
/// evaluate in the OUTER scope, so they are walked normally rather than
/// treated as inside-closure.
fn scan_expr_for_closures(
    e: &Expr,
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    if let Expr::Closure { params, body, .. } = e {
        for p in params {
            if let Some(d) = &p.default {
                scan_expr_for_closures(d, candidates, out);
            }
        }
        collect_candidate_funcrefs_in_stmts(body, candidates, out);
        return;
    }
    walk_expr_children(e, &mut |child| {
        scan_expr_for_closures(child, candidates, out)
    });
}

/// Inside a closure body, mark every candidate `FuncRef` (in any
/// position — callee or value) as unsafe. Recurses into nested stmts
/// and nested closures.
fn collect_candidate_funcrefs_in_stmts(
    stmts: &[Stmt],
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    let mut found = false;
    let mut collector = ClosureFuncRefCollector {
        candidates,
        out,
        found: &mut found,
    };
    for s in stmts {
        collector.visit_stmt(s);
    }
}

struct ClosureFuncRefCollector<'a> {
    candidates: &'a HashMap<FuncId, ProducerInfo>,
    out: &'a mut HashSet<FuncId>,
    found: &'a mut bool,
}

impl ClosureFuncRefCollector<'_> {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        let mut walker = StmtRefAllWalker {
            visit: &mut |e: &Expr| {
                if let Expr::FuncRef(id) = e {
                    if self.candidates.contains_key(id) {
                        self.out.insert(*id);
                        *self.found = true;
                    }
                }
            },
        };
        walker.visit_stmt(stmt);
    }
}

/// Generic stmt walker that invokes `visit` on every expression
/// (including those nested in closure bodies and all control flow).
struct StmtRefAllWalker<'a> {
    visit: &'a mut dyn FnMut(&Expr),
}

impl StmtRefAllWalker<'_> {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.visit_expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.visit_expr(e),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    self.visit_expr(e);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.visit_expr(condition);
                for s in then_branch {
                    self.visit_stmt(s);
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.visit_expr(condition);
                for s in body {
                    self.visit_stmt(s);
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.visit_stmt(i);
                }
                if let Some(c) = condition {
                    self.visit_expr(c);
                }
                if let Some(u) = update {
                    self.visit_expr(u);
                }
                for s in body {
                    self.visit_stmt(s);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    self.visit_stmt(s);
                }
                if let Some(c) = catch {
                    for s in &c.body {
                        self.visit_stmt(s);
                    }
                }
                if let Some(f) = finally {
                    for s in f {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.visit_expr(discriminant);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.visit_expr(t);
                    }
                    for s in &c.body {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::Labeled { body, .. } => self.visit_stmt(body),
            // Leaf statements — no nested expressions or statements.
            // Listed explicitly (no `_` catch-all) so a future variant
            // carrying expressions forces a revisit here.
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }

    fn visit_expr(&mut self, e: &Expr) {
        (self.visit)(e);
        // Descend into closure bodies too — a producer call nested in a
        // closure-within-a-closure is just as unreachable to the
        // rewriter as one in the outer closure.
        if let Expr::Closure { body, .. } = e {
            for s in body {
                self.visit_stmt(s);
            }
        }
        walk_expr_children(e, &mut |child| self.visit_expr(child));
    }
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
