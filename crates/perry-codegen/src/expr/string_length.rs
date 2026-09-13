//! Inline `.length` lowering for statically classified string receivers.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::POINTER_MASK_I64;
use crate::type_analysis::{is_array_expr, is_string_expr, string_value_is_runtime_guaranteed};
use crate::types::{DOUBLE, I1, I32, I64};

use super::{lower_expr, static_string_lowering_enabled, FnCtx};

/// Lower string `.length` as an inline UTF-16 count or a heap-header load.
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
    let sso_len = lower_sso_length(ctx, &bits);
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

/// Read an already tag-checked SSO receiver's UTF-16 length without calls or
/// heap access. ASCII keeps the stored byte count; the other arm unrolls the
/// runtime's bounded WTF-8 counter over the five inline payload bytes. Tracking
/// the next sequence boundary also preserves its malformed-byte convention.
/// Call-free lowering is required by the element-shape loop clone (#10191).
pub(super) fn lower_sso_length(ctx: &mut FnCtx<'_>, bits: &str) -> String {
    let shifted = ctx.block().lshr(I64, bits, "40");
    let byte_len = ctx.block().and(I64, &shifted, "255");
    let ascii_len = ctx.block().uitofp(I64, &byte_len, DOUBLE);
    // High bit of each of the five payload bytes; excludes the length/tag.
    let high_bits = ctx.block().and(I64, bits, &0x80_80_80_80_80u64.to_string());
    let ascii = ctx.block().icmp_eq(I64, &high_bits, "0");
    let ascii_end = ctx.block().label.clone();
    let unicode_idx = ctx.new_block("sso.utf16");
    let done_idx = ctx.new_block("sso.length.done");
    let unicode_label = ctx.block_label(unicode_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block().cond_br(&ascii, &done_label, &unicode_label);

    ctx.current_block = unicode_idx;
    let mut next = "0".to_string();
    let mut count = "0".to_string();
    for i in 0..5 {
        let index = i.to_string();
        let shifted = ctx.block().lshr(I64, bits, &(i * 8).to_string());
        let byte = ctx.block().and(I64, &shifted, "255");
        let in_payload = ctx.block().icmp_ult(I64, &index, &byte_len);
        let at_boundary = ctx.block().icmp_ule(I64, &next, &index);
        let active = ctx.block().and(I1, &in_payload, &at_boundary);
        let ascii_byte = ctx.block().icmp_ult(I64, &byte, "128");
        let multibyte = ctx.block().icmp_uge(I64, &byte, "192");
        let has_unit = ctx.block().or(I1, &ascii_byte, &multibyte);
        let astral = ctx.block().icmp_uge(I64, &byte, "240");
        let first_unit = ctx.block().zext(I1, &has_unit, I64);
        let second_unit = ctx.block().zext(I1, &astral, I64);
        let units = ctx.block().add(I64, &first_unit, &second_unit);
        let contribution = ctx.block().select(I1, &active, I64, &units, "0");
        count = ctx.block().add(I64, &count, &contribution);

        if i < 4 {
            let three_or_four =
                ctx.block()
                    .select(I1, &astral, I64, &(i + 4).to_string(), &(i + 3).to_string());
            let two_byte = ctx.block().icmp_ult(I64, &byte, "224");
            let multi_next =
                ctx.block()
                    .select(I1, &two_byte, I64, &(i + 2).to_string(), &three_or_four);
            let step_next =
                ctx.block()
                    .select(I1, &multibyte, I64, &multi_next, &(i + 1).to_string());
            next = ctx.block().select(I1, &active, I64, &step_next, &next);
        }
    }
    let unicode_len = ctx.block().uitofp(I64, &count, DOUBLE);
    let unicode_end = ctx.block().label.clone();
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
    ctx.block().phi(
        DOUBLE,
        &[(&ascii_len, &ascii_end), (&unicode_len, &unicode_end)],
    )
}
