//! Guarded direct read-modify-write for a numeric typed-array element.
//!
//! Compound computed assignments are deliberately lowered through immutable
//! base/key temporaries by the HIR (`hoist_compound_member_assign`).  That
//! preserves JavaScript's once-only reference evaluation, but it also hides a
//! specialized-entry `TaPtr` behind an `Any`-typed exact alias and turns
//! `values[key] += rhs` into three independently-lowered operations.  Each
//! operation then loses a different part of the representation proof.
//!
//! This module recognizes the representation shape after those temporaries:
//!
//! ```text
//! base[key] = base[key] + numeric_rhs
//! ```
//!
//! when `base` and `key` are immutable locals and `base` traces through exact
//! local aliases to a Uint32Array candidate.  The runtime guard, rather than a
//! TypeScript annotation, proves pointer identity, inline storage, concrete
//! kind, an exact numeric index, and bounds.  Guard failure runs the unchanged
//! generic get/add/set lowering.  The RHS runs only after the direct load, and
//! the view/kind/bounds guard is checked again after the RHS; a failure there
//! performs only the pending set with the already-computed value, so user code
//! and abrupt completion are never repeated.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr};

use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{
    BoundsState, BufferAccessMode, BufferElem, ExpectedNativeRep, LoweredValue,
    MaterializationReason,
};
use crate::types::{DOUBLE, I1, I32, I64};

use super::{lower_expr, lower_expr_native, FnCtx};

const UINT32_KIND: u64 = 5;

#[derive(Clone, Copy)]
struct Candidate<'a> {
    object: &'a Expr,
    index: &'a Expr,
    rhs: &'a Expr,
    receiver_id: u32,
}

fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_TYPED_ARRAY_RMW").as_deref(),
        Ok("0") | Ok("off") | Ok("false") | Ok("OFF") | Ok("FALSE")
    )
}

fn local_id(expr: &Expr) -> Option<u32> {
    match expr {
        Expr::LocalGet(id) => Some(*id),
        _ => None,
    }
}

fn exact_alias_root(ctx: &FnCtx<'_>, mut id: u32) -> u32 {
    // The alias map is fail-closed and normally acyclic.  Keep a small bound
    // anyway so corrupted/debug HIR cannot hang codegen.
    for _ in 0..64 {
        let Some(next) = ctx.local_value_aliases.get(&id).copied() else {
            break;
        };
        if next == id {
            break;
        }
        id = next;
    }
    id
}

fn receiver_is_uint32_candidate(ctx: &FnCtx<'_>, id: u32) -> Option<u32> {
    let root = exact_alias_root(ctx, id);
    if ctx
        .buffer_view_slots
        .get(&root)
        .is_some_and(|view| matches!(view.elem, BufferElem::U32))
    {
        return Some(root);
    }
    matches!(
        ctx.local_type_hint(&root),
        Some(perry_hir::types::Type::Named(name)) if name == "Uint32Array"
    )
    .then_some(root)
}

fn match_shape<'a>(
    ctx: &FnCtx<'_>,
    object: &'a Expr,
    index: &'a Expr,
    value: &'a Expr,
) -> Option<(u32, &'a Expr)> {
    let (object_id, index_id) = (local_id(object)?, local_id(index)?);
    // These are the semantic condition that allows one reference snapshot to
    // replace the two syntactic reads.  The HIR-generated compound-assignment
    // temps satisfy it, and ordinary immutable user locals do too.
    if ctx.reassigned_locals.contains(&object_id) || ctx.reassigned_locals.contains(&index_id) {
        return None;
    }
    let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = value
    else {
        return None;
    };
    let Expr::IndexGet {
        object: read_object,
        index: read_index,
    } = left.as_ref()
    else {
        return None;
    };
    if local_id(read_object) != Some(object_id) || local_id(read_index) != Some(index_id) {
        return None;
    }
    Some((object_id, right))
}

fn record_rejection(ctx: &mut FnCtx<'_>, receiver_id: u32, reason: &str) {
    let rejected = LoweredValue::js_value("0.0".to_string());
    ctx.record_lowered_value_with_access_mode(
        "TypedArrayRmw",
        Some(receiver_id),
        "TypedArrayRmw.rejected",
        &rejected,
        Some(BoundsState::Unknown),
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(MaterializationReason::RuntimeApi),
        false,
        false,
        vec![
            "typed_array_rmw=rejected".to_string(),
            format!("typed_array_rmw_rejection={reason}"),
        ],
    );
}

