//! Fold a plain copy of a local in any statement list: loop bodies, branches,
//! closure bodies, nested blocks of module init.
//!
//! `local_copies` handles straight-line function bodies only. The inliner
//! binds each argument to a fresh local, so an inlined call inside a loop
//! leaves `let q = p` behind, and every later pass sees two names for one
//! value: an object that is only read through `q` looks like it was handed
//! away, and stays on the heap.
//!
//! The rule needs no flow analysis because it is all-or-nothing per copy:
//!
//! * `p` is declared by a `Let` at an earlier index of the same list or of an
//!   enclosing list of the same function body, so it is initialized whenever
//!   the copy runs (a `switch` case body starts from the enclosing lists only:
//!   a jump can skip an earlier case's declarations);
//! * neither `p` nor `q` is written anywhere in the module;
//! * both are declared with the same type, so no read changes its lowering,
//!   and that type is a primitive or a class this module declares. A local
//!   of any other type may hold a `PerryPod` record in its native layout,
//!   where the binding is the boundary between two representations (see
//!   `inline_arg_needs_value_boundary`); a name that is also a type alias
//!   or an interface is not taken as the class;
//! * every occurrence of `q` in the whole module is a read in a statement
//!   that follows the copy in the copy's own list. The module-wide census
//!   counts captures, box preallocations and id-carrying intrinsics too, so
//!   a capture, a forward read, a duplicated declaration or any other use
//!   makes the counts differ and the copy stays.
//!
//! Then `q` equals `p` at each of its reads, the reads are renamed and the
//! declaration is removed. Top-level module-init declarations are registered
//! globals and are left alone; lists nested inside them are folded.

use std::collections::{HashMap, HashSet};

use perry_hir::types::{LocalId, Type};
use perry_hir::walker::{walk_expr_children, walk_expr_children_mut};
use perry_hir::{Expr, Function, Module, Stmt};

use crate::local_occurrences::{count_module, count_module_occurrences};

struct Census {
    all: HashMap<LocalId, usize>,
    writes: HashMap<LocalId, usize>,
    /// Names that can only mean a class declared by this module.
    classes: HashSet<String>,
}

impl Census {
    fn unwritten(&self, id: LocalId) -> bool {
        !self.writes.contains_key(&id)
    }

    /// A binding of this type holds the value itself in both locals.
    fn binds_by_reference(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(name) => self.classes.contains(name),
            _ => ty.is_primitive(),
        }
    }
}

/// Unwritten locals declared earlier in the enclosing lists, innermost last.
type Avail = Vec<(LocalId, Type)>;

pub fn run(module: &mut Module) {
    let mut classes: HashSet<String> = module.classes.iter().map(|c| c.name.clone()).collect();
    for other in module
        .type_aliases
        .iter()
        .map(|a| &a.name)
        .chain(module.interfaces.iter().map(|i| &i.name))
    {
        classes.remove(other);
    }
    let cx = Census {
        all: count_module_occurrences(module),
        writes: count_module(module, true),
        classes,
    };
    for f in &mut module.functions {
        fold_function(f, &cx);
    }
    for class in &mut module.classes {
        for f in class
            .constructor
            .iter_mut()
            .chain(&mut class.methods)
            .chain(&mut class.static_methods)
            .chain(class.getters.iter_mut().map(|(_, f)| f))
            .chain(class.setters.iter_mut().map(|(_, f)| f))
            .chain(class.computed_members.iter_mut().map(|m| &mut m.function))
        {
            fold_function(f, &cx);
        }
    }
    let mut avail = Avail::new();
    for stmt in &mut module.init {
        fold_nested(stmt, &mut avail, &cx);
    }
}

fn fold_function(f: &mut Function, cx: &Census) {
    fold_list(&mut f.body, &mut Avail::new(), cx);
}

fn fold_list(list: &mut Vec<Stmt>, avail: &mut Avail, cx: &Census) {
    let base = avail.len();
    let mut i = 0;
    while i < list.len() {
        if let Some((q, p)) = copy_at(&list[i], avail, cx) {
            let reads: usize = list[i + 1..].iter().map(|s| reads_in_stmt(s, q)).sum();
            if cx.all.get(&q).copied().unwrap_or(0) == reads + 1 {
                for s in &mut list[i + 1..] {
                    rename_in_stmt(s, q, p);
                }
                list.remove(i);
                continue;
            }
        }
        fold_nested(&mut list[i], avail, cx);
        if let Stmt::Let { id, ty, .. } = &list[i] {
            if cx.unwritten(*id) {
                avail.push((*id, ty.clone()));
            }
        }
        i += 1;
    }
    avail.truncate(base);
}

/// `let q = p` that the rule may fold, as `(q, p)`.
fn copy_at(stmt: &Stmt, avail: &Avail, cx: &Census) -> Option<(LocalId, LocalId)> {
    let Stmt::Let {
        id: q,
        ty,
        init: Some(Expr::LocalGet(p)),
        ..
    } = stmt
    else {
        return None;
    };
    let same_type = avail
        .iter()
        .rev()
        .find(|(id, _)| id == p)
        .is_some_and(|(_, p_ty)| p_ty == ty);
    (q != p && same_type && cx.binds_by_reference(ty) && cx.unwritten(*q)).then_some((*q, *p))
}

