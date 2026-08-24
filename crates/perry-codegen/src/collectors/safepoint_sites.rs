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
            // #8583 (unit-4 / `__33499`): object and array literals allocate via
            // a runtime call (`js_array_from_values`, `js_object_*`) that can
            // collect, so RS4GC inserts a statepoint at each. A minified data
            // table is a single giant array-of-arrays — `__33499` lowered to
            // 11,104 `js_array_from_values` calls, none of which is an `Expr::Call`,
            // so the pre-fix count saw almost no safepoints, the function was not
            // spilled, and RS4GC then fanned out for >3 h. Counting these keeps
            // the estimate an over-approximation biased toward spilling (the safe
            // direction; a hoisted/constant literal that emits no call only costs
            // a cheap shadow frame).
            | Expr::Object(_)
            | Expr::ObjectSpread { .. }
            | Expr::ObjectAssign { .. }
            | Expr::Array(_)
            | Expr::ArraySpread(_)
            // #8583 (`__AnonShape_*_constructor`): a property/index STORE lowers
            // to an allocating, collecting runtime call (`js_class_field_set_ic`
            // / `js_set_property` / the array-set helpers) that RS4GC gives a
            // statepoint. A closed-shape object literal compiles to a constructor
            // that is one long run of `this.field = v` stores (`PropertySet`);
            // with none counted the estimate was ~0, the constructor was not
            // spilled, and RS4GC grew it 34k -> 2.28M instructions, overrunning
            // the #8586 budget. Count the stores (not the reads:
            // `PropertyGet`/`IndexGet` frequently inline to a shape-cached load
            // with no call, and counting them would over-spill read-heavy hot
            // loops). `PropertyUpdate` (`x.f++`) is a read-modify-write store.
            | Expr::PropertySet { .. }
            | Expr::PropertyUpdate { .. }
            | Expr::IndexSet { .. }
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

    #[test]
    fn array_and_object_literals_are_safepoints() {
        // #8583: allocating literals lower to a collecting runtime call
        // (`js_array_from_values` / `js_object_*`) and must count. A minified
        // data table is a giant array-of-arrays with no `Expr::Call` at all —
        // the pre-fix count saw zero safepoints and the function was not spilled.
        let inner = |a, b| Expr::Array(vec![Expr::Number(a as f64), Expr::Number(b as f64)]);
        // [[..],[..],[..]] — one outer Array + three inner Arrays = 4 safepoints.
        let table = Expr::Array(vec![inner(1, 2), inner(3, 4), inner(5, 6)]);
        assert_eq!(count_safepoint_sites(&[Stmt::Expr(table)]), 4);

        // An object literal is also an allocating safepoint.
        let obj = Expr::Object(vec![("k".to_string(), Expr::Number(1.0))]);
        assert_eq!(count_safepoint_sites(&[Stmt::Expr(obj)]), 1);
    }

    #[test]
    fn nested_array_literals_recurse() {
        // Deeply nested constant arrays count every allocating level — the
        // `__33499` shape (11,104 `js_array_from_values` from one literal).
        let leaf = || Expr::Array(vec![Expr::Number(0.0)]);
        let rows: Vec<Expr> = (0..10).map(|_| leaf()).collect();
        // 1 outer + 10 inner = 11.
        assert_eq!(count_safepoint_sites(&[Stmt::Expr(Expr::Array(rows))]), 11);
    }

    #[test]
    fn property_and_index_stores_are_safepoints() {
        // #8583: `this.field = v` / `arr[i] = v` lower to a collecting runtime
        // call and must count. A closed-shape object literal is a constructor of
        // many `PropertySet` stores (the `__AnonShape_*_constructor` shape) — the
        // pre-fix count saw none, so the constructor never spilled and RS4GC
        // overran the #8586 budget.
        let this = || Expr::LocalGet(0);
        let set = |p: &str| Expr::PropertySet {
            object: Box::new(this()),
            property: p.to_string(),
            value: Box::new(Expr::Number(1.0)),
        };
        // Three field stores in the constructor body.
        let body = vec![
            Stmt::Expr(set("a")),
            Stmt::Expr(set("b")),
            Stmt::Expr(set("c")),
        ];
        assert_eq!(count_safepoint_sites(&body), 3);

        // An index store counts too; the value sub-expression still recurses
        // (a call in the value is its own safepoint).
        let idx_set = Expr::IndexSet {
            object: Box::new(this()),
            index: Box::new(Expr::Number(0.0)),
            value: Box::new(call(vec![])),
        };
        // 1 for the IndexSet + 1 for the call in `value`.
        assert_eq!(count_safepoint_sites(&[Stmt::Expr(idx_set)]), 2);

        // A read (`PropertyGet`) is deliberately NOT a safepoint (it inlines).
        let get = Expr::PropertyGet {
            object: Box::new(this()),
            property: "x".to_string(),
            byte_offset: 0,
        };
        assert_eq!(count_safepoint_sites(&[Stmt::Expr(get)]), 0);
    }
}