/// Pointer/inline-storage/kind cache guard.  Returns the unboxed header address
/// and the guard condition.  The address is used only on a passing edge.
fn emit_receiver_guard(ctx: &mut FnCtx<'_>, object_box: &str) -> (String, String) {
    let tag_mask = crate::nanbox::i64_literal(crate::nanbox::TAG_MASK);
    let blk = ctx.block();
    let object_bits = blk.bitcast_double_to_i64(object_box);
    let raw = blk.and(I64, &object_bits, POINTER_MASK_I64);
    let tagged = blk.and(I64, &object_bits, &tag_mask);
    let is_pointer = blk.icmp_eq(I64, &tagged, crate::nanbox::POINTER_TAG_I64);
    let view_guard = blk.load(I64, "@PERRY_TA_VIEW_GUARD");
    let inline_storage = blk.icmp_eq(I64, &view_guard, "0");
    let slot = blk.lshr(I64, &raw, "3");
    let slot = blk.and(I64, &slot, "63");
    let entry_ptr = blk.gep(
        "[64 x i64]",
        "@PERRY_TA_KIND_CACHE",
        &[(I64, "0"), (I64, &slot)],
    );
    let entry = blk.load(I64, &entry_ptr);
    let cached_addr = blk.lshr(I64, &entry, "8");
    let address_matches = blk.icmp_eq(I64, &cached_addr, &raw);
    let kind = blk.and(I64, &entry, "255");
    let kind_matches = blk.icmp_eq(I64, &kind, &UINT32_KIND.to_string());
    let guard = blk.and(I1, &is_pointer, &inline_storage);
    let guard = blk.and(I1, &guard, &address_matches);
    (raw, blk.and(I1, &guard, &kind_matches))
}

fn emit_index_range_guard(ctx: &mut FnCtx<'_>, index_box: &str) -> String {
    // Ordered comparisons reject NaN and every NaN-boxed non-number.  The
    // upper bound makes the following fptosi-to-i64 defined.
    let ge_zero = ctx.block().fcmp("oge", index_box, "0.0");
    let below_u32_limit = ctx.block().fcmp("olt", index_box, "4294967296.0");
    ctx.block().and(I1, &ge_zero, &below_u32_limit)
}

fn emit_safe_toint32_range_guard(ctx: &mut FnCtx<'_>, value: &str) -> String {
    // LlBlock::toint32 implements truncation/modulo through an intermediate
    // i64.  LLVM fptosi is poison outside the signed-i64 range, so unusually
    // large finite sums (and NaN/Infinity) take the set-only semantic fallback
    // instead.  The common Uint32 accumulator stays wholly inline.
    let above_min = ctx.block().fcmp("oge", value, "-9223372036854775808.0");
    let below_max = ctx.block().fcmp("olt", value, "9223372036854775808.0");
    ctx.block().and(I1, &above_min, &below_max)
}

fn emit_exact_and_bounds_guard(
    ctx: &mut FnCtx<'_>,
    raw: &str,
    index_box: &str,
) -> (String, String) {
    let index_i64 = ctx.block().fptosi(DOUBLE, index_box, I64);
    let roundtrip = ctx.block().sitofp(I64, &index_i64, DOUBLE);
    let exact = ctx.block().fcmp("oeq", &roundtrip, index_box);
    let header_ptr = ctx.block().inttoptr(I64, raw);
    let length = ctx.block().load(I32, &header_ptr);
    let length_i64 = ctx.block().zext(I32, &length, I64);
    let in_bounds = ctx.block().icmp_ult(I64, &index_i64, &length_i64);
    (index_i64, ctx.block().and(I1, &exact, &in_bounds))
}

fn emit_generic_set(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    value: &str,
) -> Result<String> {
    // Re-read the immutable reference temporaries after any allocating RHS;
    // their slots are the GC-visible source of truth.
    let object_box = lower_expr(ctx, object)?;
    let index_box = lower_expr(ctx, index)?;
    Ok(ctx.block().call(
        DOUBLE,
        "js_dyn_index_set",
        &[(DOUBLE, &object_box), (DOUBLE, &index_box), (DOUBLE, value)],
    ))
}

