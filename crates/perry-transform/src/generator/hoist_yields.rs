//! #321: hoist `yield` / `yield*` out of compound expressions into ordered
//! temps, so the linearizer only ever sees a yield at a position it already
//! handles (bare `yield E;`, `let x = yield E;`, `return yield E;`).
//!
//! The linearizer (`linearize.rs`) special-cases yields at statement, let-init,
//! and return positions. A yield buried inside a larger expression — e.g.
//! `return (yield 1) + (yield 2)` — falls into the catch-all and is emitted as
//! one unsplit statement; codegen then lowers each `Expr::Yield` via the
//! "generators not implemented" arm (evaluate operand for side effects, return
//! `0.0`), so the resumed values are dropped. The whole expression also never
//! suspends, so the generator finishes on the first `.next()`.
//!
//! This pass walks every statement left-to-right and, for each `yield` /
//! `yield*` sub-expression that is NOT already in a directly-handled position,
//! emits `let __ygen_N = yield <E>;` immediately before the containing
//! statement and replaces the occurrence with `LocalGet(__ygen_N)`. The
//! existing let-init arms then split each into a suspend/resume state, binding
//! the resumed value to the temp. The remaining (now yield-free) combining
//! expression evaluates against the temps.
//!
//! Evaluation order is preserved: children are visited in evaluation order and
//! each hoisted let is appended in that order, so `(yield a) + (yield b)`
//! yields `a` then `b` and combines `t_a + t_b` left-to-right.
//!
//! Short-circuiting (`&&`, `||`, `??`) and the ternary `?:` are handled
//! specially: a yield on a path that may not be taken must only run when that
//! path is taken. Those forms are lifted into a temp + `if`-statement so the
//! yield ends up inside the conditionally-executed branch (mirrors the
//! conditional-await lift in `async_to_generator.rs`).
//!
//! Nested closures are not descended into — a yield inside a nested
//! `function*` belongs to that generator, not this one.

use super::*;

/// Entry point: hoist non-top-level yields across a statement list, recursing
/// into nested control-flow bodies.
pub fn hoist_yields_in_stmts(stmts: &mut Vec<Stmt>, next_id: &mut LocalId) {
    let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for stmt in std::mem::take(stmts) {
        let mut hoisted: Vec<Stmt> = Vec::new();
        let new_stmt = hoist_yields_in_stmt(stmt, next_id, &mut hoisted);
        out.extend(hoisted);
        out.push(new_stmt);
    }
    *stmts = out;
}

