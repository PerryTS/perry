//! `Stmt::For`, `Stmt::While`, `Stmt::DoWhile` lowering and supporting helpers.

use super::*;

use crate::expr::{
    array_kind_fact, effect_fact, emit_typed_feedback_register_site, nanbox_pointer_inline,
    raw_f64_layout_fact, BoundedIndexPair, IntRangeFact, PackedF64LoopFact, PackedNumericLoopKind,
    TypedFeedbackContract, TypedFeedbackKind,
};
use crate::loop_purity::body_needs_asm_barrier;
use crate::lower_conditional::lower_truthy;
use crate::native_value::{
    BoundedBufferIndex, BoundsProof, BoundsState, BufferAccessMode, LengthSource, LoweredValue,
    MaterializationReason,
};
use crate::types::{DOUBLE, I1, I32, I64};

#[derive(Clone, Copy)]
enum NumericBulkFillValue {
    Const(f64),
    Iota,
}

struct NumericBulkFillLoop {
    counter_id: u32,
    array_id: u32,
    bound: perry_hir::Expr,
    value: NumericBulkFillValue,
}

#[derive(Clone, Copy)]
struct LengthHoist {
    arr_id: u32,
    counter_id: u32,
    op: perry_hir::CompareOp,
    lhs_addend: i32,
    buffer_bounds_width_units: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoopArrayLengthEffect {
    Preserves,
    AliasLengthMutation,
    ArrayLengthMutation,
    DynamicPropertyWrite,
    UnknownCallEscape,
    AsyncMicrotask,
    AggregateAliasEscape,
    MaterializationHazard,
    Reassignment,
    UnsupportedExpression,
}

impl LoopArrayLengthEffect {
    fn detail(self) -> &'static str {
        match self {
            Self::Preserves => "preserves_array_length",
            Self::AliasLengthMutation => "alias_may_mutate_array_length",
            Self::ArrayLengthMutation => "array_length_may_change",
            Self::DynamicPropertyWrite => "dynamic_property_write",
            Self::UnknownCallEscape => "unknown_call_escape",
            Self::AsyncMicrotask => "async_microtask_escape",
            Self::AggregateAliasEscape => "aggregate_alias_escape",
            Self::MaterializationHazard => "materialization_hazard",
            Self::Reassignment => "tracked_local_reassignment",
            Self::UnsupportedExpression => "unsupported_effect",
        }
    }

