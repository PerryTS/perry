//! Shared HIR walkers and id-bookkeeping helpers used across the
//! deforestation passes.

use super::*;

/// Generic child-walker for `Expr` — visits every direct sub-expression.
///
/// Delegates to [`perry_hir::walker::walk_expr_children`] so every
/// `Expr` variant is covered exhaustively. #2047: the previous local
/// implementation had a `_ => {}` catch-all and treated less-common
/// variants (e.g. `JsonStringifyFull`) as leaves — producer calls in
/// expression position inside those variants slipped past the unsafe-
/// site scan, the rewritten function returned `undefined`, and inline
/// callers like `JSON.stringify(f())` silently broke. The exhaustive
/// walker is also stricter for the substitute / out-usage analyses,
/// which share the same walker.
pub fn walk_expr_children(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    // Wrap `f` in a sized closure — the perry-hir walker is generic
    // over `F: FnMut`, which requires `Sized`; the `dyn FnMut` we take
    // here doesn't satisfy that directly.
    perry_hir::walker::walk_expr_children(e, &mut |child: &Expr| f(child));
}

/// Mutable child-walker for Expr. Mirrors `walk_expr_children`; see
/// the doc-comment there for why we delegate to the perry-hir walker.
pub fn walk_expr_children_mut(e: &mut Expr, f: &mut dyn FnMut(&mut Expr)) {
    perry_hir::walker::walk_expr_children_mut(e, &mut |child: &mut Expr| f(child));
}

/// Returns the highest LocalId seen anywhere in the module — used as
/// the seed for fresh-id allocation when adding synthetic params /
/// temporaries.
pub fn max_local_id(module: &Module) -> LocalId {
    let mut max_id: LocalId = 0;
    for f in &module.functions {
        for p in &f.params {
            max_id = max_id.max(p.id);
        }
        max_in_stmts(&f.body, &mut max_id);
    }
    max_in_stmts(&module.init, &mut max_id);
    for c in &module.classes {
        for m in &c.methods {
            for p in &m.params {
                max_id = max_id.max(p.id);
            }
            max_in_stmts(&m.body, &mut max_id);
        }
        if let Some(ctor) = &c.constructor {
            for p in &ctor.params {
                max_id = max_id.max(p.id);
            }
            max_in_stmts(&ctor.body, &mut max_id);
        }
        // Accessors and statics hold call sites too (see the matching
        // detection scans + phase-3 rewrite). Cover them so the fresh-id
        // seed never collides with a local already living in those bodies.
        for (_, getter) in &c.getters {
            for p in &getter.params {
                max_id = max_id.max(p.id);
            }
            max_in_stmts(&getter.body, &mut max_id);
        }
        for (_, setter) in &c.setters {
            for p in &setter.params {
                max_id = max_id.max(p.id);
            }
            max_in_stmts(&setter.body, &mut max_id);
        }
        for m in &c.static_methods {
            for p in &m.params {
                max_id = max_id.max(p.id);
            }
            max_in_stmts(&m.body, &mut max_id);
        }
    }
    max_id
}

pub fn max_in_stmts(stmts: &[Stmt], max_id: &mut LocalId) {
    for s in stmts {
        max_in_stmt(s, max_id);
    }
}

pub fn max_in_stmt(stmt: &Stmt, max_id: &mut LocalId) {
    match stmt {
        Stmt::Let { id, init, .. } => {
            *max_id = (*max_id).max(*id);
            if let Some(e) = init {
                max_in_expr(e, max_id);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => max_in_expr(e, max_id),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                max_in_expr(e, max_id);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            max_in_expr(condition, max_id);
            max_in_stmts(then_branch, max_id);
            if let Some(eb) = else_branch {
                max_in_stmts(eb, max_id);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            max_in_expr(condition, max_id);
            max_in_stmts(body, max_id);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                max_in_stmt(i, max_id);
            }
            if let Some(c) = condition {
                max_in_expr(c, max_id);
            }
            if let Some(u) = update {
                max_in_expr(u, max_id);
            }
            max_in_stmts(body, max_id);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            max_in_stmts(body, max_id);
            if let Some(c) = catch {
                if let Some((id, _)) = &c.param {
                    *max_id = (*max_id).max(*id);
                }
                max_in_stmts(&c.body, max_id);
            }
            if let Some(f) = finally {
                max_in_stmts(f, max_id);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            max_in_expr(discriminant, max_id);
            for c in cases {
                if let Some(t) = &c.test {
                    max_in_expr(t, max_id);
                }
                max_in_stmts(&c.body, max_id);
            }
        }
        Stmt::Labeled { body, .. } => max_in_stmt(body, max_id),
        Stmt::PreallocateBoxes(ids) => {
            for id in ids {
                *max_id = (*max_id).max(*id);
            }
        }
        _ => {}
    }
}

pub fn max_in_expr(e: &Expr, max_id: &mut LocalId) {
    match e {
        Expr::LocalGet(id) | Expr::LocalSet(id, _) => *max_id = (*max_id).max(*id),
        Expr::Update { id, .. } => *max_id = (*max_id).max(*id),
        _ => {}
    }
    walk_expr_children(e, &mut |child| max_in_expr(child, max_id));
}

pub fn max_local_id_for_func(func: &Function) -> LocalId {
    let mut max_id: LocalId = 0;
    for p in &func.params {
        max_id = max_id.max(p.id);
    }
    max_in_stmts(&func.body, &mut max_id);
    max_id
}

/// Returns true if `stmt` references `target_id` anywhere in its
/// expressions (including nested).
pub fn stmt_references_local(stmt: &Stmt, target_id: LocalId) -> bool {
    let mut found = false;
    let mut walker = StmtRefWalker {
        target: target_id,
        found: &mut found,
    };
    walker.visit_stmt(stmt);
    found
}

struct StmtRefWalker<'a> {
    target: LocalId,
    found: &'a mut bool,
}

impl<'a> StmtRefWalker<'a> {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        if *self.found {
            return;
        }
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
            _ => {}
        }
    }

    fn visit_expr(&mut self, e: &Expr) {
        if *self.found {
            return;
        }
        match e {
            Expr::LocalGet(id) | Expr::LocalSet(id, _) | Expr::Update { id, .. }
                if *id == self.target =>
            {
                *self.found = true;
                return;
            }
            _ => {}
        }
        walk_expr_children(e, &mut |child| self.visit_expr(child));
    }
}