fn hoist_yields_in_stmt(mut stmt: Stmt, next_id: &mut LocalId, hoisted: &mut Vec<Stmt>) -> Stmt {
    match &mut stmt {
        // Top-level positions: keep the *outer* yield (the linearizer handles
        // `let x = yield E`, `yield E;`, `return yield E`) but hoist any yields
        // nested inside the operand `E`.
        Stmt::Let { init: Some(e), .. } => {
            hoist_yields_avoiding_top_level(e, next_id, hoisted);
        }
        Stmt::Expr(e) => {
            hoist_yields_avoiding_top_level(e, next_id, hoisted);
        }
        Stmt::Return(Some(e)) => {
            hoist_yields_avoiding_top_level(e, next_id, hoisted);
        }
        Stmt::Throw(e) => {
            // `throw (yield x)` — hoist all yields fully (there is no
            // throw-yield top-level arm in the linearizer, so a top-level
            // yield here must be hoisted to a temp too).
            hoist_yields_in_expr_full(e, next_id, hoisted);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            // Condition is evaluated before the branch is chosen — a yield in
            // it always runs, so fully hoist it above the `if`.
            hoist_yields_in_expr_full(condition, next_id, hoisted);
            hoist_yields_in_stmts(then_branch, next_id);
            if let Some(eb) = else_branch {
                hoist_yields_in_stmts(eb, next_id);
            }
        }
        Stmt::While { condition, body } => {
            // A yield in the while-condition is re-evaluated each iteration;
            // the linearizer's While arm rebuilds the condition into a per-
            // iteration cond_state, so leave the condition's own yields in
            // place (they would need re-running, not a one-shot hoist).
            hoist_yields_in_stmts(body, next_id);
            let _ = condition;
        }
        Stmt::DoWhile { body, condition } => {
            hoist_yields_in_stmts(body, next_id);
            let _ = condition;
        }
        Stmt::For {
            init,
            condition: _,
            update: _,
            body,
        } => {
            // For-loop init runs once before the loop; hoist its yields ahead
            // of the loop. Condition/update are per-iteration — left in place
            // for the linearizer's For arm (matching the await pass's
            // single-hoist approximation for loop condition/update).
            if let Some(i) = init {
                let mut inner = Vec::new();
                let replaced = hoist_yields_in_stmt((**i).clone(), next_id, &mut inner);
                hoisted.extend(inner);
                **i = replaced;
            }
            hoist_yields_in_stmts(body, next_id);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            hoist_yields_in_stmts(body, next_id);
            if let Some(c) = catch {
                hoist_yields_in_stmts(&mut c.body, next_id);
            }
            if let Some(f) = finally {
                hoist_yields_in_stmts(f, next_id);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            hoist_yields_in_expr_full(discriminant, next_id, hoisted);
            for case in cases.iter_mut() {
                if let Some(t) = &mut case.test {
                    hoist_yields_in_expr_full(t, next_id, hoisted);
                }
                hoist_yields_in_stmts(&mut case.body, next_id);
            }
        }
        Stmt::Labeled { body, .. } => {
            let mut inner = Vec::new();
            let body_taken = std::mem::replace(body.as_mut(), Stmt::Break);
            let new_body = hoist_yields_in_stmt(body_taken, next_id, &mut inner);
            hoisted.extend(inner);
            **body = new_body;
        }
        _ => {}
    }
    stmt
}

/// Hoist all yields in an expression INCLUDING one at the expression's own top
/// level, into preceding `let __ygen_N = yield E;` statements. Used for
/// positions that are not handled by a dedicated linearizer arm (if/while/for
/// conditions, switch discriminant, call args once we recurse, etc.).
fn hoist_yields_in_expr_full(expr: &mut Expr, next_id: &mut LocalId, hoisted: &mut Vec<Stmt>) {
    if matches!(expr, Expr::Closure { .. }) {
        // Yields inside a nested closure belong to that (generator) closure.
        return;
    }

    // Short-circuiting forms: lift before the general walk so the yield ends
    // up inside the conditionally-executed branch instead of running
    // unconditionally above the statement.
    if matches!(expr, Expr::Conditional { .. }) && conditional_branches_contain_yield(expr) {
        lift_conditional_with_yield_branches(expr, next_id, hoisted);
        return;
    }
    if matches!(expr, Expr::Logical { .. }) && logical_rhs_contains_yield(expr) {
        lift_logical_with_yield_rhs(expr, next_id, hoisted);
        return;
    }

    // Recurse into children first so inner yields are hoisted before the outer
    // expression's own yield (innermost-first, left-to-right).
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        hoist_yields_in_expr_full(child, next_id, hoisted);
    });

    if matches!(expr, Expr::Yield { .. }) {
        let id = alloc_local(next_id);
        let original = std::mem::replace(expr, Expr::LocalGet(id));
        hoisted.push(Stmt::Let {
            id,
            name: format!("__ygen_{}", id),
            ty: Type::Any,
            mutable: false,
            init: Some(original),
        });
    }
}

/// Hoist nested yields but leave a top-level yield alone. Used for statement-
/// positioned operands (Let init, Stmt::Expr operand, Return operand) where the
/// linearizer already handles the outer yield.
fn hoist_yields_avoiding_top_level(
    expr: &mut Expr,
    next_id: &mut LocalId,
    hoisted: &mut Vec<Stmt>,
) {
    if let Expr::Yield { value, .. } = expr {
        // Outer is a yield the linearizer handles — keep it, but fully hoist
        // any yields nested inside its operand (`yield (yield x)`).
        if let Some(inner) = value {
            hoist_yields_in_expr_full(inner.as_mut(), next_id, hoisted);
        }
        return;
    }
    if matches!(expr, Expr::Closure { .. }) {
        return;
    }
    // Short-circuiting forms at statement-operand top level: lift so the yield
    // lands inside an if-branch rather than unconditionally above the stmt.
    if matches!(expr, Expr::Conditional { .. }) && conditional_branches_contain_yield(expr) {
        lift_conditional_with_yield_branches(expr, next_id, hoisted);
        return;
    }
    if matches!(expr, Expr::Logical { .. }) && logical_rhs_contains_yield(expr) {
        lift_logical_with_yield_rhs(expr, next_id, hoisted);
        return;
    }
    // Outer is not a yield: children may hold yields, which are nested — fully
    // hoist them.
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        hoist_yields_in_expr_full(child, next_id, hoisted);
    });
}

/// True if either branch of a `Conditional` contains a yield (outside nested
/// closures).
fn conditional_branches_contain_yield(expr: &Expr) -> bool {
    if let Expr::Conditional {
        then_expr,
        else_expr,
        ..
    } = expr
    {
        return expr_contains_yield(then_expr) || expr_contains_yield(else_expr);
    }
    false
}

/// True if the RHS of a `Logical` (`&&`/`||`/`??`) contains a yield. Only the
/// RHS is short-circuited — a yield in the LHS always runs, so it doesn't need
/// the conditional lift (it is hoisted by the normal child walk).
fn logical_rhs_contains_yield(expr: &Expr) -> bool {
    if let Expr::Logical { right, .. } = expr {
        return expr_contains_yield(right);
    }
    false
}