    fn materialization_reason(self) -> Option<MaterializationReason> {
        match self {
            Self::Preserves => None,
            Self::AliasLengthMutation | Self::AggregateAliasEscape => {
                Some(MaterializationReason::UnknownAlias)
            }
            Self::MaterializationHazard => Some(MaterializationReason::UnknownAlias),
            Self::DynamicPropertyWrite => Some(MaterializationReason::DynamicPropertyAccess),
            Self::UnknownCallEscape | Self::AsyncMicrotask => {
                Some(MaterializationReason::UnknownCallEscape)
            }
            Self::Reassignment => Some(MaterializationReason::Reassignment),
            Self::ArrayLengthMutation | Self::UnsupportedExpression => {
                Some(MaterializationReason::UnknownBounds)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct LengthHoistRejection {
    arr_id: u32,
    effect: LoopArrayLengthEffect,
}

/// Runtime-guarded i32 specialization for `i < n` loops whose bound `n` is a
/// directly accessible local but not statically proven to be an invariant i32.
/// The guard flag and `fptosi(n)` value are hoisted to stack slots once before
/// the loop; the cond block branches on the flag to choose the `icmp slt i32`
/// fast loop or the generic per-iteration comparison. The `fptosi` is emitted
/// only on a guard-passing block so NaN, infinities, fractional values, and
/// out-of-i32-range values keep JS comparison semantics.
struct DynamicI32Bound {
    op: perry_hir::CompareOp,
    /// `i1` slot: true when the guard proved, at loop entry, that the whole
    /// `icmp` loop stays inside i32 — see [`emit_guarded_i32_bound`].
    flag_slot: String,
    /// `i32` slot holding `fptosi(n)` (valid only when `flag_slot` is true).
    bound_i32_slot: String,
    /// `i32` slot the fast cond block compares against `bound_i32_slot`.
    counter_i32_slot: String,
    /// True when `counter_i32_slot` is loop-private: allocated here and
    /// deliberately NOT published in `ctx.i32_counter_slots`, so the loop body
    /// and the slow cond keep reading the counter's f64 slot (#6072). The
    /// update block bumps it by hand in that case.
    counter_is_private: bool,
}

#[derive(Clone)]
struct PackedF64VersionedLoop {
    counter_id: u32,
    array_id: u32,
    array_kind: PackedNumericLoopKind,
}

fn match_numeric_bulk_fill_loop(
    ctx: &FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Option<NumericBulkFillLoop> {
    let init = init?;
    let (counter_id, init_expr) = match init {
        Stmt::Let { id, init, .. } => (*id, init.as_ref()?),
        _ => return None,
    };
    match init_expr {
        perry_hir::Expr::Integer(0) => {}
        perry_hir::Expr::Number(n) if *n == 0.0 => {}
        _ => return None,
    }
    match update {
        Some(perry_hir::Expr::Update {
            id,
            op: perry_hir::UpdateOp::Increment,
            ..
        }) if *id == counter_id => {}
        _ => return None,
    }
    let bound = match condition? {
        perry_hir::Expr::Compare {
            op: perry_hir::CompareOp::Lt,
            left,
            right,
        } if matches!(left.as_ref(), perry_hir::Expr::LocalGet(id) if *id == counter_id) => {
            right.as_ref().clone()
        }
        _ => return None,
    };
    let (object, index, value) = match body {
        [Stmt::Expr(perry_hir::Expr::IndexSet {
            object,
            index,
            value,
        })] => (object, index, value),
        [Stmt::Expr(perry_hir::Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        })] if matches!(
            (target.as_ref(), receiver.as_ref()),
            (perry_hir::Expr::LocalGet(a), perry_hir::Expr::LocalGet(b)) if a == b
        ) =>
        {
            (target, key, value)
        }
        _ => return None,
    };
    if !matches!(index.as_ref(), perry_hir::Expr::LocalGet(id) if *id == counter_id) {
        return None;
    }
    let array_id = match object.as_ref() {
        perry_hir::Expr::LocalGet(id) => *id,
        _ => return None,
    };
    let is_numeric_array = matches!(
        ctx.local_types.get(&array_id),
        Some(perry_types::Type::Array(elem))
            if matches!(elem.as_ref(), perry_types::Type::Number | perry_types::Type::Int32)
    );
    if !is_numeric_array {
        return None;
    }
    let value = match value.as_ref() {
        perry_hir::Expr::LocalGet(id) if *id == counter_id => NumericBulkFillValue::Iota,
        perry_hir::Expr::Integer(n) => NumericBulkFillValue::Const(*n as f64),
        perry_hir::Expr::Number(n) if n.is_finite() => NumericBulkFillValue::Const(*n),
        _ => return None,
    };
    Some(NumericBulkFillLoop {
        counter_id,
        array_id,
        bound,
        value,
    })
}

fn lower_numeric_bulk_fill_loop(ctx: &mut FnCtx<'_>, matched: NumericBulkFillLoop) -> Result<bool> {
    let arr_box = lower_expr(ctx, &perry_hir::Expr::LocalGet(matched.array_id))?;
    let arr_handle = {
        let blk = ctx.block();
        let arr_bits = blk.bitcast_double_to_i64(&arr_box);
        blk.and(I64, &arr_bits, crate::nanbox::POINTER_MASK_I64)
    };

    let is_len_bound = matches!(
        &matched.bound,
        perry_hir::Expr::PropertyGet { object, property, .. }
            if property == "length"
                && matches!(object.as_ref(), perry_hir::Expr::LocalGet(id) if *id == matched.array_id)
    );
    let (new_arr, bound_i32) = if is_len_bound {
        let bound_i32 = ctx
            .block()
            .call(I32, "js_array_length", &[(I64, &arr_handle)]);
        let new_arr = match matched.value {
            NumericBulkFillValue::Const(value) => {
                let value_lit = crate::nanbox::double_literal(value);
                ctx.block().call(
                    I64,
                    "js_array_fill_f64_const_len_extend",
                    &[(I64, &arr_handle), (DOUBLE, &value_lit)],
                )
            }
            NumericBulkFillValue::Iota => ctx.block().call(
                I64,
                "js_array_fill_f64_iota_len_extend",
                &[(I64, &arr_handle)],
            ),
        };
        (new_arr, bound_i32)
    } else {
        let bound_i32 = match &matched.bound {
            perry_hir::Expr::Integer(n) if *n >= 0 && *n <= u32::MAX as i64 => n.to_string(),
            perry_hir::Expr::Number(n)
                if n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n <= u32::MAX as f64 =>
            {
                (*n as u32).to_string()
            }
            perry_hir::Expr::LocalGet(id) if ctx.integer_locals.contains(id) => {
                let bound_d = lower_expr(ctx, &matched.bound)?;
                let raw_i32 = ctx.block().fptosi(DOUBLE, &bound_d, I32);
                let positive = ctx.block().fcmp("ogt", &bound_d, "0.0");
                ctx.block().select(I1, &positive, I32, &raw_i32, "0")
            }
            _ => return Ok(false),
        };
        let new_arr = match matched.value {
            NumericBulkFillValue::Const(value) => {
                let value_lit = crate::nanbox::double_literal(value);
                ctx.block().call(
                    I64,
                    "js_array_fill_f64_const_extend",
                    &[(I64, &arr_handle), (I32, &bound_i32), (DOUBLE, &value_lit)],
                )
            }
            NumericBulkFillValue::Iota => ctx.block().call(
                I64,
                "js_array_fill_f64_iota_extend",
                &[(I64, &arr_handle), (I32, &bound_i32)],
            ),
        };
        (new_arr, bound_i32)
    };
    let new_box = nanbox_pointer_inline(ctx.block(), &new_arr);
    if let Some(slot) = ctx.locals.get(&matched.array_id).cloned() {
        ctx.block().store(DOUBLE, &new_box, &slot);
    }
    if let Some(counter_slot) = ctx.locals.get(&matched.counter_id).cloned() {
        let bound_d = ctx.block().sitofp(I32, &bound_i32, DOUBLE);
        ctx.block().store(DOUBLE, &bound_d, &counter_slot);
    }
    if let Some(i32_slot) = ctx.i32_counter_slots.get(&matched.counter_id).cloned() {
        ctx.block().store(I32, &bound_i32, &i32_slot);
    }
    Ok(true)
}

fn lower_packed_f64_versioned_for(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Result<bool> {
    let Some(matched) = match_packed_f64_versioned_loop(ctx, init, condition, update, body) else {
        return Ok(false);
    };

    let arr_expr = perry_hir::Expr::LocalGet(matched.array_id);
    let arr_box = lower_expr(ctx, &arr_expr)?;
    let guard_id = match matched.array_kind {
        PackedNumericLoopKind::F64 => "packed_f64_array_loop_guard",
        PackedNumericLoopKind::I32 => "packed_i32_array_loop_guard",
        PackedNumericLoopKind::U32 => "packed_u32_array_loop_guard",
    };
    let feedback_site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::ArrayElement,
        match matched.array_kind {
            PackedNumericLoopKind::F64 => "array[packed_f64_loop]",
            PackedNumericLoopKind::I32 => "array[packed_i32_loop]",
            PackedNumericLoopKind::U32 => "array[packed_u32_loop]",
        },
        match matched.array_kind {
            PackedNumericLoopKind::F64 => TypedFeedbackContract::packed_f64_array_loop(),
            PackedNumericLoopKind::I32 => TypedFeedbackContract::packed_i32_array_loop(),
            PackedNumericLoopKind::U32 => TypedFeedbackContract::packed_u32_array_loop(),
        },
    );
    let guard_ok = {
        let blk = ctx.block();
        let guard_fn = match matched.array_kind {
            PackedNumericLoopKind::F64 => "js_typed_feedback_packed_f64_array_loop_guard",
            PackedNumericLoopKind::I32 => "js_typed_feedback_packed_i32_array_loop_guard",
            PackedNumericLoopKind::U32 => "js_typed_feedback_packed_u32_array_loop_guard",
        };
        let guard_i32 = blk.call(
            I32,
            guard_fn,
            &[(I64, &feedback_site_id), (DOUBLE, &arr_box)],
        );
        blk.icmp_ne(I32, &guard_i32, "0")
    };

    record_packed_f64_loop_guard_artifacts(
        ctx,
        matched.array_id,
        &arr_box,
        guard_id,
        matched.array_kind,
    );

    let loop_label = matched.array_kind.loop_label();
    let fast_pre_idx = ctx.new_block(&format!("{loop_label}.loop.fast.preheader"));
    let slow_pre_idx = ctx.new_block(&format!("{loop_label}.loop.slow.preheader"));
    let merge_idx = ctx.new_block(&format!("{loop_label}.loop.merge"));
    let fast_pre_label = ctx.block_label(fast_pre_idx);
    let slow_pre_label = ctx.block_label(slow_pre_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block()
        .cond_br(&guard_ok, &fast_pre_label, &slow_pre_label);

    let packed_scope_id = ctx.next_loop_proof_scope_id();

    ctx.current_block = fast_pre_idx;
    ctx.packed_f64_loop_facts.push(PackedF64LoopFact {
        index_local_id: matched.counter_id,
        array_local_id: matched.array_id,
        scope_id: packed_scope_id,
        guard_id: guard_id.to_string(),
        store_side_exit_label: slow_pre_label.clone(),
        array_kind: matched.array_kind,
        allow_holes: false,
        window_validated: false,
    });
    lower_for_after_init(
        ctx,
        init,
        condition,
        update,
        body,
        &format!("for.{loop_label}_fast"),
    )?;
    ctx.packed_f64_loop_facts
        .retain(|fact| fact.scope_id != packed_scope_id);
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = slow_pre_idx;
    lower_for_after_init(
        ctx,
        init,
        condition,
        update,
        body,
        &format!("for.{loop_label}_slow"),
    )?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(true)
}

/// #6011: cap on the |constant offset| accepted in `arr[i ± c]` accesses by
/// the range-preguarded packed-f64 loop matcher.
const PACKED_F64_RANGE_LOOP_MAX_OFFSET: i64 = 64;

#[derive(Clone, Copy)]
enum PackedF64RangeLoopBound {
    /// `i < <integer literal>`.
    Constant(i64),
    /// `i < b` where `b` is a loop-invariant plain local or module global.
    Local(u32),
}

#[derive(Clone, Copy)]
struct PackedF64RangeArrayAccess {
    array_id: u32,
    /// Counter-relative accesses: smallest / largest constant offset `c` over
    /// all `arr[i ± c]` accesses.
    counter: Option<(i32, i32)>,
    /// Merged static index windows `(lo, hi)` over masked accesses
    /// (`arr[e & K]`, `arr[K1 + (e >>> k & K2)]`, … — see
    /// `collectors::static_index_window`). Dense mode only.
    stat: Option<(i64, i64)>,
    written: bool,
}

struct PackedF64RangeLoop {
    counter_id: u32,
    /// Loop-entry counter value (`let i = <start>`), proven in `0..=i32::MAX`.
    start: i64,
    bound: PackedF64RangeLoopBound,
    /// Per-array access windows, ordered by array local id (deterministic).
    arrays: Vec<PackedF64RangeArrayAccess>,
    /// True for the read-only masked-index mode: the body may hold several
    /// scalar statements and statically-windowed (`e & K`-shaped) reads, the
    /// entry guard is the DENSE variant (window must be hole-free), and the
    /// fast loop's loads carry no hole check and no side exit (a
    /// mid-iteration side exit could double-apply earlier statement effects
    /// on re-execution).
    dense: bool,
}

/// #6011: range-preguarded packed-f64 versioned loop.
///
/// Matches `for (let i = k0; i < B; i++) <single statement>` where `B` is an
/// integer literal or a loop-invariant local/module-global, and every array
/// access in the body is `a[i]` / `a[i ± c]` (|c| ≤ 64) on eligible
/// number-array locals. Unlike [`match_packed_f64_versioned_loop`] the bound
/// is NOT `arr.length`, so bounds cannot be proven per-array statically —
/// instead a runtime guard validates the whole static index window
/// `[k0 + min_offset, B + max_offset)` against each array's length at loop
/// entry (hole-tolerantly: `new Array(n)` slots start as TAG_HOLE).
///
/// The body is restricted to ONE statement whose only side effect (a tracked
/// array store, or a scalar `LocalSet`/`Update`) completes after every
/// potential side exit (hole-checked loads / the store's numeric-RHS check),
/// so a side exit into the slow loop re-executes the current iteration
/// without duplicating effects.
fn match_packed_f64_range_loop(
    ctx: &FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Option<PackedF64RangeLoop> {
    use perry_hir::{CompareOp, Expr, UpdateOp};
    if !ctx.pending_labels.is_empty() {
        return None;
    }
    let (counter_id, start) = match init? {
        Stmt::Let {
            id,
            init: Some(init_expr),
            ..
        } => {
            let start = match init_expr {
                Expr::Integer(n) => *n,
                Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => *n as i64,
                _ => return None,
            };
            (*id, start)
        }
        _ => return None,
    };
    if !(0..=i64::from(i32::MAX)).contains(&start) {
        return None;
    }
    let (op, left, right) = match condition? {
        Expr::Compare { op, left, right } => (*op, left.as_ref(), right.as_ref()),
        _ => return None,
    };
    if !matches!(op, CompareOp::Lt) || !matches!(left, Expr::LocalGet(id) if *id == counter_id) {
        return None;
    }
    let bound = match right {
        // Cap constants at i32::MAX - 64 so `bound + max_offset` cannot
        // overflow the guard's i32 argument.
        Expr::Integer(k)
            if (0..=i64::from(i32::MAX) - PACKED_F64_RANGE_LOOP_MAX_OFFSET).contains(k) =>
        {
            PackedF64RangeLoopBound::Constant(*k)
        }
        Expr::LocalGet(bound_id) if *bound_id != counter_id => {
            // Boxed bounds live behind a closure cell the once-per-entry load
            // below does not model. Plain locals AND module globals are fine:
            // the body walk rejects every call/await/closure, so nothing can
            // mutate the global mid-loop, and direct writes to `bound_id` in
            // cond/update/body are rejected by the invariance walker.
            if ctx.boxed_vars.contains(bound_id) {
                return None;
            }
            if !ctx.locals.contains_key(bound_id) && !ctx.module_globals.contains_key(bound_id) {
                return None;
            }
            if !local_bound_is_loop_invariant(condition?, update, body, *bound_id) {
                return None;
            }
            PackedF64RangeLoopBound::Local(*bound_id)
        }
        _ => return None,
    };
    if !matches!(
        update?,
        Expr::Update {
            id,
            op: UpdateOp::Increment,
            ..
        } if *id == counter_id
    ) {
        return None;
    }
    if !ctx.locals.contains_key(&counter_id)
        || ctx.boxed_vars.contains(&counter_id)
        || !ctx.integer_locals.contains(&counter_id)
        || !loop_counter_bounds_are_safe(ctx, counter_id, update, body)
        || !loop_counter_entry_i32_range_is_safe(init, counter_id)
    {
        return None;
    }

    let bound_local = match bound {
        PackedF64RangeLoopBound::Local(b) => Some(b),
        PackedF64RangeLoopBound::Constant(_) => None,
    };
    let mut accesses: std::collections::BTreeMap<u32, PackedF64RangeArrayAccess> =
        std::collections::BTreeMap::new();
    let dense = if packed_f64_range_loop_body_collect(body, counter_id, bound_local, &mut accesses)
    {
        false
    } else {
        // The classic shape (one statement, counter-offset indices, stores
        // allowed, hole-tolerant with side exits) didn't match. Try the
        // read-only DENSE mode: several scalar statements, masked
        // statically-windowed indices, no stores, no side exits.
        accesses.clear();
        if !packed_f64_range_loop_dense_body_collect(body, counter_id, bound_local, &mut accesses) {
            return None;
        }
        true
    };
    if accesses.is_empty() {
        // No tracked array access — nothing for the versioned loop to win.
        return None;
    }
    for access in accesses.values() {
        let arr_id = access.array_id;
        // Written arrays keep the full fact-graph eligibility (below). Reads
        // only need a declared number-array binding in addressable storage:
        // the range guard re-validates the ACTUAL runtime array — plain-array
        // shape, raw-f64 packedness, frozen/descriptor/prototype state, and
        // the whole index window — at loop entry, and the matched body admits
        // no store/call/closure/await, so nothing can reshape the array (even
        // through an alias) between the guard and the last iteration. In
        // particular this must NOT consult the materialization-hazard /
        // array-kind facts: `mark_unknown_call_escape` blanket-hazards every
        // function-local tracked array when the function contains ANY call
        // (e.g. a `console.log` after the loop), which would keep every
        // locally-built lookup table (`const S: number[] = new Array(1024)`
        // + fill loop — the Blowfish S-box shape) off the fast path forever.
        // A wrong static hint costs one failed guard → slow loop, never
        // correctness.
        if access.written {
            if !packed_loop_array_binding_is_eligible(ctx, arr_id) {
                return None;
            }
        } else if !packed_loop_array_binding_storage_is_addressable(ctx, arr_id)
            || ctx.scalar_replaced_arrays.contains_key(&arr_id)
        {
            return None;
        }
        // The guard takes i32 window endpoints; make sure `start + offset`
        // still fits (bound-side overflow is prevented by the constant cap /
        // runtime bound range check).
        if let Some((min_offset, max_offset)) = access.counter {
            let min_idx = start + i64::from(min_offset);
            let max_base = start + i64::from(max_offset);
            if !(i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&min_idx)
                || !(i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&max_base)
            {
                return None;
            }
        }
        if let Some((lo, hi)) = access.stat {
            // `hi + 1` must fit the guard's i32 `max_idx_exclusive` argument.
            if lo < 0 || hi >= i64::from(i32::MAX) {
                return None;
            }
        }
        if access.counter.is_none() && access.stat.is_none() {
            return None;
        }
        if access.written {
            if !local_allows_packed_f64_loop_store(ctx, arr_id)
                || !ctx
                    .native_facts
                    .packed_f64_eligible_for_guarded_store(arr_id)
            {
                return None;
            }
        } else if !local_is_number_array(ctx, arr_id) {
            return None;
        }
    }
    Some(PackedF64RangeLoop {
        counter_id,
        start,
        bound,
        arrays: accesses.into_values().collect(),
        dense,
    })
}

fn record_packed_f64_range_access(
    accesses: &mut std::collections::BTreeMap<u32, PackedF64RangeArrayAccess>,
    array_id: u32,
    offset: i32,
    written: bool,
) {
    let entry = accesses
        .entry(array_id)
        .or_insert(PackedF64RangeArrayAccess {
            array_id,
            counter: None,
            stat: None,
            written,
        });
    entry.counter = Some(match entry.counter {
        None => (offset, offset),
        Some((min, max)) => (min.min(offset), max.max(offset)),
    });
    entry.written |= written;
}

fn record_packed_f64_range_static_access(
    accesses: &mut std::collections::BTreeMap<u32, PackedF64RangeArrayAccess>,
    array_id: u32,
    lo: i64,
    hi: i64,
) {
    let entry = accesses
        .entry(array_id)
        .or_insert(PackedF64RangeArrayAccess {
            array_id,
            counter: None,
            stat: None,
            written: false,
        });
    entry.stat = Some(match entry.stat {
        None => (lo, hi),
        Some((cur_lo, cur_hi)) => (cur_lo.min(lo), cur_hi.max(hi)),
    });
}

/// `i` → 0, `i + c` / `c + i` → c, `i - c` → -c, with |result| ≤ 64.
fn packed_f64_range_loop_index_offset(index: &perry_hir::Expr, counter_id: u32) -> Option<i32> {
    use perry_hir::{BinaryOp, Expr};
    let offset = match index {
        Expr::LocalGet(id) if *id == counter_id => Some(0i64),
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(id), Expr::Integer(c)) if *id == counter_id => {
                    if matches!(op, BinaryOp::Sub) {
                        c.checked_neg()
                    } else {
                        Some(*c)
                    }
                }
                (Expr::Integer(c), Expr::LocalGet(id))
                    if *id == counter_id && matches!(op, BinaryOp::Add) =>
                {
                    Some(*c)
                }
                _ => None,
            }
        }
        _ => None,
    }?;
    if offset.unsigned_abs() > PACKED_F64_RANGE_LOOP_MAX_OFFSET as u64 {
        return None;
    }
    i32::try_from(offset).ok()
}

/// Body walk for [`match_packed_f64_range_loop`]: exactly one expression
/// statement whose single side effect happens after all potential side exits.
fn packed_f64_range_loop_body_collect(
    body: &[Stmt],
    counter_id: u32,
    bound_local: Option<u32>,
    accesses: &mut std::collections::BTreeMap<u32, PackedF64RangeArrayAccess>,
) -> bool {
    use perry_hir::Expr;
    let [Stmt::Expr(expr)] = body else {
        return false;
    };
    match expr {
        Expr::IndexSet {
            object,
            index,
            value,
        } => packed_f64_range_loop_store_collect(object, index, value, counter_id, accesses),
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } if matches!(
            (target.as_ref(), receiver.as_ref()),
            (Expr::LocalGet(a), Expr::LocalGet(b)) if a == b
        ) =>
        {
            packed_f64_range_loop_store_collect(target, key, value, counter_id, accesses)
        }
        // Scalar accumulator: `sum = <pure>` / `sum += a[i]`. The LocalSet
        // completes after its RHS fully evaluates, so a hole-read side exit
        // in the RHS re-executes the iteration without a double-update. The
        // counter/bound are already proven unwritten by the walkers above;
        // the "target is not a tracked array" half is validated by the
        // caller once the access map is complete.
        Expr::LocalSet(id, value) => {
            *id != counter_id
                && Some(*id) != bound_local
                && packed_f64_range_loop_pure_expr_collect(value, counter_id, false, accesses)
                && !accesses.contains_key(id)
        }
        _ => packed_f64_range_loop_pure_expr_collect(expr, counter_id, false, accesses),
    }
}

/// #6011: module globals READ (and provably never written — the matched
/// body's only possible write target is the top-level `LocalSet`, which the
/// caller passes as `written_local`) inside the matched loop body. The
/// versioned lowering caches these into entry stack slots so LLVM can keep
/// them in registers: the raw inttoptr element stores in the fast loop
/// otherwise force a re-load of every `@perry_global_*` each iteration.
fn packed_f64_range_loop_invariant_global_reads(
    ctx: &FnCtx<'_>,
    body: &[Stmt],
    written_local: Option<u32>,
) -> Vec<u32> {
    use perry_hir::Expr;
    let [Stmt::Expr(expr)] = body else {
        return Vec::new();
    };
    let mut globals = std::collections::BTreeSet::new();
    fn walk(
        ctx: &FnCtx<'_>,
        expr: &perry_hir::Expr,
        written_local: Option<u32>,
        globals: &mut std::collections::BTreeSet<u32>,
    ) {
        if let Expr::LocalGet(id) = expr {
            if Some(*id) != written_local
                && !ctx.locals.contains_key(id)
                && ctx.module_globals.contains_key(id)
            {
                globals.insert(*id);
            }
        }
        perry_hir::walker::walk_expr_children(expr, &mut |child| {
            walk(ctx, child, written_local, globals);
        });
    }
    walk(ctx, expr, written_local, &mut globals);
    globals.into_iter().collect()
}

fn packed_f64_range_loop_store_collect(
    object: &perry_hir::Expr,
    index: &perry_hir::Expr,
    value: &perry_hir::Expr,
    counter_id: u32,
    accesses: &mut std::collections::BTreeMap<u32, PackedF64RangeArrayAccess>,
) -> bool {
    use perry_hir::Expr;
    let Expr::LocalGet(arr_id) = object else {
        return false;
    };
    let Some(offset) = packed_f64_range_loop_index_offset(index, counter_id) else {
        return false;
    };
    if !packed_f64_range_loop_pure_expr_collect(value, counter_id, false, accesses) {
        return false;
    }
    record_packed_f64_range_access(accesses, *arr_id, offset, true);
    true
}

/// Body walk for the read-only DENSE range-loop mode: any number of scalar
/// statements — `const a = <pure>` / `sum = <pure>` / `n++` / bare pure
/// expressions — where every tracked array access is a READ with a
/// counter-offset or statically-windowed index. No store to a tracked array,
/// no call/closure/await, and the written scalars must be disjoint from the
/// tracked arrays, the counter, and the bound. Because the fast loop's loads
/// have no side exits, multi-statement bodies are safe: an iteration either
/// runs entirely in the fast copy or entirely in the slow copy.
fn packed_f64_range_loop_dense_body_collect(
    body: &[Stmt],
    counter_id: u32,
    bound_local: Option<u32>,
    accesses: &mut std::collections::BTreeMap<u32, PackedF64RangeArrayAccess>,
) -> bool {
    use perry_hir::Expr;
    let mut written: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for stmt in body {
        match stmt {
            Stmt::Let {
                id,
                init: Some(init),
                ..
            } => {
                if !packed_f64_range_loop_pure_expr_collect(init, counter_id, true, accesses) {
                    return false;
                }
                written.insert(*id);
            }
            Stmt::Let { id, init: None, .. } => {
                written.insert(*id);
            }
            Stmt::Expr(Expr::LocalSet(id, value)) => {
                if *id == counter_id || Some(*id) == bound_local {
                    return false;
                }
                if !packed_f64_range_loop_pure_expr_collect(value, counter_id, true, accesses) {
                    return false;
                }
                written.insert(*id);
            }
            Stmt::Expr(Expr::Update { id, .. }) => {
                if *id == counter_id || Some(*id) == bound_local {
                    return false;
                }
                written.insert(*id);
            }
            Stmt::Expr(expr) => {
                if !packed_f64_range_loop_pure_expr_collect(expr, counter_id, true, accesses) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    !accesses.is_empty()
        && accesses.values().all(|access| !access.written)
        && accesses.keys().all(|arr_id| !written.contains(arr_id))
}

/// Effect-free expression walk: tracked `a[i ± c]` reads, locals, literals and
/// pure arithmetic/Math only. Any store, call, update, closure, or index read
/// with an unrecognized receiver/index shape bails the whole match.
/// `allow_static` (dense mode) additionally admits reads whose index carries a
/// static value window (`a[e & K]`, `a[K1 + (e >>> k & K2)]`, …).
fn packed_f64_range_loop_pure_expr_collect(
    expr: &perry_hir::Expr,
    counter_id: u32,
    allow_static: bool,
    accesses: &mut std::collections::BTreeMap<u32, PackedF64RangeArrayAccess>,
) -> bool {
    use perry_hir::Expr;
    match expr {
        Expr::IndexGet { object, index } => {
            let Expr::LocalGet(arr_id) = object.as_ref() else {
                return false;
            };
            if let Some(offset) = packed_f64_range_loop_index_offset(index, counter_id) {
                record_packed_f64_range_access(accesses, *arr_id, offset, false);
                return true;
            }
            if !allow_static {
                return false;
            }
            let Some((lo, hi)) = crate::collectors::static_index_window(index) else {
                return false;
            };
            if lo < 0 || hi >= i64::from(i32::MAX) {
                return false;
            }
            // The index may nest further tracked reads — walk it too.
            if !packed_f64_range_loop_pure_expr_collect(index, counter_id, allow_static, accesses) {
                return false;
            }
            record_packed_f64_range_static_access(accesses, *arr_id, lo, hi);
            true
        }
        Expr::LocalGet(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Undefined => true,
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            packed_f64_range_loop_pure_expr_collect(left, counter_id, allow_static, accesses)
                && packed_f64_range_loop_pure_expr_collect(
                    right,
                    counter_id,
                    allow_static,
                    accesses,
                )
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::NumberCoerce(operand)
        | Expr::BooleanCoerce(operand) => {
            packed_f64_range_loop_pure_expr_collect(operand, counter_id, allow_static, accesses)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            packed_f64_range_loop_pure_expr_collect(condition, counter_id, allow_static, accesses)
                && packed_f64_range_loop_pure_expr_collect(
                    then_expr,
                    counter_id,
                    allow_static,
                    accesses,
                )
                && packed_f64_range_loop_pure_expr_collect(
                    else_expr,
                    counter_id,
                    allow_static,
                    accesses,
                )
        }
        Expr::MathImul(left, right) | Expr::MathPow(left, right) => {
            packed_f64_range_loop_pure_expr_collect(left, counter_id, allow_static, accesses)
                && packed_f64_range_loop_pure_expr_collect(
                    right,
                    counter_id,
                    allow_static,
                    accesses,
                )
        }
        Expr::MathMin(values) | Expr::MathMax(values) => values.iter().all(|expr| {
            packed_f64_range_loop_pure_expr_collect(expr, counter_id, allow_static, accesses)
        }),
        Expr::MathAbs(value)
        | Expr::MathSqrt(value)
        | Expr::MathFloor(value)
        | Expr::MathCeil(value)
        | Expr::MathRound(value)
        | Expr::MathTrunc(value)
        | Expr::MathSign(value)
        | Expr::MathF16round(value) => {
            packed_f64_range_loop_pure_expr_collect(value, counter_id, allow_static, accesses)
        }
        _ => false,
    }
}

/// #6011: lowering for [`match_packed_f64_range_loop`], modeled on
/// [`lower_packed_f64_versioned_for`]. The bound is materialized to i32 once
/// (with a runtime finite-integral check for local/global bounds), one range
/// guard runs per accessed array, and the AND of the guards picks the fast
/// loop (hole-tolerant `PackedF64LoopFact` per array; side exits resume at
/// the current `i` in the slow copy) or the slow loop.
/// Emit one range-guard call per accessed array (window endpoints merged
/// from the counter part `[start + min_offset, bound + max_offset)` and the
/// static part `[lo, hi]`), AND-reduced into a single i1.
fn emit_packed_f64_range_guards(
    ctx: &mut FnCtx<'_>,
    matched: &PackedF64RangeLoop,
    bound_i32: &str,
    guard_fn: &str,
    guard_id: &str,
) -> Result<String> {
    let mut all_guards_ok: Option<String> = None;
    for access in &matched.arrays {
        let arr_box = lower_expr(ctx, &perry_hir::Expr::LocalGet(access.array_id))?;
        let feedback_site_id = emit_typed_feedback_register_site(
            ctx,
            TypedFeedbackKind::ArrayElement,
            "array[packed_f64_range_loop]",
            TypedFeedbackContract::packed_f64_array_loop(),
        );
        let (min_idx, max_idx): (String, String) = match (access.counter, access.stat) {
            (Some((min_off, max_off)), None) => (
                (matched.start + i64::from(min_off)).to_string(),
                ctx.block().add(I32, bound_i32, &max_off.to_string()),
            ),
            (None, Some((lo, hi))) => (lo.to_string(), (hi + 1).to_string()),
            (Some((min_off, max_off)), Some((lo, hi))) => {
                let min_c = (matched.start + i64::from(min_off)).min(lo).to_string();
                let counter_max = ctx.block().add(I32, bound_i32, &max_off.to_string());
                let static_max = (hi + 1).to_string();
                let counter_wins = ctx.block().icmp_sgt(I32, &counter_max, &static_max);
                let max_r = ctx.block().select(
                    crate::types::I1,
                    &counter_wins,
                    I32,
                    &counter_max,
                    &static_max,
                );
                (min_c, max_r)
            }
            (None, None) => unreachable!("range-loop access with no window"),
        };
        let guard_i32 = ctx.block().call(
            I32,
            guard_fn,
            &[
                (I64, &feedback_site_id),
                (DOUBLE, &arr_box),
                (I32, &min_idx),
                (I32, &max_idx),
            ],
        );
        let guard_ok = ctx.block().icmp_ne(I32, &guard_i32, "0");
        all_guards_ok = Some(match all_guards_ok {
            None => guard_ok,
            Some(prev) => ctx.block().and(I1, &prev, &guard_ok),
        });
        record_packed_f64_loop_guard_artifacts(
            ctx,
            access.array_id,
            &arr_box,
            guard_id,
            PackedNumericLoopKind::F64,
        );
    }
    Ok(all_guards_ok.expect("range loop matcher requires >= 1 array"))
}

/// Push the per-array facts for one fast-loop copy: counter accesses get a
/// `PackedF64LoopFact` (hole-tolerant only in the classic non-dense mode),
/// masked accesses get a `MaskedWindowArrayFact` (`values_i32` selects the
/// i32-tier load lowering).
fn push_packed_f64_range_facts(
    ctx: &mut FnCtx<'_>,
    matched: &PackedF64RangeLoop,
    scope_id: u32,
    guard_id: &str,
    slow_pre_label: &str,
    values_i32: bool,
) {
    for access in &matched.arrays {
        if access.counter.is_some() {
            ctx.packed_f64_loop_facts.push(PackedF64LoopFact {
                index_local_id: matched.counter_id,
                array_local_id: access.array_id,
                scope_id,
                guard_id: guard_id.to_string(),
                store_side_exit_label: slow_pre_label.to_string(),
                array_kind: PackedNumericLoopKind::F64,
                // Dense mode proved the window hole-free — loads need no
                // hole check / side exit. Classic range mode stays
                // hole-tolerant.
                allow_holes: !matched.dense,
                window_validated: true,
            });
        }
        if let Some((lo, hi)) = access.stat {
            ctx.masked_window_array_facts
                .push(crate::expr::MaskedWindowArrayFact {
                    array_local_id: access.array_id,
                    scope_id,
                    guard_id: guard_id.to_string(),
                    min_idx: lo,
                    max_idx_exclusive: hi + 1,
                    values_i32,
                });
        }
    }
}

fn lower_packed_f64_range_versioned_for(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Result<bool> {
    let Some(matched) = match_packed_f64_range_loop(ctx, init, condition, update, body) else {
        return Ok(false);
    };
    // The inline load/store fast paths read the counter through its i32
    // shadow slot; without one the versioned copy would win nothing.
    let mut counter_i32_was_fresh = false;
    if !ctx.i32_counter_slots.contains_key(&matched.counter_id) {
        // The Let site only allocates the shadow for *directly* index-used
        // locals; a masked index (`S[i & 1023]`) hides the counter from that
        // analysis. With a CONSTANT bound the counter provably stays in i32
        // range (the matcher caps constants at `i32::MAX - 64`), so allocate
        // the parallel slot here — mirroring the `i < n` local-bound path in
        // `lower_for`. Runtime local bounds keep requiring a pre-existing
        // slot (their range is only proven inside this lowering, after the
        // slot would already be live).
        if !matches!(matched.bound, PackedF64RangeLoopBound::Constant(_))
            || !ctx.integer_locals.contains(&matched.counter_id)
        {
            return Ok(false);
        }
        let Some(counter_slot) = ctx.locals.get(&matched.counter_id).cloned() else {
            return Ok(false);
        };
        let i32_slot = ctx.func.alloca_entry(I32);
        let cur_dbl = ctx.block().load(DOUBLE, &counter_slot);
        let cur_i32 = ctx.block().fptosi(DOUBLE, &cur_dbl, I32);
        ctx.block().store(I32, &cur_i32, &i32_slot);
        ctx.i32_counter_slots.insert(matched.counter_id, i32_slot);
        counter_i32_was_fresh = true;
    }

    // Cache loop-invariant module-global reads (e.g. `alpha` in the EMA
    // recurrence) into entry stack slots and alias them into `ctx.locals`
    // for the duration of both loop copies. The matched body cannot write
    // them (its only writable target is the top-level LocalSet, which is
    // excluded) and contains no calls/closures/awaits, so the cached value
    // is exact for the whole loop — and, unlike a `@perry_global_*` load,
    // a non-escaping alloca is promotable to a register even with the fast
    // loop's raw inttoptr element stores in the way.
    let written_local = match body {
        [Stmt::Expr(perry_hir::Expr::LocalSet(id, _))] => Some(*id),
        _ => None,
    };
    let mut global_override_ids: Vec<u32> = Vec::new();
    for gid in packed_f64_range_loop_invariant_global_reads(ctx, body, written_local) {
        let Some(global_name) = ctx.module_globals.get(&gid).cloned() else {
            continue;
        };
        let slot = ctx.func.alloca_entry(DOUBLE);
        let g_ref = format!("@{global_name}");
        let val = ctx.block().load(DOUBLE, &g_ref);
        ctx.block().store(DOUBLE, &val, &slot);
        ctx.locals.insert(gid, slot);
        global_override_ids.push(gid);
    }

    let fast_pre_idx = ctx.new_block("packed_f64_range.loop.fast.preheader");
    let slow_pre_idx = ctx.new_block("packed_f64_range.loop.slow.preheader");
    let merge_idx = ctx.new_block("packed_f64_range.loop.merge");
    let fast_pre_label = ctx.block_label(fast_pre_idx);
    let slow_pre_label = ctx.block_label(slow_pre_idx);
    let merge_label = ctx.block_label(merge_idx);

    let bound_i32: String = match matched.bound {
        PackedF64RangeLoopBound::Constant(k) => k.to_string(),
        PackedF64RangeLoopBound::Local(bound_id) => {
            // One-time finite-integral-i32 materialization of the bound.
            // Non-number / NaN / fractional / out-of-range bounds keep full
            // JS trip-count semantics in the slow loop. The upper cap leaves
            // room for `bound + max_offset` in i32. The fptosi lives in its
            // own guarded block so its result is never poison when used.
            let bound_d = lower_expr(ctx, &perry_hir::Expr::LocalGet(bound_id))?;
            let is_number = emit_js_value_is_number(ctx, &bound_d);
            let range_idx = ctx.new_block("packed_f64_range.bound.range");
            let convert_idx = ctx.new_block("packed_f64_range.bound.convert");
            let guards_idx = ctx.new_block("packed_f64_range.guards");
            let range_label = ctx.block_label(range_idx);
            let convert_label = ctx.block_label(convert_idx);
            let guards_label = ctx.block_label(guards_idx);
            ctx.block()
                .cond_br(&is_number, &range_label, &slow_pre_label);

            ctx.current_block = range_idx;
            let ge_zero = ctx.block().fcmp("oge", &bound_d, "0.0");
            let le_max = {
                let max_literal = format!(
                    "{:.1}",
                    (i64::from(i32::MAX) - PACKED_F64_RANGE_LOOP_MAX_OFFSET) as f64
                );
                ctx.block().fcmp("ole", &bound_d, &max_literal)
            };
            let in_range = ctx.block().and(I1, &ge_zero, &le_max);
            ctx.block()
                .cond_br(&in_range, &convert_label, &slow_pre_label);

            ctx.current_block = convert_idx;
            let bound_i32 = ctx.block().fptosi(DOUBLE, &bound_d, I32);
            let roundtrip = ctx.block().sitofp(I32, &bound_i32, DOUBLE);
            let is_integral = ctx.block().fcmp("oeq", &roundtrip, &bound_d);
            ctx.block()
                .cond_br(&is_integral, &guards_label, &slow_pre_label);

            ctx.current_block = guards_idx;
            bound_i32
        }
    };

    if matched.dense {
        // Read-only dense mode: two guard tiers. The i32 tier additionally
        // proves every window value is an i32-representable integer, so its
        // fast copy materializes loads with a bare exact `fptosi` (bit-mixing
        // chains stay in integer registers); the f64 tier keeps raw-double
        // loads for float lookup tables. Either failing falls through.
        let try_f64_idx = ctx.new_block("packed_f64_range.dense.try_f64");
        let try_f64_label = ctx.block_label(try_f64_idx);
        let fast_i32_pre_idx = ctx.new_block("packed_f64_range.loop.fast_i32.preheader");
        let fast_i32_pre_label = ctx.block_label(fast_i32_pre_idx);

        let ok_i32 = emit_packed_f64_range_guards(
            ctx,
            &matched,
            &bound_i32,
            "js_typed_feedback_packed_f64_range_loop_guard_dense_i32",
            "packed_f64_range_loop_guard_dense_i32",
        )?;
        ctx.block()
            .cond_br(&ok_i32, &fast_i32_pre_label, &try_f64_label);

        ctx.current_block = try_f64_idx;
        let ok_f64 = emit_packed_f64_range_guards(
            ctx,
            &matched,
            &bound_i32,
            "js_typed_feedback_packed_f64_range_loop_guard_dense",
            "packed_f64_range_loop_guard_dense",
        )?;
        ctx.block()
            .cond_br(&ok_f64, &fast_pre_label, &slow_pre_label);

        ctx.current_block = fast_i32_pre_idx;
        let scope_i32 = ctx.next_loop_proof_scope_id();
        push_packed_f64_range_facts(
            ctx,
            &matched,
            scope_i32,
            "packed_f64_range_loop_guard_dense_i32",
            &slow_pre_label,
            true,
        );
        lower_for_after_init_with_i32_bound(
            ctx,
            init,
            condition,
            update,
            body,
            "for.packed_f64_range_fast_i32",
            Some((matched.counter_id, bound_i32.clone())),
        )?;
        ctx.packed_f64_loop_facts
            .retain(|fact| fact.scope_id != scope_i32);
        ctx.masked_window_array_facts
            .retain(|fact| fact.scope_id != scope_i32);
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }

        ctx.current_block = fast_pre_idx;
        let scope_f64 = ctx.next_loop_proof_scope_id();
        push_packed_f64_range_facts(
            ctx,
            &matched,
            scope_f64,
            "packed_f64_range_loop_guard_dense",
            &slow_pre_label,
            false,
        );
        lower_for_after_init_with_i32_bound(
            ctx,
            init,
            condition,
            update,
            body,
            "for.packed_f64_range_fast",
            Some((matched.counter_id, bound_i32.clone())),
        )?;
        ctx.packed_f64_loop_facts
            .retain(|fact| fact.scope_id != scope_f64);
        ctx.masked_window_array_facts
            .retain(|fact| fact.scope_id != scope_f64);
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }
    } else {
        let all_guards_ok = emit_packed_f64_range_guards(
            ctx,
            &matched,
            &bound_i32,
            "js_typed_feedback_packed_f64_range_loop_guard",
            "packed_f64_range_loop_guard",
        )?;
        ctx.block()
            .cond_br(&all_guards_ok, &fast_pre_label, &slow_pre_label);

        let packed_scope_id = ctx.next_loop_proof_scope_id();

        ctx.current_block = fast_pre_idx;
        push_packed_f64_range_facts(
            ctx,
            &matched,
            packed_scope_id,
            "packed_f64_range_loop_guard",
            &slow_pre_label,
            false,
        );
        lower_for_after_init_with_i32_bound(
            ctx,
            init,
            condition,
            update,
            body,
            "for.packed_f64_range_fast",
            Some((matched.counter_id, bound_i32.clone())),
        )?;
        ctx.packed_f64_loop_facts
            .retain(|fact| fact.scope_id != packed_scope_id);
        ctx.masked_window_array_facts
            .retain(|fact| fact.scope_id != packed_scope_id);
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }
    }

    ctx.current_block = slow_pre_idx;
    lower_for_after_init(
        ctx,
        init,
        condition,
        update,
        body,
        "for.packed_f64_range_slow",
    )?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    for gid in &global_override_ids {
        ctx.locals.remove(gid);
    }
    if counter_i32_was_fresh {
        ctx.i32_counter_slots.remove(&matched.counter_id);
    }
    ctx.current_block = merge_idx;
    Ok(true)
}

/// #5093: property names with dedicated branches in the property-get/set
/// lowering dispatch ahead of the class-field diamond (`length` header loads,
/// `errors` runtime call, accessor-ish names, …). A tracked field must not
/// collide or the fast clone's access would lower through a different —
/// possibly calling — path, breaking the call-free guarantee.
const CLASS_FIELD_LOOP_PROP_DENYLIST: &[&str] = &[
    "length",
    "errors",
    "size",
    "prototype",
    "constructor",
    "__proto__",
    "caller",
    "arguments",
    "name",
    "message",
    "stack",
    "toString",
    "valueOf",
];

/// #5093: class names with dedicated (builtin-flavored) branches in the
/// property lowering dispatch; a user class sharing one of these names could
/// be intercepted before the class-field diamond.
const CLASS_FIELD_LOOP_CLASS_DENYLIST: &[&str] = &[
    "Headers",
    "URLPattern",
    "ClientRequest",
    "Agent",
    "Socket",
    "Server",
    "BlockList",
    "ReadableStream",
    "ReadableStreamDefaultReader",
    "WritableStream",
    "WritableStreamDefaultWriter",
    "URL",
    "URLSearchParams",
    "Function",
];

#[derive(Clone, Copy)]
enum ClassFieldLoopBound {
    /// `i < <integer literal>`.
    Constant(i64),
    /// `i < b` where `b` is a loop-invariant plain local or module global.
    Local(u32),
}

struct ClassFieldVersionedLoop {
    counter_id: u32,
    bound: ClassFieldLoopBound,
    recv_id: u32,
    class_name: String,
    expected_class_id: u32,
    keys_global_name: String,
    /// property -> (packed slot index, written). All raw-f64 candidates.
    fields: std::collections::BTreeMap<String, (u32, bool)>,
}

/// #5093: effect-free expression walk for the class-field versioned loop.
/// Tracked `recv.prop` reads, numeric locals, numeric literals and pure
/// arithmetic/Math only — the same shapes `packed_f64_range_loop_pure_expr_
/// collect` admits, minus array accesses, plus class-field reads. Everything
/// here must lower without emitting a call that can allocate (libm intrinsic
/// calls are fine: they cannot trigger a GC).
fn class_field_loop_pure_expr_collect(
    ctx: &FnCtx<'_>,
    expr: &perry_hir::Expr,
    counter_id: u32,
    recv: &mut Option<u32>,
    props: &mut std::collections::BTreeMap<String, bool>,
) -> bool {
    use perry_hir::Expr;
    match expr {
        Expr::PropertyGet {
            object, property, ..
        } => {
            let Expr::LocalGet(obj_id) = object.as_ref() else {
                return false;
            };
            if *obj_id == counter_id {
                return false;
            }
            match recv {
                Some(r) if *r == *obj_id => {}
                Some(_) => return false, // single receiver per loop
                None => *recv = Some(*obj_id),
            }
            props.entry(property.clone()).or_insert(false);
            true
        }
        // Reading the receiver as a VALUE (outside a tracked field access)
        // could flow it into arbitrary lowering; only allow scalar reads the
        // type analysis proves numeric.
        Expr::LocalGet(id) => {
            recv.map_or(true, |r| r != *id) && crate::type_analysis::is_numeric_expr(ctx, expr)
        }
        Expr::Number(_) | Expr::Integer(_) => true,
        Expr::Binary { left, right, .. } => {
            crate::type_analysis::is_numeric_expr(ctx, expr)
                && class_field_loop_pure_expr_collect(ctx, left, counter_id, recv, props)
                && class_field_loop_pure_expr_collect(ctx, right, counter_id, recv, props)
        }
        Expr::NumberCoerce(operand) => {
            class_field_loop_pure_expr_collect(ctx, operand, counter_id, recv, props)
        }
        Expr::MathImul(left, right) | Expr::MathPow(left, right) => {
            class_field_loop_pure_expr_collect(ctx, left, counter_id, recv, props)
                && class_field_loop_pure_expr_collect(ctx, right, counter_id, recv, props)
        }
        Expr::MathMin(values) | Expr::MathMax(values) => values
            .iter()
            .all(|expr| class_field_loop_pure_expr_collect(ctx, expr, counter_id, recv, props)),
        Expr::MathAbs(value)
        | Expr::MathSqrt(value)
        | Expr::MathFloor(value)
        | Expr::MathCeil(value)
        | Expr::MathRound(value)
        | Expr::MathTrunc(value)
        | Expr::MathSign(value)
        | Expr::MathF16round(value) => {
            class_field_loop_pure_expr_collect(ctx, value, counter_id, recv, props)
        }
        _ => false,
    }
}

/// #5093: class-field versioned loop — the "collapse" this issue tracks.
///
/// Matches `for (let i = k0; i < B; i++) <single statement>` where `B` is an
/// integer literal or a loop-invariant local/module-global and the statement's
/// only side effect is a raw-f64 class-field store on a loop-invariant
/// receiver of statically known class (or a scalar `LocalSet` accumulator),
/// with every other subexpression pure per the walker above.
///
/// The single-statement / effect-last restriction is the side-exit protocol
/// (same as the #6011 range loop): the fast clone's only mid-loop bail is the
/// store's inline plain-finite value check, which fires BEFORE the store — so
/// jumping to the slow clone's preheader re-executes the current iteration
/// without duplicating any effect.
fn match_class_field_versioned_loop(
    ctx: &FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Option<ClassFieldVersionedLoop> {
    use perry_hir::{CompareOp, Expr, UpdateOp};
    // Oversized modules full-outline the class-field diamonds for code size;
    // keep the versioned clone (which would re-inline them) off there.
    if crate::codegen::full_outline_ic_enabled() {
        return None;
    }
    if !ctx.pending_labels.is_empty() {
        return None;
    }
    let (counter_id, start) = match init? {
        Stmt::Let {
            id,
            init: Some(init_expr),
            ..
        } => {
            let start = match init_expr {
                Expr::Integer(n) => *n,
                Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => *n as i64,
                _ => return None,
            };
            (*id, start)
        }
        _ => return None,
    };
    if !(0..=i64::from(i32::MAX)).contains(&start) {
        return None;
    }
    let (op, left, right) = match condition? {
        Expr::Compare { op, left, right } => (*op, left.as_ref(), right.as_ref()),
        _ => return None,
    };
    if !matches!(op, CompareOp::Lt) || !matches!(left, Expr::LocalGet(id) if *id == counter_id) {
        return None;
    }
    let bound = match right {
        Expr::Integer(k) if (0..=i64::from(i32::MAX)).contains(k) => {
            ClassFieldLoopBound::Constant(*k)
        }
        Expr::LocalGet(bound_id) if *bound_id != counter_id => {
            if ctx.boxed_vars.contains(bound_id) {
                return None;
            }
            if !ctx.locals.contains_key(bound_id) && !ctx.module_globals.contains_key(bound_id) {
                return None;
            }
            if !local_bound_is_loop_invariant(condition?, update, body, *bound_id) {
                return None;
            }
            ClassFieldLoopBound::Local(*bound_id)
        }
        _ => return None,
    };
    if !matches!(
        update?,
        Expr::Update {
            id,
            op: UpdateOp::Increment,
            ..
        } if *id == counter_id
    ) {
        return None;
    }
    if !ctx.locals.contains_key(&counter_id)
        || ctx.boxed_vars.contains(&counter_id)
        || !ctx.integer_locals.contains(&counter_id)
        || !loop_counter_bounds_are_safe(ctx, counter_id, update, body)
        || !loop_counter_entry_i32_range_is_safe(init, counter_id)
    {
        return None;
    }

    // Single-statement body whose only side effect commits after every
    // potential side exit.
    let [Stmt::Expr(effect)] = body else {
        return None;
    };
    let mut recv: Option<u32> = None;
    let mut props: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
    match effect {
        // `recv.prop = <pure numeric>` — the benchmark shape. Lowering
        // rewrites the static-key PutValueSet through the PropertySet
        // class-field diamond (`put_value_static_property_fast_path`).
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            let (Expr::LocalGet(t), Expr::LocalGet(r)) = (target.as_ref(), receiver.as_ref())
            else {
                return None;
            };
            if t != r {
                return None;
            }
            let Expr::String(prop) = key.as_ref() else {
                return None;
            };
            recv = Some(*t);
            if !class_field_loop_pure_expr_collect(ctx, value, counter_id, &mut recv, &mut props) {
                return None;
            }
            props
                .entry(prop.clone())
                .and_modify(|written| *written = true)
                .or_insert(true);
        }
        Expr::PropertySet {
            object,
            property,
            value,
        } => {
            let Expr::LocalGet(obj_id) = object.as_ref() else {
                return None;
            };
            recv = Some(*obj_id);
            if !class_field_loop_pure_expr_collect(ctx, value, counter_id, &mut recv, &mut props) {
                return None;
            }
            props
                .entry(property.clone())
                .and_modify(|written| *written = true)
                .or_insert(true);
        }
        // Scalar accumulator: `acc = <pure numeric over tracked reads>`. No
        // store side exit exists, so re-execution can never happen; the
        // LocalSet itself must still target a plain numeric non-shadow local.
        Expr::LocalSet(id, value) => {
            if *id == counter_id
                || !ctx.locals.contains_key(id)
                || ctx.boxed_vars.contains(id)
                || ctx.module_globals.contains_key(id)
                || ctx.shadow_slot_map.contains_key(id)
                || !crate::type_analysis::is_numeric_expr(ctx, &Expr::LocalGet(*id))
            {
                return None;
            }
            if !class_field_loop_pure_expr_collect(ctx, value, counter_id, &mut recv, &mut props) {
                return None;
            }
            if recv == Some(*id) {
                return None;
            }
            if let ClassFieldLoopBound::Local(bound_id) = bound {
                if bound_id == *id {
                    return None;
                }
            }
        }
        _ => return None,
    }
    let recv_id = recv?;
    if props.is_empty() || recv_id == counter_id {
        return None;
    }
    if let ClassFieldLoopBound::Local(bound_id) = bound {
        if bound_id == recv_id {
            return None;
        }
    }

    // Receiver: loop-invariant, directly addressable, not aliased by another
    // representation (POD / scalar replacement take different lowering paths).
    if ctx.boxed_vars.contains(&recv_id)
        || ctx.pod_records.contains_key(&recv_id)
        || ctx.scalar_replaced.contains_key(&recv_id)
    {
        return None;
    }
    if !ctx.locals.contains_key(&recv_id) && !ctx.module_globals.contains_key(&recv_id) {
        return None;
    }
    if !local_bound_is_loop_invariant(condition?, update, body, recv_id) {
        return None;
    }
    let class_name =
        crate::type_analysis::receiver_class_name(ctx, &perry_hir::Expr::LocalGet(recv_id))?;
    if CLASS_FIELD_LOOP_CLASS_DENYLIST.contains(&class_name.as_str()) {
        return None;
    }
    let class = ctx.classes.get(&class_name)?;
    if !class.computed_members.is_empty() {
        return None;
    }
    let expected_class_id = *ctx.class_ids.get(&class_name)?;
    let keys_global_name = ctx.class_keys_globals.get(&class_name)?.clone();

    let mut fields = std::collections::BTreeMap::new();
    for (prop, written) in props {
        if CLASS_FIELD_LOOP_PROP_DENYLIST.contains(&prop.as_str()) {
            return None;
        }
        // Accessors route through synthesized __get_/__set_ methods before
        // the class-field diamond; `class_field_global_index` also rejects
        // accessor-shadowed names, but mirror the dispatch gate exactly.
        if ctx
            .methods
            .contains_key(&(class_name.clone(), format!("__get_{prop}")))
            || ctx
                .methods
                .contains_key(&(class_name.clone(), format!("__set_{prop}")))
        {
            return None;
        }
        let field_index = crate::type_analysis::class_field_global_index(ctx, &class_name, &prop)?;
        let raw_f64 = crate::type_analysis::class_field_declared_type(ctx, &class_name, &prop)
            .as_ref()
            .is_some_and(crate::typed_shape::type_is_raw_f64_candidate);
        if !raw_f64 {
            return None;
        }
        fields.insert(prop, (field_index, written));
    }

    Some(ClassFieldVersionedLoop {
        counter_id,
        bound,
        recv_id,
        class_name,
        expected_class_id,
        keys_global_name,
        fields,
    })
}

/// #5093: lowering for [`match_class_field_versioned_loop`], modeled on
/// [`lower_packed_f64_range_versioned_for`]. The bound is materialized to i32
/// once (with a finite-integral check for local/global bounds), the inline
/// class-field shape check runs once in the preheader, and the fast clone
/// lowers with a scoped [`crate::expr::ClassFieldLoopFact`] so every tracked
/// field access is a bare GEP load/store on the preheader-cached object
/// pointer. Store side exits resume at the current `i` in the slow clone.
///
/// SAFETY (memory-corruption class — see #5093): between the preheader's
/// receiver load and the end of the fast clone, NO call may be emitted. The
/// matcher enforces this by shape (single pure-arithmetic statement, all
/// field accesses tracked, counter/bound machinery call-free); the preheader
/// itself emits only bit ops, loads, and the finite-integral bound checks.
/// Call-free ⇒ allocation-free ⇒ no GC ⇒ the object cannot move and none of
/// the checked shape facts can change while the fast clone runs.
fn lower_class_field_versioned_for(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Result<bool> {
    let Some(matched) = match_class_field_versioned_loop(ctx, init, condition, update, body) else {
        return Ok(false);
    };
    // The fast clone's cond reads the counter through its i32 slot; without
    // one the versioned copy would win nothing.
    if !ctx.i32_counter_slots.contains_key(&matched.counter_id) {
        return Ok(false);
    }

    let fast_pre_idx = ctx.new_block("class_field.loop.fast.preheader");
    let slow_pre_idx = ctx.new_block("class_field.loop.slow.preheader");
    let merge_idx = ctx.new_block("class_field.loop.merge");
    let fast_pre_label = ctx.block_label(fast_pre_idx);
    let slow_pre_label = ctx.block_label(slow_pre_idx);
    let merge_label = ctx.block_label(merge_idx);

    // One-time i32 materialization of the bound (mirrors the #6011 range
    // loop): non-number / NaN / fractional / out-of-range bounds keep full JS
    // trip-count semantics in the slow clone.
    let bound_i32: String = match matched.bound {
        ClassFieldLoopBound::Constant(k) => k.to_string(),
        ClassFieldLoopBound::Local(bound_id) => {
            let bound_d = lower_expr(ctx, &perry_hir::Expr::LocalGet(bound_id))?;
            let is_number = emit_js_value_is_number(ctx, &bound_d);
            let range_idx = ctx.new_block("class_field.loop.bound.range");
            let convert_idx = ctx.new_block("class_field.loop.bound.convert");
            let check_idx = ctx.new_block("class_field.loop.shape_check");
            let range_label = ctx.block_label(range_idx);
            let convert_label = ctx.block_label(convert_idx);
            let check_label = ctx.block_label(check_idx);
            ctx.block()
                .cond_br(&is_number, &range_label, &slow_pre_label);

            ctx.current_block = range_idx;
            let ge_zero = ctx.block().fcmp("oge", &bound_d, "0.0");
            let le_max = {
                let max_literal = format!("{:.1}", i32::MAX as f64);
                ctx.block().fcmp("ole", &bound_d, &max_literal)
            };
            let in_range = ctx.block().and(I1, &ge_zero, &le_max);
            ctx.block()
                .cond_br(&in_range, &convert_label, &slow_pre_label);

            ctx.current_block = convert_idx;
            let bound_i32 = ctx.block().fptosi(DOUBLE, &bound_d, I32);
            let roundtrip = ctx.block().sitofp(I32, &bound_i32, DOUBLE);
            let is_integral = ctx.block().fcmp("oeq", &roundtrip, &bound_d);
            ctx.block()
                .cond_br(&is_integral, &check_label, &slow_pre_label);

            ctx.current_block = check_idx;
            bound_i32
        }
    };

    // Receiver load + hoisted shape check. From here to loop entry the
    // emitted IR is call-free, so the pointer the check validates is the
    // pointer the fast clone uses.
    let recv_box = lower_expr(ctx, &perry_hir::Expr::LocalGet(matched.recv_id))?;
    let (obj_bits, obj_handle, expected_keys) = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(&recv_box);
        let obj_handle = blk.and(I64, &obj_bits, crate::nanbox::POINTER_MASK_I64);
        let expected_keys = blk.load(I64, &format!("@{}", matched.keys_global_name));
        (obj_bits, obj_handle, expected_keys)
    };
    let max_field_index = matched
        .fields
        .values()
        .map(|(field_index, _)| *field_index)
        .max()
        .expect("matcher requires >= 1 tracked field");
    let has_store = matched.fields.values().any(|(_, written)| *written);
    let expected_class_id_str = matched.expected_class_id.to_string();
    let (obj_ptr, shape_ok) =
        crate::expr::class_field_inline_guard::emit_class_field_loop_preheader_check(
            ctx,
            &obj_bits,
            &obj_handle,
            &expected_class_id_str,
            &expected_keys,
            max_field_index,
            // Every tracked field is a raw-f64 candidate: reads rely on the
            // intact bit, so require it whether or not the loop stores.
            true,
            has_store,
            &slow_pre_label,
        );
    // The deref block is left unterminated on purpose: it branches into the
    // fast clone only after the clone is PROVEN call-free below.
    let deref_idx = ctx.current_block;

    let scope_id = ctx.next_loop_proof_scope_id();
    let fast_scan_start = ctx.func.num_blocks();
    ctx.current_block = fast_pre_idx;
    ctx.class_field_loop_facts
        .push(crate::expr::ClassFieldLoopFact {
            recv_local_id: matched.recv_id,
            scope_id,
            class_name: matched.class_name.clone(),
            obj_ptr,
            side_exit_label: slow_pre_label.clone(),
            fields: matched
                .fields
                .iter()
                .map(|(prop, (field_index, _))| (prop.clone(), *field_index))
                .collect(),
        });
    lower_for_after_init_with_i32_bound(
        ctx,
        init,
        condition,
        update,
        body,
        "for.class_field_fast",
        Some((matched.counter_id, bound_i32)),
    )?;
    ctx.class_field_loop_facts
        .retain(|fact| fact.scope_id != scope_id);
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }
    let fast_scan_end = ctx.func.num_blocks();

    // Compile-time verification of the safety invariant: the fast clone must
    // be call-free (no runtime call ⇒ no allocation ⇒ no GC ⇒ the cached
    // `obj_ptr` cannot move and the hoisted shape check stays true). The
    // matcher makes this true by construction; if some unpredicted lowering
    // path emitted a call anyway, never enter the fast clone — run the slow
    // clone unconditionally and leave the fast blocks as unreachable code.
    let fast_clone_call_free = !ctx.func.blocks()[fast_pre_idx].contains_gc_unsafe_call()
        && (fast_scan_start..fast_scan_end)
            .all(|idx| !ctx.func.blocks()[idx].contains_gc_unsafe_call());
    ctx.current_block = deref_idx;
    if fast_clone_call_free {
        ctx.block()
            .cond_br(&shape_ok, &fast_pre_label, &slow_pre_label);
    } else {
        ctx.block().br(&slow_pre_label);
    }

    ctx.current_block = slow_pre_idx;
    lower_for_after_init(ctx, init, condition, update, body, "for.class_field_slow")?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(true)
}

fn record_packed_f64_loop_guard_artifacts(
    ctx: &mut FnCtx<'_>,
    arr_id: u32,
    arr_box: &str,
    guard_id: &str,
    array_kind: PackedNumericLoopKind,
) {
    let guarded_arr = LoweredValue::js_value(arr_box.to_string());
    ctx.record_lowered_value_with_access_mode_and_facts(
        array_kind.guard_expr_kind(),
        Some(arr_id),
        array_kind.guard_consumer(),
        &guarded_arr,
        Some(BoundsState::Guarded {
            guard_id: guard_id.to_string(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        vec![
            array_kind_fact(
                Some(arr_id),
                "consumed",
                array_kind.array_kind_label(),
                None,
            ),
            raw_f64_layout_fact(Some(arr_id), "consumed", guard_id, None),
        ],
        Vec::new(),
        false,
        false,
        vec![
            format!("loop_versioning={}", array_kind.loop_label()),
            "index_range=nonnegative_i32".to_string(),
            "length_range=guarded_i32".to_string(),
            "storage_layout=raw_f64_numeric_slots".to_string(),
        ],
    );

    let fallback_arr = LoweredValue::js_value(arr_box.to_string());
    ctx.record_lowered_value_with_access_mode_and_facts(
        array_kind.guard_expr_kind(),
        Some(arr_id),
        array_kind.fallback_consumer(),
        &fallback_arr,
        Some(BoundsState::Unknown),
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(MaterializationReason::RuntimeApi),
        None,
        None,
        Vec::new(),
        vec![
            array_kind_fact(
                Some(arr_id),
                "rejected",
                array_kind.array_kind_label(),
                Some(MaterializationReason::RuntimeApi),
            ),
            raw_f64_layout_fact(
                Some(arr_id),
                "rejected",
                guard_id,
                Some(MaterializationReason::RuntimeApi),
            ),
            raw_f64_layout_fact(
                Some(arr_id),
                "invalidated",
                "runtime_api",
                Some(MaterializationReason::RuntimeApi),
            ),
        ],
        false,
        false,
        vec![format!(
            "loop_versioning={}_fallback",
            array_kind.loop_label()
        )],
    );
}

fn record_loop_array_length_effect(
    ctx: &mut FnCtx<'_>,
    arr_id: u32,
    effect: LoopArrayLengthEffect,
    consumed: bool,
) {
    let lowered = LoweredValue::js_value("0.0");
    let fact = effect_fact(
        Some(arr_id),
        if consumed { "consumed" } else { "rejected" },
        effect.detail(),
        effect.materialization_reason(),
    );
    let mut consumed_facts = Vec::new();
    let mut rejected_facts = Vec::new();
    if consumed {
        consumed_facts.push(fact);
    } else {
        rejected_facts.push(fact);
    }
    ctx.record_lowered_value_with_access_mode_and_facts(
        "LoopArrayLengthEffect",
        Some(arr_id),
        "loop_array_length_effect",
        &lowered,
        None,
        None,
        None,
        None,
        None,
        None,
        consumed_facts,
        rejected_facts,
        false,
        false,
        vec![
            format!("loop_length_effect={}", effect.detail()),
            format!(
                "loop_length_proof={}",
                if consumed { "accepted" } else { "rejected" }
            ),
        ],
    );
}

fn match_packed_f64_versioned_loop(
    ctx: &FnCtx<'_>,
    init: Option<&perry_hir::Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Option<PackedF64VersionedLoop> {
    if !ctx.pending_labels.is_empty() {
        return None;
    }
    let hoist = condition.and_then(|cond| classify_for_length_hoist(ctx, cond, update, body))?;
    if !matches!(hoist.op, perry_hir::CompareOp::Lt) || hoist.lhs_addend != 0 {
        return None;
    }
    if !ctx.integer_locals.contains(&hoist.counter_id)
        || !loop_counter_bounds_are_safe(ctx, hoist.counter_id, update, body)
        || !loop_counter_entry_i32_range_is_safe(init, hoist.counter_id)
    {
        return None;
    }
    if !packed_loop_array_binding_is_eligible(ctx, hoist.arr_id) {
        return None;
    }
    let store_array_kind =
        supported_packed_numeric_loop_store_kind(ctx, body, hoist.arr_id, hoist.counter_id);
    let array_kind = if let Some(store_array_kind) = store_array_kind {
        if !ctx.native_facts.proves_noalias_array(hoist.arr_id) {
            return None;
        }
        store_array_kind
    } else if ctx.native_facts.proves_packed_i32_array(hoist.arr_id)
        && local_is_int32_array(ctx, hoist.arr_id)
    {
        PackedNumericLoopKind::I32
    } else if ctx.native_facts.proves_packed_u32_array(hoist.arr_id)
        && local_is_u32_array(ctx, hoist.arr_id)
    {
        PackedNumericLoopKind::U32
    } else if ctx.native_facts.proves_packed_f64_array(hoist.arr_id) {
        PackedNumericLoopKind::F64
    } else {
        return None;
    };
    if !local_is_number_array(ctx, hoist.arr_id) {
        return None;
    }
    let body_is_supported = store_array_kind.is_some()
        || body
            .iter()
            .all(|stmt| stmt_is_packed_f64_loop_safe(ctx, stmt, hoist.arr_id, hoist.counter_id));
    if !body_is_supported {
        return None;
    }
    Some(PackedF64VersionedLoop {
        counter_id: hoist.counter_id,
        array_id: hoist.arr_id,
        array_kind,
    })
}

/// #6011: element type of an array-typed local, accepting BOTH the
/// `Type::Array(elem)` spelling (`prices: number[]`) and the generic spelling
/// `Type::Generic { base: "Array", type_args: [elem] }` that `new
/// Array<number>(n)` declarations carry.
fn local_array_element_type<'t>(
    ctx: &'t FnCtx<'_>,
    local_id: u32,
) -> Option<&'t perry_types::Type> {
    match ctx.local_types.get(&local_id) {
        Some(perry_types::Type::Array(elem)) => Some(elem.as_ref()),
        Some(perry_types::Type::Generic { base, type_args })
            if base == "Array" && type_args.len() == 1 =>
        {
            Some(&type_args[0])
        }
        _ => None,
    }
}

/// #6369: which *bindings* a packed-numeric loop may version on.
///
/// The lowered fast loop reads the array box out of the binding once per
/// iteration and then works on raw element slots, so the binding must be one
/// whose read is a plain load of the array value:
///
/// - a stack local (`ctx.locals`) — the original case; or
/// - a module-scope global (`@perry_global_*`) — the shape a bundle is made of
///   (`const rows: number[] = […]` at module scope, read from a function or an
///   arrow closure). Its read is a `load double, ptr @perry_global_*`, and the
///   matched loop body admits no call / `await` / closure, so nothing can rebind
///   the global or reshape the array between the entry guard and the last
///   iteration. Before this, a captured array was rejected here and fell to the
///   per-element guarded path (or, with no declared type reaching the body at
///   all, to fully generic `js_dyn_index_get`) — 27× slower than the identical
///   array passed as a parameter.
///
/// Still rejected: a BOXED stack slot (it holds a box pointer, not the array), a
/// closure-capture slot (its read is a `js_closure_get_capture_*` call, which the
/// raw-slot fast loop cannot host), a scalar-replaced array, and anything the
/// fact graph flagged with a materialization hazard.
///
/// The storage test mirrors `Expr::LocalGet`'s own precedence (capture slot →
/// box slot → alloca → module global) exactly, which is what makes the
/// module-global arm safe from the boxed set: `compile_closure` seeds
/// `ctx.boxed_vars` with the module-wide boxed UNION, so a module global that is
/// boxed *in some other scope* shows up as boxed here — while its read in this
/// body is still a plain `@perry_global_*` load, because the box slot arm needs
/// an alloca (`ctx.locals`) this body does not have. Reading the flag without
/// that distinction is what kept a captured `const rows: number[]` off the fast
/// loop in a closure while the same code in a plain function got it.
fn packed_loop_array_binding_is_eligible(ctx: &FnCtx<'_>, arr_id: u32) -> bool {
    packed_loop_array_binding_storage_is_addressable(ctx, arr_id)
        && !ctx.scalar_replaced_arrays.contains_key(&arr_id)
        && !ctx.native_facts.has_materialization_hazard(arr_id)
}

/// The storage half of [`packed_loop_array_binding_is_eligible`]: the binding
/// read is a plain load (stack alloca or `@perry_global_*`), not a capture
/// slot or box.
fn packed_loop_array_binding_storage_is_addressable(ctx: &FnCtx<'_>, arr_id: u32) -> bool {
    if ctx.closure_captures.contains_key(&arr_id) {
        false
    } else if ctx.locals.contains_key(&arr_id) {
        !ctx.boxed_vars.contains(&arr_id)
    } else {
        ctx.module_globals.contains_key(&arr_id)
    }
}

fn local_is_number_array(ctx: &FnCtx<'_>, local_id: u32) -> bool {
    matches!(
        local_array_element_type(ctx, local_id),
        Some(perry_types::Type::Number | perry_types::Type::Int32)
    ) || matches!(
        local_array_element_type(ctx, local_id),
        Some(perry_types::Type::Named(name)) if name == "PerryU32"
    )
}

fn local_allows_packed_f64_loop_store(ctx: &FnCtx<'_>, local_id: u32) -> bool {
    matches!(
        local_array_element_type(ctx, local_id),
        Some(perry_types::Type::Number)
    )
}

fn local_is_int32_array(ctx: &FnCtx<'_>, local_id: u32) -> bool {
    matches!(
        local_array_element_type(ctx, local_id),
        Some(perry_types::Type::Int32)
    )
}

fn local_is_u32_array(ctx: &FnCtx<'_>, local_id: u32) -> bool {
    matches!(
        local_array_element_type(ctx, local_id),
        Some(perry_types::Type::Named(name)) if name == "PerryU32"
    )
}

fn stmt_is_packed_f64_loop_safe(
    ctx: &FnCtx<'_>,
    stmt: &Stmt,
    arr_id: u32,
    counter_id: u32,
) -> bool {
    match stmt {
        Stmt::Expr(expr) => expr_is_packed_f64_loop_safe(ctx, expr, arr_id, counter_id),
        Stmt::Let { init, .. } => init
            .as_ref()
            .is_none_or(|expr| expr_is_packed_f64_loop_safe(ctx, expr, arr_id, counter_id)),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_is_packed_f64_loop_safe(ctx, condition, arr_id, counter_id)
                && then_branch
                    .iter()
                    .all(|stmt| stmt_is_packed_f64_loop_safe(ctx, stmt, arr_id, counter_id))
                && else_branch.as_ref().is_none_or(|branch| {
                    branch
                        .iter()
                        .all(|stmt| stmt_is_packed_f64_loop_safe(ctx, stmt, arr_id, counter_id))
                })
        }
        Stmt::Labeled { body, .. } => {
            stmt_is_packed_f64_loop_safe(ctx, body.as_ref(), arr_id, counter_id)
        }
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => true,
        Stmt::Return(_)
        | Stmt::Throw(_)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::While { .. }
        | Stmt::DoWhile { .. }
        | Stmt::For { .. }
        | Stmt::Try { .. }
        | Stmt::Switch { .. } => false,
    }
}

fn supported_packed_numeric_loop_store_kind(
    ctx: &FnCtx<'_>,
    body: &[Stmt],
    arr_id: u32,
    counter_id: u32,
) -> Option<PackedNumericLoopKind> {
    let [Stmt::Expr(perry_hir::Expr::IndexSet {
        object,
        index,
        value,
    })] = body
    else {
        return None;
    };
    if !is_packed_f64_loop_index(object, index, arr_id, counter_id) {
        return None;
    }
    if local_is_int32_array(ctx, arr_id)
        && expr_is_packed_i32_loop_store_rhs_safe(ctx, value, arr_id, counter_id)
    {
        return Some(PackedNumericLoopKind::I32);
    }
    if local_allows_packed_f64_loop_store(ctx, arr_id)
        && expr_is_packed_f64_loop_store_rhs_safe(ctx, value, arr_id, counter_id)
    {
        return Some(PackedNumericLoopKind::F64);
    }
    None
}

fn expr_is_packed_f64_loop_store_rhs_safe(
    ctx: &FnCtx<'_>,
    expr: &perry_hir::Expr,
    arr_id: u32,
    counter_id: u32,
) -> bool {
    use perry_hir::Expr;

    match expr {
        Expr::IndexGet { object, index } => {
            is_packed_f64_loop_index(object, index, arr_id, counter_id)
        }
        Expr::LocalGet(id) => *id != arr_id && crate::type_analysis::is_numeric_expr(ctx, expr),
        Expr::Number(_) | Expr::Integer(_) => true,
        Expr::Binary { left, right, .. } => {
            if !crate::type_analysis::is_numeric_expr(ctx, expr) {
                return false;
            }
            expr_is_packed_f64_loop_store_rhs_safe(ctx, left, arr_id, counter_id)
                && expr_is_packed_f64_loop_store_rhs_safe(ctx, right, arr_id, counter_id)
        }
        Expr::MathAbs(value) => {
            expr_is_packed_f64_loop_store_abs_rhs_safe(ctx, value, arr_id, counter_id)
        }
        _ => false,
    }
}

fn expr_is_packed_f64_loop_store_abs_rhs_safe(
    ctx: &FnCtx<'_>,
    expr: &perry_hir::Expr,
    arr_id: u32,
    counter_id: u32,
) -> bool {
    crate::type_analysis::is_numeric_expr(ctx, expr)
        && matches!(
            expr,
            perry_hir::Expr::IndexGet { object, index }
                if is_packed_f64_loop_index(object, index, arr_id, counter_id)
        )
}

fn expr_is_packed_i32_loop_store_rhs_safe(
    ctx: &FnCtx<'_>,
    expr: &perry_hir::Expr,
    arr_id: u32,
    counter_id: u32,
) -> bool {
    use perry_hir::{BinaryOp, Expr};

    match expr {
        Expr::IndexGet { object, index } => {
            is_packed_f64_loop_index(object, index, arr_id, counter_id)
        }
        Expr::LocalGet(id) => *id != arr_id && local_is_int32_value(ctx, *id),
        Expr::Integer(n) => (i32::MIN as i64..=i32::MAX as i64).contains(n),
        Expr::Number(n)
            if n.is_finite()
                && n.fract() == 0.0
                && *n >= i32::MIN as f64
                && *n <= i32::MAX as f64 =>
        {
            true
        }
        Expr::MathImul(left, right) => {
            expr_is_packed_i32_loop_store_rhs_safe(ctx, left, arr_id, counter_id)
                && expr_is_packed_i32_loop_store_rhs_safe(ctx, right, arr_id, counter_id)
        }
        Expr::Binary {
            op: BinaryOp::BitOr,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Integer(0)) => {
            expr_is_packed_i32_loop_store_rhs_safe(ctx, left, arr_id, counter_id)
        }
        Expr::Binary { op, left, right }
            if matches!(
                op,
                BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
                    | BinaryOp::UShr
            ) =>
        {
            expr_is_packed_i32_loop_store_rhs_safe(ctx, left, arr_id, counter_id)
                && expr_is_packed_i32_loop_store_rhs_safe(ctx, right, arr_id, counter_id)
        }
        _ => false,
    }
}

fn local_is_int32_value(ctx: &FnCtx<'_>, local_id: u32) -> bool {
    matches!(
        ctx.local_types.get(&local_id),
        Some(perry_types::Type::Int32)
    ) || ctx.integer_locals.contains(&local_id)
}

fn expr_is_packed_f64_loop_safe(
    ctx: &FnCtx<'_>,
    expr: &perry_hir::Expr,
    arr_id: u32,
    counter_id: u32,
) -> bool {
    use perry_hir::{ArrayElement, Expr};
    match expr {
        Expr::IndexGet { object, index } => {
            is_packed_f64_loop_index(object, index, arr_id, counter_id)
        }
        // A numeric-store fallback can downgrade/invalidate raw-f64 layout.
        // Without a loop restart, later packed-loop loads would keep using the
        // loop-entry raw-f64 proof, so store-bearing loops stay on guarded paths.
        Expr::IndexSet { .. } | Expr::PutValueSet { .. } => false,
        Expr::LocalSet(id, value) => {
            *id != arr_id
                && *id != counter_id
                && expr_is_packed_f64_loop_safe(ctx, value, arr_id, counter_id)
        }
        Expr::Update { id, .. } => *id != arr_id && *id != counter_id,
        Expr::PropertyGet {
            object, property, ..
        } => {
            if matches!(object.as_ref(), Expr::LocalGet(id) if *id == arr_id) {
                property == "length"
            } else {
                false
            }
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            expr_is_packed_f64_loop_safe(ctx, left, arr_id, counter_id)
                && expr_is_packed_f64_loop_safe(ctx, right, arr_id, counter_id)
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::NumberCoerce(operand)
        | Expr::BooleanCoerce(operand) => {
            expr_is_packed_f64_loop_safe(ctx, operand, arr_id, counter_id)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_is_packed_f64_loop_safe(ctx, condition, arr_id, counter_id)
                && expr_is_packed_f64_loop_safe(ctx, then_expr, arr_id, counter_id)
                && expr_is_packed_f64_loop_safe(ctx, else_expr, arr_id, counter_id)
        }
        Expr::MathImul(left, right) | Expr::MathPow(left, right) => {
            expr_is_packed_f64_loop_safe(ctx, left, arr_id, counter_id)
                && expr_is_packed_f64_loop_safe(ctx, right, arr_id, counter_id)
        }
        Expr::MathMin(values) | Expr::MathMax(values) => values
            .iter()
            .all(|expr| expr_is_packed_f64_loop_safe(ctx, expr, arr_id, counter_id)),
        Expr::MathAbs(value)
        | Expr::MathSqrt(value)
        | Expr::MathFloor(value)
        | Expr::MathCeil(value)
        | Expr::MathRound(value)
        | Expr::MathTrunc(value)
        | Expr::MathSign(value)
        | Expr::MathF16round(value) => expr_is_packed_f64_loop_safe(ctx, value, arr_id, counter_id),
        Expr::Array(elements) => elements
            .iter()
            .all(|expr| expr_is_packed_f64_loop_safe(ctx, expr, arr_id, counter_id)),
        Expr::ArraySpread(elements) => elements.iter().all(|element| match element {
            ArrayElement::Expr(expr) => expr_is_packed_f64_loop_safe(ctx, expr, arr_id, counter_id),
            ArrayElement::Spread(_) | ArrayElement::Hole => false,
        }),
        Expr::LocalGet(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Undefined => true,
        Expr::Call { .. } | Expr::NativeMethodCall { .. } | Expr::CallSpread { .. } => false,
        Expr::Closure { .. }
        | Expr::PropertySet { .. }
        | Expr::PropertyUpdate { .. }
        | Expr::IndexUpdate { .. }
        | Expr::ArrayPush { .. }
        | Expr::ArrayPushSpread { .. }
        | Expr::ArrayPop(_)
        | Expr::ArrayShift(_)
        | Expr::ArrayUnshift { .. }
        | Expr::ArraySplice { .. } => false,
        _ => false,
    }
}

fn is_packed_f64_loop_index(
    object: &perry_hir::Expr,
    index: &perry_hir::Expr,
    arr_id: u32,
    counter_id: u32,
) -> bool {
    matches!(
        (object, index),
        (perry_hir::Expr::LocalGet(object_id), perry_hir::Expr::LocalGet(index_id))
            if *object_id == arr_id && *index_id == counter_id
    )
}

/// Emit the one-time loop-entry guard behind the dynamic-bound `icmp` fast
/// loop, and pick the i32 counter it compares.
///
/// The counter comes from one of two places:
///
/// * It already owns a **shared** i32 shadow (`ctx.i32_counter_slots`, put
///   there at its `Let` site because it is index-used / strictly-i32-bounded).
///   Every read of the local in this loop already comes from that shadow, so
///   reusing it for the `icmp` introduces no new representation and no new
///   hazard — the array-index fast path keeps working exactly as before.
/// * It has no shadow. #6072: the old code installed one **into the shared
///   map** right here, with nothing proving that the counter stays inside i32.
///   A runtime bound above `INT32_MAX` — `for (let i = 2147483640; i < lim;
///   i++)` with `lim = 2147483653` — wrapped the shadow to `INT32_MIN`, and
///   because every `LocalGet` prefers the shadow over the f64 slot (issue #48),
///   the counter went negative and the loop spun forever. Even the *slow*
///   (guard-failed) cond read the wrapped shadow, so the runtime guard could
///   not save it. Now we allocate a **loop-private** i32 counter that never
///   enters the map: only the fast cond block reads it, the update block bumps
///   it, and the body / slow cond keep reading the f64 slot, which `Update`
///   maintains with exact JS semantics.
///
/// The guard proves, once, that the fast loop cannot leave i32 range:
///
/// * `n` is a number, integral, and `>= INT32_MIN`;
/// * `n <= INT32_MAX` for `i < n` — the counter is only bumped after a taken
///   `i < n`, so it tops out at `n`;
/// * `n <= INT32_MAX - 1` for `i <= n` — there the counter tops out at `n + 1`;
/// * (private counter only) the counter's entry value is itself an integral
///   number in i32 range, so the initial `fptosi` is well-defined and the
///   counter starts no higher than `INT32_MAX`.
///
/// Anything else (NaN, infinities, fractional or out-of-i32-range bounds,
/// non-numbers, a counter seeded past 2^31) leaves the flag false and runs the
/// generic per-iteration comparison with full JS semantics.
fn emit_guarded_i32_bound(
    ctx: &mut FnCtx<'_>,
    counter_id: u32,
    bound_id: u32,
    op: perry_hir::CompareOp,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
    label_prefix: &str,
) -> Option<DynamicI32Bound> {
    let bound_slot = ctx.locals.get(&bound_id).cloned()?;
    let counter_slot = ctx.locals.get(&counter_id).cloned()?;
    let shared_counter_i32 = ctx.i32_counter_slots.get(&counter_id).cloned();
    let counter_is_private = shared_counter_i32.is_none();
    if counter_is_private && !dynamic_bound_private_counter_is_safe(ctx, counter_id, update, body) {
        return None;
    }
    let counter_i32_slot = match shared_counter_i32 {
        Some(slot) => slot,
        None => ctx.func.alloca_entry(I32),
    };

    // `i <= n` bumps the counter one past the bound on the last iteration, so
    // the largest bound it can carry without overflowing is `INT32_MAX - 1`.
    let max_bound = match op {
        perry_hir::CompareOp::Le => "2147483646.0",
        _ => "2147483647.0",
    };

    let flag_slot = ctx.func.alloca_entry(I1);
    let bound_i32_slot = ctx.func.alloca_entry(I32);
    ctx.block().store(I1, "false", &flag_slot);
    ctx.block().store(I32, "0", &bound_i32_slot);
    if counter_is_private {
        ctx.block().store(I32, "0", &counter_i32_slot);
    }

    let n_dbl = ctx.block().load(DOUBLE, &bound_slot);
    let is_number = emit_js_value_is_number(ctx, &n_dbl);

    let number_idx = ctx.new_block(&format!("{label_prefix}.bound_i32.number"));
    let convert_idx = ctx.new_block(&format!("{label_prefix}.bound_i32.convert"));
    let merge_idx = ctx.new_block(&format!("{label_prefix}.bound_i32.merge"));
    let number_label = ctx.block_label(number_idx);
    let convert_label = ctx.block_label(convert_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&is_number, &number_label, &merge_label);

    ctx.current_block = number_idx;
    let ge_min = ctx.block().fcmp("oge", &n_dbl, "-2147483648.0");
    let le_max = ctx.block().fcmp("ole", &n_dbl, max_bound);
    let in_i32_range = ctx.block().and(I1, &ge_min, &le_max);
    ctx.block()
        .cond_br(&in_i32_range, &convert_label, &merge_label);

    ctx.current_block = convert_idx;
    let bound_i32 = ctx.block().fptosi(DOUBLE, &n_dbl, I32);
    let roundtrip = ctx.block().sitofp(I32, &bound_i32, DOUBLE);
    let is_integral = ctx.block().fcmp("oeq", &roundtrip, &n_dbl);
    ctx.block().store(I32, &bound_i32, &bound_i32_slot);
    if !counter_is_private {
        // The shared shadow was already seeded (and range-checked) at the
        // counter's `Let` site; only the bound needs proving here.
        ctx.block().store(I1, &is_integral, &flag_slot);
        ctx.block().br(&merge_label);
        ctx.current_block = merge_idx;
        return Some(DynamicI32Bound {
            op,
            flag_slot,
            bound_i32_slot,
            counter_i32_slot,
            counter_is_private,
        });
    }

    // Private counter: seed it from the f64 slot, but only on a block the
    // range check dominates — `fptosi` of an out-of-range double is poison.
    // A non-number counter (every NaN-boxed tag is a NaN double) fails the
    // ordered compares below and takes the generic path.
    let counter_idx = ctx.new_block(&format!("{label_prefix}.counter_i32.range"));
    let counter_conv_idx = ctx.new_block(&format!("{label_prefix}.counter_i32.convert"));
    let counter_label = ctx.block_label(counter_idx);
    let counter_conv_label = ctx.block_label(counter_conv_idx);
    ctx.block()
        .cond_br(&is_integral, &counter_label, &merge_label);

    ctx.current_block = counter_idx;
    let c_dbl = ctx.block().load(DOUBLE, &counter_slot);
    let c_ge_min = ctx.block().fcmp("oge", &c_dbl, "-2147483648.0");
    let c_le_max = ctx.block().fcmp("ole", &c_dbl, "2147483647.0");
    let c_in_range = ctx.block().and(I1, &c_ge_min, &c_le_max);
    ctx.block()
        .cond_br(&c_in_range, &counter_conv_label, &merge_label);

    ctx.current_block = counter_conv_idx;
    let c_i32 = ctx.block().fptosi(DOUBLE, &c_dbl, I32);
    let c_roundtrip = ctx.block().sitofp(I32, &c_i32, DOUBLE);
    let c_is_integral = ctx.block().fcmp("oeq", &c_roundtrip, &c_dbl);
    ctx.block().store(I32, &c_i32, &counter_i32_slot);
    ctx.block().store(I1, &c_is_integral, &flag_slot);
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Some(DynamicI32Bound {
        op,
        flag_slot,
        bound_i32_slot,
        counter_i32_slot,
        counter_is_private,
    })
}

/// Static preconditions for handing a dynamic-bound loop a *loop-private* i32
/// counter (#6072).
///
/// The private shadow is maintained by this loop alone — the update block bumps
/// it by hand, because the counter is not in `ctx.i32_counter_slots` and so the
/// generic `Update` / `LocalSet` lowerings never see it. That is only correct
/// when the loop's own `i++` is the *only* thing that ever advances the
/// counter, and when the counter lives in a plain f64 alloca (a boxed/captured
/// or module-global counter is read through a box/root helper, which a stack
/// shadow could not track).
fn dynamic_bound_private_counter_is_safe(
    ctx: &crate::expr::FnCtx<'_>,
    counter_id: u32,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> bool {
    use perry_hir::{Expr, UpdateOp};
    if !ctx.locals.contains_key(&counter_id)
        || ctx.boxed_vars.contains(&counter_id)
        || ctx.module_globals.contains_key(&counter_id)
    {
        return false;
    }
    let advanced_by_increment = matches!(
        update,
        Some(Expr::Update {
            id,
            op: UpdateOp::Increment,
            ..
        }) if *id == counter_id
    );
    advanced_by_increment && !stmts_mutate_local(body, counter_id)
}

fn emit_js_value_is_number(ctx: &mut FnCtx<'_>, value: &str) -> String {
    let n_bits = ctx.block().bitcast_double_to_i64(value);
    let tag = ctx.block().and(
        I64,
        &n_bits,
        &crate::nanbox::i64_literal(crate::nanbox::TAG_MASK),
    );
    let below = ctx.block().icmp_ult(
        I64,
        &tag,
        &crate::nanbox::i64_literal(crate::nanbox::SHORT_STRING_TAG),
    );
    let above = ctx.block().icmp_ugt(
        I64,
        &tag,
        &crate::nanbox::i64_literal(crate::nanbox::STRING_TAG),
    );
    ctx.block().or(I1, &below, &above)
}

/// For-loop lowering: classic init / cond / body / update / exit CFG.
///
/// ```text
///   <current>:
///     <init>
///     br cond
///   for.cond:
///     <condition>          ; if missing, treat as `true` (infinite loop)
///     fcmp one cond, 0.0
///     br i1, body, exit
///   for.body:
///     <body>
///     br update            ; if not already terminated
///   for.update:
///     <update>
///     br cond              ; if not already terminated
///   for.exit:
///     <continues here>
/// ```
///
/// Phase 2.1 does not support `break` / `continue`. The body must fall
/// through to update; otherwise codegen produces dead code that LLVM will
/// reject. We don't yet pass the loop's break/continue targets through
/// FnCtx — that lands when we need it.
pub(crate) fn lower_for(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Result<()> {
    // Init runs once in the current block. A `let i = 0` here adds `i` to
    // ctx.locals, which the body can then load via LocalGet.
    if let Some(init_stmt) = init {
        lower_stmt(ctx, init_stmt)?;
    }

    if let Some(matched) = match_numeric_bulk_fill_loop(ctx, init, condition, update, body) {
        if lower_numeric_bulk_fill_loop(ctx, matched)? {
            return Ok(());
        }
    }

    if lower_packed_f64_versioned_for(ctx, init, condition, update, body)? {
        return Ok(());
    }

    // #6011: `i < N`-bounded loops (N an integer literal or loop-invariant
    // local/module-global) with `a[i ± c]` accesses — EMA-style recurrences.
    // Tried only after the `i < arr.length` matcher above declined.
    if lower_packed_f64_range_versioned_for(ctx, init, condition, update, body)? {
        return Ok(());
    }

    // #5093: monomorphic class-field hot loops (`counter.value = counter.value
    // + 1` after method inlining). Shape check hoisted to a preheader; fast
    // clone is call-free raw slot access.
    if lower_class_field_versioned_for(ctx, init, condition, update, body)? {
        return Ok(());
    }

    lower_for_after_init(ctx, init, condition, update, body, "for")
}

fn lower_for_after_init(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
    label_prefix: &str,
) -> Result<()> {
    lower_for_after_init_with_i32_bound(ctx, init, condition, update, body, label_prefix, None)
}

/// #6011: like [`lower_for_after_init`], but the range-versioned fast copy can
/// hand down its already-materialized (finite-integral-validated) i32 loop
/// bound so the condition block emits `icmp slt i32` instead of re-lowering
/// the generic `i < N` comparison (a module-global load + `fcmp` per
/// iteration that LLVM cannot hoist past the loop's raw element stores). The
/// value must dominate the block this is emitted from — only the fast
/// preheader of the range-versioned loop qualifies.
#[allow(clippy::too_many_arguments)]
fn lower_for_after_init_with_i32_bound(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
    label_prefix: &str,
    precomputed_i32_bound: Option<(u32, String)>,
) -> Result<()> {
    let loop_proof_scope_id = ctx.next_loop_proof_scope_id();

    // Loop-invariant length hoisting peephole. Detect the very common
    // shape `for (...; i < arr.length; ...)` where `arr` is a local
    // that the body never mutates length-wise, and pre-load
    // `arr.length` into a stack slot before entering the cond block.
    // The length load inside the cond is then replaced with a load
    // from the slot — saves two instructions per iteration (the
    // `and` to unbox arr + the `ldr` of the length field) and lets
    // LLVM hoist a couple more downstream loads now that the slot
    // is the loop-invariant source of truth.
    //
    // Without this, LLVM's LICM declines to hoist the length load
    // because the loop body's `IndexSet` slow path (`js_array_set_f64
    // _extend`) is an external call that LLVM can't prove won't
    // modify the array's length field. We do the analysis ourselves
    // and only hoist when our (more domain-specific) walker can
    // prove the body won't change `arr.length`.
    //
    // Saves ~25-30% on `for (let i = 0; i < arr.length; i++) arr[i] = i`
    // and `for (let i = 0; i < arr.length; i++) for (let j = 0; j <
    // arr.length; j++) ...` patterns.
    let raw_hoist_classification: Option<LengthHoist> =
        condition.and_then(|cond| classify_for_length_hoist(ctx, cond, update, body));
    let hoist_rejection = if raw_hoist_classification.is_none() {
        condition.and_then(|cond| classify_for_length_hoist_rejection(ctx, cond, update, body))
    } else {
        None
    };
    let hoist_classification: Option<LengthHoist> = raw_hoist_classification
        // `__arr_N` is the for-of desugar's holder — an ALIAS of the user's
        // iterable local. Body mutations go through the user's name
        // (`array.push(1)` → ArrayPush on the user id), so the walker above
        // can't see them against the holder id. Spec ForOf reads the live
        // length every step (array-expand/contract in test262), so never
        // hoist for desugared for-of loops; user-written `i < arr.length`
        // loops keep the peephole.
        .filter(|hoist| {
            !ctx.local_id_to_name
                .get(&hoist.arr_id)
                .is_some_and(|n| n.starts_with("__arr_"))
        });
    if let Some(hoist) = hoist_classification {
        record_loop_array_length_effect(ctx, hoist.arr_id, LoopArrayLengthEffect::Preserves, true);
    } else if let Some(rejection) = hoist_rejection {
        record_loop_array_length_effect(ctx, rejection.arr_id, rejection.effect, false);
    }
    let hoisted_length_arr_id: Option<u32> = hoist_classification.map(|hoist| hoist.arr_id);
    let hoisted_index_bounds_are_safe = hoist_classification.is_some_and(|hoist| {
        matches!(hoist.op, perry_hir::CompareOp::Lt)
            && hoist.lhs_addend == 0
            && loop_counter_bounds_are_safe(ctx, hoist.counter_id, update, body)
    });
    let hoisted_buffer_bounds_width = hoist_classification.and_then(|hoist| {
        hoist.buffer_bounds_width_units.filter(|_| {
            ctx.buffer_view_slots.contains_key(&hoist.arr_id)
                && loop_counter_bounds_are_safe(ctx, hoist.counter_id, update, body)
        })
    });
    let hoisted_length_slot: Option<String> = if let Some(hoist) = hoist_classification {
        let arr_box_loaded = lower_expr(
            ctx,
            &perry_hir::Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(perry_hir::Expr::LocalGet(hoist.arr_id)),
                property: "length".to_string(),
            },
        )?;
        let slot = ctx.func.alloca_entry(DOUBLE);
        ctx.block().store(DOUBLE, &arr_box_loaded, &slot);
        ctx.cached_lengths.insert(hoist.arr_id, slot.clone());
        // Also tell `lower_index_set_fast` (and similar sites) that
        // `arr[counter_id]` is statically inbounds for this body, so
        // it can skip the runtime length-load + bound check.
        if hoisted_index_bounds_are_safe {
            ctx.bounded_index_pairs.push(BoundedIndexPair {
                index_local_id: hoist.counter_id,
                array_local_id: hoist.arr_id,
                scope_id: loop_proof_scope_id,
            });
        }
        if let Some(bounds_width_units) = hoisted_buffer_bounds_width {
            ctx.bounded_buffer_index_pairs.push(BoundedBufferIndex {
                index_local_id: hoist.counter_id,
                buffer_local_id: hoist.arr_id,
                scope_id: loop_proof_scope_id,
                bounds_width_units,
                bounds: BoundsState::Proven {
                    proof: BoundsProof::LoopGuard,
                },
            });
        }

        // If the counter is provably integer-valued (initialized from
        // an Integer literal, only mutated via Update ++/--), allocate
        // a parallel i32 slot. The Update lowering will keep it in sync,
        // and IndexGet/IndexSet will load the i32 directly instead of
        // emitting a `fptosi double → i32` on every iteration.
        if ctx.integer_locals.contains(&hoist.counter_id) {
            if let Some(counter_slot) = ctx.locals.get(&hoist.counter_id).cloned() {
                let i32_slot = ctx.func.alloca_entry(I32);
                // Initialize from the current double value.
                let cur_dbl = ctx.block().load(DOUBLE, &counter_slot);
                let cur_i32 = ctx.block().fptosi(DOUBLE, &cur_dbl, I32);
                ctx.block().store(I32, &cur_i32, &i32_slot);
                ctx.i32_counter_slots.insert(hoist.counter_id, i32_slot);
            }
        }

        Some(slot)
    } else {
        None
    };

    // If we have an i32 counter AND a hoisted length, pre-compute the
    // length as i32 so the loop condition can use `icmp slt/sle i32`
    // instead of `fcmp olt/ole double`. This eliminates the float counter fadd +
    // fcmp per iteration — saves ~2 instructions on the inner loop of
    // nested_loops and similar patterns.
    let i32_length_slot: Option<String> = if let Some(hoist) = hoist_classification {
        if let (Some(_), Some(len_dbl_slot)) = (
            ctx.i32_counter_slots.get(&hoist.counter_id).cloned(),
            hoisted_length_slot.as_ref(),
        ) {
            let len_dbl = ctx.block().load(DOUBLE, len_dbl_slot);
            let len_i32 = ctx.block().fptosi(DOUBLE, &len_dbl, I32);
            let slot = ctx.func.alloca_entry(I32);
            ctx.block().store(I32, &len_i32, &slot);
            Some(slot)
        } else {
            None
        }
    } else {
        None
    };

    // Issue #168: when the `i < arr.length` peephole didn't fire, also
    // detect the simpler `i < n` shape where `n` is a statically proven
    // loop-invariant i32 local. Emitting `fptosi(n)` once at the loop head
    // and using `icmp slt i32 %i, %n.i32` in the condition block replaces
    // `fcmp olt double`, letting LLVM's SCEV model `i` as a clean integer
    // induction variable.
    let local_bound_classification: Option<(u32, u32, perry_hir::CompareOp)> =
        if hoist_classification.is_none() {
            condition.and_then(|cond| classify_for_local_bound(cond, update, body, ctx))
        } else {
            None
        };
    // Track whether *we* allocated the counter's i32 slot (vs. the Let
    // site having done so already).  Only the site that inserted should
    // remove it at loop exit to avoid disturbing a pre-existing slot.
    let local_bound_counter_i32_was_fresh: bool;
    let i32_local_bound_slot: Option<String> =
        if let Some((counter_id, bound_id, _op)) = local_bound_classification {
            // Allocate a parallel i32 slot for the counter if not already
            // present.  Counters that fall outside `integer_locals`
            // (e.g. `for (let i = 0; i < arr.length; i++)` where `i` is
            // captured by a closure or escapes) skip the Let-site
            // allocation; providing one here enables both `icmp slt i32`
            // in the condition and `add i32 1` in Update.
            let fresh = if !ctx.i32_counter_slots.contains_key(&counter_id) {
                if let Some(counter_slot) = ctx.locals.get(&counter_id).cloned() {
                    let i32_slot = ctx.func.alloca_entry(I32);
                    let cur_dbl = ctx.block().load(DOUBLE, &counter_slot);
                    let cur_i32 = ctx.block().fptosi(DOUBLE, &cur_dbl, I32);
                    ctx.block().store(I32, &cur_i32, &i32_slot);
                    ctx.i32_counter_slots.insert(counter_id, i32_slot);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            local_bound_counter_i32_was_fresh = fresh;
            // Hoist `fptosi(n)` to a fresh i32 alloca before the cond block
            // so LLVM sees a loop-invariant integer bound — critical for
            // SCEV / LoopVectorizer to recognize the induction variable.
            if let Some(bound_slot) = ctx.locals.get(&bound_id).cloned() {
                let bound_dbl = ctx.block().load(DOUBLE, &bound_slot);
                let bound_i32 = ctx.block().fptosi(DOUBLE, &bound_dbl, I32);
                let slot = ctx.func.alloca_entry(I32);
                ctx.block().store(I32, &bound_i32, &slot);
                Some(slot)
            } else {
                None
            }
        } else {
            local_bound_counter_i32_was_fresh = false;
            None
        };
    // Issue #168 follow-up: when neither the `arr.length` hoist nor the static
    // `i < n` peephole fired, try the runtime-guarded path. We emit a
    // finite-integral-i32 guard and `fptosi(n)` once here, in the pre-loop
    // block, so the cond block can pick an `icmp slt/sle i32` fast loop when
    // safe and fall back to the generic comparison otherwise.
    let dynamic_i32_bound: Option<DynamicI32Bound> = if hoist_classification.is_none()
        && local_bound_classification.is_none()
    {
        condition
            .and_then(|cond| classify_for_local_bound_dynamic(cond, update, body, ctx))
            .and_then(|(counter_id, bound_id, op)| {
                emit_guarded_i32_bound(ctx, counter_id, bound_id, op, update, body, label_prefix)
            })
    } else {
        None
    };
    let local_bound_index_bounds_are_safe =
        local_bound_classification.is_some_and(|(counter_id, _, op)| {
            matches!(op, perry_hir::CompareOp::Lt)
                && loop_counter_bounds_are_safe(ctx, counter_id, update, body)
        });
    if let Some((counter_id, bound_id, _op)) = local_bound_classification {
        if local_bound_index_bounds_are_safe {
            if let Some(buffer_ids) = ctx.min_length_bounds.get(&bound_id).cloned() {
                for buffer_local_id in buffer_ids {
                    if ctx.buffer_view_slots.contains_key(&buffer_local_id) {
                        ctx.bounded_buffer_index_pairs.push(BoundedBufferIndex {
                            index_local_id: counter_id,
                            buffer_local_id,
                            scope_id: loop_proof_scope_id,
                            bounds_width_units: 1,
                            bounds: BoundsState::Proven {
                                proof: BoundsProof::MinLength,
                            },
                        });
                    }
                }
            }
            let alloc_bound_ids: Vec<u32> = ctx
                .buffer_view_slots
                .iter()
                .filter_map(|(buffer_local_id, view)| match &view.length_source {
                    Some(LengthSource::Local { id, addend }) if *id == bound_id && *addend >= 0 => {
                        Some(*buffer_local_id)
                    }
                    _ => None,
                })
                .collect();
            for buffer_local_id in alloc_bound_ids {
                ctx.bounded_buffer_index_pairs.push(BoundedBufferIndex {
                    index_local_id: counter_id,
                    buffer_local_id,
                    scope_id: loop_proof_scope_id,
                    bounds_width_units: 1,
                    bounds: BoundsState::Proven {
                        proof: BoundsProof::LoopGuard,
                    },
                });
            }
        }
    }
    if let Some(fact) =
        classify_for_counter_range(init, condition, update, body, ctx, loop_proof_scope_id)
    {
        ctx.int_range_facts.push(fact);
    }

    let cond_idx = ctx.new_block(&format!("{label_prefix}.cond"));
    let body_idx = ctx.new_block(&format!("{label_prefix}.body"));
    let update_idx = ctx.new_block(&format!("{label_prefix}.update"));
    let exit_idx = ctx.new_block(&format!("{label_prefix}.exit"));

    let cond_label = ctx.block_label(cond_idx);
    let body_label = ctx.block_label(body_idx);
    let update_label = ctx.block_label(update_idx);
    let exit_label = ctx.block_label(exit_idx);

    // Branch from the block holding the init into the cond block.
    ctx.block().br(&cond_label);

    // Cond block — fast i32 path when both counter and length are i32.
    ctx.current_block = cond_idx;
    let used_precomputed_i32_cond = if let Some((counter_id, bound_i32)) = &precomputed_i32_bound {
        // #6011: range-versioned fast copy — the caller already materialized
        // and validated the loop bound as i32 (finite, integral, in range),
        // and the matcher proved the strict `i < bound` shape with an
        // increment-only integer counter, so `icmp slt i32` is trip-count
        // exact.
        if let Some(ctr_i32_slot) = ctx.i32_counter_slots.get(counter_id).cloned() {
            let ctr = ctx.block().load(I32, &ctr_i32_slot);
            let cmp = ctx.block().icmp_slt(I32, &ctr, bound_i32);
            ctx.block().cond_br(&cmp, &body_label, &exit_label);
            true
        } else {
            false
        }
    } else {
        false
    };
    let used_i32_cond = if used_precomputed_i32_cond {
        true
    } else if let (Some(hoist), Some(ref len_i32_slot)) = (hoist_classification, &i32_length_slot) {
        // Existing path: `i < arr.length` / `i <= arr.length` with
        // hoisted i32 length.
        if let Some(ctr_i32_slot) = ctx.i32_counter_slots.get(&hoist.counter_id).cloned() {
            let mut ctr = ctx.block().load(I32, &ctr_i32_slot);
            if hoist.lhs_addend != 0 {
                ctr = ctx.block().add(I32, &ctr, &hoist.lhs_addend.to_string());
            }
            let len = ctx.block().load(I32, len_i32_slot);
            let cmp = match hoist.op {
                perry_hir::CompareOp::Le => ctx.block().icmp_sle(I32, &ctr, &len),
                _ => ctx.block().icmp_slt(I32, &ctr, &len),
            };
            ctx.block().cond_br(&cmp, &body_label, &exit_label);
            true
        } else {
            false
        }
    } else if let (Some((counter_id, _, op)), Some(ref bound_i32_slot)) =
        (local_bound_classification, &i32_local_bound_slot)
    {
        // Issue #168: `i < n` / `i <= n` where `n` is statically proven
        // safe for unguarded i32 materialization. The fptosi(n) was
        // hoisted above; use icmp i32.
        if let Some(ctr_i32_slot) = ctx.i32_counter_slots.get(&counter_id).cloned() {
            let ctr = ctx.block().load(I32, &ctr_i32_slot);
            let bound = ctx.block().load(I32, bound_i32_slot);
            let cmp = match op {
                perry_hir::CompareOp::Le => ctx.block().icmp_sle(I32, &ctr, &bound),
                _ => ctx.block().icmp_slt(I32, &ctr, &bound),
            };
            ctx.block().cond_br(&cmp, &body_label, &exit_label);
            true
        } else {
            false
        }
    } else if let Some(ref dyn_bound) = dynamic_i32_bound {
        // Issue #168 follow-up: `i < n` / `i <= n` with a runtime-guarded
        // local bound. Branch on the one-time guard flag hoisted above: the
        // fast loop uses `icmp`, and the slow loop keeps full JS comparison
        // semantics. The branch is loop-invariant, so LLVM's LoopUnswitch peels
        // it into two loops at -O2+; even unswitched, the hot path executes
        // pure integer compares with no per-iteration `sitofp` / call.
        //
        // #6072: when the counter's i32 slot is loop-private, the slow cond
        // below re-lowers the condition with the counter absent from
        // `ctx.i32_counter_slots`, so it reads the f64 slot — the one the
        // `Update` lowering keeps at exact JS semantics. That is what makes a
        // guard failure (e.g. a bound past `INT32_MAX`) merely slow instead of
        // an infinite loop over a wrapped counter.
        let ctr_i32_slot = dyn_bound.counter_i32_slot.clone();
        let fast_idx = ctx.new_block(&format!("{label_prefix}.cond.fast"));
        let slow_idx = ctx.new_block(&format!("{label_prefix}.cond.slow"));
        let fast_label = ctx.block_label(fast_idx);
        let slow_label = ctx.block_label(slow_idx);
        let flag = ctx.block().load(I1, &dyn_bound.flag_slot);
        ctx.block().cond_br(&flag, &fast_label, &slow_label);

        // Fast path: integer induction variable + `icmp`.
        ctx.current_block = fast_idx;
        let ctr = ctx.block().load(I32, &ctr_i32_slot);
        let bound = ctx.block().load(I32, &dyn_bound.bound_i32_slot);
        let cmp = match dyn_bound.op {
            perry_hir::CompareOp::Le => ctx.block().icmp_sle(I32, &ctr, &bound),
            _ => ctx.block().icmp_slt(I32, &ctr, &bound),
        };
        ctx.block().cond_br(&cmp, &body_label, &exit_label);

        // Slow path: generic per-iteration comparison (full coercion).
        ctx.current_block = slow_idx;
        if let Some(cond_expr) = condition {
            let cv = lower_expr(ctx, cond_expr)?;
            let i1 = lower_truthy(ctx, &cv, cond_expr);
            ctx.block().cond_br(&i1, &body_label, &exit_label);
        } else {
            ctx.block().br(&body_label);
        }
        true
    } else {
        false
    };
    if !used_i32_cond {
        if let Some(cond_expr) = condition {
            let cv = lower_expr(ctx, cond_expr)?;
            let i1 = lower_truthy(ctx, &cv, cond_expr);
            ctx.block().cond_br(&i1, &body_label, &exit_label);
        } else {
            // `for (;;)` — unconditional jump into the body. May be an
            // infinite loop unless the body contains a `break`.
            ctx.block().br(&body_label);
        }
    }

    // Push break/continue targets so nested `break`/`continue` know where
    // to jump. For for-loops, continue runs the update step.
    ctx.loop_targets
        .push((update_label.clone(), exit_label.clone(), ctx.try_depth));

    // If this for-loop has a pending label (from an enclosing Stmt::Labeled),
    // register it so `break label;` / `continue label;` resolve here.
    let consumed_labels = std::mem::take(&mut ctx.pending_labels);
    let previous_region_id = ctx.active_region_id.clone();
    for lbl in &consumed_labels {
        ctx.label_targets.insert(
            lbl.clone(),
            (update_label.clone(), exit_label.clone(), ctx.try_depth),
        );
    }
    if let Some(lbl) = consumed_labels.last() {
        ctx.active_region_id = Some(ctx.region_id_for_label(lbl));
    }

    // Body block.
    ctx.current_block = body_idx;
    if let Some(cond) = condition {
        let mut guarded =
            crate::expr::guarded_buffer_indices_for_condition(ctx, cond, loop_proof_scope_id);
        guarded.retain(|fact| loop_counter_bounds_are_safe(ctx, fact.index_local_id, update, body));
        ctx.guarded_buffer_index_pairs.extend(guarded);
    }
    lower_stmts(ctx, body)?;
    clear_loop_body_shadow_slots(ctx, body);
    // Issue #74: insert an empty `asm sideeffect` in bodies whose
    // statements are all LLVM-pure (local-only arithmetic, no calls,
    // no heap mutation). Without this, clang -O3's loop-deletion
    // pass folds patterns like `for (let i=0;i<N;i++) sum+=1;` to
    // `sum=N` and eliminates the loop entirely — so two `Date.now()`
    // calls bracketing the loop end up adjacent in the binary and
    // report 0ms wall-clock. The barrier emits zero machine
    // instructions but is opaque to IndVarSimplify.
    if !ctx.block().is_terminated() && body_needs_asm_barrier(body) {
        ctx.block().asm_sideeffect_barrier();
    }
    if !ctx.block().is_terminated() {
        emit_gc_loop_safepoint(ctx);
        ctx.block().br(&update_label);
    }

    // Update block.
    ctx.current_block = update_idx;
    if let Some(update_expr) = update {
        let _ = lower_expr(ctx, update_expr)?;
    }
    // #6072: a loop-private i32 counter is invisible to the `Update` lowering
    // (it is not in `ctx.i32_counter_slots`), so advance it here. The classifier
    // proved the update is exactly `counter++` and that nothing else writes the
    // counter, so this stays in lockstep with the f64 slot. The `add` wraps
    // (LLVM `add` without `nsw`) if the guard failed, but nothing reads this
    // slot then — only the fast cond block does, and it is unreachable with a
    // false flag.
    if let Some(ref dyn_bound) = dynamic_i32_bound {
        if dyn_bound.counter_is_private && !ctx.block().is_terminated() {
            let slot = dyn_bound.counter_i32_slot.clone();
            let blk = ctx.block();
            let cur = blk.load(I32, &slot);
            let next = blk.add(I32, &cur, "1");
            blk.store(I32, &next, &slot);
        }
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&cond_label);
    }
    ctx.active_region_id = previous_region_id;

    ctx.loop_targets.pop();

    // Pop the hoisted-length entry so nested loops or sibling loops
    // don't see a stale slot.
    if let Some(hoist) = hoist_classification {
        ctx.i32_counter_slots.remove(&hoist.counter_id);
    }
    if let Some(arr_id) = hoisted_length_arr_id {
        ctx.cached_lengths.remove(&arr_id);
    }
    let _ = hoisted_length_slot;
    // Pop the i32 counter slot we inserted for the `i < n` number-bound
    // path, but only if *we* were the ones that inserted it (the Let site
    // may have already provided a slot, which should outlive the loop).
    if local_bound_counter_i32_was_fresh {
        if let Some((counter_id, _, _)) = local_bound_classification {
            ctx.i32_counter_slots.remove(&counter_id);
        }
    }
    let _ = i32_local_bound_slot;
    // The runtime-guarded `any`-bound path needs no cleanup: it either reuses
    // the counter's existing (Let-site) i32 slot or keeps its own private one
    // out of `ctx.i32_counter_slots` entirely (#6072).
    let _ = dynamic_i32_bound;
    ctx.bounded_index_pairs
        .retain(|fact| fact.scope_id != loop_proof_scope_id);
    ctx.bounded_buffer_index_pairs
        .retain(|fact| fact.scope_id != loop_proof_scope_id);
    ctx.guarded_buffer_index_pairs
        .retain(|fact| fact.scope_id != loop_proof_scope_id);
    ctx.int_range_facts
        .retain(|fact| fact.scope_id != loop_proof_scope_id);

    // Exit block — subsequent statements continue here.
    ctx.current_block = exit_idx;
    Ok(())
}

/// Whether to emit loop back-edge safepoint polls — OPT-IN, default OFF
/// (`PERRY_GC_MOVING_LOOP_POLLS=1`). The moving GC is the default at the
/// event-loop safepoint, but the loop poll emits a `js_gc_loop_safepoint()`
/// CALL at every loop back-edge, which defeats LLVM auto-vectorization and
/// violates the native-region "no runtime calls in hot loop" proofs. Until the
/// poll is emitted only in loops that actually ALLOCATE (so numeric/vectorizable
/// loops stay call-free), it is opt-in and a tight allocating loop defers to the
/// event-loop safepoint instead.
fn moving_safepoint_polls_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_GC_MOVING_LOOP_POLLS").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

/// Emit a `js_gc_loop_safepoint()` poll at a loop back-edge. Call this AFTER
/// `clear_loop_body_shadow_slots` and only where the block is not terminated:
/// at that point the loop-body expression has completed, so every live heap
/// value is a named local on the shadow stack (no unspilled register temps) —
/// a precise-root safepoint where a deferred copying minor can MOVE survivors.
///
/// COVERAGE (Phase 2, follow-up): currently wired into the generic `while`,
/// `do..while`, and `for` back-edges. The specialized/versioned `for`-loop
/// lowering paths in this file (i32-bound-optimized, packed-f64/i32/u32,
/// bulk-fill) and `for-of`/`for-in` do NOT yet emit it, so a hot allocating
/// loop that takes one of those paths won't drain a deferred moving minor until
/// the next event-loop safepoint. Adding the poll to every back-edge across
/// those paths is the remaining Phase 2 codegen work.
pub(crate) fn emit_gc_loop_safepoint(ctx: &mut FnCtx<'_>) {
    if !moving_safepoint_polls_enabled() || ctx.block().is_terminated() {
        return;
    }
    ctx.block().call_void("js_gc_loop_safepoint", &[]);
}

pub(crate) fn clear_loop_body_shadow_slots(ctx: &mut FnCtx<'_>, body: &[Stmt]) {
    if ctx.block().is_terminated() || ctx.shadow_slot_map.is_empty() {
        return;
    }
    let slots =
        crate::collectors::collect_declared_shadow_slots_in_stmts(body, &ctx.shadow_slot_map);
    if slots.is_empty() {
        return;
    }
    emit_shadow_slot_clears(ctx, &slots);
}

fn guarded_array_aliases_for_loop(
    ctx: &crate::expr::FnCtx<'_>,
    arr_id: u32,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> std::collections::HashSet<u32> {
    let mut aliases = std::collections::HashSet::new();
    aliases.insert(arr_id);
    let guarded_root = crate::expr::local_value_alias_root(ctx, arr_id);
    aliases.insert(guarded_root);
    for alias_id in ctx.local_value_aliases.keys() {
        if crate::expr::local_value_alias_root(ctx, *alias_id) == guarded_root {
            aliases.insert(*alias_id);
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        if let Some(update) = update {
            changed |= collect_guarded_array_aliases_in_expr(ctx, arr_id, update, &mut aliases);
        }
        changed |= collect_guarded_array_aliases_in_stmts(ctx, arr_id, body, &mut aliases);
    }
    aliases
}

fn local_may_alias_guarded_array(
    ctx: &crate::expr::FnCtx<'_>,
    arr_id: u32,
    local_id: u32,
    aliases: &std::collections::HashSet<u32>,
) -> bool {
    aliases.contains(&local_id)
        || crate::expr::local_value_alias_root(ctx, local_id)
            == crate::expr::local_value_alias_root(ctx, arr_id)
}

fn expr_may_resolve_to_guarded_array_alias(
    ctx: &crate::expr::FnCtx<'_>,
    arr_id: u32,
    expr: &perry_hir::Expr,
    aliases: &std::collections::HashSet<u32>,
) -> bool {
    use perry_hir::Expr;
    match expr {
        Expr::LocalGet(id) => local_may_alias_guarded_array(ctx, arr_id, *id, aliases),
        Expr::LocalSet(_, value) => {
            expr_may_resolve_to_guarded_array_alias(ctx, arr_id, value, aliases)
        }
        Expr::Sequence(exprs) => exprs.last().is_some_and(|expr| {
            expr_may_resolve_to_guarded_array_alias(ctx, arr_id, expr, aliases)
        }),
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            expr_may_resolve_to_guarded_array_alias(ctx, arr_id, then_expr, aliases)
                || expr_may_resolve_to_guarded_array_alias(ctx, arr_id, else_expr, aliases)
        }
        _ => false,
    }
}

fn collect_guarded_array_alias_for_local_write(
    ctx: &crate::expr::FnCtx<'_>,
    arr_id: u32,
    target_id: u32,
    value: &perry_hir::Expr,
    aliases: &mut std::collections::HashSet<u32>,
) -> bool {
    target_id != arr_id
        && expr_may_resolve_to_guarded_array_alias(ctx, arr_id, value, aliases)
        && aliases.insert(target_id)
}

fn collect_guarded_array_aliases_in_stmts(
    ctx: &crate::expr::FnCtx<'_>,
    arr_id: u32,
    stmts: &[perry_hir::Stmt],
    aliases: &mut std::collections::HashSet<u32>,
) -> bool {
    stmts
        .iter()
        .any(|stmt| collect_guarded_array_aliases_in_stmt(ctx, arr_id, stmt, aliases))
}

fn collect_guarded_array_aliases_in_stmt(
    ctx: &crate::expr::FnCtx<'_>,
    arr_id: u32,
    stmt: &perry_hir::Stmt,
    aliases: &mut std::collections::HashSet<u32>,
) -> bool {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { id, init, .. } => init.as_ref().is_some_and(|expr| {
            collect_guarded_array_alias_for_local_write(ctx, arr_id, *id, expr, aliases)
                | collect_guarded_array_aliases_in_expr(ctx, arr_id, expr, aliases)
        }),
        Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
            collect_guarded_array_aliases_in_expr(ctx, arr_id, expr, aliases)
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => false,
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_guarded_array_aliases_in_expr(ctx, arr_id, condition, aliases)
                | collect_guarded_array_aliases_in_stmts(ctx, arr_id, then_branch, aliases)
                | else_branch.as_ref().is_some_and(|body| {
                    collect_guarded_array_aliases_in_stmts(ctx, arr_id, body, aliases)
                })
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_guarded_array_aliases_in_expr(ctx, arr_id, condition, aliases)
                | collect_guarded_array_aliases_in_stmts(ctx, arr_id, body, aliases)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref().is_some_and(|stmt| {
                collect_guarded_array_aliases_in_stmt(ctx, arr_id, stmt, aliases)
            }) | condition.as_ref().is_some_and(|expr| {
                collect_guarded_array_aliases_in_expr(ctx, arr_id, expr, aliases)
            }) | update.as_ref().is_some_and(|expr| {
                collect_guarded_array_aliases_in_expr(ctx, arr_id, expr, aliases)
            }) | collect_guarded_array_aliases_in_stmts(ctx, arr_id, body, aliases)
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            collect_guarded_array_aliases_in_stmts(ctx, arr_id, body, aliases)
                | catch.as_ref().is_some_and(|catch| {
                    collect_guarded_array_aliases_in_stmts(ctx, arr_id, &catch.body, aliases)
                })
                | finally.as_ref().is_some_and(|body| {
                    collect_guarded_array_aliases_in_stmts(ctx, arr_id, body, aliases)
                })
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_guarded_array_aliases_in_expr(ctx, arr_id, discriminant, aliases)
                | cases.iter().any(|case| {
                    case.test.as_ref().is_some_and(|expr| {
                        collect_guarded_array_aliases_in_expr(ctx, arr_id, expr, aliases)
                    }) | collect_guarded_array_aliases_in_stmts(ctx, arr_id, &case.body, aliases)
                })
        }
        Stmt::Labeled { body, .. } => {
            collect_guarded_array_aliases_in_stmt(ctx, arr_id, body.as_ref(), aliases)
        }
    }
}

fn collect_guarded_array_aliases_in_expr(
    ctx: &crate::expr::FnCtx<'_>,
    arr_id: u32,
    expr: &perry_hir::Expr,
    aliases: &mut std::collections::HashSet<u32>,
) -> bool {
    use perry_hir::Expr;
    let mut changed = match expr {
        Expr::LocalSet(id, value) => {
            collect_guarded_array_alias_for_local_write(ctx, arr_id, *id, value, aliases)
        }
        _ => false,
    };
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        changed |= collect_guarded_array_aliases_in_expr(ctx, arr_id, child, aliases);
    });
    changed
}

/// Inspect a `for` loop's condition expression and body, and return
/// `Some(...)` if the loop is the well-known shape
/// `for (let i = ...; i < <arr>.length; ...) { body }` (or `<=`) AND the
/// body is provably free of operations that can change `arr.length`.
///
/// Also recognizes fixed-width native-buffer guards such as
/// `i + 4 <= buf.length`. The hoist descriptor keeps the LHS addend so the
/// fast condition remains `i + 4 <= len`, not `i <= len`.
///
/// The walker also accepts `arr[i] = expr` IndexSets where `i` is the
/// loop counter from a strict `<` condition — those are guaranteed
/// inbounds and therefore can't trigger the realloc slow path that would
/// extend `arr.length`. Under `<=`, `i == arr.length` is reachable, so
/// array writes must go through the normal extension-capable path.
///
/// The proof is intentionally disabled when the guarded array has a local alias
/// in scope, or when the loop/update creates one. The existing walker reasons
/// about one local id; accepting `const alias = arr; alias.push(...)` would let
/// a length mutation bypass both the cached-length slot and the derived
/// bounded-index facts.
fn classify_for_length_hoist(
    ctx: &crate::expr::FnCtx<'_>,
    cond: &perry_hir::Expr,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> Option<LengthHoist> {
    use perry_hir::{BinaryOp, CompareOp, Expr};
    let (op, left, right) = match cond {
        Expr::Compare { op, left, right } => (*op, left.as_ref(), right.as_ref()),
        _ => return None,
    };
    if !matches!(op, CompareOp::Lt | CompareOp::Le) {
        return None;
    }
    let arr_id = match right {
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => match object.as_ref() {
            Expr::LocalGet(id) => *id,
            _ => return None,
        },
        _ => return None,
    };
    if !array_length_receiver_is_loop_local(ctx, arr_id) {
        return None;
    }
    let guarded_aliases = guarded_array_aliases_for_loop(ctx, arr_id, update, body);
    let (bounded_idx_id, lhs_addend) = match left {
        Expr::LocalGet(id) => (*id, 0),
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(id), Expr::Integer(addend)) => {
                    let addend = if matches!(op, BinaryOp::Sub) {
                        addend.checked_neg()?
                    } else {
                        *addend
                    };
                    if !(0..=i32::MAX as i64).contains(&addend) {
                        return None;
                    }
                    (*id, addend as i32)
                }
                (Expr::Integer(addend), Expr::LocalGet(id)) if matches!(op, BinaryOp::Add) => {
                    if !(0..=i32::MAX as i64).contains(addend) {
                        return None;
                    }
                    (*id, *addend as i32)
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    let has_strict_bound = matches!(op, CompareOp::Lt) && lhs_addend == 0;
    if !body.iter().all(|s| {
        stmt_preserves_array_length(
            ctx,
            s,
            arr_id,
            bounded_idx_id,
            has_strict_bound,
            &guarded_aliases,
        )
    }) {
        return None;
    }
    if update.is_some_and(|e| {
        !expr_preserves_array_length(ctx, e, arr_id, u32::MAX, false, &guarded_aliases)
    }) {
        return None;
    }
    let buffer_bounds_width_units = match op {
        CompareOp::Lt => i64::from(lhs_addend).checked_add(1),
        CompareOp::Le => Some(i64::from(lhs_addend)),
        _ => None,
    }
    .filter(|width| *width >= 1 && *width <= u32::MAX as i64)
    .map(|width| width as u32);
    Some(LengthHoist {
        arr_id,
        counter_id: bounded_idx_id,
        op,
        lhs_addend,
        buffer_bounds_width_units,
    })
}

fn classify_for_length_hoist_rejection(
    ctx: &crate::expr::FnCtx<'_>,
    cond: &perry_hir::Expr,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> Option<LengthHoistRejection> {
    use perry_hir::{BinaryOp, CompareOp, Expr};
    let (op, left, right) = match cond {
        Expr::Compare { op, left, right } => (*op, left.as_ref(), right.as_ref()),
        _ => return None,
    };
    if !matches!(op, CompareOp::Lt | CompareOp::Le) {
        return None;
    }
    let arr_id = match right {
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => match object.as_ref() {
            Expr::LocalGet(id) => *id,
            _ => return None,
        },
        _ => return None,
    };
    let receiver_has_materialization_hazard = ctx.native_facts.has_materialization_hazard(arr_id);
    if !array_length_receiver_is_loop_local(ctx, arr_id) && !receiver_has_materialization_hazard {
        return None;
    }
    let (bounded_idx_id, lhs_addend) = match left {
        Expr::LocalGet(id) => (*id, 0),
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(id), Expr::Integer(addend)) => {
                    let addend = if matches!(op, BinaryOp::Sub) {
                        addend.checked_neg()?
                    } else {
                        *addend
                    };
                    if !(0..=i32::MAX as i64).contains(&addend) {
                        return None;
                    }
                    (*id, addend as i32)
                }
                (Expr::Integer(addend), Expr::LocalGet(id)) if matches!(op, BinaryOp::Add) => {
                    if !(0..=i32::MAX as i64).contains(addend) {
                        return None;
                    }
                    (*id, *addend as i32)
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    let has_strict_bound = matches!(op, CompareOp::Lt) && lhs_addend == 0;
    let guarded_aliases = guarded_array_aliases_for_loop(ctx, arr_id, update, body);
    let body_effect = stmts_array_length_effect(
        ctx,
        body,
        arr_id,
        bounded_idx_id,
        has_strict_bound,
        &guarded_aliases,
    );
    if body_effect != LoopArrayLengthEffect::Preserves {
        return Some(LengthHoistRejection {
            arr_id,
            effect: body_effect,
        });
    }
    if let Some(update) = update {
        let update_effect =
            expr_array_length_effect(ctx, update, arr_id, u32::MAX, false, &guarded_aliases);
        if update_effect != LoopArrayLengthEffect::Preserves {
            return Some(LengthHoistRejection {
                arr_id,
                effect: update_effect,
            });
        }
    }
    if receiver_has_materialization_hazard {
        return Some(LengthHoistRejection {
            arr_id,
            effect: LoopArrayLengthEffect::MaterializationHazard,
        });
    }
    None
}

fn array_length_receiver_is_loop_local(ctx: &crate::expr::FnCtx<'_>, arr_id: u32) -> bool {
    ctx.locals.contains_key(&arr_id)
        && !ctx.boxed_vars.contains(&arr_id)
        && !ctx.module_globals.contains_key(&arr_id)
        && !ctx.scalar_replaced_arrays.contains_key(&arr_id)
        && !ctx.native_facts.has_materialization_hazard(arr_id)
}

/// Inspect a `for` loop's condition and return `Some((counter_id, bound_id,
/// op))` if the condition is the shape `counter < bound` (or `<=`) where
/// both sides are `LocalGet` ids, the counter is in `integer_locals`, and the
/// bound is an accessible, loop-invariant local that is statically safe to
/// materialize as signed i32.
///
/// Used by `lower_for` to enable the same i32 counter specialization as
/// the `i < arr.length` peephole (`classify_for_length_hoist`) on the
/// common case where the loop bound is a local variable with a proven i32
/// representation. Ambiguous `number`/`any` bounds are handled by the guarded
/// dynamic classifier or the generic JS comparison path instead.
pub(crate) fn classify_for_local_bound(
    cond: &perry_hir::Expr,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
    ctx: &crate::expr::FnCtx<'_>,
) -> Option<(u32, u32, perry_hir::CompareOp)> {
    use perry_hir::{CompareOp, Expr};
    let (op, left, right) = match cond {
        Expr::Compare { op, left, right } => (*op, left.as_ref(), right.as_ref()),
        _ => return None,
    };
    if !matches!(op, CompareOp::Lt | CompareOp::Le) {
        return None;
    }
    let counter_id = match left {
        Expr::LocalGet(id) => *id,
        _ => return None,
    };
    let bound_id = match right {
        Expr::LocalGet(id) => *id,
        _ => return None,
    };
    // Counter must be provably integer-valued (initialized from integer
    // literal, only mutated by Update ++/--).
    if !ctx.integer_locals.contains(&counter_id) {
        return None;
    }
    // Bound is safe to hoist only when it is both i32-proven and loop
    // invariant. A `number`-typed local can hold 1.5/NaN/Infinity at runtime;
    // using unguarded `fptosi` for those values changes JS trip counts.
    if !local_bound_storage_accessible(ctx, bound_id)
        || !local_bound_is_loop_invariant(cond, update, body, bound_id)
        || !local_bound_can_use_static_i32(ctx, bound_id)
    {
        return None;
    }
    Some((counter_id, bound_id, op))
}

/// Like [`classify_for_local_bound`], but for the case the static classifier
/// deliberately rejects: an `i < n` / `i <= n` loop whose bound `n` is an
/// accessible (unboxed, non-module-global), loop-invariant local that is not
/// statically proven safe for unguarded `fptosi`.
///
/// The caller emits a one-time finite-integral-i32 guard at the loop head and
/// runs the `icmp slt/sle i32` fast loop only when the guard holds. Non-number,
/// NaN, infinity, fractional, and out-of-i32-range bounds fall back to the
/// generic per-iteration comparison, preserving JS semantics.
pub(crate) fn classify_for_local_bound_dynamic(
    cond: &perry_hir::Expr,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
    ctx: &crate::expr::FnCtx<'_>,
) -> Option<(u32, u32, perry_hir::CompareOp)> {
    use perry_hir::{CompareOp, Expr};
    let (op, left, right) = match cond {
        Expr::Compare { op, left, right } => (*op, left.as_ref(), right.as_ref()),
        _ => return None,
    };
    if !matches!(op, CompareOp::Lt | CompareOp::Le) {
        return None;
    }
    let counter_id = match left {
        Expr::LocalGet(id) => *id,
        _ => return None,
    };
    let bound_id = match right {
        Expr::LocalGet(id) => *id,
        _ => return None,
    };
    if !ctx.integer_locals.contains(&counter_id) {
        return None;
    }
    if !local_bound_storage_accessible(ctx, bound_id)
        || !local_bound_is_loop_invariant(cond, update, body, bound_id)
    {
        return None;
    }
    Some((counter_id, bound_id, op))
}

fn local_bound_storage_accessible(ctx: &crate::expr::FnCtx<'_>, bound_id: u32) -> bool {
    ctx.locals.contains_key(&bound_id)
        && !ctx.boxed_vars.contains(&bound_id)
        && !ctx.module_globals.contains_key(&bound_id)
}

fn local_bound_is_loop_invariant(
    cond: &perry_hir::Expr,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
    bound_id: u32,
) -> bool {
    !expr_mutates_local(cond, bound_id)
        && update.is_none_or(|expr| !expr_mutates_local(expr, bound_id))
        && !stmts_mutate_local(body, bound_id)
}

fn local_bound_can_use_static_i32(ctx: &crate::expr::FnCtx<'_>, bound_id: u32) -> bool {
    if ctx.integer_locals.contains(&bound_id)
        && crate::expr::int_range_expr(ctx, &perry_hir::Expr::LocalGet(bound_id))
            .is_some_and(|range| range.min >= i32::MIN as i64 && range.max <= i32::MAX as i64)
    {
        return true;
    }
    min_length_bound_can_use_static_i32(ctx, bound_id)
}

fn min_length_bound_can_use_static_i32(ctx: &crate::expr::FnCtx<'_>, bound_id: u32) -> bool {
    let Some(buffer_ids) = ctx.min_length_bounds.get(&bound_id) else {
        return false;
    };
    !buffer_ids.is_empty()
        && buffer_ids.iter().all(|buffer_id| {
            ctx.buffer_view_slots
                .get(buffer_id)
                .and_then(|view| view.length_source.as_ref())
                .is_some_and(|source| length_source_can_use_static_i32(ctx, source))
        })
}

fn length_source_can_use_static_i32(ctx: &crate::expr::FnCtx<'_>, source: &LengthSource) -> bool {
    match source {
        LengthSource::Constant(n) => (0..=i64::from(i32::MAX)).contains(n),
        LengthSource::Local { id, addend } => {
            let Some(range) = crate::expr::int_range_expr(ctx, &perry_hir::Expr::LocalGet(*id))
            else {
                return false;
            };
            range
                .min
                .checked_add(*addend)
                .zip(range.max.checked_add(*addend))
                .is_some_and(|(min, max)| min >= 0 && max <= i64::from(i32::MAX))
        }
        LengthSource::Unknown => false,
    }
}

fn loop_counter_bounds_are_safe(
    ctx: &crate::expr::FnCtx<'_>,
    counter_id: u32,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> bool {
    loop_counter_is_nonnegative_at_entry(ctx, counter_id)
        && update_is_absent_or_counter_increment(update, counter_id)
        && !stmts_mutate_local(body, counter_id)
}

fn loop_counter_entry_i32_range_is_safe(init: Option<&perry_hir::Stmt>, counter_id: u32) -> bool {
    use perry_hir::{Expr, Stmt};
    let Some(Stmt::Let {
        id,
        init: Some(init),
        ..
    }) = init
    else {
        return false;
    };
    if *id != counter_id {
        return false;
    }
    match init {
        Expr::Integer(n) => (0..=i64::from(i32::MAX)).contains(n),
        Expr::Number(n) => {
            n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n <= f64::from(i32::MAX)
        }
        _ => false,
    }
}

fn loop_counter_is_nonnegative_at_entry(ctx: &crate::expr::FnCtx<'_>, counter_id: u32) -> bool {
    ctx.nonnegative_integer_locals.contains(&counter_id)
        || crate::expr::int_range_expr(ctx, &perry_hir::Expr::LocalGet(counter_id))
            .is_some_and(|range| range.min >= 0)
}

fn update_is_absent_or_counter_increment(
    update: Option<&perry_hir::Expr>,
    counter_id: u32,
) -> bool {
    use perry_hir::{Expr, UpdateOp};
    update.is_none_or(|expr| {
        matches!(
            expr,
            Expr::Update {
                id,
                op: UpdateOp::Increment,
                ..
            } if *id == counter_id
        )
    })
}

fn stmts_mutate_local(stmts: &[perry_hir::Stmt], local_id: u32) -> bool {
    stmts.iter().any(|stmt| stmt_mutates_local(stmt, local_id))
}

fn stmt_mutates_local(stmt: &perry_hir::Stmt, local_id: u32) -> bool {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { init, .. } => init
            .as_ref()
            .is_some_and(|expr| expr_mutates_local(expr, local_id)),
        Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
            expr_mutates_local(expr, local_id)
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => false,
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_mutates_local(condition, local_id)
                || stmts_mutate_local(then_branch, local_id)
                || else_branch
                    .as_ref()
                    .is_some_and(|body| stmts_mutate_local(body, local_id))
        }
        Stmt::While { condition, body } => {
            expr_mutates_local(condition, local_id) || stmts_mutate_local(body, local_id)
        }
        Stmt::DoWhile { body, condition } => {
            stmts_mutate_local(body, local_id) || expr_mutates_local(condition, local_id)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .is_some_and(|stmt| stmt_mutates_local(stmt.as_ref(), local_id))
                || condition
                    .as_ref()
                    .is_some_and(|expr| expr_mutates_local(expr, local_id))
                || update
                    .as_ref()
                    .is_some_and(|expr| expr_mutates_local(expr, local_id))
                || stmts_mutate_local(body, local_id)
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            stmts_mutate_local(body, local_id)
                || catch
                    .as_ref()
                    .is_some_and(|catch| stmts_mutate_local(&catch.body, local_id))
                || finally
                    .as_ref()
                    .is_some_and(|body| stmts_mutate_local(body, local_id))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_mutates_local(discriminant, local_id)
                || cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(|expr| expr_mutates_local(expr, local_id))
                        || stmts_mutate_local(&case.body, local_id)
                })
        }
        Stmt::Labeled { body, .. } => stmt_mutates_local(body.as_ref(), local_id),
    }
}

fn expr_mutates_local(expr: &perry_hir::Expr, local_id: u32) -> bool {
    use perry_hir::Expr;
    match expr {
        Expr::LocalSet(id, value) => *id == local_id || expr_mutates_local(value, local_id),
        Expr::Update { id, .. } => *id == local_id,
        Expr::Closure { params, body, .. } => {
            params.iter().any(|param| {
                param
                    .default
                    .as_ref()
                    .is_some_and(|expr| expr_mutates_local(expr, local_id))
            }) || stmts_mutate_local(body, local_id)
        }
        _ => {
            let mut found = false;
            perry_hir::walker::walk_expr_children(expr, &mut |child| {
                if !found && expr_mutates_local(child, local_id) {
                    found = true;
                }
            });
            found
        }
    }
}

fn classify_for_counter_range(
    init: Option<&perry_hir::Stmt>,
    cond: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
    ctx: &crate::expr::FnCtx<'_>,
    scope_id: u32,
) -> Option<IntRangeFact> {
    use perry_hir::{CompareOp, Expr, Stmt, UpdateOp};
    let (counter_id, start) = match init? {
        Stmt::Let {
            id,
            init: Some(Expr::Integer(start)),
            ..
        } => (*id, *start),
        _ => return None,
    };
    let Expr::Compare { op, left, right } = cond? else {
        return None;
    };
    if !matches!(op, CompareOp::Lt | CompareOp::Le) {
        return None;
    }
    if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == counter_id) {
        return None;
    }
    if !matches!(
        update?,
        Expr::Update {
            id,
            op: UpdateOp::Increment,
            ..
        } if *id == counter_id
    ) {
        return None;
    }
    if let Expr::LocalGet(bound_id) = right.as_ref() {
        if !local_bound_is_loop_invariant(cond?, update, body, *bound_id) {
            return None;
        }
    }
    let bound_range = crate::expr::int_range_expr(ctx, right)?;
    if bound_range.min != bound_range.max {
        return None;
    }
    let upper = bound_range
        .max
        .checked_sub(if matches!(op, CompareOp::Lt) { 1 } else { 0 })?;
    if start <= upper {
        Some(IntRangeFact {
            local_id: counter_id,
            scope_id,
            range: crate::expr::IntRange {
                min: start,
                max: upper,
            },
        })
    } else {
        None
    }
}

fn first_blocking_loop_effect<I>(effects: I) -> LoopArrayLengthEffect
where
    I: IntoIterator<Item = LoopArrayLengthEffect>,
{
    effects
        .into_iter()
        .find(|effect| *effect != LoopArrayLengthEffect::Preserves)
        .unwrap_or(LoopArrayLengthEffect::Preserves)
}

fn stmts_array_length_effect(
    ctx: &crate::expr::FnCtx<'_>,
    stmts: &[perry_hir::Stmt],
    arr_id: u32,
    bounded_idx_id: u32,
    has_strict_bound: bool,
    aliases: &std::collections::HashSet<u32>,
) -> LoopArrayLengthEffect {
    first_blocking_loop_effect(stmts.iter().map(|stmt| {
        stmt_array_length_effect(ctx, stmt, arr_id, bounded_idx_id, has_strict_bound, aliases)
    }))
}

fn stmt_array_length_effect(
    ctx: &crate::expr::FnCtx<'_>,
    s: &perry_hir::Stmt,
    arr_id: u32,
    bounded_idx_id: u32,
    has_strict_bound: bool,
    aliases: &std::collections::HashSet<u32>,
) -> LoopArrayLengthEffect {
    use perry_hir::Stmt;
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) => {
            expr_array_length_effect(ctx, e, arr_id, bounded_idx_id, has_strict_bound, aliases)
        }
        Stmt::Return(opt) => opt.as_ref().map_or(LoopArrayLengthEffect::Preserves, |e| {
            expr_array_length_effect(ctx, e, arr_id, bounded_idx_id, has_strict_bound, aliases)
        }),
        Stmt::Let { init, .. } => init.as_ref().map_or(LoopArrayLengthEffect::Preserves, |e| {
            expr_array_length_effect(ctx, e, arr_id, bounded_idx_id, has_strict_bound, aliases)
        }),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => first_blocking_loop_effect(
            std::iter::once(expr_array_length_effect(
                ctx,
                condition,
                arr_id,
                bounded_idx_id,
                has_strict_bound,
                aliases,
            ))
            .chain(then_branch.iter().map(|stmt| {
                stmt_array_length_effect(
                    ctx,
                    stmt,
                    arr_id,
                    bounded_idx_id,
                    has_strict_bound,
                    aliases,
                )
            }))
            .chain(else_branch.iter().flat_map(|body| {
                body.iter().map(|stmt| {
                    stmt_array_length_effect(
                        ctx,
                        stmt,
                        arr_id,
                        bounded_idx_id,
                        has_strict_bound,
                        aliases,
                    )
                })
            })),
        ),
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            first_blocking_loop_effect(
                std::iter::once(expr_array_length_effect(
                    ctx,
                    condition,
                    arr_id,
                    bounded_idx_id,
                    has_strict_bound,
                    aliases,
                ))
                .chain(body.iter().map(|stmt| {
                    stmt_array_length_effect(
                        ctx,
                        stmt,
                        arr_id,
                        bounded_idx_id,
                        has_strict_bound,
                        aliases,
                    )
                })),
            )
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => first_blocking_loop_effect(
            init.iter()
                .map(|stmt| {
                    stmt_array_length_effect(
                        ctx,
                        stmt,
                        arr_id,
                        bounded_idx_id,
                        has_strict_bound,
                        aliases,
                    )
                })
                .chain(condition.iter().map(|expr| {
                    expr_array_length_effect(
                        ctx,
                        expr,
                        arr_id,
                        bounded_idx_id,
                        has_strict_bound,
                        aliases,
                    )
                }))
                .chain(update.iter().map(|expr| {
                    expr_array_length_effect(
                        ctx,
                        expr,
                        arr_id,
                        bounded_idx_id,
                        has_strict_bound,
                        aliases,
                    )
                }))
                .chain(body.iter().map(|stmt| {
                    stmt_array_length_effect(
                        ctx,
                        stmt,
                        arr_id,
                        bounded_idx_id,
                        has_strict_bound,
                        aliases,
                    )
                })),
        ),
        Stmt::Try {
            body,
            catch,
            finally,
        } => first_blocking_loop_effect(
            body.iter()
                .map(|stmt| {
                    stmt_array_length_effect(
                        ctx,
                        stmt,
                        arr_id,
                        bounded_idx_id,
                        has_strict_bound,
                        aliases,
                    )
                })
                .chain(catch.iter().flat_map(|catch| {
                    catch.body.iter().map(|stmt| {
                        stmt_array_length_effect(
                            ctx,
                            stmt,
                            arr_id,
                            bounded_idx_id,
                            has_strict_bound,
                            aliases,
                        )
                    })
                }))
                .chain(finally.iter().flat_map(|body| {
                    body.iter().map(|stmt| {
                        stmt_array_length_effect(
                            ctx,
                            stmt,
                            arr_id,
                            bounded_idx_id,
                            has_strict_bound,
                            aliases,
                        )
                    })
                })),
        ),
        Stmt::Switch {
            discriminant,
            cases,
        } => first_blocking_loop_effect(
            std::iter::once(expr_array_length_effect(
                ctx,
                discriminant,
                arr_id,
                bounded_idx_id,
                has_strict_bound,
                aliases,
            ))
            .chain(cases.iter().flat_map(|case| {
                case.test
                    .iter()
                    .map(|expr| {
                        expr_array_length_effect(
                            ctx,
                            expr,
                            arr_id,
                            bounded_idx_id,
                            has_strict_bound,
                            aliases,
                        )
                    })
                    .chain(case.body.iter().map(|stmt| {
                        stmt_array_length_effect(
                            ctx,
                            stmt,
                            arr_id,
                            bounded_idx_id,
                            has_strict_bound,
                            aliases,
                        )
                    }))
            })),
        ),
        Stmt::Labeled { body, .. } => stmt_array_length_effect(
            ctx,
            body.as_ref(),
            arr_id,
            bounded_idx_id,
            has_strict_bound,
            aliases,
        ),
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {
            LoopArrayLengthEffect::Preserves
        }
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {
            LoopArrayLengthEffect::Preserves
        }
    }
}

fn expr_array_length_effect(
    ctx: &crate::expr::FnCtx<'_>,
    e: &perry_hir::Expr,
    arr_id: u32,
    bounded_idx_id: u32,
    has_strict_bound: bool,
    aliases: &std::collections::HashSet<u32>,
) -> LoopArrayLengthEffect {
    use perry_hir::{ArrayElement, Expr};
    let walk = |sub: &Expr| {
        expr_array_length_effect(ctx, sub, arr_id, bounded_idx_id, has_strict_bound, aliases)
    };
    match e {
        Expr::ArrayPush { array_id, value } => {
            if local_may_alias_guarded_array(ctx, arr_id, *array_id, aliases) {
                LoopArrayLengthEffect::AliasLengthMutation
            } else {
                walk(value)
            }
        }
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
            if local_may_alias_guarded_array(ctx, arr_id, *id, aliases) {
                LoopArrayLengthEffect::AliasLengthMutation
            } else {
                LoopArrayLengthEffect::Preserves
            }
        }
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            if local_may_alias_guarded_array(ctx, arr_id, *array_id, aliases) {
                LoopArrayLengthEffect::AliasLengthMutation
            } else {
                first_blocking_loop_effect(
                    std::iter::once(walk(start))
                        .chain(delete_count.iter().map(|expr| walk(expr)))
                        .chain(items.iter().map(walk)),
                )
            }
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if local_may_alias_guarded_array(ctx, arr_id, *id, aliases) {
                    if has_strict_bound
                        && matches!(index.as_ref(), Expr::LocalGet(idx_id) if *idx_id == bounded_idx_id)
                    {
                        return walk(value);
                    }
                    return LoopArrayLengthEffect::ArrayLengthMutation;
                }
            }
            first_blocking_loop_effect([walk(object), walk(index), walk(value)])
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            let target_is_arr = matches!(target.as_ref(), Expr::LocalGet(id) if local_may_alias_guarded_array(ctx, arr_id, *id, aliases));
            let receiver_is_arr = matches!(receiver.as_ref(), Expr::LocalGet(id) if local_may_alias_guarded_array(ctx, arr_id, *id, aliases));
            if target_is_arr || receiver_is_arr {
                if target_is_arr
                    && receiver_is_arr
                    && has_strict_bound
                    && matches!(key.as_ref(), Expr::LocalGet(idx_id) if *idx_id == bounded_idx_id)
                {
                    return walk(value);
                }
                return LoopArrayLengthEffect::DynamicPropertyWrite;
            }
            first_blocking_loop_effect([walk(target), walk(key), walk(value), walk(receiver)])
        }
        Expr::LocalSet(id, value) => {
            if *id == arr_id || *id == bounded_idx_id {
                LoopArrayLengthEffect::Reassignment
            } else {
                walk(value)
            }
        }
        Expr::Update { id, .. } => {
            if *id == arr_id || *id == bounded_idx_id {
                LoopArrayLengthEffect::Reassignment
            } else {
                LoopArrayLengthEffect::Preserves
            }
        }
        Expr::Call { callee, args, .. } => {
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if is_buffer_numeric_read_method(property) && is_static_buffer_receiver(ctx, object)
                {
                    return first_blocking_loop_effect(
                        std::iter::once(walk(object)).chain(args.iter().map(walk)),
                    );
                }
            }
            LoopArrayLengthEffect::UnknownCallEscape
        }
        Expr::NativeMethodCall {
            object: Some(object),
            method,
            args,
            ..
        } => {
            if is_buffer_numeric_read_method(method) && is_static_buffer_receiver(ctx, object) {
                first_blocking_loop_effect(
                    std::iter::once(walk(object)).chain(args.iter().map(walk)),
                )
            } else {
                LoopArrayLengthEffect::UnknownCallEscape
            }
        }
        Expr::NativeMethodCall { .. } | Expr::CallSpread { .. } => {
            LoopArrayLengthEffect::UnknownCallEscape
        }
        Expr::Closure { .. } => LoopArrayLengthEffect::UnknownCallEscape,
        Expr::Await(operand) | Expr::QueueMicrotask(operand) => {
            let operand_effect = walk(operand);
            if operand_effect != LoopArrayLengthEffect::Preserves {
                operand_effect
            } else {
                LoopArrayLengthEffect::AsyncMicrotask
            }
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            first_blocking_loop_effect([walk(left), walk(right)])
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => walk(operand),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => first_blocking_loop_effect([walk(condition), walk(then_expr), walk(else_expr)]),
        Expr::PropertyGet { object, .. } => walk(object),
        Expr::PropertySet { .. } => LoopArrayLengthEffect::DynamicPropertyWrite,
        Expr::IndexGet { object, index } => first_blocking_loop_effect([walk(object), walk(index)]),
        Expr::Uint8ArrayGet { array, index } => {
            first_blocking_loop_effect([walk(array), walk(index)])
        }
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => first_blocking_loop_effect([walk(array), walk(index), walk(value)]),
        Expr::BufferIndexGet { buffer, index } => {
            first_blocking_loop_effect([walk(buffer), walk(index)])
        }
        Expr::BufferIndexSet {
            buffer,
            index,
            value,
        } => first_blocking_loop_effect([walk(buffer), walk(index), walk(value)]),
        Expr::MathImul(a, b) | Expr::MathPow(a, b) => {
            first_blocking_loop_effect([walk(a), walk(b)])
        }
        Expr::MathMin(elems) | Expr::MathMax(elems) => {
            first_blocking_loop_effect(elems.iter().map(walk))
        }
        Expr::MathAbs(a)
        | Expr::MathSqrt(a)
        | Expr::MathFloor(a)
        | Expr::MathCeil(a)
        | Expr::MathRound(a)
        | Expr::MathTrunc(a)
        | Expr::MathSign(a)
        | Expr::MathF16round(a) => walk(a),
        Expr::Array(elements) => first_blocking_loop_effect(elements.iter().map(|expr| {
            if expr_may_resolve_to_guarded_array_alias(ctx, arr_id, expr, aliases) {
                LoopArrayLengthEffect::AggregateAliasEscape
            } else {
                walk(expr)
            }
        })),
        Expr::ArraySpread(elements) => {
            first_blocking_loop_effect(elements.iter().map(|el| match el {
                ArrayElement::Expr(e) => {
                    if expr_may_resolve_to_guarded_array_alias(ctx, arr_id, e, aliases) {
                        LoopArrayLengthEffect::AggregateAliasEscape
                    } else {
                        walk(e)
                    }
                }
                ArrayElement::Spread(e) => walk(e),
                ArrayElement::Hole => LoopArrayLengthEffect::Preserves,
            }))
        }
        Expr::Object(fields) => first_blocking_loop_effect(fields.iter().map(|(_, value)| {
            if expr_may_resolve_to_guarded_array_alias(ctx, arr_id, value, aliases) {
                LoopArrayLengthEffect::AggregateAliasEscape
            } else {
                walk(value)
            }
        })),
        Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::String(_)
        | Expr::WtfString(_) => LoopArrayLengthEffect::Preserves,
        _ => LoopArrayLengthEffect::UnsupportedExpression,
    }
}

