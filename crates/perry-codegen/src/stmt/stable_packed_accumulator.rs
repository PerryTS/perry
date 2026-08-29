//! Reduce-accumulator admission for the stable-packed fast clone.
//!
//! Split out of `stable_packed_loop.rs` to keep it under the 2,000-line file
//! gate. The entry point is [`collect_numeric_accumulators`]; the fact it
//! feeds is consumed by `type_analysis::is_numeric_expr`'s `LocalGet` arm and
//! the tag tests the caller emits in the fast preheader.

use perry_hir::{Expr, Stmt};

use crate::expr::FnCtx;

/// `PERRY_PACKED_LOOP_NUMERIC_ACCUMULATOR` gate (default on): admit reduce
/// accumulators into the fast clone's numeric proof. `=0`/`off`/`false` keeps
/// the pre-existing lowering (dynamic `+` per element) for A/B bisection.
pub(super) fn packed_loop_numeric_accumulators_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_PACKED_LOOP_NUMERIC_ACCUMULATOR").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// Fail-closed walk: is `expr` numeric with every leaf provable numeric
/// INSIDE the fast clone? Leaves are literals, the exact `array[counter]`
/// read (proven raw f64 by the clone's guard — the caller only admits
/// accumulators when `numeric_elements` is set), locals that are numeric on
/// their own, and the candidate accumulators themselves (the induction
/// hypothesis — the preheader tag test is the base case). Anything else —
/// calls, property reads, other indexed reads, closures — declines the
/// accumulator. `Add` is safe here for the same reason the element-shape walk
/// documents: with every admitted leaf numeric, string concatenation is
/// unreachable.
fn accumulator_rhs_is_numeric(
    ctx: &FnCtx<'_>,
    expr: &Expr,
    array_id: u32,
    counter_id: u32,
    candidates: &std::collections::BTreeSet<u32>,
) -> bool {
    match expr {
        Expr::Number(_) | Expr::Integer(_) => true,
        Expr::IndexGet { object, index } => matches!(
            (object.as_ref(), index.as_ref()),
            (Expr::LocalGet(a), Expr::LocalGet(i)) if *a == array_id && *i == counter_id
        ),
        Expr::LocalGet(id) => {
            candidates.contains(id) || crate::type_analysis::is_numeric_expr(ctx, expr)
        }
        Expr::Binary { left, right, .. } => {
            accumulator_rhs_is_numeric(ctx, left, array_id, counter_id, candidates)
                && accumulator_rhs_is_numeric(ctx, right, array_id, counter_id, candidates)
        }
        Expr::NumberCoerce(operand) => {
            accumulator_rhs_is_numeric(ctx, operand, array_id, counter_id, candidates)
        }
        Expr::Unary { op, operand } => {
            matches!(
                op,
                perry_hir::UnaryOp::Neg | perry_hir::UnaryOp::Pos | perry_hir::UnaryOp::BitNot
            ) && accumulator_rhs_is_numeric(ctx, operand, array_id, counter_id, candidates)
        }
        Expr::MathAbs(v)
        | Expr::MathSqrt(v)
        | Expr::MathFloor(v)
        | Expr::MathCeil(v)
        | Expr::MathRound(v)
        | Expr::MathTrunc(v)
        | Expr::MathSign(v)
        | Expr::MathFround(v) => {
            accumulator_rhs_is_numeric(ctx, v, array_id, counter_id, candidates)
        }
        Expr::MathImul(l, r) | Expr::MathPow(l, r) => {
            accumulator_rhs_is_numeric(ctx, l, array_id, counter_id, candidates)
                && accumulator_rhs_is_numeric(ctx, r, array_id, counter_id, candidates)
        }
        Expr::MathMin(values) | Expr::MathMax(values) => values
            .iter()
            .all(|v| accumulator_rhs_is_numeric(ctx, v, array_id, counter_id, candidates)),
        _ => false,
    }
}