pub(super) fn expr_contains_yield(expr: &Expr) -> bool {
    if matches!(expr, Expr::Yield { .. }) {
        return true;
    }
    if matches!(expr, Expr::Closure { .. }) {
        return false;
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        if !found && expr_contains_yield(child) {
            found = true;
        }
    });
    found
}

/// Replace `cond ? then_e : else_e` (where a branch contains a yield) with
/// `LocalGet(__ycond_N)` and emit before the containing statement:
///
///   let __ycond_N: any;
///   if (cond) { __ycond_N = then_e; } else { __ycond_N = else_e; }
///
/// Yields inside each branch's assignment are then hoisted by the recursive
/// `hoist_yields_in_stmts` so they land at the top of their own if-branch (the
/// position the linearizer splits states at). The `cond` itself is fully
/// hoisted because it always runs before either branch.
fn lift_conditional_with_yield_branches(
    expr: &mut Expr,
    next_id: &mut LocalId,
    hoisted: &mut Vec<Stmt>,
) {
    let temp_id = alloc_local(next_id);
    let owned = std::mem::replace(expr, Expr::LocalGet(temp_id));
    if let Expr::Conditional {
        mut condition,
        then_expr,
        else_expr,
    } = owned
    {
        // The condition always runs first; hoist any yields it holds above
        // the lifted `if`.
        hoist_yields_in_expr_full(condition.as_mut(), next_id, hoisted);

        hoisted.push(Stmt::Let {
            id: temp_id,
            name: format!("__ycond_{}", temp_id),
            ty: Type::Any,
            mutable: true,
            init: None,
        });

        let mut then_branch = vec![Stmt::Expr(Expr::LocalSet(temp_id, then_expr))];
        hoist_yields_in_stmts(&mut then_branch, next_id);

        let mut else_branch = vec![Stmt::Expr(Expr::LocalSet(temp_id, else_expr))];
        hoist_yields_in_stmts(&mut else_branch, next_id);

        hoisted.push(Stmt::If {
            condition: *condition,
            then_branch,
            else_branch: Some(else_branch),
        });
    }
}

/// Replace `lhs && rhs` / `lhs || rhs` / `lhs ?? rhs` (where `rhs` contains a
/// yield) with `LocalGet(__ylogic_N)` and emit before the containing statement
/// an `if` that only evaluates `rhs` when the operator does not short-circuit:
///
///   &&:  let t = lhs; if (t) { t = rhs; }            // rhs only when lhs truthy
///   ||:  let t = lhs; if (!t) { t = rhs; }           // rhs only when lhs falsy
///   ??:  let t = lhs; if (t === null || t === undefined) { t = rhs; }
///
/// Yields inside `rhs`'s assignment are hoisted into the if-branch by the
/// recursive `hoist_yields_in_stmts`. A yield in `lhs` always runs and is
/// hoisted by the full walk of `lhs`. The temp `t` carries the operator's
/// result value, preserving `&&`/`||`/`??` value semantics (returns the operand
/// that determined the result, not a coerced boolean).
fn lift_logical_with_yield_rhs(expr: &mut Expr, next_id: &mut LocalId, hoisted: &mut Vec<Stmt>) {
    let temp_id = alloc_local(next_id);
    let owned = std::mem::replace(expr, Expr::LocalGet(temp_id));
    if let Expr::Logical {
        op,
        mut left,
        right,
    } = owned
    {
        // `left` always evaluates; hoist any yields it holds above the temp.
        hoist_yields_in_expr_full(left.as_mut(), next_id, hoisted);

        // let t = <left>;
        hoisted.push(Stmt::Let {
            id: temp_id,
            name: format!("__ylogic_{}", temp_id),
            ty: Type::Any,
            mutable: true,
            init: Some(*left),
        });

        // Build the guard that decides whether `right` is evaluated.
        let guard = match op {
            LogicalOp::And => Expr::LocalGet(temp_id),
            LogicalOp::Or => Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(Expr::LocalGet(temp_id)),
            },
            LogicalOp::Coalesce => Expr::Logical {
                op: LogicalOp::Or,
                left: Box::new(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(temp_id)),
                    right: Box::new(Expr::Null),
                }),
                right: Box::new(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(temp_id)),
                    right: Box::new(Expr::Undefined),
                }),
            },
        };

        let mut then_branch = vec![Stmt::Expr(Expr::LocalSet(temp_id, right))];
        hoist_yields_in_stmts(&mut then_branch, next_id);

        hoisted.push(Stmt::If {
            condition: guard,
            then_branch,
            else_branch: None,
        });
    }
}
