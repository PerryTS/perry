//! Fast/slow splits of re-entering runtime helpers (deferred-collection RFC,
//! step S2).
//!
//! A call that can collect is an RS4GC statepoint: every GC value live across
//! it is spilled before the call and reloaded after it. Several helpers can
//! collect only on a rare arm — a getter, a setter, a throw, an allocating
//! miss — yet every call paid that cost. Each lowering here puts the common
//! case on a path that is either no call at all or a call to an audited
//! `CannotCollect` leaf (`gc_call_effects.rs`, which is what makes it
//! `"gc-leaf-function"`), and keeps the original collecting call on a cold
//! arm. RS4GC then places `gc.relocate`s only at that cold statepoint.
//!
//! Every value the cold arm passes is computed before the fast test, so it
//! dominates the arm; the arm rejoins through a phi.

use super::FnCtx;
use crate::types::{LlvmType, DOUBLE, I1, I32, I64};

/// `fast(fast_args)`; when it answers `TAG_HOLE`, a cold arm calls
/// `slow(slow_args)`. Returns the joined DOUBLE.
///
/// `fast` must be a `CannotCollect` symbol whose contract is "serve the hit,
/// `TAG_HOLE` for anything else", and `slow` must reproduce the full helper
/// for every case `fast` declines.
pub(crate) fn emit_hole_declining_split(
    ctx: &mut FnCtx<'_>,
    tag: &str,
    fast: &str,
    fast_args: &[(LlvmType, &str)],
    slow: &str,
    slow_args: &[(LlvmType, &str)],
) -> String {
    let slow_idx = ctx.new_block(&format!("{tag}.split_slow"));
    let merge_idx = ctx.new_block(&format!("{tag}.split_merge"));
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    let fast_val = ctx.block().call(DOUBLE, fast, fast_args);
    let fast_bits = ctx.block().bitcast_double_to_i64(&fast_val);
    let served = ctx
        .block()
        .icmp_ne(I64, &fast_bits, crate::nanbox::TAG_HOLE_I64);
    let fast_end = ctx.block().label.clone();
    // The SERVED edge is the true edge, like every guard-passing edge in the
    // property towers (#7883).
    ctx.block().cond_br(&served, &merge_label, &slow_label);

    ctx.current_block = slow_idx;
    let slow_val = ctx.block().call(DOUBLE, slow, slow_args);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[(fast_val.as_str(), fast_end.as_str()), (slow_val.as_str(), slow_end.as_str())],
    )
}

/// The full-outline class-field SET: `js_class_field_set_ic_fast` answers a
/// status; anything but DONE (1) calls `js_class_field_set_ic_fast_miss` with
/// the status prepended to the same operands.
pub(crate) fn emit_class_field_set_split(ctx: &mut FnCtx<'_>, args: &[(LlvmType, &str)]) {
    let slow_idx = ctx.new_block("class_field_set.split_slow");
    let merge_idx = ctx.new_block("class_field_set.split_merge");
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    let status = ctx.block().call(I32, "js_class_field_set_ic_fast", args);
    let done = ctx.block().icmp_eq(I32, &status, "1");
    ctx.block().cond_br(&done, &merge_label, &slow_label);

    ctx.current_block = slow_idx;
    let mut slow_args: Vec<(LlvmType, &str)> = Vec::with_capacity(args.len() + 1);
    slow_args.push((I32, &status));
    slow_args.extend_from_slice(args);
    ctx.block()
        .call_void("js_class_field_set_ic_fast_miss", &slow_args);
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
}

/// `` `${v}` `` for an operand that is already a string (heap or SSO) is the
/// operand itself — `js_template_string_coerce_box`'s second short-circuit.
/// That arm is now inline and calls nothing; every other operand (numbers,
/// which allocate their text, and objects, whose `toString` is user code)
/// keeps the call on a cold arm.
pub(crate) fn emit_template_string_coerce(ctx: &mut FnCtx<'_>, v: &str) -> String {
    let slow_idx = ctx.new_block("tmpl_coerce.slow");
    let merge_idx = ctx.new_block("tmpl_coerce.merge");
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    let entry_end = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(v);
        let top16 = blk.lshr(I64, &bits, "48");
        let is_heap = blk.icmp_eq(I64, &top16, crate::nanbox::STRING_TAG_TOP16_I64);
        let is_sso = blk.icmp_eq(I64, &top16, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
        let is_str = blk.or(I1, &is_heap, &is_sso);
        let end = blk.label.clone();
        blk.cond_br(&is_str, &merge_label, &slow_label);
        end
    };

    ctx.current_block = slow_idx;
    let coerced = ctx
        .block()
        .call(DOUBLE, "js_template_string_coerce_box", &[(DOUBLE, v)]);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[(v, entry_end.as_str()), (coerced.as_str(), slow_end.as_str())],
    )
}

/// The checked dynamic-callee unbox (#5504) with its hit inline: a
/// `POINTER_TAG` value unboxes to its low 48 bits, which is everything
/// `js_closure_unbox_callee_checked` does on that arm. Every other value
/// takes the call on a cold arm, where it throws `TypeError: value is not a
/// function` exactly as before.
pub(crate) fn emit_checked_callee_unbox(ctx: &mut FnCtx<'_>, callee: &str) -> String {
    let slow_idx = ctx.new_block("callee_unbox.slow");
    let merge_idx = ctx.new_block("callee_unbox.merge");
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    let (handle, entry_end) = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(callee);
        let top16 = blk.lshr(I64, &bits, "48");
        let is_ptr = blk.icmp_eq(I64, &top16, crate::nanbox::POINTER_TAG_TOP16_I64);
        let handle = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
        let end = blk.label.clone();
        blk.cond_br(&is_ptr, &merge_label, &slow_label);
        (handle, end)
    };

    ctx.current_block = slow_idx;
    let checked = ctx
        .block()
        .call(I64, "js_closure_unbox_callee_checked", &[(DOUBLE, callee)]);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I64,
        &[(handle.as_str(), entry_end.as_str()), (checked.as_str(), slow_end.as_str())],
    )
}