/// Collect every write (`LocalSet` / `Update`) per local in `body`, without
/// descending into nested closures (their writes go through boxes, and a
/// boxed local is excluded from admission anyway).
pub(super) fn collect_local_writes<'a>(
    stmts: &'a [Stmt],
    out: &mut std::collections::BTreeMap<u32, Vec<Option<&'a Expr>>>,
) {
    fn walk_expr<'a>(
        expr: &'a Expr,
        out: &mut std::collections::BTreeMap<u32, Vec<Option<&'a Expr>>>,
    ) {
        match expr {
            Expr::LocalSet(id, value) => {
                out.entry(*id).or_default().push(Some(value));
                walk_expr(value, out);
            }
            Expr::Update { id, .. } => {
                out.entry(*id).or_default().push(None);
            }
            Expr::Closure { .. } => {}
            other => {
                perry_hir::walker::walk_expr_children(other, &mut |child| walk_expr(child, out));
            }
        }
    }
    fn walk_stmt<'a>(
        stmt: &'a Stmt,
        out: &mut std::collections::BTreeMap<u32, Vec<Option<&'a Expr>>>,
    ) {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(init) = init {
                    walk_expr(init, out);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => walk_expr(e, out),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    walk_expr(e, out);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                walk_expr(condition, out);
                for s in then_branch {
                    walk_stmt(s, out);
                }
                if let Some(body) = else_branch {
                    for s in body {
                        walk_stmt(s, out);
                    }
                }
            }
            Stmt::While { condition, body } => {
                walk_expr(condition, out);
                for s in body {
                    walk_stmt(s, out);
                }
            }
            Stmt::DoWhile { body, condition } => {
                for s in body {
                    walk_stmt(s, out);
                }
                walk_expr(condition, out);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    walk_stmt(init, out);
                }
                if let Some(condition) = condition {
                    walk_expr(condition, out);
                }
                if let Some(update) = update {
                    walk_expr(update, out);
                }
                for s in body {
                    walk_stmt(s, out);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    walk_stmt(s, out);
                }
                if let Some(catch) = catch {
                    for s in &catch.body {
                        walk_stmt(s, out);
                    }
                }
                if let Some(body) = finally {
                    for s in body {
                        walk_stmt(s, out);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                walk_expr(discriminant, out);
                for case in cases {
                    if let Some(test) = &case.test {
                        walk_expr(test, out);
                    }
                    for s in &case.body {
                        walk_stmt(s, out);
                    }
                }
            }
            Stmt::Labeled { body, .. } => walk_stmt(body, out),
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }
    for stmt in stmts {
        walk_stmt(stmt, out);
    }
}

/// Reduce accumulators the fast clone may prove numeric: plain uncaptured
/// locals whose every write in `body` is numeric-preserving under the
/// fixpoint. `Update` writes preserve Number-ness on a Number (BigInt cannot
/// appear: the preheader proves the base case and no admitted write produces
/// one). Fail-closed at every step.
pub(super) fn collect_numeric_accumulators(
    ctx: &FnCtx<'_>,
    body: &[Stmt],
    array_id: u32,
    counter_id: u32,
) -> Vec<u32> {
    if !packed_loop_numeric_accumulators_enabled() {
        return Vec::new();
    }
    let mut writes = std::collections::BTreeMap::new();
    collect_local_writes(body, &mut writes);
    let mut candidates: std::collections::BTreeSet<u32> = writes
        .keys()
        .copied()
        .filter(|id| {
            *id != array_id
                && *id != counter_id
                && ctx.locals.contains_key(id)
                && !ctx.boxed_vars.contains(id)
                && !ctx.closure_captures.contains_key(id)
                && !ctx.module_globals.contains_key(id)
                && !ctx.i32_counter_slots.contains_key(id)
                && ctx.shadow_slot_map.contains_key(id)
        })
        .collect();
    loop {
        let rejected: Vec<u32> = candidates
            .iter()
            .copied()
            .filter(|id| {
                !writes[id].iter().all(|write| match write {
                    Some(rhs) => {
                        accumulator_rhs_is_numeric(ctx, rhs, array_id, counter_id, &candidates)
                    }
                    // `Update` (++/--): ToNumeric(Number) ± 1 is a Number.
                    None => true,
                })
            })
            .collect();
        if rejected.is_empty() {
            break;
        }
        for id in rejected {
            candidates.remove(&id);
        }
    }
    candidates.into_iter().collect()
}
