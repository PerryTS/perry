//! #6750 follow-up: masked-window versioning for STRAIGHT-LINE statement runs.
//!
//! The dense range-loop tiers (`loops.rs`) hoist per-access array-read guards
//! to the loop preheader — but bcryptjs ships `_encipher` fully UNROLLED: 16
//! Feistel rounds of `S[l >>> 24]` / `S[0x100 | ((l >> 16) & 0xff)]` /
//! `P[k]` reads as ~130 consecutive scalar statements with no loop to
//! version. This module applies the same speculation to a maximal run of
//! region-safe statements: probe the accessed arrays once at region entry,
//! branch into a fast copy whose masked reads are bare inline loads (via the
//! shared [`MaskedWindowArrayFact`] machinery), or fall through to the
//! ordinary per-access lowering.
//!
//! A region-safe statement is `Stmt::Expr` of a scalar `LocalSet` / `Update`
//! / pure expression — the same effect-free walk the dense loop matcher uses
//! (`packed_f64_range_loop_pure_expr_collect`): no calls, closures, awaits,
//! stores, or `Stmt::Let` (a Let lowered once per copy would leave post-region
//! reads pointing at only the last copy's alloca). Reads on ineligible
//! receivers (dynamic indices like `lr[off]`, non-array bindings) don't stop
//! the region — they simply lower per-access in every copy. An array binding
//! REASSIGNED inside the region is dropped from the eligible set, so its
//! reads keep full JS semantics.
//!
//! Tier chain (each copy duplicates the region, so only the two tiers that
//! matter are emitted — rarer shapes keep the per-access path):
//!   1. `ta_i32` — every eligible array probes as an Int32Array whose length
//!      covers the merged window (`js_typed_feedback_masked_window_ta_kind`,
//!      O(1)); loads are `load i32` through the hoisted data pointer.
//!   2. `plain_f64` — every eligible array passes the dense plain-array
//!      window guard (O(1) once the RawF64 layout flag is set; the dense-i32
//!      plain tier is deliberately NOT emitted here — its per-entry window
//!      scan is O(window), which a hot small function would pay on every
//!      call).
//!   3. slow — the untouched per-access lowering.
//!
//! Safety mirrors the dense-loop fast copies: the fast copies' statements
//! cannot write memory (no stores/calls admitted), typed-array storage never
//! moves and view backings are thread-lifetime allocations, and plain-array
//! loads re-derive the element base from the binding's slot at every access,
//! so a GC triggered by an allocating scalar op (string concat) cannot leave
//! a stale pointer behind.

use anyhow::Result;
use perry_hir::{Expr, Stmt};

use super::loops::{
    local_is_number_array, local_is_untyped_candidate, packed_f64_range_loop_pure_expr_collect,
    packed_loop_array_binding_storage_is_addressable, PackedF64RangeArrayAccess,
};
use super::{emit_shadow_clears_after_stmt, lower_stmt};
use crate::expr::{
    emit_typed_feedback_register_site, lower_expr, FnCtx, MaskedWindowArrayFact, MaskedWindowElem,
    TypedFeedbackContract, TypedFeedbackKind,
};
use crate::types::{DOUBLE, I1, I32, I64};

/// Minimum number of masked static-window reads on eligible arrays a region
/// must contain before the probe call + region duplication pays for itself.
/// `_encipher` has ~130; hand-rolled crypto/codec rounds have ≥ 16.
const REGION_MIN_TRACKED_READS: usize = 8;

/// Counter-id sentinel for the shared pure-expression walk: no HIR local uses
/// `u32::MAX`, so the walk's counter-relative arm never fires and every
/// tracked read must carry a static index window.
const REGION_NO_COUNTER: u32 = u32::MAX;

pub(super) struct MaskedWindowRegionArray {
    pub array_id: u32,
    /// Merged static window over every tracked read of this array.
    pub lo: i64,
    pub hi: i64,
}

