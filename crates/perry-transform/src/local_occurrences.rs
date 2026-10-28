//! Complete LocalId census shared by nonescaping-object projections.
use perry_hir::types::LocalId;
use perry_hir::walker::walk_expr_children;
use perry_hir::{Decorator, Expr, Function, Module, Param, Stmt};
use std::collections::HashMap;
type Counts = HashMap<LocalId, usize>;

pub(crate) fn bump(counts: &mut Counts, id: LocalId) {
    *counts.entry(id).or_insert(0) += 1;
}

/// Count every place a LocalId appears, in every container of the module.
/// Over-counting only makes the pass bail; the container list mirrors
/// `generator::compute_max_local_id` so nothing is under-counted.
pub(crate) fn count_module_occurrences(module: &Module) -> Counts {
    count_module(module, false)
}

/// `writes_only`: count just `LocalSet` / `Update` targets.
pub(crate) fn count_module(module: &Module, writes_only: bool) -> Counts {
    let mut counter = Counter {
        counts: Counts::new(),
        writes_only,
    };
    let c = &mut counter;
    for f in &module.functions {
        count_function(f, c);
    }
    count_stmts(&module.init, c);
    for g in &module.globals {
        c.any(g.id);
        if let Some(init) = &g.init {
            count_expr(init, c);
        }
    }
    for class in &module.classes {
        for f in class
            .constructor
            .iter()
            .chain(&class.methods)
            .chain(&class.static_methods)
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
        {
            count_function(f, c);
        }
        for m in &class.computed_members {
            count_expr(&m.key_expr, c);
            count_function(&m.function, c);
        }
        for field in class.fields.iter().chain(&class.static_fields) {
            if let Some(init) = &field.init {
                count_expr(init, c);
            }
            if let Some(key) = &field.key_expr {
                count_expr(key, c);
            }
            count_decorators(&field.decorators, c);
        }
        count_decorators(&class.decorators, c);
        if let Some(e) = &class.extends_expr {
            count_expr(e, c);
        }
    }
    counter.counts
}

struct Counter {
    counts: Counts,
    writes_only: bool,
}

impl Counter {
    /// Any occurrence that is not a write.
    fn any(&mut self, id: LocalId) {
        if !self.writes_only {
            bump(&mut self.counts, id);
        }
    }

    fn write(&mut self, id: LocalId) {
        bump(&mut self.counts, id);
    }
}

fn count_decorators(decorators: &[Decorator], c: &mut Counter) {
    for d in decorators {
        for a in &d.args {
            count_expr(a, c);
        }
    }
}

fn count_params(params: &[Param], c: &mut Counter) {
    for p in params {
        c.any(p.id);
        if let Some(d) = &p.default {
            count_expr(d, c);
        }
        count_decorators(&p.decorators, c);
        if let Some(meta) = &p.arguments_object {
            for (_, local) in &meta.mapped_parameter_ids {
                c.any(*local);
            }
        }
    }
}

fn count_function(f: &Function, c: &mut Counter) {
    count_params(&f.params, c);
    count_decorators(&f.decorators, c);
    for id in &f.captures {
        c.any(*id);
    }
    count_stmts(&f.body, c);
}

fn count_stmts(stmts: &[Stmt], c: &mut Counter) {
    for s in stmts {
        count_stmt(s, c);
    }
}

fn count_stmt(stmt: &Stmt, c: &mut Counter) {
    match stmt {
        Stmt::Let { id, init, .. } => {
            c.any(*id);
            if let Some(e) = init {
                count_expr(e, c);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => count_expr(e, c),
        Stmt::Return(e) => {
            if let Some(e) = e {
                count_expr(e, c);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            count_expr(condition, c);
            count_stmts(then_branch, c);
            if let Some(b) = else_branch {
                count_stmts(b, c);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            count_expr(condition, c);
            count_stmts(body, c);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                count_stmt(i, c);
            }
            if let Some(e) = condition {
                count_expr(e, c);
            }
            if let Some(e) = update {
                count_expr(e, c);
            }
            count_stmts(body, c);
        }
        Stmt::Labeled { body, .. } => count_stmt(body, c),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            count_stmts(body, c);
            if let Some(catch) = catch {
                if let Some((id, _)) = catch.param {
                    c.any(id);
                }
                count_stmts(&catch.body, c);
            }
            if let Some(f) = finally {
                count_stmts(f, c);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            count_expr(discriminant, c);
            for case in cases {
                if let Some(t) = &case.test {
                    count_expr(t, c);
                }
                count_stmts(&case.body, c);
            }
        }
        Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) | Stmt::ReleaseBoxes(ids) => {
            for id in ids {
                c.any(*id);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
    }
}

fn count_expr(expr: &Expr, c: &mut Counter) {
    match expr {
        Expr::LocalGet(id) | Expr::ScopedTemp { id, .. } => c.any(*id),
        Expr::WithSet { fallback, .. } => {
            if let perry_hir::WithSetFallback::Local(id)
            | perry_hir::WithSetFallback::SloppyImplicit(id) = fallback
            {
                c.write(*id);
            }
        }
        Expr::ClassExprFresh {
            evaluation_owner: Some(id),
            ..
        } => c.any(*id),
        Expr::LinkGeneratorPrototype {
            i32_controls,
            i1_controls,
            ..
        } => {
            for id in i32_controls.iter().chain(i1_controls) {
                c.any(*id);
            }
        }
        Expr::LocalSet(id, _) | Expr::Update { id, .. } => c.write(*id),
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => c.any(*array_id),
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => c.any(*id),
        Expr::SetAdd { set_id, .. } => c.any(*set_id),
        Expr::Closure {
            params,
            body,
            captures,
            mutable_captures,
            ..
        } => {
            // Param defaults are reached by the walker below.
            for p in params {
                c.any(p.id);
                count_decorators(&p.decorators, c);
                if let Some(meta) = &p.arguments_object {
                    for (_, local) in &meta.mapped_parameter_ids {
                        c.any(*local);
                    }
                }
            }
            for id in captures.iter().chain(mutable_captures) {
                c.any(*id);
            }
            count_stmts(body, c);
        }
        _ => {}
    }
    walk_expr_children(expr, &mut |child| count_expr(child, c));
}
