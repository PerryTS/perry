//! Inline `.length` lowering for statically classified string receivers.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::POINTER_MASK_I64;
use crate::type_analysis::{is_array_expr, is_string_expr, string_value_is_runtime_guaranteed};
use crate::types::{DOUBLE, I32, I64};

use super::{lower_expr, static_string_lowering_enabled, FnCtx};

/// Lower string `.length` as SSO-byte extraction or a heap-header load.
///
/// A declared type is only a dispatch candidate, so its miss retains ordinary
/// property semantics. A constructive proof (including the guarded string
/// window used by #9160) makes the receiver exactly SSO-or-heap-string and
/// removes both the second tag branch and the runtime helper from the clone.
pub(crate) fn try_lower(ctx: &mut FnCtx<'_>, object: &Expr) -> Result<Option<String>> {
    if !static_string_lowering_enabled()
        || !is_string_expr(ctx, object)
        || is_array_expr(ctx, object)
    {
        return Ok(None);
    }

    let proven_string = string_value_is_runtime_guaranteed(ctx, object);
    let recv_box = lower_expr(ctx, object)?;
    let bits = ctx.block().bitcast_double_to_i64(&recv_box);
    let tag = ctx.block().lshr(I64, &bits, "48");
    let is_sso = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
    let sso_idx = ctx.new_block("strlen.sso");
    let chk_idx = (!proven_string).then(|| ctx.new_block("strlen.chk"));
    let heap_idx = ctx.new_block("strlen.heap");
    let slow_idx = chk_idx.map(|_| ctx.new_block("strlen.slow"));
    let merge_idx = ctx.new_block("strlen.merge");
    let sso_label = ctx.block_label(sso_idx);
    let heap_label = ctx.block_label(heap_idx);
    let merge_label = ctx.block_label(merge_idx);
    let non_sso_label = chk_idx
        .map(|idx| ctx.block_label(idx))
        .unwrap_or_else(|| heap_label.clone());
    ctx.block().cond_br(&is_sso, &sso_label, &non_sso_label);

    ctx.current_block = sso_idx;
    let len_shifted = ctx.block().lshr(I64, &bits, "40");
    let len_byte = ctx.block().and(I64, &len_shifted, "255");
    let sso_len = ctx.block().uitofp(I64, &len_byte, DOUBLE);
    let sso_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    if let (Some(chk_idx), Some(slow_idx)) = (chk_idx, slow_idx) {
        ctx.current_block = chk_idx;
        let is_heap = ctx
            .block()
            .icmp_eq(I64, &tag, crate::nanbox::STRING_TAG_TOP16_I64);
        let slow_label = ctx.block_label(slow_idx);
        ctx.block().cond_br(&is_heap, &heap_label, &slow_label);

        ctx.current_block = slow_idx;
        let slow_len = ctx.block().call(
            DOUBLE,
            "js_value_length_property_f64",
            &[(DOUBLE, &recv_box)],
        );
        let slow_pred = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = heap_idx;
        let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
        let len_i32 = ctx.block().safe_load_i32_from_ptr(&handle);
        let heap_len = ctx.block().uitofp(I32, &len_i32, DOUBLE);
        let heap_pred = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        return Ok(Some(ctx.block().phi(
            DOUBLE,
            &[
                (&sso_len, &sso_pred),
                (&heap_len, &heap_pred),
                (&slow_len, &slow_pred),
            ],
        )));
    }

    ctx.current_block = heap_idx;
    let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let len_i32 = ctx.block().safe_load_i32_from_ptr(&handle);
    let heap_len = ctx.block().uitofp(I32, &len_i32, DOUBLE);
    let heap_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(Some(ctx.block().phi(
        DOUBLE,
        &[(&sso_len, &sso_pred), (&heap_len, &heap_pred)],
    )))
}