pub(crate) fn stmt_preserves_array_length(
    ctx: &crate::expr::FnCtx<'_>,
    s: &perry_hir::Stmt,
    arr_id: u32,
    bounded_idx_id: u32,
    has_strict_bound: bool,
    aliases: &std::collections::HashSet<u32>,
) -> bool {
    use perry_hir::Stmt;
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) => {
            expr_preserves_array_length(ctx, e, arr_id, bounded_idx_id, has_strict_bound, aliases)
        }
        Stmt::Return(opt) => opt.as_ref().is_none_or(|e| {
            expr_preserves_array_length(ctx, e, arr_id, bounded_idx_id, has_strict_bound, aliases)
        }),
        Stmt::Let { init, .. } => init.as_ref().is_none_or(|e| {
            expr_preserves_array_length(ctx, e, arr_id, bounded_idx_id, has_strict_bound, aliases)
        }),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_preserves_array_length(
                ctx,
                condition,
                arr_id,
                bounded_idx_id,
                has_strict_bound,
                aliases,
            ) && then_branch.iter().all(|s| {
                stmt_preserves_array_length(
                    ctx,
                    s,
                    arr_id,
                    bounded_idx_id,
                    has_strict_bound,
                    aliases,
                )
            }) && else_branch.as_ref().is_none_or(|b| {
                b.iter().all(|s| {
                    stmt_preserves_array_length(
                        ctx,
                        s,
                        arr_id,
                        bounded_idx_id,
                        has_strict_bound,
                        aliases,
                    )
                })
            })
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_preserves_array_length(
                ctx,
                condition,
                arr_id,
                bounded_idx_id,
                has_strict_bound,
                aliases,
            ) && body.iter().all(|s| {
                stmt_preserves_array_length(
                    ctx,
                    s,
                    arr_id,
                    bounded_idx_id,
                    has_strict_bound,
                    aliases,
                )
            })
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref().is_none_or(|s| {
                stmt_preserves_array_length(
                    ctx,
                    s,
                    arr_id,
                    bounded_idx_id,
                    has_strict_bound,
                    aliases,
                )
            }) && condition.as_ref().is_none_or(|e| {
                expr_preserves_array_length(
                    ctx,
                    e,
                    arr_id,
                    bounded_idx_id,
                    has_strict_bound,
                    aliases,
                )
            }) && update.as_ref().is_none_or(|e| {
                expr_preserves_array_length(
                    ctx,
                    e,
                    arr_id,
                    bounded_idx_id,
                    has_strict_bound,
                    aliases,
                )
            }) && body.iter().all(|s| {
                stmt_preserves_array_length(
                    ctx,
                    s,
                    arr_id,
                    bounded_idx_id,
                    has_strict_bound,
                    aliases,
                )
            })
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().all(|s| {
                stmt_preserves_array_length(
                    ctx,
                    s,
                    arr_id,
                    bounded_idx_id,
                    has_strict_bound,
                    aliases,
                )
            }) && catch.as_ref().is_none_or(|c| {
                c.body.iter().all(|s| {
                    stmt_preserves_array_length(
                        ctx,
                        s,
                        arr_id,
                        bounded_idx_id,
                        has_strict_bound,
                        aliases,
                    )
                })
            }) && finally.as_ref().is_none_or(|b| {
                b.iter().all(|s| {
                    stmt_preserves_array_length(
                        ctx,
                        s,
                        arr_id,
                        bounded_idx_id,
                        has_strict_bound,
                        aliases,
                    )
                })
            })
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_preserves_array_length(
                ctx,
                discriminant,
                arr_id,
                bounded_idx_id,
                has_strict_bound,
                aliases,
            ) && cases.iter().all(|c| {
                c.test.as_ref().is_none_or(|e| {
                    expr_preserves_array_length(
                        ctx,
                        e,
                        arr_id,
                        bounded_idx_id,
                        has_strict_bound,
                        aliases,
                    )
                }) && c.body.iter().all(|s| {
                    stmt_preserves_array_length(
                        ctx,
                        s,
                        arr_id,
                        bounded_idx_id,
                        has_strict_bound,
                        aliases,
                    )
                })
            })
        }
        Stmt::Labeled { body, .. } => stmt_preserves_array_length(
            ctx,
            body.as_ref(),
            arr_id,
            bounded_idx_id,
            has_strict_bound,
            aliases,
        ),
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => true,
    }
}