/// One scheduled fast-copy type refinement: after lowering the statement at
/// `stmt_offset`, override (or restore) `local_id`'s static type.
pub(super) struct RegionRefinement {
    pub stmt_offset: usize,
    pub local_id: u32,
    /// `true` → set `Type::Number`; `false` → restore the original type (the
    /// local was reassigned a value we can no longer prove numeric).
    pub set_number: bool,
}

pub(super) struct MaskedWindowRegion {
    /// Number of consecutive statements the region consumes.
    pub len: usize,
    /// Eligible arrays (static-window reads only, never written in-region,
    /// addressable number-array or untyped bindings).
    pub arrays: Vec<MaskedWindowRegionArray>,
    /// Flow-ordered type refinements applied ONLY inside the fast copies:
    /// an untyped local written a provably-numeric value (a fact-covered
    /// read, or any ToNumber/ToInt32-producing operator) is `Type::Number`
    /// from that statement on, so downstream scalar ops lower numerically
    /// (inline coercion towers) instead of through the `js_dynamic_*`
    /// dispatch calls. The slow copy sees the original types — full dynamic
    /// semantics — and the fast copies compute identical VALUES for numeric
    /// inputs, which the entry guards established.
    pub refinements: Vec<RegionRefinement>,
}

/// True when `stmt` contains at least one `LocalGet`-received read with a
/// static index window — the cheap pre-filter that keeps the quadratic-ish
/// region scan off plain arithmetic runs.
fn stmt_has_masked_read(stmt: &Stmt) -> bool {
    fn expr_has(expr: &Expr) -> bool {
        if let Expr::IndexGet { object, index } = expr {
            if matches!(object.as_ref(), Expr::LocalGet(_))
                && crate::collectors::static_index_window(index).is_some()
            {
                return true;
            }
        }
        let mut found = false;
        perry_hir::walker::walk_expr_children(expr, &mut |child| {
            found = found || expr_has(child);
        });
        found
    }
    matches!(stmt, Stmt::Expr(expr) if expr_has(expr))
}

/// Count masked static-window reads on `eligible` arrays inside `expr`.
fn count_masked_reads(expr: &Expr, eligible: &std::collections::HashSet<u32>) -> usize {
    let mut count = 0;
    if let Expr::IndexGet { object, index } = expr {
        if let Expr::LocalGet(id) = object.as_ref() {
            if eligible.contains(id) && crate::collectors::static_index_window(index).is_some() {
                count += 1;
            }
        }
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        count += count_masked_reads(child, eligible);
    });
    count
}

