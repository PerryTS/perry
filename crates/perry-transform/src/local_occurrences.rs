//! Complete LocalId census shared by nonescaping-object projections.
use perry_hir::types::LocalId;
use perry_hir::walker::walk_expr_children;
use perry_hir::{Decorator, Expr, Function, Module, Param, Stmt};
use std::collections::{HashMap, HashSet};
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

/// Which locals anything in the module writes, taken once before a pass
/// that substitutes call arguments for parameters.
pub(crate) struct WriteCensus {
    writes: Counts,
    /// Locals that only ever hold a primitive: declared by a `let` whose
    /// initializer is primitive whatever its operands hold
    /// ([`primitive_valued`]), and never stored anything else.
    primitive_only: HashSet<LocalId>,
    /// Ids from here on were allocated after the census; it knows nothing
    /// about their writes.
    limit: LocalId,
}

impl WriteCensus {
    pub(crate) fn of(module: &Module) -> Self {
        let counter = count_module_with(module, true);
        let mut primitive_only = counter.primitive_lets;
        primitive_only.retain(|id| !counter.other_stores.contains(id));
        Self {
            writes: counter.counts,
            primitive_only,
            limit: crate::generator::compute_max_local_id(module).saturating_add(1),
        }
    }

    /// A census of a module that writes no local (unit tests).
    #[cfg(test)]
    pub(crate) fn unwritten() -> Self {
        Self {
            writes: Counts::new(),
            primitive_only: HashSet::new(),
            limit: LocalId::MAX,
        }
    }

    /// True unless the census shows `id` is never written.
    pub(crate) fn may_be_written(&self, id: LocalId) -> bool {
        id >= self.limit || self.writes.contains_key(&id)
    }

    /// An argument that may be substituted for its parameter: nothing the
    /// callee's body or a later argument does can change what it reads. A
    /// literal, or a read of a local that nothing in the module writes. A
    /// read of any other local, or of a global, is evaluated where the call
    /// is: substituting it would move the read past the body's effects.
    pub(crate) fn is_stable_arg(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Integer(_)
            | Expr::Number(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::WtfString(_)
            | Expr::Null
            | Expr::Undefined => true,
            Expr::LocalGet(x) => !self.may_be_written(*x),
            _ => false,
        }
    }

    /// The value of `arg` is a primitive whatever the program does: its
    /// conversions (`valueOf`, `toString`) run no code.
    fn primitive_arg(&self, arg: &Expr) -> bool {
        match arg {
            Expr::LocalGet(x) => *x < self.limit && self.primitive_only.contains(x),
            other => primitive_valued(other),
        }
    }

    /// The one rule for substituting the argument at `index` for its
    /// parameter in an inlined `body`. First [`Self::is_stable_arg`]. Second,
    /// a read of any local, written or not, when the read cannot move past
    /// anything that could change it (`arg_read_order`): no later argument
    /// can run code or store to the local, and every read of the parameter
    /// in the body comes before anything in the body that could. An operator
    /// over parameters whose arguments are primitive runs no code.
    pub(crate) fn substitutes_arg(
        &self,
        params: &[Param],
        args: &[Expr],
        index: usize,
        body: &[Stmt],
    ) -> bool {
        use crate::arg_read_order::{enabled, may_run_code, reads_precede_effects};
        let arg = &args[index];
        if self.is_stable_arg(arg) {
            return true;
        }
        let Expr::LocalGet(local) = arg else {
            return false;
        };
        if !enabled() || args[index + 1..].iter().any(|a| may_run_code(a, *local)) {
            return false;
        }
        // A parameter with no argument is `undefined`.
        let primitive = params
            .iter()
            .enumerate()
            .filter(|(i, _)| args.get(*i).is_none_or(|a| self.primitive_arg(a)))
            .map(|(_, p)| p.id)
            .collect();
        reads_precede_effects(body, params[index].id, *local, primitive)
    }
}

/// An expression whose value is a primitive whatever its operands hold: a
/// literal, or an operator that always yields one. A read of a local is not
/// (an annotation is not a proof), so substituting arguments for reads
/// never changes the answer.
pub(crate) fn primitive_valued(expr: &Expr) -> bool {
    match expr {
        Expr::Integer(_)
        | Expr::Number(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::WtfString(_)
        | Expr::BigInt(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::Binary { .. }
        | Expr::Compare { .. }
        | Expr::Unary { .. }
        | Expr::Update { .. } => true,
        Expr::Logical { left, right, .. } => primitive_valued(left) && primitive_valued(right),
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => primitive_valued(then_expr) && primitive_valued(else_expr),
        _ => false,
    }
}

/// Every place a LocalId appears inside `stmts` and everything nested in
/// them, by the same rules as [`count_module_occurrences`].
pub(crate) fn count_list_occurrences(stmts: &[Stmt]) -> Counts {
    let mut counter = Counter::new(false);
    count_stmts(stmts, &mut counter);
    counter.counts
}

/// `writes_only`: count just `LocalSet` / `Update` targets.
pub(crate) fn count_module(module: &Module, writes_only: bool) -> Counts {
    count_module_with(module, writes_only).counts
}

fn count_module_with(module: &Module, writes_only: bool) -> Counter {
    let mut counter = Counter::new(writes_only);
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
    counter
}

struct Counter {
    counts: Counts,
    writes_only: bool,
    /// `let` locals whose initializer is [`primitive_valued`].
    primitive_lets: HashSet<LocalId>,
    /// Locals stored a value that is not, or by something that is no plain
    /// store.
    other_stores: HashSet<LocalId>,
}

impl Counter {
    fn new(writes_only: bool) -> Self {
        Counter {
            counts: Counts::new(),
            writes_only,
            primitive_lets: HashSet::new(),
            other_stores: HashSet::new(),
        }
    }

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
            // A second `let` of the same id (a rebuilt frame) with any other
            // initializer, or none, takes the local out.
            if init.as_ref().is_some_and(primitive_valued) {
                c.primitive_lets.insert(*id);
            } else {
                c.other_stores.insert(*id);
            }
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
                c.other_stores.insert(*id);
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
        Expr::LocalSet(id, value) => {
            c.write(*id);
            if !primitive_valued(value) {
                c.other_stores.insert(*id);
            }
        }
        // `++` / `--` store a Number or a BigInt.
        Expr::Update { id, .. } => c.write(*id),
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
