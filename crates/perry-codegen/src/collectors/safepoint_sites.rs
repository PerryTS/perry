//! Count the GC safepoints in a function body (#8583).
//!
//! `rewrite-statepoints-for-gc` inserts, at every safepoint, one relocation
//! per GC value live across it — so the optimizer's post-rewrite work grows
//! with `live_roots × safepoints`. A function whose product is large enough
//! makes the `-Os`/`-O3` middle-end super-linear: the 68 MB minified entry
//! body of the Claude Code bundle measured 795 root slots × ~106k safepoints
//! and grew 439k → 6.5M instructions under RS4GC, and a single `-Os` pass on
//! the result did not finish in practical time (#8583).
//! `codegen/helpers::maybe_spill_roots_to_shadow_frame` multiplies this count
//! by the function's root-slot count and, past a threshold, keeps that
//! function's roots in a shadow frame instead of statepoints.
//!
//! A safepoint is any call-like expression: a call can re-enter the runtime
//! and collect. The count is an over-approximation biased toward spilling —
//! a false positive is a shadow frame on a function that would have been fine
//! (cheap; the shadow lowering is the pre-#7370 default), while a false
//! negative would let relocation fan-out reach the optimizer. Nested closures
//! are NOT counted: each compiles to its own `LlFunction` with its own frame,
//! so its safepoints belong to it (`walk_expr_children` does not descend into
//! a closure's body, only its parameter defaults).

use perry_hir::{Expr, Stmt};

/// Total call-like expressions reachable from `stmts` without descending into
/// nested closures.
pub fn count_safepoint_sites(stmts: &[Stmt]) -> usize {
    let mut n = 0usize;
    for s in stmts {
        count_in_stmt(s, &mut n);
    }
    n
}

/// A call-like expression is a potential safepoint: anything whose lowering
/// emits a call that can re-enter the runtime. Nodes not listed contribute
/// nothing themselves but are still recursed into, so adding a new call
/// variant can only make the estimate more conservative (a possible
/// under-count that the post-RS4GC instruction-budget assertion backstops),
/// never wrong in a way that hides a fan-out.
fn is_safepoint(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Call { .. }
            | Expr::CallSpread { .. }
            | Expr::NativeMethodCall { .. }
            | Expr::StaticMethodCall { .. }
            | Expr::SuperCall(_)
            | Expr::SuperCallSpread(_)
            | Expr::SuperMethodCall { .. }
            | Expr::SuperMethodCallSpread { .. }
            | Expr::ObjectSuperMethodCall { .. }
            | Expr::New { .. }
            | Expr::NewDynamic { .. }
            | Expr::NewDynamicSpread { .. }
            | Expr::Await(_)
            | Expr::Yield { .. }
            | Expr::AsyncFirstCall { .. }
    )
}

fn count_in_expr(e: &Expr, n: &mut usize) {
    if is_safepoint(e) {
        *n += 1;
    }
    // Generic recursion into direct sub-expressions. `walk_expr_children` does
    // not descend into a closure's statement body (only its param defaults),
    // which is exactly the boundary we want: a nested closure is a separate
    // frame and its safepoints are not this function's.
    perry_hir::walker::walk_expr_children(e, &mut |child| count_in_expr(child, n));
}

fn count_in_stmt(s: &Stmt, n: &mut usize) {
    match s {
        Stmt::Let { init: Some(e), .. }
        | Stmt::Expr(e)
        | Stmt::Throw(e)
        | Stmt::Return(Some(e)) => count_in_expr(e, n),
        Stmt::Let { init: None, .. } | Stmt::Return(None) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            count_in_expr(condition, n);
            for st in then_branch {
                count_in_stmt(st, n);
            }
            if let Some(else_branch) = else_branch {
                for st in else_branch {
                    count_in_stmt(st, n);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            count_in_expr(condition, n);
            for st in body {
                count_in_stmt(st, n);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                count_in_stmt(init, n);
            }
            if let Some(condition) = condition {
                count_in_expr(condition, n);
            }
            if let Some(update) = update {
                count_in_expr(update, n);
            }
            for st in body {
                count_in_stmt(st, n);
            }
        }
        Stmt::Labeled { body, .. } => count_in_stmt(body, n),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for st in body {
                count_in_stmt(st, n);
            }
            if let Some(catch) = catch {
                for st in &catch.body {
                    count_in_stmt(st, n);
                }
            }
            if let Some(finally) = finally {
                for st in finally {
                    count_in_stmt(st, n);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            count_in_expr(discriminant, n);
            for c in cases {
                if let Some(t) = &c.test {
                    count_in_expr(t, n);
                }
                for st in &c.body {
                    count_in_stmt(st, n);
                }
            }
        }
        // No expression children.
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::count_safepoint_sites;
    use perry_hir::types::Type;
    use perry_hir::{Expr, Stmt};

    fn call(args: Vec<Expr>) -> Expr {
        Expr::Call {
            callee: Box::new(Expr::Undefined),
            args,
            type_args: vec![],
            byte_offset: 0,
        }
    }

    fn empty_closure(body: Vec<Stmt>) -> Expr {
        Expr::Closure {
            func_id: 0,
            params: vec![],
            return_type: Type::Any,
            body,
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: false,
        }
    }

    #[test]
    fn counts_calls_across_control_flow_but_not_into_closures() {
        let stmts = vec![
            Stmt::Expr(call(vec![])),
            Stmt::While {
                condition: Expr::Bool(true),
                body: vec![Stmt::Expr(call(vec![]))],
            },
            // A call buried in a nested closure body must NOT be counted.
            Stmt::Expr(empty_closure(vec![Stmt::Expr(call(vec![]))])),
            Stmt::Return(Some(call(vec![]))),
        ];
        assert_eq!(count_safepoint_sites(&stmts), 3);
    }

    #[test]
    fn call_arguments_are_themselves_safepoints() {
        // f(g(), h()) is three calls.
        let nested = call(vec![call(vec![]), call(vec![])]);
        assert_eq!(count_safepoint_sites(&[Stmt::Expr(nested)]), 3);
    }
}