/// True when `expr` provably evaluates to a JS number in a fast copy, under
/// `refined` (locals already proven number at this program point) and
/// `eligible` (arrays whose static-window reads the entry guard proved
/// numeric).
///
/// BigInt is the trap here: `1n * 1n`, `-1n`, `~1n`, `1n << 1n` are all
/// BigInts, so arithmetic/bitwise operators do NOT unconditionally produce
/// numbers. What IS sound is the mixed-type rule: when at least one operand
/// is a proven number, `-`/`*`/`/`/`%`/`**`/`&`/`|`/`^`/`<<`/`>>` either
/// produce a Number or THROW a TypeError ("cannot mix BigInt") — and a
/// statement that throws never completes, so its scheduled refinement is
/// unobservable. `>>>` and unary `+` throw on ANY BigInt operand, so they
/// are unconditionally number-or-throw; `+` (Add) needs BOTH sides proven
/// (string concatenation); `-x`/`~x` need the operand proven.
fn expr_is_number_under(
    ctx: &FnCtx<'_>,
    refined: &std::collections::HashSet<u32>,
    eligible: &std::collections::HashSet<u32>,
    expr: &Expr,
) -> bool {
    use perry_hir::{BinaryOp, UnaryOp};
    match expr {
        Expr::Number(_) | Expr::Integer(_) | Expr::NumberCoerce(_) => true,
        Expr::LocalGet(id) => {
            refined.contains(id)
                || matches!(
                    ctx.local_types.get(id),
                    Some(perry_hir::types::Type::Number | perry_hir::types::Type::Int32)
                )
        }
        Expr::IndexGet { object, index } => {
            matches!(object.as_ref(), Expr::LocalGet(id) if eligible.contains(id))
                && crate::collectors::static_index_window(index).is_some()
        }
        Expr::Binary { op, left, right } => match op {
            // ToUint32 has no BigInt form — `1n >>> 0n` throws — so the
            // result, when the statement completes, is always a Number.
            BinaryOp::UShr => true,
            // Number-or-throw when one side is a proven number: the BigInt
            // forms of these ops require BOTH operands BigInt (mixing
            // throws), and every non-BigInt primitive coerces to Number.
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Pow => {
                expr_is_number_under(ctx, refined, eligible, left)
                    || expr_is_number_under(ctx, refined, eligible, right)
            }
            BinaryOp::Add => {
                expr_is_number_under(ctx, refined, eligible, left)
                    && expr_is_number_under(ctx, refined, eligible, right)
            }
        },
        Expr::Unary { op, operand } => match op {
            // Unary `+` is ToNumber, which throws on BigInt.
            UnaryOp::Pos => true,
            // `-x` / `~x` on a BigInt yield BigInts — need the operand proven.
            UnaryOp::Neg | UnaryOp::BitNot => expr_is_number_under(ctx, refined, eligible, operand),
            UnaryOp::Not => false,
        },
        Expr::Conditional {
            condition: _,
            then_expr,
            else_expr,
        } => {
            expr_is_number_under(ctx, refined, eligible, then_expr)
                && expr_is_number_under(ctx, refined, eligible, else_expr)
        }
        Expr::Logical { left, right, .. } => {
            expr_is_number_under(ctx, refined, eligible, left)
                && expr_is_number_under(ctx, refined, eligible, right)
        }
        Expr::MathImul(_, _)
        | Expr::MathPow(_, _)
        | Expr::MathMin(_)
        | Expr::MathMax(_)
        | Expr::MathAbs(_)
        | Expr::MathSqrt(_)
        | Expr::MathFloor(_)
        | Expr::MathCeil(_)
        | Expr::MathRound(_)
        | Expr::MathTrunc(_)
        | Expr::MathSign(_)
        | Expr::MathF16round(_) => true,
        _ => false,
    }
}

