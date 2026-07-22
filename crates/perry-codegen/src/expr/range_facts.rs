use perry_hir::{BinaryOp, CompareOp, Expr, UpdateOp};

use crate::native_value::{
    layout_decision_for_type, AliasState, BoundsProof, BoundsState, BufferViewSlot,
    GuardedBufferIndex, LengthSource, MaterializationReason, PodLayoutDecision,
};

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

pub(crate) fn local_value_alias_root(ctx: &FnCtx<'_>, mut id: u32) -> u32 {
    let mut seen = std::collections::HashSet::new();
    while let Some(next) = ctx.local_value_aliases.get(&id).copied() {
        if !seen.insert(id) {
            break;
        }
        id = next;
    }
    id
}

pub(crate) fn record_local_value_alias_for_write(ctx: &mut FnCtx<'_>, id: u32, value: &Expr) {
    if let Expr::LocalGet(source_id) = value {
        let root = local_value_alias_root(ctx, *source_id);
        if root != id {
            ctx.local_value_aliases.insert(id, root);
            return;
        }
    }
    ctx.local_value_aliases.remove(&id);
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

fn checked_range_div(lhs: IntRange, rhs: IntRange) -> Option<IntRange> {
    if rhs.min == rhs.max && rhs.min > 0 && lhs.min % rhs.min == 0 && lhs.max % rhs.min == 0 {
        return Some(IntRange {
            min: lhs.min / rhs.min,
            max: lhs.max / rhs.min,
        });
    }
    None
}

/// Smallest all-ones value covering `value` (`255 → 255`, `256 → 511`,
/// `0 → 0`). An upper bound for `a | b` / `a & b` results whose operands are
/// bounded by `value`: OR/AND cannot set a bit above the highest bit of
/// either operand's cover.
fn ones_cover(value: i64) -> i64 {
    debug_assert!(value >= 0);
    if value == 0 {
        return 0;
    }
    ((1u64 << (64 - (value as u64).leading_zeros())) - 1) as i64
}

/// `a & b` where both operands carry non-negative ranges bounded by
/// `i32::MAX`: `ToInt32` of a value in `[0, 2^31)` cannot wrap or change
/// sign, and AND of two non-negative i32 values is bounded by each operand.
fn checked_range_bitand(lhs: IntRange, rhs: IntRange) -> Option<IntRange> {
    if lhs.min >= 0 && rhs.min >= 0 && lhs.max <= i32::MAX as i64 && rhs.max <= i32::MAX as i64 {
        return Some(IntRange {
            min: 0,
            max: lhs.max.min(rhs.max),
        });
    }
    None
}

/// `a | b` where both operands carry non-negative ranges bounded by
/// `i32::MAX`: OR cannot clear bits (so it is at least each operand) and
/// cannot set a bit above either operand's ones-cover.
fn checked_range_bitor(lhs: IntRange, rhs: IntRange) -> Option<IntRange> {
    if lhs.min >= 0 && rhs.min >= 0 && lhs.max <= i32::MAX as i64 && rhs.max <= i32::MAX as i64 {
        return Some(IntRange {
            min: lhs.min.max(rhs.min),
            max: ones_cover(lhs.max) | ones_cover(rhs.max),
        });
    }
    None
}

/// A constant operand of `&` usable as a result mask: a non-negative integer
/// `≤ i32::MAX`, so `ToInt32` leaves it unchanged and its sign bit is clear.
fn bitand_mask_constant(ctx: &FnCtx<'_>, expr: &Expr) -> Option<i64> {
    let mask = constant_i64_expr(ctx, expr)?;
    (0..=i64::from(i32::MAX)).contains(&mask).then_some(mask)
}

/// Value range of an `object[index]` load from a width-tracked integer-element
/// typed array when the index is provably in bounds: the element kind bounds
/// the value (an in-bounds `Int32Array` read is always an i32 integer, a
/// `Uint8Array` read is `[0, 255]`, …). Out of bounds would read `undefined`,
/// so the same static bounds proof the unchecked native load relies on gates
/// the range. The index range is computed through the SAME `seen` set as the
/// enclosing walk so mutually-recursive local aliases keep their cycle
/// breaker.
fn int_typed_array_load_range(
    ctx: &FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    seen: &mut std::collections::HashSet<u32>,
) -> Option<IntRange> {
    use crate::native_value::{BufferElem, BufferIndexUnit, MaterializationReason};
    if ctx.disable_buffer_fast_path {
        return None;
    }
    let Expr::LocalGet(id) = object else {
        return None;
    };
    let view = ctx.buffer_view_slots.get(id)?;
    if view.index_unit != BufferIndexUnit::Element
        || !view.alias.allows_noalias()
        || view.scope_idx.is_none()
    {
        return None;
    }
    if ctx.closure_captures.contains_key(id)
        || matches!(
            ctx.buffer_hazard_reasons.get(id),
            Some(MaterializationReason::ClosureCapture)
        )
    {
        return None;
    }
    let elem_range = match view.elem {
        BufferElem::I8 => IntRange {
            min: -128,
            max: 127,
        },
        BufferElem::U8 | BufferElem::U8Clamped => IntRange { min: 0, max: 255 },
        BufferElem::I16 => IntRange {
            min: -32768,
            max: 32767,
        },
        BufferElem::U16 => IntRange { min: 0, max: 65535 },
        BufferElem::I32 => IntRange {
            min: i32::MIN as i64,
            max: i32::MAX as i64,
        },
        BufferElem::U32 => IntRange {
            min: 0,
            max: u32::MAX as i64,
        },
        BufferElem::F32 | BufferElem::F64 => return None,
    };
    let index_range = int_range_expr_inner(ctx, index, seen)?;
    let length_min = view
        .length_source
        .as_ref()
        .and_then(|source| length_source_range(ctx, source))?
        .min;
    if index_range.min >= 0 && index_range.max < length_min {
        Some(elem_range)
    } else {
        None
    }
}

fn pod_layout_constant_i64(ctx: &FnCtx<'_>, expr: &Expr) -> Option<i64> {
    match expr {
        Expr::PodLayoutSizeOf { ty } => match layout_decision_for_type(ctx, ty) {
            PodLayoutDecision::Layout(layout) => Some(i64::from(layout.size)),
            _ => None,
        },
        Expr::PodLayoutAlignOf { ty } => match layout_decision_for_type(ctx, ty) {
            PodLayoutDecision::Layout(layout) => Some(i64::from(layout.alignment)),
            _ => None,
        },
        Expr::PodLayoutOffsetOf { ty, field_path } => match layout_decision_for_type(ctx, ty) {
            PodLayoutDecision::Layout(layout) => layout
                .fields
                .iter()
                .find(|field| field.path == *field_path)
                .map(|field| i64::from(field.offset)),
            _ => None,
        },
        _ => None,
    }
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
        Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. } => pod_layout_constant_i64(ctx, expr).map(IntRange::exact),
        Expr::LocalGet(id) => int_range_for_local(ctx, *id, seen),
        Expr::IndexGet { object, index } => int_typed_array_load_range(ctx, object, index, seen),
        Expr::Binary { op, left, right } => {
            // Result-shape rules that need no range on one (or either)
            // operand. `e & K` with a non-negative i32 constant `K` is
            // `ToInt32(e) & K ∈ [0, K]` for EVERY `e` (NaN, fractional,
            // negative, non-numeric — `ToInt32` coerces first, the mask
            // bounds last), and `e >>> k` is a `ToUint32` result shifted
            // right, bounded by the shift amount alone. Both results are
            // integral by construction, so they are safe to feed the
            // unchecked buffer-bounds proofs (a fractional index would read
            // a named property, not an element — these ops cannot produce
            // one). This is what lets `S[i & 1023]` / `S[x >>> 24]` /
            // `S[0x100 | ((x >> 16) & 0xff)]` (the bcryptjs Blowfish S-box
            // shapes) prove bounds against a known view length.
            if matches!(op, BinaryOp::BitAnd) {
                if let Some(mask) =
                    bitand_mask_constant(ctx, left).or_else(|| bitand_mask_constant(ctx, right))
                {
                    return Some(IntRange { min: 0, max: mask });
                }
            }
            if matches!(op, BinaryOp::UShr) {
                // JS `>>>` shifts by `ToUint32(rhs) & 31`; any result is a
                // Uint32. A constant shift of `k ∈ [1, 31]` tightens the
                // bound to `2^(32-k) - 1`.
                let max = match constant_i64_expr(ctx, right).map(|k| (k as u64) & 31) {
                    Some(k) if k > 0 => (1i64 << (32 - k)) - 1,
                    _ => i64::from(u32::MAX),
                };
                return Some(IntRange { min: 0, max });
            }
            let lhs = int_range_expr_inner(ctx, left, seen)?;
            let rhs = int_range_expr_inner(ctx, right, seen)?;
            match op {
                BinaryOp::Add => checked_range_add(lhs, rhs),
                BinaryOp::Sub => checked_range_sub(lhs, rhs),
                BinaryOp::Mul => checked_range_mul(lhs, rhs),
                BinaryOp::Div => checked_range_div(lhs, rhs),
                // `| 0` keeps the (possibly negative) operand range exactly.
                BinaryOp::BitOr if rhs.min == 0 && rhs.max == 0 => {
                    if lhs.min >= i32::MIN as i64 && lhs.max <= i32::MAX as i64 {
                        Some(lhs)
                    } else {
                        None
                    }
                }
                BinaryOp::BitOr if lhs.min == 0 && lhs.max == 0 => {
                    if rhs.min >= i32::MIN as i64 && rhs.max <= i32::MAX as i64 {
                        Some(rhs)
                    } else {
                        None
                    }
                }
                BinaryOp::BitOr => checked_range_bitor(lhs, rhs),
                BinaryOp::BitAnd => checked_range_bitand(lhs, rhs),
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
        Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. } => pod_layout_constant_i64(ctx, expr),
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
    // Drop the forward link AND any alias whose chain passes through `id` —
    // a stale `other -> id` link would otherwise resolve `other` to the
    // REASSIGNED `id`'s fresh facts (same chain hygiene as the
    // `native_i32_aliases` retain below).
    let value_aliases = ctx.local_value_aliases.clone();
    ctx.local_value_aliases
        .retain(|alias_id, _| !native_i32_alias_chain_mentions(&value_aliases, *alias_id, id));

    let aliases = ctx.native_i32_aliases.clone();
    ctx.native_i32_aliases
        .retain(|alias_id, _| !native_i32_alias_chain_mentions(&aliases, *alias_id, id));

    ctx.min_length_bounds
        .retain(|bound_id, buffer_ids| *bound_id != id && !buffer_ids.contains(&id));

    ctx.bounded_buffer_index_pairs
        .retain(|fact| fact.index_local_id != id && fact.buffer_local_id != id);
    ctx.guarded_buffer_index_pairs
        .retain(|fact| fact.index_local_id != id && fact.buffer_local_id != id);
    ctx.bounded_index_pairs
        .retain(|fact| fact.index_local_id != id && fact.array_local_id != id);

    let mut stale_length_views = Vec::new();
    let mut owner_reassignment_views = Vec::new();
    for (view_id, view) in ctx.buffer_view_slots.iter_mut() {
        if matches!(
            view.length_source.as_ref(),
            Some(LengthSource::Local { id: source_id, .. }) if *source_id == id
        ) {
            view.length_source = Some(LengthSource::Unknown);
            stale_length_views.push(*view_id);
        }
        if view
            .native_owned
            .as_ref()
            .is_some_and(|native| native.owner_local_id == id)
        {
            view.alias = AliasState::MayAlias;
            view.scope_idx = None;
            if let Some(native) = view.native_owned.as_mut() {
                native.owner_rooted = false;
            }
            owner_reassignment_views.push(*view_id);
        }
    }
    for view_id in stale_length_views {
        ctx.buffer_hazard_reasons
            .insert(view_id, MaterializationReason::StaleViewLength);
    }
    for view_id in owner_reassignment_views {
        ctx.buffer_hazard_reasons
            .insert(view_id, MaterializationReason::MissingOwnerRoot);
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

// #854: width-1 convenience wrapper over bounds_for_buffer_access_width; all
// current callers pass an explicit width, so this seam is unused for now.
#[allow(dead_code)]
pub(crate) fn bounds_for_buffer_access(
    ctx: &FnCtx<'_>,
    buffer_local_id: u32,
    index: &Expr,
) -> BoundsState {
    bounds_for_buffer_access_width(ctx, buffer_local_id, index, 1)
}

pub(crate) fn bounds_for_buffer_access_width(
    ctx: &FnCtx<'_>,
    buffer_local_id: u32,
    index: &Expr,
    bounds_width_units: u32,
) -> BoundsState {
    let bounds_width_units = bounds_width_units.max(1);
    if let Some(index_local_id) = native_index_source_local(ctx, index) {
        if let Some(bounds) = ctx
            .bounded_buffer_index_pairs
            .iter()
            .rev()
            .find(|fact| {
                fact.index_local_id == index_local_id
                    && fact.buffer_local_id == buffer_local_id
                    && fact.bounds_width_units >= bounds_width_units
            })
            .map(|fact| fact.bounds.clone())
        {
            return bounds;
        }
        if let Some(bounds) = ctx
            .guarded_buffer_index_pairs
            .iter()
            .rev()
            .find(|fact| {
                fact.index_local_id == index_local_id
                    && fact.buffer_local_id == buffer_local_id
                    && fact.bounds_width_units >= bounds_width_units
            })
            .map(|fact| BoundsState::Guarded {
                guard_id: fact.guard_id.clone(),
            })
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
            let width = i64::from(bounds_width_units);
            if index_value >= 0
                && index_value
                    .checked_add(width)
                    .is_some_and(|end| end <= length)
            {
                return BoundsState::Proven {
                    proof: BoundsProof::ExplicitGuard,
                };
            }
            return BoundsState::Unknown;
        }
    }
    range_bounds_for_buffer_access(ctx, buffer_local_id, index, bounds_width_units)
}

fn range_bounds_for_buffer_access(
    ctx: &FnCtx<'_>,
    buffer_local_id: u32,
    index: &Expr,
    bounds_width_units: u32,
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
    let width = i64::from(bounds_width_units.max(1));
    if index_range.min >= 0
        && index_range
            .max
            .checked_add(width)
            .is_some_and(|end| end <= length_range.min)
    {
        BoundsState::Proven {
            proof: BoundsProof::LoopGuard,
        }
    } else {
        BoundsState::Unknown
    }
}

pub(crate) fn guarded_buffer_indices_for_condition(
    ctx: &FnCtx<'_>,
    condition: &Expr,
    scope_id: u32,
) -> Vec<GuardedBufferIndex> {
    use perry_hir::{CompareOp, Expr, LogicalOp};
    match condition {
        Expr::Logical {
            op: LogicalOp::And,
            left,
            right,
        } => {
            let mut out = guarded_buffer_indices_for_condition(ctx, left, scope_id);
            out.extend(guarded_buffer_indices_for_condition(ctx, right, scope_id));
            out
        }
        Expr::Compare { op, left, right } => match op {
            CompareOp::Le => guarded_buffer_indices_from_ordered_cmp(
                ctx,
                left,
                right,
                GuardComparison::LessEqual,
                scope_id,
            )
            .into_iter()
            .collect(),
            CompareOp::Lt => guarded_buffer_indices_from_ordered_cmp(
                ctx,
                left,
                right,
                GuardComparison::LessThan,
                scope_id,
            )
            .into_iter()
            .collect(),
            CompareOp::Ge => guarded_buffer_indices_from_ordered_cmp(
                ctx,
                right,
                left,
                GuardComparison::LessEqual,
                scope_id,
            )
            .into_iter()
            .collect(),
            CompareOp::Gt => guarded_buffer_indices_from_ordered_cmp(
                ctx,
                right,
                left,
                GuardComparison::LessThan,
                scope_id,
            )
            .into_iter()
            .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

#[derive(Clone, Copy)]
enum GuardComparison {
    LessEqual,
    LessThan,
}

fn guarded_buffer_indices_from_ordered_cmp(
    ctx: &FnCtx<'_>,
    left: &Expr,
    right: &Expr,
    cmp: GuardComparison,
    scope_id: u32,
) -> Option<GuardedBufferIndex> {
    if let Some((index_local_id, addend)) = index_expr_plus_constant(ctx, left) {
        if let Some(buffer_local_id) = local_buffer_length_expr(right) {
            let width = match cmp {
                GuardComparison::LessEqual => addend,
                GuardComparison::LessThan => addend.checked_add(1)?,
            };
            return guarded_buffer_index(ctx, index_local_id, buffer_local_id, width, scope_id);
        }
    }
    let (buffer_local_id, subtrahend) = local_buffer_length_minus_constant(ctx, right)?;
    let (index_local_id, addend) = index_expr_plus_constant(ctx, left)?;
    let width = match cmp {
        GuardComparison::LessEqual => subtrahend.checked_add(addend)?,
        GuardComparison::LessThan => subtrahend.checked_add(addend)?.checked_add(1)?,
    };
    guarded_buffer_index(ctx, index_local_id, buffer_local_id, width, scope_id)
}

fn guarded_buffer_index(
    ctx: &FnCtx<'_>,
    index_local_id: u32,
    buffer_local_id: u32,
    width: i64,
    scope_id: u32,
) -> Option<GuardedBufferIndex> {
    if width < 1 || width > u32::MAX as i64 {
        return None;
    }
    if !ctx.buffer_view_slots.contains_key(&buffer_local_id) {
        return None;
    }
    let nonnegative = ctx.nonnegative_integer_locals.contains(&index_local_id)
        || int_range_for_local(ctx, index_local_id, &mut std::collections::HashSet::new())
            .is_some_and(|range| range.min >= 0);
    if !nonnegative {
        return None;
    }
    Some(GuardedBufferIndex {
        index_local_id,
        buffer_local_id,
        scope_id,
        bounds_width_units: width as u32,
        guard_id: format!("explicit_guard_width_{}", width),
    })
}

fn index_expr_plus_constant(ctx: &FnCtx<'_>, expr: &Expr) -> Option<(u32, i64)> {
    match expr {
        Expr::LocalGet(id) => Some((resolve_native_i32_alias(ctx, *id), 0)),
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(id), Expr::Integer(addend)) => {
                    let addend = if matches!(op, BinaryOp::Sub) {
                        addend.checked_neg()?
                    } else {
                        *addend
                    };
                    Some((resolve_native_i32_alias(ctx, *id), addend))
                }
                (Expr::Integer(addend), Expr::LocalGet(id)) if matches!(op, BinaryOp::Add) => {
                    Some((resolve_native_i32_alias(ctx, *id), *addend))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn local_buffer_length_expr(expr: &Expr) -> Option<u32> {
    match expr {
        Expr::Uint8ArrayLength(inner) | Expr::BufferLength(inner) => match inner.as_ref() {
            Expr::LocalGet(id) => Some(*id),
            _ => None,
        },
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => match object.as_ref() {
            Expr::LocalGet(id) => Some(*id),
            _ => None,
        },
        _ => None,
    }
}

fn local_buffer_length_minus_constant(ctx: &FnCtx<'_>, expr: &Expr) -> Option<(u32, i64)> {
    match expr {
        Expr::Binary {
            op: BinaryOp::Sub,
            left,
            right,
        } => {
            let id = local_buffer_length_expr(left)?;
            let n = exact_i64_expr(ctx, right)?;
            Some((id, n))
        }
        _ => None,
    }
}

pub(crate) fn effective_alias_state_for_access(
    ctx: &FnCtx<'_>,
    view: &BufferViewSlot,
) -> AliasState {
    if !view.alias.allows_noalias() || view.scope_idx.is_none() {
        return view.alias.clone();
    }
    if view.native_owned.is_some() {
        return if native_owned_view_has_overlapping_alias(ctx, view) {
            AliasState::MayAlias
        } else {
            view.alias.clone()
        };
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

fn native_owned_view_has_overlapping_alias(ctx: &FnCtx<'_>, view: &BufferViewSlot) -> bool {
    let Some(native) = view.native_owned.as_ref() else {
        return false;
    };
    let scope_idx = view.scope_idx;
    ctx.buffer_view_slots.values().any(|other| {
        if other.scope_idx == scope_idx {
            return false;
        }
        let Some(other_native) = other.native_owned.as_ref() else {
            return false;
        };
        other_native.owner_local_id == native.owner_local_id
            && native_owned_ranges_may_overlap(
                native.byte_offset,
                native.byte_length,
                other_native.byte_offset,
                other_native.byte_length,
            )
    })
}

fn native_owned_ranges_may_overlap(
    a_offset: Option<i64>,
    a_len: Option<i64>,
    b_offset: Option<i64>,
    b_len: Option<i64>,
) -> bool {
    let (Some(a_offset), Some(a_len), Some(b_offset), Some(b_len)) =
        (a_offset, a_len, b_offset, b_len)
    else {
        return true;
    };
    if a_len <= 0 || b_len <= 0 {
        return false;
    }
    let a_start = a_offset as i128;
    let a_end = a_start + a_len as i128;
    let b_start = b_offset as i128;
    let b_end = b_start + b_len as i128;
    a_start < b_end && b_start < a_end
}
