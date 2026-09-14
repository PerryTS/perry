//! Remove copy bookkeeping before codegen turns it into observable runtime
//! calls (string sharing and incremental root shading).
//!
//! This first pass handles straight-line, synchronous function bodies. An
//! alias must copy an initialized, unwritten local, and itself have no writes.
//! Closure/class creation, control flow, TDZ preallocation, parameter defaults
//! and arguments objects reject the body. Unrecognized expression forms,
//! including with and LocalId-bearing intrinsics, also reject it. Constant
//! eval already lowered to ordinary local reads/writes follows the same rules.

use std::collections::{HashMap, HashSet};

use perry_hir::types::LocalId;
use perry_hir::walker::{walk_expr_children, walk_expr_children_mut};
use perry_hir::{Expr, Function, Module, Stmt};

pub fn run(module: &mut Module) {
    // Retained function bodies can refer to a sibling's locals. Never change
    // the storage named in their capture metadata, even for hand-built HIR.
    let mut captured = HashSet::new();
    for f in &module.functions {
        captured.extend(f.captures.iter().copied());
    }
    for class in &module.classes {
        for f in class
            .constructor
            .iter()
            .chain(&class.methods)
            .chain(&class.static_methods)
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
            .chain(class.computed_members.iter().map(|m| &m.function))
        {
            captured.extend(f.captures.iter().copied());
        }
    }
    // Module-init bindings are registered globals, not function-local copies.
    for f in &mut module.functions {
        loop {
            let before = f.body.len();
            clean_function(f, &captured);
            if f.body.len() == before {
                break;
            }
        }
    }
}

#[derive(Default)]
struct Uses {
    reads: HashSet<LocalId>,
    writes: HashSet<LocalId>,
}

fn inspect_expr(expr: &Expr, uses: &mut Uses) -> bool {
    match expr {
        Expr::LocalGet(id) => {
            uses.reads.insert(*id);
        }
        Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
            uses.writes.insert(*id);
            if matches!(expr, Expr::Update { .. }) {
                uses.reads.insert(*id);
            }
        }
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::WtfString(_)
        | Expr::BigInt(_)
        | Expr::GlobalGet(_)
        | Expr::GlobalSet(_, _)
        | Expr::FuncRef(_)
        | Expr::ExternFuncRef { .. }
        | Expr::This
        | Expr::Binary { .. }
        | Expr::Unary { .. }
        | Expr::Compare { .. }
        | Expr::Logical { .. }
        | Expr::Conditional { .. }
        | Expr::Sequence(_)
        | Expr::TypeOf(_)
        | Expr::Void(_)
        | Expr::Call { .. }
        | Expr::PropertyGet { .. }
        | Expr::PropertySet { .. }
        | Expr::IndexGet { .. }
        | Expr::IndexSet { .. }
        | Expr::Array(_) => {}
        _ => return false,
    }
    let mut supported = true;
    walk_expr_children(expr, &mut |e| supported &= inspect_expr(e, uses));
    supported
}

fn clean_function(f: &mut Function, captured: &HashSet<LocalId>) {
    if f.is_async
        || f.is_generator
        || !f.captures.is_empty()
        || f.params.iter().any(|p| {
            p.default.is_some() || p.arguments_object.is_some() || !p.decorators.is_empty()
        })
    {
        return;
    }
    let mut uses = Uses::default();
    let mut declared: HashSet<_> = f.params.iter().map(|p| p.id).collect();
    let mut mutable = HashSet::new();
    for stmt in &f.body {
        let expr = match stmt {
            Stmt::Let {
                id,
                mutable: m,
                init,
                ..
            } => {
                if !declared.insert(*id) {
                    return;
                }
                if *m {
                    mutable.insert(*id);
                }
                init.as_ref()
            }
            Stmt::Expr(e) | Stmt::Throw(e) => Some(e),
            Stmt::Return(e) => e.as_ref(),
            _ => return,
        };
        if expr.is_some_and(|e| !inspect_expr(e, &mut uses)) {
            return;
        }
    }
    if !declared.is_disjoint(captured) {
        return;
    }
    // Do not rely on the lowering pass having already inserted explicit TDZ
    // boxes. Reject forward reads/writes before changing any declaration.
    let mut available: HashSet<_> = f.params.iter().map(|p| p.id).collect();
    for stmt in &f.body {
        let expr = match stmt {
            Stmt::Let { init, .. } | Stmt::Return(init) => init.as_ref(),
            Stmt::Expr(e) | Stmt::Throw(e) => Some(e),
            _ => unreachable!(),
        };
        let mut at_stmt = Uses::default();
        if let Some(expr) = expr {
            inspect_expr(expr, &mut at_stmt);
        }
        if at_stmt
            .reads
            .union(&at_stmt.writes)
            .any(|id| declared.contains(id) && !available.contains(id))
        {
            return;
        }
        if let Stmt::Let { id, .. } = stmt {
            available.insert(*id);
        }
    }
    let mut initialized: HashSet<_> = f.params.iter().map(|p| p.id).collect();
    let mut aliases = HashMap::new();
    f.body.retain_mut(|stmt| {
        // Rewrite only reads, in source order: earlier reads must retain TDZ
        // behavior, and assignment targets must keep their original binding.
        let expr = match stmt {
            Stmt::Let { init, .. } | Stmt::Return(init) => init.as_mut(),
            Stmt::Expr(e) | Stmt::Throw(e) => Some(e),
            _ => unreachable!(),
        };
        if let Some(expr) = expr {
            rewrite_reads(expr, &aliases);
        }
        match stmt {
            Stmt::Let { id, init, .. } => {
                let alias = match init {
                    Some(Expr::LocalGet(src))
                        if initialized.contains(src)
                            && !uses.writes.contains(src)
                            && !uses.writes.contains(id) =>
                    {
                        Some(*src)
                    }
                    _ => None,
                };
                let dead = !uses.reads.contains(id)
                    && !uses.writes.contains(id)
                    && init.as_ref().is_none_or(|e| inert(e, &initialized));
                initialized.insert(*id);
                if let Some(src) = alias {
                    aliases.insert(*id, src);
                    false
                } else {
                    !dead
                }
            }
            Stmt::Expr(Expr::LocalSet(id, value)) => {
                !(mutable.contains(id)
                    && initialized.contains(id)
                    && !uses.reads.contains(id)
                    && inert(value, &initialized))
            }
            _ => true,
        }
    });
}

fn inert(expr: &Expr, initialized: &HashSet<LocalId>) -> bool {
    match expr {
        Expr::LocalGet(id) => initialized.contains(id),
        Expr::Undefined | Expr::Null | Expr::Bool(_) | Expr::Number(_) | Expr::Integer(_) => true,
        _ => false,
    }
}

fn rewrite_reads(expr: &mut Expr, aliases: &HashMap<LocalId, LocalId>) {
    if let Expr::LocalGet(id) = expr {
        if let Some(src) = aliases.get(id) {
            *id = *src;
        }
    }
    walk_expr_children_mut(expr, &mut |e| rewrite_reads(e, aliases));
}

#[cfg(test)]
#[path = "local_copies_tests.rs"]
mod tests;