/// Match a masked-window region starting at `stmts[0]`. Returns `None` when
/// the run is too short, tracks no eligible array, or carries fewer than
/// [`REGION_MIN_TRACKED_READS`] tracked reads.
pub(super) fn try_match_masked_window_region(
    ctx: &FnCtx<'_>,
    stmts: &[Stmt],
) -> Option<MaskedWindowRegion> {
    if !stmts.first().is_some_and(stmt_has_masked_read) {
        return None;
    }
    let mut accesses: std::collections::BTreeMap<u32, PackedF64RangeArrayAccess> =
        std::collections::BTreeMap::new();
    let mut written: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut len = 0usize;
    for stmt in stmts {
        let ok = match stmt {
            Stmt::Expr(Expr::LocalSet(id, value)) => {
                let mut trial = accesses.clone();
                if packed_f64_range_loop_pure_expr_collect(
                    value,
                    REGION_NO_COUNTER,
                    true,
                    &mut trial,
                ) {
                    accesses = trial;
                    written.insert(*id);
                    true
                } else {
                    false
                }
            }
            Stmt::Expr(Expr::Update { id, .. }) => {
                written.insert(*id);
                true
            }
            Stmt::Expr(expr) => {
                let mut trial = accesses.clone();
                if packed_f64_range_loop_pure_expr_collect(
                    expr,
                    REGION_NO_COUNTER,
                    true,
                    &mut trial,
                ) {
                    accesses = trial;
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if !ok {
            break;
        }
        len += 1;
    }
    if len == 0 || accesses.is_empty() {
        return None;
    }

    let mut arrays = Vec::new();
    for access in accesses.values() {
        // A binding written anywhere in the region (`S = T` rebinding, or a
        // tracked store) is dropped from the eligible set — its reads keep
        // the ordinary per-access lowering in every copy.
        if access.written || written.contains(&access.array_id) {
            continue;
        }
        if access.counter.is_some() {
            continue;
        }
        let Some((lo, hi)) = access.stat else {
            continue;
        };
        if lo < 0 || hi >= i64::from(i32::MAX) {
            continue;
        }
        if !packed_loop_array_binding_storage_is_addressable(ctx, access.array_id)
            || ctx.scalar_replaced_arrays.contains_key(&access.array_id)
        {
            continue;
        }
        // Already covered by an active fact (this run sits inside a dense
        // range-loop fast copy) — its reads inline through that fact; a
        // second, nested versioning would only add per-iteration probes.
        if ctx
            .masked_window_array_facts
            .iter()
            .any(|fact| fact.array_local_id == access.array_id)
        {
            continue;
        }
        if !local_is_number_array(ctx, access.array_id)
            && !local_is_untyped_candidate(ctx, access.array_id)
        {
            continue;
        }
        arrays.push(MaskedWindowRegionArray {
            array_id: access.array_id,
            lo,
            hi,
        });
    }
    if arrays.is_empty() {
        return None;
    }

    let eligible: std::collections::HashSet<u32> =
        arrays.iter().map(|array| array.array_id).collect();
    let mut reads = 0usize;
    for stmt in &stmts[..len] {
        if let Stmt::Expr(expr) = stmt {
            reads += count_masked_reads(expr, &eligible);
        }
    }
    if reads < REGION_MIN_TRACKED_READS {
        return None;
    }

    // Flow-ordered fast-copy type refinements. A refinement lands strictly
    // AFTER its statement: the statement's own RHS may read the local's
    // pre-write (possibly non-number) value and must keep coercing
    // semantics; every later statement may assume Number. A subsequent
    // write we cannot prove numeric restores the original type.
    let mut refinements = Vec::new();
    let mut refined: std::collections::HashSet<u32> = std::collections::HashSet::new();
    // Only plain stack locals whose static type is not already numeric are
    // worth refining (boxed/captured storage keeps its own access lowering).
    let refinable = |ctx: &FnCtx<'_>, id: u32| {
        ctx.locals.contains_key(&id)
            && !ctx.boxed_vars.contains(&id)
            && !ctx.closure_captures.contains_key(&id)
            && !matches!(
                ctx.local_types.get(&id),
                Some(perry_hir::types::Type::Number | perry_hir::types::Type::Int32)
            )
    };
    for (offset, stmt) in stmts[..len].iter().enumerate() {
        match stmt {
            Stmt::Expr(Expr::LocalSet(id, value)) => {
                if expr_is_number_under(ctx, &refined, &eligible, value) {
                    if refinable(ctx, *id) && refined.insert(*id) {
                        refinements.push(RegionRefinement {
                            stmt_offset: offset,
                            local_id: *id,
                            set_number: true,
                        });
                    }
                } else if refined.remove(id) {
                    refinements.push(RegionRefinement {
                        stmt_offset: offset,
                        local_id: *id,
                        set_number: false,
                    });
                }
            }
            // `x++` on a BigInt yields a BigInt (ToNumeric, not ToNumber) —
            // an Update proves nothing about the local's type. If the local
            // was previously refined, the refinement stays valid (++ on a
            // number is a number); an unrefined local stays unrefined.
            Stmt::Expr(Expr::Update { .. }) => {}
            _ => {}
        }
    }

    Some(MaskedWindowRegion {
        len,
        arrays,
        refinements,
    })
}

/// Lower one copy of the region, mirroring `lower_stmts_inner`'s per-statement
/// bookkeeping (shadow-slot clears at the original statement indices). Fast
/// copies pass the region's flow-ordered type `refinements`; each lands
/// strictly AFTER its statement (the statement's own RHS may read the
/// pre-write, possibly non-number value) and every original type is restored
/// before returning, so the next copy — and everything after the region —
/// sees the untouched static types.
fn lower_region_copy(
    ctx: &mut FnCtx<'_>,
    region_stmts: &[Stmt],
    base_idx: usize,
    emit_shadow_clears: bool,
    refinements: &[RegionRefinement],
    privatize: bool,
) -> Result<()> {
    // Locals refined to Number and never un-refined for the rest of the
    // region. When `privatize` holds (no enclosing `try` — an exception
    // unwinds the whole frame, so a stale original slot is unobservable),
    // the fast copy moves each such local into a FRESH entry alloca at its
    // refinement point and copies the value back at region end. The original
    // slot's address escaped through `js_shadow_slot_bind`, which blocks
    // LLVM from promoting it to a register; the private slot never escapes,
    // so the whole call-free region SROAs into register-resident bit-mixing
    // chains (the bcryptjs `_encipher` win).
    let unset_ids: std::collections::HashSet<u32> = refinements
        .iter()
        .filter(|refinement| !refinement.set_number)
        .map(|refinement| refinement.local_id)
        .collect();
    let mut privatized: Vec<(u32, String)> = Vec::new();
    let mut saved: Vec<(u32, Option<perry_hir::types::Type>)> = Vec::new();
    let mut saved_ids: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut result = Ok(());
    'stmts: for (offset, stmt) in region_stmts.iter().enumerate() {
        result = lower_stmt(ctx, stmt);
        if result.is_err() || ctx.block().is_terminated() {
            break;
        }
        if emit_shadow_clears {
            emit_shadow_clears_after_stmt(ctx, base_idx + offset);
            if ctx.block().is_terminated() {
                break 'stmts;
            }
        }
        for r in 0..refinements.len() {
            if refinements[r].stmt_offset != offset {
                continue;
            }
            let id = refinements[r].local_id;
            let set_number = refinements[r].set_number;
            if saved_ids.insert(id) {
                saved.push((id, ctx.local_types.get(&id).cloned()));
            }
            if set_number {
                ctx.local_types.insert(id, perry_hir::types::Type::Number);
                // The local now provably holds a number for the rest of the
                // copy (or until an unset): clear its shadow slot once and
                // suppress the per-statement shadow updates — numbers need
                // no GC root, and the region admits no statement that could
                // store a pointer while suppressed. When the statement's own
                // shadow update already emitted a clear (its RHS was a known
                // non-pointer shape), don't emit a second one.
                if let Some(slot_idx) = ctx.shadow_slot_map.get(&id).copied() {
                    if ctx.masked_region_scalar_locals.insert(id) {
                        let already_cleared = matches!(
                            stmt,
                            Stmt::Expr(Expr::LocalSet(_, rhs))
                                if crate::expr::expr_is_known_non_pointer_shadow_value(ctx, rhs)
                        );
                        if !already_cleared {
                            crate::expr::emit_shadow_slot_clear(ctx, slot_idx);
                        }
                    }
                }
                if privatize && !unset_ids.contains(&id) {
                    if let Some(original_slot) = ctx.locals.get(&id).cloned() {
                        let private_slot = ctx.func.alloca_entry(DOUBLE);
                        let current = ctx.block().load(DOUBLE, &original_slot);
                        ctx.block().store(DOUBLE, &current, &private_slot);
                        ctx.locals.insert(id, private_slot);
                        privatized.push((id, original_slot));
                    }
                }
            } else {
                // Restore the pre-region type for the rest of this copy.
                match saved.iter().find(|(saved_id, _)| *saved_id == id) {
                    Some((_, Some(original))) => {
                        ctx.local_types.insert(id, original.clone());
                    }
                    _ => {
                        ctx.local_types.remove(&id);
                    }
                }
                // The statement just lowered stored a value we can no longer
                // prove numeric while its shadow update was suppressed —
                // re-bind the slot from the local's current value so GC sees
                // it again.
                if ctx.masked_region_scalar_locals.remove(&id) {
                    if let Some(slot_idx) = ctx.shadow_slot_map.get(&id).copied() {
                        if let Some(local_slot) = ctx.locals.get(&id).cloned() {
                            crate::expr::emit_shadow_slot_bind_for_local(ctx, id);
                            let current = ctx.block().load(DOUBLE, &local_slot);
                            let bits = ctx.block().bitcast_double_to_i64(&current);
                            ctx.block().call_void(
                                "js_shadow_slot_set",
                                &[(I32, &slot_idx.to_string()), (I64, &bits)],
                            );
                        }
                    }
                }
            }
        }
    }
    // Copy privatized values back into the original (shadow-visible) slots
    // and restore the binding map — post-region code reads the originals.
    for (id, original_slot) in &privatized {
        if result.is_ok() && !ctx.block().is_terminated() {
            if let Some(private_slot) = ctx.locals.get(id).cloned() {
                let value = ctx.block().load(DOUBLE, &private_slot);
                ctx.block().store(DOUBLE, &value, original_slot);
            }
        }
        ctx.locals.insert(*id, original_slot.clone());
    }
    // Drop any still-active suppressions before leaving the copy — the slow
    // copy and post-region code use the ordinary shadow protocol.
    for (id, _) in &saved {
        ctx.masked_region_scalar_locals.remove(id);
    }
    for (id, original) in saved {
        match original {
            Some(original) => {
                ctx.local_types.insert(id, original);
            }
            None => {
                ctx.local_types.remove(&id);
            }
        }
    }
    result
}

/// Emit the versioned region: TA probe chain → `ta_i32` fast copy, plain
/// dense-window guard chain → `plain_f64` fast copy, else the slow copy.
pub(super) fn lower_masked_window_region(
    ctx: &mut FnCtx<'_>,
    region_stmts: &[Stmt],
    base_idx: usize,
    emit_shadow_clears: bool,
    region: &MaskedWindowRegion,
) -> Result<()> {
    let ta_pre_idx = ctx.new_block("masked_region.ta_i32.preheader");
    let try_plain_idx = ctx.new_block("masked_region.try_plain");
    let plain_pre_idx = ctx.new_block("masked_region.plain_f64.preheader");
    let slow_pre_idx = ctx.new_block("masked_region.slow");
    let merge_idx = ctx.new_block("masked_region.merge");
    let ta_pre_label = ctx.block_label(ta_pre_idx);
    let try_plain_label = ctx.block_label(try_plain_idx);
    let plain_pre_label = ctx.block_label(plain_pre_idx);
    let slow_pre_label = ctx.block_label(slow_pre_idx);
    let merge_label = ctx.block_label(merge_idx);

    // TA tier probe: every eligible array must classify as an Int32Array
    // covering its window. Kind code 1 = MASKED_WINDOW_TA_KIND_I32 (see
    // perry-runtime/src/typed_feedback.rs).
    let mut all_i32: Option<String> = None;
    for array in &region.arrays {
        let arr_box = lower_expr(ctx, &Expr::LocalGet(array.array_id))?;
        let feedback_site_id = emit_typed_feedback_register_site(
            ctx,
            TypedFeedbackKind::ArrayElement,
            "array[masked_region_ta_probe]",
            TypedFeedbackContract::masked_window_ta_probe(),
        );
        let kind = ctx.block().call(
            I32,
            "js_typed_feedback_masked_window_ta_kind",
            &[
                (I64, &feedback_site_id),
                (DOUBLE, &arr_box),
                (I32, &array.lo.to_string()),
                (I32, &(array.hi + 1).to_string()),
            ],
        );
        let is_i32 = ctx.block().icmp_eq(I32, &kind, "1");
        all_i32 = Some(match all_i32 {
            None => is_i32,
            Some(prev) => ctx.block().and(I1, &prev, &is_i32),
        });
    }
    let all_i32 = all_i32.expect("region matcher requires >= 1 eligible array");
    ctx.block()
        .cond_br(&all_i32, &ta_pre_label, &try_plain_label);

    // Plain tier: the dense window guard (hole-free, raw-f64) — O(1) once the
    // RawF64 layout flag is set.
    ctx.current_block = try_plain_idx;
    let mut all_plain: Option<String> = None;
    for array in &region.arrays {
        let arr_box = lower_expr(ctx, &Expr::LocalGet(array.array_id))?;
        let feedback_site_id = emit_typed_feedback_register_site(
            ctx,
            TypedFeedbackKind::ArrayElement,
            "array[masked_region_plain]",
            TypedFeedbackContract::packed_f64_array_loop(),
        );
        let guard_i32 = ctx.block().call(
            I32,
            "js_typed_feedback_packed_f64_range_loop_guard_dense",
            &[
                (I64, &feedback_site_id),
                (DOUBLE, &arr_box),
                (I32, &array.lo.to_string()),
                (I32, &(array.hi + 1).to_string()),
            ],
        );
        let guard_ok = ctx.block().icmp_ne(I32, &guard_i32, "0");
        all_plain = Some(match all_plain {
            None => guard_ok,
            Some(prev) => ctx.block().and(I1, &prev, &guard_ok),
        });
    }
    let all_plain = all_plain.expect("region matcher requires >= 1 eligible array");
    ctx.block()
        .cond_br(&all_plain, &plain_pre_label, &slow_pre_label);

    // ta_i32 fast copy: hoist each array's element-0 pointer, then bare
    // `load i32` element reads (values_i32 keeps bit-mixing chains in i32).
    ctx.current_block = ta_pre_idx;
    let mut hoisted: Vec<(u32, String)> = Vec::new();
    for array in &region.arrays {
        let arr_box = lower_expr(ctx, &Expr::LocalGet(array.array_id))?;
        let data_ptr = ctx.block().call(
            I64,
            "js_typed_array_masked_window_data_ptr",
            &[(DOUBLE, &arr_box)],
        );
        hoisted.push((array.array_id, data_ptr));
    }
    let ta_scope_id = ctx.next_loop_proof_scope_id();
    for (array, (arr_id, data_ptr)) in region.arrays.iter().zip(hoisted) {
        ctx.masked_window_array_facts.push(MaskedWindowArrayFact {
            array_local_id: arr_id,
            scope_id: ta_scope_id,
            guard_id: "masked_region_ta_i32".to_string(),
            min_idx: array.lo,
            max_idx_exclusive: array.hi + 1,
            values_i32: true,
            elem: MaskedWindowElem::TaI32 { data_ptr },
        });
    }
    let privatize = ctx.try_depth == 0;
    lower_region_copy(
        ctx,
        region_stmts,
        base_idx,
        emit_shadow_clears,
        &region.refinements,
        privatize,
    )?;
    ctx.masked_window_array_facts
        .retain(|fact| fact.scope_id != ta_scope_id);
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // plain_f64 fast copy: bare raw-f64 window loads on the boxed handle.
    ctx.current_block = plain_pre_idx;
    let plain_scope_id = ctx.next_loop_proof_scope_id();
    for array in &region.arrays {
        ctx.masked_window_array_facts.push(MaskedWindowArrayFact {
            array_local_id: array.array_id,
            scope_id: plain_scope_id,
            guard_id: "masked_region_plain_f64".to_string(),
            min_idx: array.lo,
            max_idx_exclusive: array.hi + 1,
            values_i32: false,
            elem: MaskedWindowElem::PlainF64,
        });
    }
    lower_region_copy(
        ctx,
        region_stmts,
        base_idx,
        emit_shadow_clears,
        &region.refinements,
        privatize,
    )?;
    ctx.masked_window_array_facts
        .retain(|fact| fact.scope_id != plain_scope_id);
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // Slow copy: the untouched per-access lowering, original static types.
    ctx.current_block = slow_pre_idx;
    lower_region_copy(ctx, region_stmts, base_idx, emit_shadow_clears, &[], false)?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(())
}