fn is_static_buffer_receiver(ctx: &crate::expr::FnCtx<'_>, object: &perry_hir::Expr) -> bool {
    matches!(
        crate::type_analysis::static_type_of(ctx, object),
        Some(perry_types::Type::Named(name)) if name == "Buffer"
    )
}

fn is_buffer_numeric_read_method(method: &str) -> bool {
    matches!(
        method,
        "readUInt8"
            | "readUint8"
            | "readInt8"
            | "readUInt16BE"
            | "readUint16BE"
            | "readUInt16LE"
            | "readUint16LE"
            | "readInt16BE"
            | "readInt16LE"
            | "readUInt32BE"
            | "readUint32BE"
            | "readUInt32LE"
            | "readUint32LE"
            | "readInt32BE"
            | "readInt32LE"
            | "readFloatBE"
            | "readFloatLE"
            | "readDoubleBE"
            | "readDoubleLE"
    )
}

pub(crate) fn expr_preserves_array_length(
    ctx: &crate::expr::FnCtx<'_>,
    e: &perry_hir::Expr,
    arr_id: u32,
    bounded_idx_id: u32,
    has_strict_bound: bool,
    aliases: &std::collections::HashSet<u32>,
) -> bool {
    use perry_hir::{ArrayElement, Expr};
    let walk = |sub: &Expr| {
        expr_preserves_array_length(ctx, sub, arr_id, bounded_idx_id, has_strict_bound, aliases)
    };
    match e {
        Expr::ArrayPush { array_id, value } => {
            !local_may_alias_guarded_array(ctx, arr_id, *array_id, aliases) && walk(value)
        }
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
            !local_may_alias_guarded_array(ctx, arr_id, *id, aliases)
        }
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            !local_may_alias_guarded_array(ctx, arr_id, *array_id, aliases)
                && walk(start)
                && delete_count.as_ref().is_none_or(|e| walk(e))
                && items.iter().all(&walk)
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            // `arr[bounded_i] = expr` is the only IndexSet on `arr`
            // we accept, and only under a strict `i < arr.length`
            // guard. With `i <= arr.length`, `i == length` can extend
            // the array and invalidate a hoisted length.
            if let Expr::LocalGet(id) = object.as_ref() {
                if local_may_alias_guarded_array(ctx, arr_id, *id, aliases) {
                    if has_strict_bound {
                        if let Expr::LocalGet(idx_id) = index.as_ref() {
                            if *idx_id == bounded_idx_id {
                                return walk(value);
                            }
                        }
                    }
                    return false;
                }
            }
            walk(object) && walk(index) && walk(value)
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            let target_is_arr = matches!(target.as_ref(), Expr::LocalGet(id) if local_may_alias_guarded_array(ctx, arr_id, *id, aliases));
            let receiver_is_arr = matches!(receiver.as_ref(), Expr::LocalGet(id) if local_may_alias_guarded_array(ctx, arr_id, *id, aliases));
            if target_is_arr || receiver_is_arr {
                if target_is_arr && receiver_is_arr && has_strict_bound {
                    if let Expr::LocalGet(idx_id) = key.as_ref() {
                        if *idx_id == bounded_idx_id {
                            return walk(value);
                        }
                    }
                }
                return false;
            }
            walk(target) && walk(key) && walk(value) && walk(receiver)
        }
        // Reassigning the bounded index would invalidate the bound.
        // Reassigning the array variable would also invalidate (we'd
        // be tracking the wrong array).
        Expr::LocalSet(id, value) => *id != arr_id && *id != bounded_idx_id && walk(value),
        // Mutating either the array binding or the bounded index invalidates
        // the loop-local inbounds proof. The normal `for` update expression is
        // outside the body and is checked separately before facts are emitted.
        Expr::Update { id, .. } => *id != arr_id && *id != bounded_idx_id,
        // Calls are dynamic boundaries until an effect summary proves the
        // callee cannot mutate or expose the guarded array. Accepting
        // `mutate([arr])`, `mutate({ arr })`, or a closure captured from an
        // outer scope would make the cached length and bounded-index facts
        // unsound.
        Expr::Call { callee, args, .. } => {
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if is_buffer_numeric_read_method(property) && is_static_buffer_receiver(ctx, object)
                {
                    return walk(object) && args.iter().all(&walk);
                }
            }
            false
        }
        Expr::NativeMethodCall {
            object: Some(object),
            method,
            args,
            ..
        } => {
            is_buffer_numeric_read_method(method)
                && is_static_buffer_receiver(ctx, object)
                && walk(object)
                && args.iter().all(&walk)
        }
        Expr::NativeMethodCall { .. } | Expr::CallSpread { .. } => false,
        Expr::Closure { .. } => false,
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => walk(left) && walk(right),
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => walk(operand),
        // Await can resume after user code/microtasks have run, so it cannot
        // preserve cached array length or bounded-index facts without a future
        // effect summary for the awaited value.
        Expr::Await(_) => false,
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => walk(condition) && walk(then_expr) && walk(else_expr),
        Expr::PropertyGet { object, .. } => walk(object),
        // A property write can be `arr.length = ...`, can hit a setter, or can
        // otherwise run dynamic object semantics. Keep length hoisting behind a
        // future effect summary instead of assuming writes preserve the guarded
        // array length.
        Expr::PropertySet { .. } => false,
        Expr::IndexGet { object, index } => walk(object) && walk(index),
        // Buffer / Uint8Array reads + writes preserve the underlying array
        // length — Buffer.alloc allocates a fixed-capacity blob, and the
        // GEP-based fast path (`Expr::Uint8ArrayGet`/`Set`,
        // `Expr::BufferIndexGet`/`Set`) doesn't extend it. Without these
        // arms the default `_ => false` arm rejects bodies that touch
        // a Buffer, blocking the `i < dst.length` peephole on
        // `for (let i = 0; i < dst.length; i++) dst[i]` patterns —
        // image_convolution's FNV-1a checksum loop is the canonical
        // example, ~24M iterations through `fcmp olt double` instead of
        // `icmp slt i32`.
        Expr::Uint8ArrayGet { array, index } => walk(array) && walk(index),
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => walk(array) && walk(index) && walk(value),
        Expr::BufferIndexGet { buffer, index } => walk(buffer) && walk(index),
        Expr::BufferIndexSet {
            buffer,
            index,
            value,
        } => walk(buffer) && walk(index) && walk(value),
        // Pure arithmetic intrinsics — `Math.imul(a, b)` lowers to
        // `Expr::MathImul`, `Math.abs/sqrt/pow/floor/ceil/round` etc. all
        // bottom out as numeric ops with no side effects on the bounded
        // array. image_conv's FNV-1a body uses Math.imul and was rejecting
        // the peephole until this arm landed.
        Expr::MathImul(a, b) | Expr::MathPow(a, b) => walk(a) && walk(b),
        Expr::MathMin(elems) | Expr::MathMax(elems) => elems.iter().all(&walk),
        Expr::MathAbs(a)
        | Expr::MathSqrt(a)
        | Expr::MathFloor(a)
        | Expr::MathCeil(a)
        | Expr::MathRound(a)
        | Expr::MathTrunc(a)
        | Expr::MathSign(a)
        | Expr::MathF16round(a) => walk(a),
        Expr::Array(elements) => elements.iter().all(|expr| {
            !expr_may_resolve_to_guarded_array_alias(ctx, arr_id, expr, aliases) && walk(expr)
        }),
        Expr::ArraySpread(elements) => elements.iter().all(|el| match el {
            ArrayElement::Expr(e) => {
                !expr_may_resolve_to_guarded_array_alias(ctx, arr_id, e, aliases) && walk(e)
            }
            ArrayElement::Spread(e) => walk(e),
            ArrayElement::Hole => true,
        }),
        Expr::Object(fields) => fields.iter().all(|(_, v)| {
            !expr_may_resolve_to_guarded_array_alias(ctx, arr_id, v, aliases) && walk(v)
        }),
        Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::String(_)
        | Expr::WtfString(_) => true,
        // Default: conservative reject for HIR variants we haven't
        // analyzed. Better to lose the optimization than to silently
        // hoist past a body that mutates the array.
        _ => false,
    }
}

