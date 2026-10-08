//! Select canonical i32 storage inside the existing Number-local clone.
//! A stable strict upper bound and an increment budget prove no wrap can occur.
use super::*;
use perry_hir::{CompareOp, UpdateOp};

pub(super) struct Plan {
    pub counter: u32,
    pub bound: u32,
    pub extra_increments: u32,
}

pub(super) fn match_plan(
    ctx: &FnCtx<'_>,
    numbers: &[u32],
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Option<Plan> {
    if !crate::expr::canonical_i32_locals_enabled() || !ctx.repsel_context_allows_canonical_i32 {
        return None;
    }
    let Expr::Compare {
        op: CompareOp::Lt,
        left,
        right,
    } = condition?
    else {
        return None;
    };
    let (Expr::LocalGet(counter), Expr::LocalGet(bound)) = (left.as_ref(), right.as_ref()) else {
        return None;
    };
    if counter == bound
        || !numbers.contains(counter)
        || !matches!(update, Some(Expr::Update { id, op: UpdateOp::Increment, .. }) if id == counter)
        || !ctx.locals.contains_key(bound)
        || ctx.boxed_vars.contains(bound)
        || ctx.closure_captures.contains_key(bound)
        || ctx.repsel_closure_ref_locals.contains(bound)
        || ctx.module_globals.contains_key(bound)
        || ctx.i32_counter_slots.contains_key(bound)
        || ctx.numeric_accumulator_f64_slots.contains_key(bound)
    {
        return None;
    }
    fn expr_budget(e: &Expr, counter: u32, bound: u32) -> Option<u32> {
        match e {
            Expr::LocalSet(id, _) if *id == counter || *id == bound => return None,
            Expr::Update { id, op, .. }
                if *id == bound || (*id == counter && *op != UpdateOp::Increment) =>
            {
                return None
            }
            Expr::Closure { .. } | Expr::Await(_) | Expr::Yield { .. } => return None,
            _ => {}
        }
        let mut budget = Some(u32::from(
            matches!(e, Expr::Update { id, .. } if *id == counter),
        ));
        perry_hir::walker::walk_expr_children(e, &mut |child| {
            budget = budget
                .and_then(|n| expr_budget(child, counter, bound).and_then(|c| n.checked_add(c)));
        });
        budget
    }
    fn stmts_budget(stmts: &[Stmt], counter: u32, bound: u32) -> Option<u32> {
        let mut total = 0u32;
        for stmt in stmts {
            let n = match stmt {
                Stmt::Let { id, init, .. } if *id != counter && *id != bound => init
                    .as_ref()
                    .map_or(Some(0), |e| expr_budget(e, counter, bound)),
                Stmt::Expr(e) | Stmt::Throw(e) => expr_budget(e, counter, bound),
                Stmt::Return(e) => e
                    .as_ref()
                    .map_or(Some(0), |e| expr_budget(e, counter, bound)),
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    let c = expr_budget(condition, counter, bound)?;
                    let t = stmts_budget(then_branch, counter, bound)?;
                    let f = else_branch
                        .as_deref()
                        .map_or(Some(0), |s| stmts_budget(s, counter, bound))?;
                    c.checked_add(t.max(f))
                }
                Stmt::Break | Stmt::Continue => Some(0),
                // Nested loops, try and labels have no finite per-iteration budget here.
                _ => None,
            }?;
            total = total.checked_add(n)?;
        }
        Some(total)
    }
    let extra_increments = stmts_budget(body, *counter, *bound)?;
    if extra_increments >= i32::MAX as u32 {
        return None;
    }
    Some(Plan {
        counter: *counter,
        bound: *bound,
        extra_increments,
    })
}

pub(super) struct Active {
    counter: u32,
    slot: String,
    real_slot: String,
    was_nonnegative: bool,
}

impl Active {
    pub(super) fn begin(ctx: &mut FnCtx<'_>, plan: &Plan, slot: String) -> Self {
        let state = Self {
            counter: plan.counter,
            real_slot: ctx.locals[&plan.counter].clone(),
            slot: slot.clone(),
            was_nonnegative: ctx.nonnegative_integer_locals.contains(&plan.counter),
        };
        ctx.i32_counter_slots.insert(plan.counter, slot);
        ctx.local_slot_reps
            .insert(plan.counter, crate::expr::SlotRep::I32);
        // The dominating guard proves nonnegative integral entry; only ++ writes
        // execute, and the stable bound reserves every possible body increment.
        ctx.nonnegative_integer_locals.insert(plan.counter);
        state
    }
    pub(super) fn sync(&self, ctx: &mut FnCtx<'_>) {
        let n = ctx.block().load(crate::types::I32, &self.slot);
        let value = ctx
            .block()
            .sitofp(crate::types::I32, &n, crate::types::DOUBLE);
        // GC_STORE_AUDIT(STACK): the guarded Number value has no heap edge.
        // GC_STORE_AUDIT(STACK): the guarded Number value has no heap edge.
        ctx.block()
            .store(crate::types::DOUBLE, &value, &self.real_slot);
    }
    pub(super) fn finish(self, ctx: &mut FnCtx<'_>) {
        ctx.i32_counter_slots.remove(&self.counter);
        ctx.local_slot_reps.remove(&self.counter);
        if !self.was_nonnegative {
            ctx.nonnegative_integer_locals.remove(&self.counter);
        }
        // Do not export facts learned only in the guarded clone to its sibling.
        ctx.int_range_facts
            .retain(|fact| fact.local_id != self.counter);
        ctx.int_range_aliases.remove(&self.counter);
    }
}