/// Try the guarded Uint32Array `base[key] += numeric_rhs` lowering.
///
/// `Ok(None)` means the expression is outside this representation contract and
/// the ordinary IndexSet lowering must remain byte-for-byte in charge.
pub(super) fn try_lower_guarded_uint32_add(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    value: &Expr,
) -> Result<Option<String>> {
    if !enabled() {
        return Ok(None);
    }
    let Some((object_id, rhs)) = match_shape(ctx, object, index, value) else {
        return Ok(None);
    };
    let Some(receiver_id) = receiver_is_uint32_candidate(ctx, object_id) else {
        return Ok(None);
    };
    // `+` can concatenate or operate on BigInt.  Admit only a value whose
    // runtime representation is already proven to be a canonical Number;
    // declared annotations are intentionally insufficient.
    if !crate::type_analysis::expr_produces_canonical_raw_f64(ctx, rhs) {
        record_rejection(ctx, receiver_id, "rhs_not_canonical_number");
        return Ok(None);
    }
    let candidate = Candidate {
        object,
        index,
        rhs,
        receiver_id,
    };

    // The HIR has already evaluated the source base and computed key once into
    // immutable locals.  Loading those values here therefore has no user-code
    // effect and is the correct reference snapshot for both arms.
    let object_box = lower_expr(ctx, candidate.object)?;
    let index_box = lower_expr(ctx, candidate.index)?;
    let (raw, receiver_ok) = emit_receiver_guard(ctx, &object_box);
    let index_range_ok = emit_index_range_guard(ctx, &index_box);
    let precheck_ok = ctx.block().and(I1, &receiver_ok, &index_range_ok);

    let convert_idx = ctx.new_block("ta.rmw.index.convert");
    let load_idx = ctx.new_block("ta.rmw.load");
    let full_fallback_idx = ctx.new_block("ta.rmw.full_fallback");
    let post_guard_idx = ctx.new_block("ta.rmw.post_rhs_guard");
    let store_idx = ctx.new_block("ta.rmw.store");
    let set_fallback_idx = ctx.new_block("ta.rmw.set_fallback");
    let merge_idx = ctx.new_block("ta.rmw.merge");
    let convert_label = ctx.block_label(convert_idx);
    let load_label = ctx.block_label(load_idx);
    let full_fallback_label = ctx.block_label(full_fallback_idx);
    let post_guard_label = ctx.block_label(post_guard_idx);
    let store_label = ctx.block_label(store_idx);
    let set_fallback_label = ctx.block_label(set_fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block()
        .cond_br(&precheck_ok, &convert_label, &full_fallback_label);

    ctx.current_block = convert_idx;
    let (index_i64, exact_and_in_bounds) = emit_exact_and_bounds_guard(ctx, &raw, &index_box);
    ctx.block()
        .cond_br(&exact_and_in_bounds, &load_label, &full_fallback_label);

    // Fast read and JS-number addition.  Load before RHS evaluation: that
    // ordering is observable when the RHS mutates the same element.
    ctx.current_block = load_idx;
    let old_value = {
        let blk = ctx.block();
        let data_base = blk.add(I64, &raw, "16");
        let byte_offset = blk.shl(I64, &index_i64, "2");
        let address = blk.add(I64, &data_base, &byte_offset);
        let ptr = blk.inttoptr(I64, &address);
        let raw_u32 = blk.load(I32, &ptr);
        blk.uitofp(I32, &raw_u32, DOUBLE)
    };
    let rhs = lower_expr_native(ctx, candidate.rhs, ExpectedNativeRep::F64)?.value;
    let sum = ctx.block().fadd(&old_value, &rhs);
    ctx.block().br(&post_guard_label);
    let fast_sum_end = ctx.block().label.clone();

    // The RHS may expose/detach backing storage or otherwise invalidate the
    // cache.  Revalidate before deriving the store address.  On failure only
    // PutValue remains; the read and RHS must not be repeated.
    ctx.current_block = post_guard_idx;
    // The RHS can allocate and trigger a moving collection.  Reload the
    // immutable reference temporary from its GC-visible slot instead of
    // retaining the pre-RHS NaN-boxed pointer SSA value.
    let post_object_box = lower_expr(ctx, candidate.object)?;
    let (post_raw, post_receiver_ok) = emit_receiver_guard(ctx, &post_object_box);
    let (_, post_bounds_ok) = emit_exact_and_bounds_guard(ctx, &post_raw, &index_box);
    let post_ok = ctx.block().and(I1, &post_receiver_ok, &post_bounds_ok);
    let conversion_ok = emit_safe_toint32_range_guard(ctx, &sum);
    let post_ok = ctx.block().and(I1, &post_ok, &conversion_ok);
    ctx.block()
        .cond_br(&post_ok, &store_label, &set_fallback_label);

    ctx.current_block = store_idx;
    {
        let blk = ctx.block();
        let data_base = blk.add(I64, &post_raw, "16");
        let byte_offset = blk.shl(I64, &index_i64, "2");
        let address = blk.add(I64, &data_base, &byte_offset);
        let ptr = blk.inttoptr(I64, &address);
        let wrapped = blk.toint32(&sum);
        // GC_STORE_AUDIT(POINTER_FREE): Uint32Array backing bytes cannot hold
        // a heap edge; ToUint32 shares the runtime's modulo-2^32 bit result.
        blk.store(I32, &wrapped, &ptr);
        blk.br(&merge_label);
    }
    let store_end = ctx.block().label.clone();

    ctx.current_block = set_fallback_idx;
    let set_fallback_value = emit_generic_set(ctx, candidate.object, candidate.index, &sum)?;
    ctx.block().br(&merge_label);
    let set_fallback_end = ctx.block().label.clone();

    // Full semantic fallback: lower the original get/add tree unchanged, then
    // perform the pending generic set.  This owns non-number keys, OOB,
    // fractional/negative/NaN indices, proxy/annotation lies, views, detached
    // stores, and every abrupt-completion case.
    ctx.current_block = full_fallback_idx;
    let generic_sum = lower_expr(ctx, value)?;
    let full_fallback_value =
        emit_generic_set(ctx, candidate.object, candidate.index, &generic_sum)?;
    ctx.block().br(&merge_label);
    let full_fallback_end = ctx.block().label.clone();

    ctx.current_block = merge_idx;
    let result = ctx.block().phi(
        DOUBLE,
        &[
            (&sum, &store_end),
            (&set_fallback_value, &set_fallback_end),
            (&full_fallback_value, &full_fallback_end),
        ],
    );

    let lowered = LoweredValue::f64(result.clone());
    ctx.record_lowered_value_with_access_mode(
        "TypedArrayRmw",
        Some(candidate.receiver_id),
        "TypedArrayRmw.guarded_direct_uint32_add",
        &lowered,
        Some(BoundsState::Guarded {
            guard_id: "typed_array_rmw_exact_index_and_bounds".to_string(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        false,
        false,
        vec![
            "typed_array_rmw=selected".to_string(),
            "typed_array_kind=Uint32Array".to_string(),
            "typed_array_guard=pointer+inline_storage+kind_cache+exact_numeric_index+bounds"
                .to_string(),
            "post_rhs_guard=backing_store+kind+bounds".to_string(),
            "post_rhs_receiver=reload_gc_visible_local".to_string(),
            "uint32_conversion_guard=signed_i64_range".to_string(),
            "full_fallback=generic_get+js_add+generic_set".to_string(),
            "post_rhs_fallback=generic_set_without_repeating_rhs".to_string(),
        ],
    );
    let fallback = LoweredValue::js_value(generic_sum);
    ctx.record_lowered_value_with_access_mode(
        "TypedArrayRmw",
        Some(candidate.receiver_id),
        "TypedArrayRmw.explicit_fallback",
        &fallback,
        Some(BoundsState::Unknown),
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(MaterializationReason::RuntimeApi),
        false,
        false,
        vec![
            "typed_array_rmw_fallback=guard_failure".to_string(),
            "evaluation_order=base,key,get,rhs,add,set".to_string(),
        ],
    );

    // `fast_sum_end` is deliberately retained as an assertion of CFG shape:
    // the RHS block must terminate at the post-RHS guard, not at the store.
    debug_assert_ne!(fast_sum_end, store_end);
    Ok(Some(result))
}
