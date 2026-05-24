use perry_hir::{BinaryOp, CompareOp, Expr, UpdateOp};

use crate::native_value::{AliasState, BoundsProof, BoundsState, BufferViewSlot, LengthSource};

use super::FnCtx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IntRange {
    pub min: i64,
    pub max: i64,
}

impl IntRange {
    pub(crate) fn exact(value: i64) -> Self {
        Self {
            min: value,
            max: value,
        }
    }

    fn is_nonnegative(self) -> bool {
        self.min >= 0
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct IntRangeFact {
    pub local_id: u32,
    pub scope_id: u32,
    pub range: IntRange,
}

fn resolve_native_i32_alias(ctx: &FnCtx<'_>, mut id: u32) -> u32 {
    let mut seen = std::collections::HashSet::new();
    while let Some(next) = ctx.native_i32_aliases.get(&id).copied() {
        if !seen.insert(id) {
            break;
        }
        id = next;
    }
    id
}

fn native_i32_alias_chain_mentions(
    aliases: &std::collections::HashMap<u32, u32>,
    alias_id: u32,
    target_id: u32,
) -> bool {
    if alias_id == target_id {
        return true;
    }
    let mut id = alias_id;
    let mut seen = std::collections::HashSet::new();
    while let Some(next) = aliases.get(&id).copied() {
        if next == target_id {
            return true;
        }
        if !seen.insert(id) {
            break;
        }
        id = next;
    }
    false
}

fn native_index_source_local(ctx: &FnCtx<'_>, expr: &Expr) -> Option<u32> {
    match expr {
        Expr::LocalGet(id) => Some(resolve_native_i32_alias(ctx, *id)),
        Expr::Binary {
            op: BinaryOp::BitOr,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Integer(0)) => native_index_source_local(ctx, left),
        Expr::Call { callee, args, .. } if args.len() == 1 => {
            let Expr::FuncRef(fid) = callee.as_ref() else {
                return None;
            };
            if ctx.i32_identity_functions.contains(fid) {
                native_index_source_local(ctx, &args[0])
            } else {
                None
            }
        }
        _ => None,
    }
}

fn f64_to_i64_constant(value: f64) -> Option<i64> {
    if value.is_finite() && value.fract() == 0.0 {
        let min = i64::MIN as f64;
        let max = i64::MAX as f64;
        if value >= min && value <= max {
            return Some(value as i64);
        }
    }
    None
}

fn checked_range_add(lhs: IntRange, rhs: IntRange) -> Option<IntRange> {
    Some(IntRange {
        min: lhs.min.checked_add(rhs.min)?,
        max: lhs.max.checked_add(rhs.max)?,
    })
}

fn checked_range_sub(lhs: IntRange, rhs: IntRange) -> Option<IntRange> {
    Some(IntRange {
        min: lhs.min.checked_sub(rhs.max)?,
        max: lhs.max.checked_sub(rhs.min)?,
    })
}

fn checked_range_mul(lhs: IntRange, rhs: IntRange) -> Option<IntRange> {
    let candidates = [
        lhs.min.checked_mul(rhs.min)?,
        lhs.min.checked_mul(rhs.max)?,
        lhs.max.checked_mul(rhs.min)?,
        lhs.max.checked_mul(rhs.max)?,
    ];
    Some(IntRange {
        min: *candidates.iter().min()?,
        max: *candidates.iter().max()?,
    })
}

fn int_range_for_local(
    ctx: &FnCtx<'_>,
    id: u32,
    seen: &mut std::collections::HashSet<u32>,
) -> Option<IntRange> {
    if let Some(fact) = ctx
        .int_range_facts
        .iter()
        .rev()
        .find(|fact| fact.local_id == id)
    {
        return Some(fact.range);
    }
    if !seen.insert(id) {
        return None;
    }
    let result = if let Some(alias) = ctx.int_range_aliases.get(&id) {
        int_range_expr_inner(ctx, alias, seen)
    } else {
        ctx.compile_time_constants
            .get(&id)
            .and_then(|value| f64_to_i64_constant(*value))
            .map(IntRange::exact)
    };
    seen.remove(&id);
    result
}

fn int_range_expr_inner(
    ctx: &FnCtx<'_>,
    expr: &Expr,
    seen: &mut std::collections::HashSet<u32>,
) -> Option<IntRange> {
    match expr {
        Expr::Integer(n) => Some(IntRange::exact(*n)),
        Expr::Number(n) => f64_to_i64_constant(*n).map(IntRange::exact),
        Expr::LocalGet(id) => int_range_for_local(ctx, *id, seen),
        Expr::Binary { op, left, right } => {
            let lhs = int_range_expr_inner(ctx, left, seen)?;
            let rhs = int_range_expr_inner(ctx, right, seen)?;
            match op {
                BinaryOp::Add => checked_range_add(lhs, rhs),
                BinaryOp::Sub => checked_range_sub(lhs, rhs),
                BinaryOp::Mul => checked_range_mul(lhs, rhs),
                BinaryOp::BitOr if rhs.min == 0 && rhs.max == 0 => {
                    if lhs.min >= i32::MIN as i64 && lhs.max <= i32::MAX as i64 {
                        Some(lhs)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
        Expr::Call { callee, args, .. } if args.len() == 3 => {
            let Expr::FuncRef(fid) = callee.as_ref() else {
                return None;
            };
            if !ctx.clamp3_functions.contains(fid) {
                return None;
            }
            let lo = int_range_expr_inner(ctx, &args[1], seen)?;
            let hi = int_range_expr_inner(ctx, &args[2], seen)?;
            if lo.max <= hi.min {
                Some(IntRange {
                    min: lo.min,
                    max: hi.max,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn int_range_expr(ctx: &FnCtx<'_>, expr: &Expr) -> Option<IntRange> {
    int_range_expr_inner(ctx, expr, &mut std::collections::HashSet::new())
}

fn exact_i64_expr(ctx: &FnCtx<'_>, expr: &Expr) -> Option<i64> {
    let range = int_range_expr(ctx, expr)?;
    (range.min == range.max).then_some(range.min)
}

fn constant_i64_expr(ctx: &FnCtx<'_>, expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Integer(n) => Some(*n),
        Expr::Number(n) => f64_to_i64_constant(*n),
        Expr::LocalGet(id) => ctx
            .compile_time_constants
            .get(id)
            .and_then(|value| f64_to_i64_constant(*value))
            .or_else(|| exact_i64_expr(ctx, expr)),
        Expr::Binary { op, left, right } => {
            let lhs = constant_i64_expr(ctx, left)?;
            let rhs = constant_i64_expr(ctx, right)?;
            match op {
                BinaryOp::Add => lhs.checked_add(rhs),
                BinaryOp::Sub => lhs.checked_sub(rhs),
                BinaryOp::Mul => lhs.checked_mul(rhs),
                BinaryOp::Div if rhs != 0 && lhs % rhs == 0 => Some(lhs / rhs),
                BinaryOp::BitOr => Some(lhs | rhs),
                BinaryOp::BitAnd => Some(lhs & rhs),
                BinaryOp::BitXor => Some(lhs ^ rhs),
                BinaryOp::Shl if (0..63).contains(&rhs) => lhs.checked_shl(rhs as u32),
                BinaryOp::Shr if (0..63).contains(&rhs) => lhs.checked_shr(rhs as u32),
                _ => None,
            }
        }
        _ => None,
    }
}

fn length_source_range(ctx: &FnCtx<'_>, source: &LengthSource) -> Option<IntRange> {
    match source {
        LengthSource::Constant(n) => Some(IntRange::exact(*n)),
        LengthSource::Local { id, addend } => {
            let base = int_range_for_local(ctx, *id, &mut std::collections::HashSet::new())?;
            checked_range_add(base, IntRange::exact(*addend))
        }
        LengthSource::Unknown => None,
    }
}

fn length_source_constant(ctx: &FnCtx<'_>, source: &LengthSource) -> Option<i64> {
    let range = length_source_range(ctx, source)?;
    (range.min == range.max).then_some(range.min)
}

pub(crate) fn record_int_facts_for_let(
    ctx: &mut FnCtx<'_>,
    id: u32,
    init: Option<&Expr>,
    mutable: bool,
) {
    let Some(init_expr) = init else {
        ctx.int_range_aliases.remove(&id);
        ctx.nonnegative_integer_locals.remove(&id);
        return;
    };
    let range = int_range_expr(ctx, init_expr);
    if !mutable && range.is_some() {
        ctx.int_range_aliases.insert(id, init_expr.clone());
    } else {
        ctx.int_range_aliases.remove(&id);
    }
    if range.is_some_and(IntRange::is_nonnegative) {
        ctx.nonnegative_integer_locals.insert(id);
    } else {
        ctx.nonnegative_integer_locals.remove(&id);
    }
}

pub(crate) fn record_int_facts_for_local_set(ctx: &mut FnCtx<'_>, id: u32, value: &Expr) {
    ctx.int_range_aliases.remove(&id);
    let remains_nonnegative = int_range_expr(ctx, value).is_some_and(IntRange::is_nonnegative);
    ctx.int_range_facts.retain(|fact| fact.local_id != id);
    if remains_nonnegative {
        ctx.nonnegative_integer_locals.insert(id);
    } else {
        ctx.nonnegative_integer_locals.remove(&id);
    }
}

pub(crate) fn invalidate_local_write_facts(ctx: &mut FnCtx<'_>, id: u32) {
    let aliases = ctx.native_i32_aliases.clone();
    ctx.native_i32_aliases
        .retain(|alias_id, _| !native_i32_alias_chain_mentions(&aliases, *alias_id, id));

    ctx.min_length_bounds
        .retain(|bound_id, buffer_ids| *bound_id != id && !buffer_ids.contains(&id));

    ctx.bounded_buffer_index_pairs
        .retain(|fact| fact.index_local_id != id && fact.buffer_local_id != id);
    ctx.bounded_index_pairs
        .retain(|fact| fact.index_local_id != id && fact.array_local_id != id);

    for view in ctx.buffer_view_slots.values_mut() {
        if matches!(
            view.length_source.as_ref(),
            Some(LengthSource::Local { id: source_id, .. }) if *source_id == id
        ) {
            view.length_source = Some(LengthSource::Unknown);
        }
    }
}

pub(crate) fn record_int_facts_for_update(ctx: &mut FnCtx<'_>, id: u32, op: UpdateOp) {
    ctx.int_range_aliases.remove(&id);
    let remains_nonnegative = match op {
        UpdateOp::Increment => ctx.nonnegative_integer_locals.contains(&id),
        UpdateOp::Decrement => int_range_for_local(ctx, id, &mut std::collections::HashSet::new())
            .is_some_and(|range| range.min >= 1),
    };
    ctx.int_range_facts.retain(|fact| fact.local_id != id);
    if remains_nonnegative {
        ctx.nonnegative_integer_locals.insert(id);
    } else {
        ctx.nonnegative_integer_locals.remove(&id);
    }
}

fn index_local_with_addend(expr: &Expr) -> Option<(u32, i64)> {
    match expr {
        Expr::LocalGet(id) => Some((*id, 0)),
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(id), Expr::Integer(addend)) => {
                    let addend = if matches!(op, BinaryOp::Sub) {
                        addend.checked_neg()?
                    } else {
                        *addend
                    };
                    Some((*id, addend))
                }
                (Expr::Integer(addend), Expr::LocalGet(id)) if matches!(op, BinaryOp::Add) => {
                    Some((*id, *addend))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

pub(crate) fn while_condition_range_fact(
    ctx: &FnCtx<'_>,
    condition: &Expr,
    scope_id: u32,
) -> Option<IntRangeFact> {
    let Expr::Compare { op, left, right } = condition else {
        return None;
    };
    if !matches!(op, CompareOp::Lt | CompareOp::Le) {
        return None;
    }
    let (local_id, addend) = index_local_with_addend(left)?;
    let upper = exact_i64_expr(ctx, right)?
        .checked_sub(addend)?
        .checked_sub(if matches!(op, CompareOp::Lt) { 1 } else { 0 })?;
    let lower = if let Some(range) =
        int_range_for_local(ctx, local_id, &mut std::collections::HashSet::new())
    {
        range.min
    } else if ctx.nonnegative_integer_locals.contains(&local_id) {
        0
    } else {
        return None;
    };
    if lower <= upper {
        Some(IntRangeFact {
            local_id,
            scope_id,
            range: IntRange {
                min: lower.max(0),
                max: upper,
            },
        })
    } else {
        None
    }
}

pub(crate) fn bounds_for_buffer_access(
    ctx: &FnCtx<'_>,
    buffer_local_id: u32,
    index: &Expr,
) -> BoundsState {
    if let Some(index_local_id) = native_index_source_local(ctx, index) {
        if let Some(bounds) = ctx
            .bounded_buffer_index_pairs
            .iter()
            .rev()
            .find(|fact| {
                fact.index_local_id == index_local_id && fact.buffer_local_id == buffer_local_id
            })
            .map(|fact| fact.bounds.clone())
        {
            return bounds;
        }
    }
    if let Some(index_value) = constant_i64_expr(ctx, index) {
        let Some(view) = ctx.buffer_view_slots.get(&buffer_local_id) else {
            return BoundsState::Unknown;
        };
        let length = view
            .length_source
            .as_ref()
            .and_then(|source| length_source_constant(ctx, source));
        if let Some(length) = length {
            if index_value >= 0 && index_value < length {
                return BoundsState::Proven {
                    proof: BoundsProof::ExplicitGuard,
                };
            }
            return BoundsState::Unknown;
        }
    }
    range_bounds_for_buffer_access(ctx, buffer_local_id, index)
}

fn range_bounds_for_buffer_access(
    ctx: &FnCtx<'_>,
    buffer_local_id: u32,
    index: &Expr,
) -> BoundsState {
    let Some(view) = ctx.buffer_view_slots.get(&buffer_local_id) else {
        return BoundsState::Unknown;
    };
    let Some(index_range) = int_range_expr(ctx, index) else {
        return BoundsState::Unknown;
    };
    let Some(length_range) = view
        .length_source
        .as_ref()
        .and_then(|source| length_source_range(ctx, source))
    else {
        return BoundsState::Unknown;
    };
    if index_range.min >= 0 && index_range.max < length_range.min {
        BoundsState::Proven {
            proof: BoundsProof::LoopGuard,
        }
    } else {
        BoundsState::Unknown
    }
}

pub(crate) fn effective_alias_state_for_access(
    ctx: &FnCtx<'_>,
    view: &BufferViewSlot,
) -> AliasState {
    if !view.alias.allows_noalias() || view.scope_idx.is_none() {
        return view.alias.clone();
    }
    let noalias_candidate_count = ctx
        .buffer_view_slots
        .values()
        .filter(|slot| slot.scope_idx.is_some() && slot.alias.allows_noalias())
        .count();
    if noalias_candidate_count >= 2 {
        view.alias.clone()
    } else {
        AliasState::MayAlias
    }
}
