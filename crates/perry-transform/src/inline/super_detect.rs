//! Recursion guarding for expression inlining and lexical-`super` detection.
//!
//! These helpers keep `call_inliner` from recursing without bound when inlining
//! deeply-nested expressions, and detect whether a method body references
//! `super` in any form (which would be unsafe to inline onto a different
//! receiver).

use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Function, Stmt};
use std::cell::Cell;

pub(crate) const MAX_INLINE_EXPR_RECURSION_DEPTH: usize = 128;

/// Per-top-level-walk budget of guarded entries (see
/// `enter_inline_expr_recursion`). The depth cap alone bounds STACK, not
/// WORK: with candidate-subtree cloning at every Conditional/Logical/
/// Sequence node and re-walks of freshly inlined bodies, a single huge
/// generated function can keep the walk churning within the depth limit
/// for hours (observed on #6593's 13.3MB esbuild bundle: the pass ran
/// 2h45m+ CPU-bound in clone glue after the stack fix, where it previously
/// crashed in ~90s). Once a walk has consumed this many entries, further
/// entries bail — the identical semantics-preserving skip as the depth
/// bail. One entry is roughly one visited stmt-list/expr node, so ordinary
/// bodies (even tens of thousands of nodes) keep full inlining coverage;
/// only generated-code monsters hit the cap, and those merely lose
/// inlining of the subtrees beyond it.
pub(crate) const MAX_INLINE_WALK_WORK: usize = 250_000;

thread_local! {
    static INLINE_EXPR_RECURSION_DEPTH: Cell<usize> = const { Cell::new(0) };
    /// Guarded entries consumed by the current top-level walk. Reset when a
    /// new walk starts (an entry at depth 0); never refunded on unwind.
    static INLINE_WALK_WORK: Cell<usize> = const { Cell::new(0) };
}

pub(crate) struct InlineExprRecursionGuard;

impl Drop for InlineExprRecursionGuard {
    fn drop(&mut self) {
        INLINE_EXPR_RECURSION_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

pub(crate) fn enter_inline_expr_recursion() -> Option<InlineExprRecursionGuard> {
    // The guard must only ever exist when the increment actually happened.
    // The previous `entered.then_some(InlineExprRecursionGuard)` constructed
    // the guard EAGERLY (`then_some` takes its argument by value), so on the
    // bail path `then_some` dropped that guard — and its `Drop` decremented a
    // depth unit this call never took. Every bail refunded one level of
    // budget, so any AST node that makes two or more sibling recursive
    // descents (Conditional then/else, a freshly-inlined-result re-walk, …)
    // could burn the first sibling's bail to push the next sibling one level
    // deeper, forever: the cap stopped bounding recursion at all (#6593, the
    // 13.3MB pi bundle overflowed a 512MB stack this way). Linear chains —
    // like the #733 nested-closure regression test — never exposed this,
    // because with a single descent per level there is no later sibling to
    // spend the refunded budget.
    INLINE_EXPR_RECURSION_DEPTH.with(|depth| {
        let current = depth.get();
        if current == 0 {
            // New top-level walk (Phase 4 init / a Phase 5 function body / a
            // Phase 6 method body): fresh work budget.
            INLINE_WALK_WORK.with(|work| work.set(0));
        }
        if current >= MAX_INLINE_EXPR_RECURSION_DEPTH {
            return None;
        }
        let over_work_budget = INLINE_WALK_WORK.with(|work| {
            let used = work.get();
            if used >= MAX_INLINE_WALK_WORK {
                true
            } else {
                work.set(used + 1);
                false
            }
        });
        if over_work_budget {
            return None;
        }
        depth.set(current + 1);
        Some(InlineExprRecursionGuard)
    })
}

fn expr_contains_lexical_super(expr: &Expr) -> bool {
    // Every `super` form binds the implicit receiver lexically to the method's
    // defining class, so inlining any of them onto a different `this` would
    // re-resolve `super` against the wrong parent. Reject reads, writes, calls,
    // and the object-literal `super` variants alike — not just property sets.
    if matches!(
        expr,
        Expr::SuperCall(..)
            | Expr::SuperCallSpread(..)
            | Expr::SuperMethodCall { .. }
            | Expr::SuperPropertyGet { .. }
            | Expr::SuperPropertySet { .. }
            | Expr::ObjectSuperPropertyGet { .. }
            | Expr::ObjectSuperPropertySet { .. }
            | Expr::ObjectSuperMethodCall { .. }
    ) {
        return true;
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| {
        if !found && expr_contains_lexical_super(child) {
            found = true;
        }
    });
    found
}

fn stmt_contains_lexical_super(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let { init, .. } => init.as_ref().is_some_and(expr_contains_lexical_super),
        Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
            expr_contains_lexical_super(expr)
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_contains_lexical_super(condition)
                || then_branch.iter().any(stmt_contains_lexical_super)
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| branch.iter().any(stmt_contains_lexical_super))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_contains_lexical_super(condition) || body.iter().any(stmt_contains_lexical_super)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .is_some_and(|stmt| stmt_contains_lexical_super(stmt.as_ref()))
                || condition.as_ref().is_some_and(expr_contains_lexical_super)
                || update.as_ref().is_some_and(expr_contains_lexical_super)
                || body.iter().any(stmt_contains_lexical_super)
        }
        Stmt::Labeled { body, .. } => stmt_contains_lexical_super(body),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(stmt_contains_lexical_super)
                || catch
                    .as_ref()
                    .is_some_and(|catch| catch.body.iter().any(stmt_contains_lexical_super))
                || finally
                    .as_ref()
                    .is_some_and(|body| body.iter().any(stmt_contains_lexical_super))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_contains_lexical_super(discriminant)
                || cases.iter().any(|case| {
                    case.test.as_ref().is_some_and(expr_contains_lexical_super)
                        || case.body.iter().any(stmt_contains_lexical_super)
                })
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => false,
    }
}