/// `while (cond) { body }` — classic 3-block CFG (cond / body / exit).
///
/// ```text
///   <current>:
///     br cond
///   while.cond:
///     <condition>
///     truthy → body, falsey → exit
///   while.body:
///     <body>
///     br cond                 ; if not already terminated
///   while.exit:
///     <continues here>
/// ```
///
/// No break/continue support yet — body must fall through to the next
/// loop iteration. Same limitation as `for`.
pub(crate) fn lower_while(
    ctx: &mut FnCtx<'_>,
    condition: &perry_hir::Expr,
    body: &[Stmt],
) -> Result<()> {
    let cond_idx = ctx.new_block("while.cond");
    let body_idx = ctx.new_block("while.body");
    let exit_idx = ctx.new_block("while.exit");

    let cond_label = ctx.block_label(cond_idx);
    let body_label = ctx.block_label(body_idx);
    let exit_label = ctx.block_label(exit_idx);

    ctx.block().br(&cond_label);

    ctx.current_block = cond_idx;
    let cv = lower_expr(ctx, condition)?;
    let i1 = lower_truthy(ctx, &cv, condition);
    ctx.block().cond_br(&i1, &body_label, &exit_label);

    // For while-loops, continue jumps back to the cond block.
    ctx.loop_targets
        .push((cond_label.clone(), exit_label.clone(), ctx.try_depth));
    let loop_proof_scope_id = ctx.next_loop_proof_scope_id();

    // Consume pending label (from enclosing Stmt::Labeled).
    let consumed_labels = std::mem::take(&mut ctx.pending_labels);
    let previous_region_id = ctx.active_region_id.clone();
    for lbl in &consumed_labels {
        ctx.label_targets.insert(
            lbl.clone(),
            (cond_label.clone(), exit_label.clone(), ctx.try_depth),
        );
    }
    if let Some(lbl) = consumed_labels.last() {
        ctx.active_region_id = Some(ctx.region_id_for_label(lbl));
    }

    if let Some(fact) = crate::expr::while_condition_range_fact(ctx, condition, loop_proof_scope_id)
    {
        ctx.int_range_facts.push(fact);
    }
    let mut guarded =
        crate::expr::guarded_buffer_indices_for_condition(ctx, condition, loop_proof_scope_id);
    guarded.retain(|fact| !stmts_mutate_local(body, fact.index_local_id));
    ctx.guarded_buffer_index_pairs.extend(guarded);

    ctx.current_block = body_idx;
    lower_stmts(ctx, body)?;
    clear_loop_body_shadow_slots(ctx, body);
    // Issue #74: see lower_for for rationale.
    if !ctx.block().is_terminated() && body_needs_asm_barrier(body) {
        ctx.block().asm_sideeffect_barrier();
    }
    if !ctx.block().is_terminated() {
        emit_gc_loop_safepoint(ctx);
        ctx.block().br(&cond_label);
    }
    ctx.active_region_id = previous_region_id;

    ctx.loop_targets.pop();
    ctx.guarded_buffer_index_pairs
        .retain(|fact| fact.scope_id != loop_proof_scope_id);
    ctx.int_range_facts
        .retain(|fact| fact.scope_id != loop_proof_scope_id);

    ctx.current_block = exit_idx;
    Ok(())
}

