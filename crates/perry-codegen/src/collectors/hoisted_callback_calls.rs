//! Immutable method callbacks whose dispatch can be resolved once at entry.
//!
//! A TypeScript function annotation only nominates a possible callback. The
//! runtime still validates the actual value and admits only plain arrow
//! closures whose arity needs no padding or rest bundling. This collector's
//! job is narrower: find direct call arities of immutable callback parameters
//! and their immutable local aliases, without entering nested closure bodies.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use perry_hir::types::Type;
use perry_hir::{Expr, Function, Stmt};

const MAX_CALLBACK_ARITY: usize = 16;
const MAX_ARITIES_PER_PARAMETER: usize = 4;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct HoistedCallbackCall {
    pub source_param: u32,
    pub callee_local: u32,
    pub arity: usize,
}

fn walk_expr(expr: &Expr, visit: &mut impl FnMut(&Expr)) {
    visit(expr);
    // A nested closure has a different frame and cannot reuse an SSA target
    // resolved in this method's entry block. Stop before calling the shared
    // child walker: while it omits the closure body, it intentionally visits
    // parameter defaults, and those also execute in the nested frame.
    if matches!(expr, Expr::Closure { .. }) {
        return;
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| walk_expr(child, visit));
}

fn walk_stmt(stmt: &Stmt, visit: &mut impl FnMut(&Expr)) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(init) = init {
                walk_expr(init, visit);
            }
        }
        Stmt::Expr(expr) | Stmt::Throw(expr) => walk_expr(expr, visit),
        Stmt::Return(expr) => {
            if let Some(expr) = expr {
                walk_expr(expr, visit);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            walk_expr(condition, visit);
            walk_stmts(then_branch, visit);
            if let Some(else_branch) = else_branch {
                walk_stmts(else_branch, visit);
            }
        }
        Stmt::While { condition, body } => {
            walk_expr(condition, visit);
            walk_stmts(body, visit);
        }
        Stmt::DoWhile { body, condition } => {
            walk_stmts(body, visit);
            walk_expr(condition, visit);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                walk_stmt(init, visit);
            }
            if let Some(condition) = condition {
                walk_expr(condition, visit);
            }
            if let Some(update) = update {
                walk_expr(update, visit);
            }
            walk_stmts(body, visit);
        }
        Stmt::Labeled { body, .. } => walk_stmt(body, visit),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            walk_stmts(body, visit);
            if let Some(catch) = catch {
                walk_stmts(&catch.body, visit);
            }
            if let Some(finally) = finally {
                walk_stmts(finally, visit);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            walk_expr(discriminant, visit);
            for case in cases {
                if let Some(test) = &case.test {
                    walk_expr(test, visit);
                }
                walk_stmts(&case.body, visit);
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

fn walk_stmts(stmts: &[Stmt], visit: &mut impl FnMut(&Expr)) {
    for stmt in stmts {
        walk_stmt(stmt, visit);
    }
}

fn collect_immutable_alias_edges(stmts: &[Stmt], edges: &mut Vec<(u32, u32)>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                id,
                mutable: false,
                init: Some(Expr::LocalGet(source)),
                ..
            } => edges.push((*id, *source)),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_immutable_alias_edges(then_branch, edges);
                if let Some(else_branch) = else_branch {
                    collect_immutable_alias_edges(else_branch, edges);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_immutable_alias_edges(body, edges)
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    collect_immutable_alias_edges(std::slice::from_ref(init.as_ref()), edges);
                }
                collect_immutable_alias_edges(body, edges);
            }
            Stmt::Labeled { body, .. } => {
                collect_immutable_alias_edges(std::slice::from_ref(body.as_ref()), edges)
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_immutable_alias_edges(body, edges);
                if let Some(catch) = catch {
                    collect_immutable_alias_edges(&catch.body, edges);
                }
                if let Some(finally) = finally {
                    collect_immutable_alias_edges(finally, edges);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_immutable_alias_edges(&case.body, edges);
                }
            }
            _ => {}
        }
    }
}

fn immutable_aliases(root: u32, edges: &[(u32, u32)]) -> HashSet<u32> {
    let mut aliases = HashSet::from([root]);
    loop {
        let before = aliases.len();
        for &(alias, source) in edges {
            if aliases.contains(&source) {
                aliases.insert(alias);
            }
        }
        if aliases.len() == before {
            return aliases;
        }
    }
}

/// Find callback parameter/alias call sites that may reuse one entry-resolved
/// arrow target. Runtime validation remains mandatory; annotations never prove
/// the incoming value's class, arity, rest shape, or arrow semantics.
pub(crate) fn collect_hoisted_callback_calls(method: &Function) -> Vec<HoistedCallbackCall> {
    if method.is_async
        || method.is_generator
        || method.was_plain_async
        || method
            .params
            .iter()
            .any(|param| param.arguments_object.is_some())
    {
        return Vec::new();
    }

    let reassigned = super::reassigned_locals(&method.body);
    let mut alias_edges = Vec::new();
    collect_immutable_alias_edges(&method.body, &mut alias_edges);
    let mut result = BTreeSet::new();

    for param in &method.params {
        if param.default.is_some()
            || param.is_rest
            || reassigned.contains(&param.id)
            || !matches!(&param.ty, Type::Function(function) if !function.is_async && !function.is_generator)
        {
            continue;
        }

        let aliases = immutable_aliases(param.id, &alias_edges);
        let mut calls_by_arity: BTreeMap<usize, BTreeSet<u32>> = BTreeMap::new();
        walk_stmts(&method.body, &mut |expr| {
            let Expr::Call { callee, args, .. } = expr else {
                return;
            };
            let Expr::LocalGet(callee_local) = callee.as_ref() else {
                return;
            };
            if args.len() <= MAX_CALLBACK_ARITY && aliases.contains(callee_local) {
                calls_by_arity
                    .entry(args.len())
                    .or_default()
                    .insert(*callee_local);
            }
        });
        if calls_by_arity.len() > MAX_ARITIES_PER_PARAMETER {
            continue;
        }
        for (arity, callees) in calls_by_arity {
            for callee_local in callees {
                result.insert(HoistedCallbackCall {
                    source_param: param.id,
                    callee_local,
                    arity,
                });
            }
        }
    }

    result.into_iter().collect()
}