/// Folds the lists and closure bodies inside one statement.
fn fold_nested(stmt: &mut Stmt, avail: &mut Avail, cx: &Census) {
    match stmt {
        Stmt::Let { init, .. } | Stmt::Return(init) => {
            if let Some(e) = init {
                fold_expr(e, cx);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => fold_expr(e, cx),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            fold_expr(condition, cx);
            fold_list(then_branch, avail, cx);
            if let Some(b) = else_branch {
                fold_list(b, avail, cx);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            fold_expr(condition, cx);
            fold_list(body, avail, cx);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                fold_nested(i, avail, cx);
            }
            if let Some(e) = condition {
                fold_expr(e, cx);
            }
            if let Some(e) = update {
                fold_expr(e, cx);
            }
            fold_list(body, avail, cx);
        }
        Stmt::Labeled { body, .. } => fold_nested(body, avail, cx),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            fold_list(body, avail, cx);
            if let Some(catch) = catch {
                fold_list(&mut catch.body, avail, cx);
            }
            if let Some(f) = finally {
                fold_list(f, avail, cx);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            fold_expr(discriminant, cx);
            for case in cases {
                if let Some(t) = &mut case.test {
                    fold_expr(t, cx);
                }
                fold_list(&mut case.body, avail, cx);
            }
        }
        Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => {}
    }
}

/// A closure body is its own function body: nothing outside it is known to
/// be initialized when it runs.
fn fold_expr(expr: &mut Expr, cx: &Census) {
    if let Expr::Closure { body, .. } = expr {
        fold_list(body, &mut Avail::new(), cx);
    }
    walk_expr_children_mut(expr, &mut |e| fold_expr(e, cx));
}

fn reads_in_expr(expr: &Expr, q: LocalId) -> usize {
    let mut n = usize::from(matches!(expr, Expr::LocalGet(id) if *id == q));
    walk_expr_children(expr, &mut |e| n += reads_in_expr(e, q));
    n
}

fn reads_in_stmts(stmts: &[Stmt], q: LocalId) -> usize {
    stmts.iter().map(|s| reads_in_stmt(s, q)).sum()
}

fn reads_in_stmt(stmt: &Stmt, q: LocalId) -> usize {
    let opt = |e: &Option<Expr>| e.as_ref().map_or(0, |e| reads_in_expr(e, q));
    match stmt {
        Stmt::Let { init, .. } | Stmt::Return(init) => opt(init),
        Stmt::Expr(e) | Stmt::Throw(e) => reads_in_expr(e, q),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            reads_in_expr(condition, q)
                + reads_in_stmts(then_branch, q)
                + else_branch.as_ref().map_or(0, |b| reads_in_stmts(b, q))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            reads_in_expr(condition, q) + reads_in_stmts(body, q)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref().map_or(0, |s| reads_in_stmt(s, q))
                + opt(condition)
                + opt(update)
                + reads_in_stmts(body, q)
        }
        Stmt::Labeled { body, .. } => reads_in_stmt(body, q),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            reads_in_stmts(body, q)
                + catch.as_ref().map_or(0, |c| reads_in_stmts(&c.body, q))
                + finally.as_ref().map_or(0, |f| reads_in_stmts(f, q))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            reads_in_expr(discriminant, q)
                + cases
                    .iter()
                    .map(|c| opt(&c.test) + reads_in_stmts(&c.body, q))
                    .sum::<usize>()
        }
        Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => 0,
    }
}

fn rename_in_expr(expr: &mut Expr, q: LocalId, p: LocalId) {
    if let Expr::LocalGet(id) = expr {
        if *id == q {
            *id = p;
        }
    }
    walk_expr_children_mut(expr, &mut |e| rename_in_expr(e, q, p));
}

fn rename_in_stmts(stmts: &mut [Stmt], q: LocalId, p: LocalId) {
    for s in stmts {
        rename_in_stmt(s, q, p);
    }
}

fn rename_in_stmt(stmt: &mut Stmt, q: LocalId, p: LocalId) {
    match stmt {
        Stmt::Let { init, .. } | Stmt::Return(init) => {
            if let Some(e) = init {
                rename_in_expr(e, q, p);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => rename_in_expr(e, q, p),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rename_in_expr(condition, q, p);
            rename_in_stmts(then_branch, q, p);
            if let Some(b) = else_branch {
                rename_in_stmts(b, q, p);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            rename_in_expr(condition, q, p);
            rename_in_stmts(body, q, p);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                rename_in_stmt(s, q, p);
            }
            if let Some(e) = condition {
                rename_in_expr(e, q, p);
            }
            if let Some(e) = update {
                rename_in_expr(e, q, p);
            }
            rename_in_stmts(body, q, p);
        }
        Stmt::Labeled { body, .. } => rename_in_stmt(body, q, p),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rename_in_stmts(body, q, p);
            if let Some(c) = catch {
                rename_in_stmts(&mut c.body, q, p);
            }
            if let Some(f) = finally {
                rename_in_stmts(f, q, p);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            rename_in_expr(discriminant, q, p);
            for c in cases {
                if let Some(t) = &mut c.test {
                    rename_in_expr(t, q, p);
                }
                rename_in_stmts(&mut c.body, q, p);
            }
        }
        Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => {}
    }
}

#[cfg(test)]
#[path = "scoped_copies_tests.rs"]
mod tests;
