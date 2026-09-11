//! Prove that a local suffix is observed only through length/character reads.
//! Remove admitted uses in a temporary HIR copy, then use the exhaustive HIR
//! reference collector to reject every remaining read, write, and capture.

use perry_hir::{Expr, Stmt};
use std::collections::{HashMap, HashSet};

#[cfg(test)]
mod tests;

pub(crate) fn literal_index(expr: &Expr) -> Option<i32> {
    match expr {
        Expr::Integer(n) => i32::try_from(*n).ok(),
        Expr::Number(n)
            if n.is_finite()
                && n.fract() == 0.0
                && *n >= i32::MIN as f64
                && *n <= i32::MAX as f64 =>
        {
            Some(*n as i32)
        }
        _ => None,
    }
}

pub(crate) fn method(expr: &Expr, name: &str) -> Option<(u32, i32)> {
    let Expr::Call { callee, args, .. } = expr else {
        return None;
    };
    let Expr::PropertyGet {
        object, property, ..
    } = callee.as_ref()
    else {
        return None;
    };
    let Expr::LocalGet(id) = object.as_ref() else {
        return None;
    };
    if property != name {
        return None;
    }
    let index = match args.as_slice() {
        [] => 0,
        [arg] => literal_index(arg)?,
        _ => return None,
    };
    Some((*id, index))
}

pub(crate) fn collect(
    stmts: &[Stmt],
    boxed: &HashSet<u32>,
    globals: &HashMap<u32, String>,
) -> HashSet<u32> {
    // Top-level declarations dominate the admitted loops. Nested declarations
    // remain materialized; in particular, no cursor crosses a closure region.
    let mut candidates: HashSet<u32> = stmts
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Let {
                id,
                init: Some(_),
                mutable: true,
                ..
            } if !boxed.contains(id) && !globals.contains_key(id) => Some(*id),
            _ => None,
        })
        .collect();
    if candidates.is_empty() || !stmts.iter().any(|s| has_update(s, &candidates)) {
        return HashSet::new();
    }
    let mut masked = stmts.to_vec();
    let mut updates = HashSet::new();
    for stmt in &mut masked {
        mask_stmt(stmt, &candidates, &mut updates);
    }
    let mut refs = Vec::new();
    let mut visited = HashSet::new();
    for stmt in &masked {
        perry_hir::analysis::collect_local_refs_stmt(stmt, &mut refs, &mut visited);
    }
    for id in refs {
        candidates.remove(&id);
    }
    candidates.retain(|id| updates.contains(id));
    candidates
}

fn has_update(stmt: &Stmt, candidates: &HashSet<u32>) -> bool {
    let update = |expr: &Expr| match expr {
        Expr::LocalSet(id, value) => {
            candidates.contains(id)
                && method(value, "slice")
                    .is_some_and(|(receiver, count)| receiver == *id && count >= 0)
        }
        _ => false,
    };
    match stmt {
        Stmt::Expr(expr) => update(expr),
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            body.iter().any(|s| has_update(s, candidates))
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().any(|s| has_update(s, candidates))
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| branch.iter().any(|s| has_update(s, candidates)))
        }
        Stmt::For {
            init,
            update: step,
            body,
            ..
        } => {
            init.as_ref().is_some_and(|s| has_update(s, candidates))
                || step.as_ref().is_some_and(update)
                || body.iter().any(|s| has_update(s, candidates))
        }
        _ => false,
    }
}

fn mask_expr(expr: &mut Expr, allowed: &HashSet<u32>, updates: &mut HashSet<u32>, discarded: bool) {
    if discarded {
        if let Expr::LocalSet(id, value) = expr {
            if let Some((receiver, count)) = method(value, "slice") {
                if receiver == *id && count >= 0 && allowed.contains(id) {
                    updates.insert(*id);
                    *expr = Expr::Undefined;
                    return;
                }
            }
        }
    }
    let admitted = match expr {
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => {
            matches!(object.as_ref(), Expr::LocalGet(id) if allowed.contains(id))
        }
        _ => method(expr, "charCodeAt").is_some_and(|(id, _)| allowed.contains(&id)),
    };
    if admitted {
        *expr = Expr::Undefined;
        return;
    }
    // Captures, closure defaults, and bodies must remain visible to the
    // reference collector, even if their reads look locally fusible.
    if matches!(expr, Expr::Closure { .. }) {
        return;
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        mask_expr(child, allowed, updates, false)
    });
}

fn mask_stmt(stmt: &mut Stmt, allowed: &HashSet<u32>, updates: &mut HashSet<u32>) {
    match stmt {
        Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) | Stmt::Throw(e) => {
            mask_expr(e, allowed, updates, false)
        }
        Stmt::Expr(e) => mask_expr(e, allowed, updates, true),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            mask_expr(condition, allowed, updates, false);
            for s in then_branch {
                mask_stmt(s, allowed, updates);
            }
            if let Some(branch) = else_branch {
                for s in branch {
                    mask_stmt(s, allowed, updates);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
            mask_expr(condition, allowed, updates, false);
            for s in body {
                mask_stmt(s, allowed, updates);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                mask_stmt(s, allowed, updates);
            }
            if let Some(e) = condition {
                mask_expr(e, allowed, updates, false);
            }
            if let Some(e) = update {
                mask_expr(e, allowed, updates, true);
            }
            for s in body {
                mask_stmt(s, allowed, updates);
            }
        }
        // Unhandled control-flow shapes remain intact and conservatively
        // reject any referenced candidate through the exhaustive collector.
        _ => {}
    }
}
