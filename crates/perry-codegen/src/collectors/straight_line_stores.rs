//! Count the property-store sites a function body executes at most once per
//! call (#10663).
//!
//! A static-key store `o.k = v` lowers to an inline polymorphic cache
//! (`expr/put_value_store_ic.rs`): four compared ways, a key-add memo, the
//! write barrier and the layout notes — about twenty basic blocks and four
//! non-leaf calls per site. That code pays for itself only when the site runs
//! more than once. A generated constant table (`mysql2/lib/constants/errors.js`:
//! 3,942 `exports.X = N` / `exports[N] = 'X'` lines in one CommonJS factory)
//! runs each site exactly once, yet emitted ~80k blocks and 15.8k calls into a
//! single function that LLVM then spent minutes on.
//!
//! The count here is what [`crate::codegen::helpers::decide_straight_line_store_outline`]
//! compares against its threshold. It covers every assignment store
//! (`PutValueSet`, `PropertySet`) that is not inside a loop body and not
//! inside a nested closure (a closure is its own function with its own
//! decision). Not every counted site reaches the inline cache — class-field
//! and scalar-replaced stores take their own paths — so the count
//! over-approximates the inline-cache population, which only moves the
//! threshold, never correctness: an outlined store is the same
//! `js_put_value_set_packed_miss` call every inline-cache miss already makes.

use perry_hir::{Expr, Stmt};

/// Store sites reachable from `stmts` outside every loop body and nested
/// closure.
pub fn count_straight_line_store_sites(stmts: &[Stmt]) -> usize {
    let mut n = 0usize;
    for s in stmts {
        count_in_stmt(s, &mut n);
    }
    n
}

fn count_in_expr(e: &Expr, n: &mut usize) {
    if matches!(e, Expr::PutValueSet { .. } | Expr::PropertySet { .. }) {
        *n += 1;
    }
    // `walk_expr_children` does not enter a closure's body, and no expression
    // other than a closure contains a loop.
    perry_hir::walker::walk_expr_children(e, &mut |child| count_in_expr(child, n));
}

fn count_in_stmt(s: &Stmt, n: &mut usize) {
    match s {
        Stmt::Let { init: Some(e), .. }
        | Stmt::Expr(e)
        | Stmt::Throw(e)
        | Stmt::Return(Some(e)) => count_in_expr(e, n),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            count_in_expr(condition, n);
            for st in then_branch {
                count_in_stmt(st, n);
            }
            for st in else_branch.iter().flatten() {
                count_in_stmt(st, n);
            }
        }
        // A loop's sites run once per iteration: they keep the inline cache,
        // so nothing under a loop is counted.
        Stmt::While { .. } | Stmt::DoWhile { .. } | Stmt::For { .. } => {}
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
            for st in finally.iter().flatten() {
                count_in_stmt(st, n);
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
        Stmt::Let { init: None, .. }
        | Stmt::Return(None)
        | Stmt::Break
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
    use super::count_straight_line_store_sites;
    use perry_hir::types::Type;
    use perry_hir::{Expr, Stmt};

    fn store(key: &str) -> Expr {
        Expr::PutValueSet {
            target: Box::new(Expr::LocalGet(0)),
            key: Box::new(Expr::String(key.to_string())),
            value: Box::new(Expr::Number(1.0)),
            receiver: Box::new(Expr::LocalGet(0)),
            strict: true,
        }
    }

    #[test]
    fn counts_top_level_and_branch_stores() {
        let stmts = vec![
            Stmt::Expr(store("a")),
            Stmt::If {
                condition: Expr::Bool(true),
                then_branch: vec![Stmt::Expr(store("b"))],
                else_branch: Some(vec![Stmt::Expr(store("c"))]),
            },
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(0)),
                property: "d".to_string(),
                value: Box::new(Expr::Number(2.0)),
            }),
        ];
        assert_eq!(count_straight_line_store_sites(&stmts), 4);
    }

    #[test]
    fn loop_bodies_are_not_counted() {
        let stmts = vec![
            Stmt::While {
                condition: Expr::Bool(true),
                body: vec![Stmt::Expr(store("a")), Stmt::Expr(store("b"))],
            },
            Stmt::For {
                init: None,
                condition: None,
                update: Some(store("c")),
                body: vec![Stmt::Expr(store("d"))],
            },
            Stmt::Expr(store("e")),
        ];
        assert_eq!(count_straight_line_store_sites(&stmts), 1);
    }

    #[test]
    fn nested_closure_bodies_are_not_counted() {
        let closure = Expr::Closure {
            func_id: 0,
            params: vec![],
            return_type: Type::Any,
            body: vec![Stmt::Expr(store("inner"))],
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: false,
        };
        let stmts = vec![Stmt::Expr(closure), Stmt::Expr(store("outer"))];
        assert_eq!(count_straight_line_store_sites(&stmts), 1);
    }

    fn decided(stmts: &[Stmt]) -> bool {
        let mut func = crate::function::LlFunction::new("f", crate::types::DOUBLE, vec![]);
        crate::codegen::helpers::decide_straight_line_store_outline(&mut func, stmts);
        func.outlines_straight_line_store_ics()
    }

    /// #10663: the decision fires at the threshold and not one below it, and
    /// stores under a loop never push a body over it.
    #[test]
    fn decision_boundary_counts_only_straight_line_stores() {
        let min = crate::codegen::helpers::STRAIGHT_LINE_STORE_OUTLINE_MIN_SITES;
        let stores = |n: usize| -> Vec<Stmt> {
            (0..n)
                .map(|i| Stmt::Expr(store(&format!("k{i}"))))
                .collect()
        };
        assert!(decided(&stores(min)));
        assert!(!decided(&stores(min - 1)));

        let mut looped = stores(min - 1);
        looped.push(Stmt::While {
            condition: Expr::Bool(false),
            body: stores(min),
        });
        assert!(!decided(&looped));
    }
}