/// `do { body } while (cond)` — body runs at least once. Same blocks as
/// `while`, but the initial branch goes to body, not cond.
pub(crate) fn lower_do_while(
    ctx: &mut FnCtx<'_>,
    body: &[Stmt],
    condition: &perry_hir::Expr,
) -> Result<()> {
    let body_idx = ctx.new_block("dowhile.body");
    let cond_idx = ctx.new_block("dowhile.cond");
    let exit_idx = ctx.new_block("dowhile.exit");

    let body_label = ctx.block_label(body_idx);
    let cond_label = ctx.block_label(cond_idx);
    let exit_label = ctx.block_label(exit_idx);

    ctx.block().br(&body_label);

    // Push break/continue targets BEFORE compiling the body so nested
    // break/continue see them.
    ctx.loop_targets
        .push((cond_label.clone(), exit_label.clone(), ctx.try_depth));

    // Consume pending label (from enclosing Stmt::Labeled).
    let consumed_labels = std::mem::take(&mut ctx.pending_labels);
    let previous_region_id = ctx.active_region_id.clone();
    for lbl in &consumed_labels {
        ctx.label_targets.insert(
            lbl.clone(),
            (cond_label.clone(), exit_label.clone(), ctx.try_depth),
        );
    }
    if let Some(lbl) = consumed_labels.last() {
        ctx.active_region_id = Some(ctx.region_id_for_label(lbl));
    }

    ctx.current_block = body_idx;
    lower_stmts(ctx, body)?;
    clear_loop_body_shadow_slots(ctx, body);
    // Issue #74: see lower_for for rationale.
    if !ctx.block().is_terminated() && body_needs_asm_barrier(body) {
        ctx.block().asm_sideeffect_barrier();
    }
    if !ctx.block().is_terminated() {
        emit_gc_loop_safepoint(ctx);
        ctx.block().br(&cond_label);
    }

    ctx.current_block = cond_idx;
    let cv = lower_expr(ctx, condition)?;
    let i1 = lower_truthy(ctx, &cv, condition);
    ctx.block().cond_br(&i1, &body_label, &exit_label);
    ctx.active_region_id = previous_region_id;

    ctx.loop_targets.pop();

    ctx.current_block = exit_idx;
    Ok(())
}