pub(crate) fn method_contains_lexical_super(method: &Function) -> bool {
    method.body.iter().any(stmt_contains_lexical_super)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #6593 regression: a failed entry (cap hit) must NOT refund depth
    /// budget. With the old eager `then_some(InlineExprRecursionGuard)`, the
    /// bail path dropped an eagerly-built guard, decrementing the counter it
    /// never incremented — so the attempt right after a bail wrongly
    /// succeeded, and branching recursion could descend without bound.
    #[test]
    fn bail_does_not_refund_depth_budget() {
        let guards: Vec<InlineExprRecursionGuard> = (0..MAX_INLINE_EXPR_RECURSION_DEPTH)
            .map(|_| enter_inline_expr_recursion().expect("budget not yet exhausted"))
            .collect();
        assert!(
            enter_inline_expr_recursion().is_none(),
            "entry at cap must bail"
        );
        assert!(
            enter_inline_expr_recursion().is_none(),
            "a bail must not refund budget: the next attempt at cap must bail too"
        );
        drop(guards);
        assert!(
            enter_inline_expr_recursion().is_some(),
            "budget must be restored once real guards unwind"
        );
    }

    /// #6593 companion: the depth cap bounds stack but not total work — a
    /// walk that keeps entering/unwinding within the depth limit must
    /// eventually exhaust a per-walk work budget and bail, and a NEW
    /// top-level walk (entry at depth 0) must start with a fresh budget.
    #[test]
    fn work_budget_is_spent_per_walk_and_resets_at_top_level() {
        // Hold one outer guard so the walk stays "in progress" (depth >= 1)
        // while we burn the budget with enter/unwind churn.
        let outer = enter_inline_expr_recursion().expect("fresh walk must enter");
        for _ in 0..MAX_INLINE_WALK_WORK - 1 {
            let inner = enter_inline_expr_recursion();
            assert!(inner.is_some(), "within budget, entries must succeed");
            drop(inner);
        }
        assert!(
            enter_inline_expr_recursion().is_none(),
            "an exhausted work budget must bail even though depth unwound"
        );
        drop(outer);
        assert!(
            enter_inline_expr_recursion().is_some(),
            "a new top-level walk must reset the work budget"
        );
    }
}
