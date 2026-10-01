//! Compiler-only lexical names for the existing async JS fallback.
use super::*;
use std::collections::BTreeSet;

pub(super) fn scoped_js_names(stmts: &[Stmt]) -> BTreeSet<String> {
    fn expr(e: &Expr, active: &mut BTreeMap<LocalId, u32>, names: &mut BTreeSet<String>) {
        if let Expr::ScopedTemp { id, value, body } = e {
            expr(value, active, names);
            let depth = active.get(id).copied().unwrap_or(0);
            names.insert(format!("__scoped_v_{id}_{depth}"));
            names.insert(format!("__scoped_r_{id}_{depth}"));
            active.insert(*id, depth + 1);
            expr(body, active, names);
            if depth == 0 {
                active.remove(id);
            } else {
                active.insert(*id, depth);
            }
        } else if !matches!(e, Expr::Closure { .. }) {
            perry_hir::walker::walk_expr_children(e, &mut |child| expr(child, active, names));
        }
    }
    fn visit(stmts: &[Stmt], names: &mut BTreeSet<String>) {
        let mut active = BTreeMap::new();
        for stmt in stmts {
            match stmt {
                Stmt::Let { init, .. } | Stmt::Return(init) => {
                    if let Some(e) = init {
                        expr(e, &mut active, names);
                    }
                }
                Stmt::Expr(e) | Stmt::Throw(e) => expr(e, &mut active, names),
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    expr(condition, &mut active, names);
                    visit(then_branch, names);
                    if let Some(body) = else_branch {
                        visit(body, names);
                    }
                }
                Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                    expr(condition, &mut active, names);
                    visit(body, names);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(init) = init {
                        visit(std::slice::from_ref(init.as_ref()), names);
                    }
                    for e in [condition, update].into_iter().flatten() {
                        expr(e, &mut active, names);
                    }
                    visit(body, names);
                }
                Stmt::Labeled { body, .. } => visit(std::slice::from_ref(body.as_ref()), names),
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    visit(body, names);
                    if let Some(catch) = catch {
                        visit(&catch.body, names);
                    }
                    if let Some(body) = finally {
                        visit(body, names);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    expr(discriminant, &mut active, names);
                    for case in cases {
                        if let Some(e) = &case.test {
                            expr(e, &mut active, names);
                        }
                        visit(&case.body, names);
                    }
                }
                _ => {}
            }
        }
    }
    let mut names = BTreeSet::new();
    visit(stmts, &mut names);
    names
}
