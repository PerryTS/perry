//! Array element reads through a dynamic key: the exact runtime-key helper,
//! the canonical signed-i32 split in front of it, and the boxed-receiver
//! string-key read.
//!
//! Split out of `index_get.rs` to stay under the 2,000-line cap (#10750).
//! Pure mechanical move — the items below are verbatim copies (only their
//! visibility is widened to `pub(super)` so the trunk's call sites keep
//! compiling).
//!
//! # Rooting (Layer 1, slice 4)
//!
//! Listed in `crate::rooting`'s `MIGRATED_MODULES`, and the listing is
//! vacuous on the committed source: these helpers take operands their caller
//! has already lowered (and rooted), and lower no user expression of their
//! own, so there is no window for a root to span.

use anyhow::Result;

use crate::types::{DOUBLE, I1, I32, I64};

use super::{lower_guarded_array_index_get, FnCtx};

pub(super) fn lower_array_index_get_via_runtime_key(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_double: &str,
    coerce_numeric_fallback: bool,
) -> String {
    super::guarded_array::emit_read_cold(
        ctx,
        arr_box,
        "0",
        idx_double,
        "0",
        false,
        coerce_numeric_fallback,
        None,
    )
}

/// Split a dynamic numeric array key into the signed-i32 element tier and the
/// full JavaScript property-key tier without speculatively truncating it.
///
/// A numeric type annotation does not prove array-index semantics: fractional,
/// negative, non-finite and large integral values are all named properties (or
/// out-of-range indices), not signed-i32 elements. At the same time, branded
/// numeric aliases and number-returning calls frequently lose the static range
/// fact that would let [`numeric_index_has_integer_array_index_proof`] select
/// the guarded element load. Recognize the profitable subset at runtime and
/// leave every rejected value on the existing exact helper.
///
/// The `select` of `0.0` is load-bearing: LLVM `fptosi` is poison for NaN and
/// out-of-range inputs, so conversion must consume the range-sanitized value,
/// not merely be followed by a range branch.
pub(super) fn lower_array_index_get_via_canonical_i32_split(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_double: &str,
    require_numeric_layout: bool,
    coerce_numeric_fallback: bool,
    preserve_claimed_receiver_fallback: bool,
    receiver_slot: Option<&str>,
) -> Result<String> {
    let element_idx = ctx.new_block("aidx.canonical");
    let runtime_idx = ctx.new_block("aidx.runtime_key");
    let merge_idx = ctx.new_block("aidx.dynamic_merge");
    let element_label = ctx.block_label(element_idx);
    let runtime_label = ctx.block_label(runtime_idx);
    let merge_label = ctx.block_label(merge_idx);

    let (idx_i32, is_canonical_i32) = {
        let blk = ctx.block();

        // Ordinary JS numbers are raw IEEE doubles. Comparisons reject NaN
        // (including Perry's tagged values) and infinities before conversion.
        let raw_ge_zero = blk.fcmp("oge", idx_double, "0.0");
        let raw_le_i32_max = blk.fcmp("ole", idx_double, "2147483647.0");
        let raw_in_range = blk.and(I1, &raw_ge_zero, &raw_le_i32_max);
        let safe_raw = blk.select(I1, &raw_in_range, DOUBLE, idx_double, "0.0");
        let raw_i32 = blk.fptosi(DOUBLE, &safe_raw, I32);
        let raw_round_trip = blk.sitofp(I32, &raw_i32, DOUBLE);
        let raw_is_integral = blk.fcmp("oeq", &raw_round_trip, idx_double);
        let raw_is_canonical = blk.and(I1, &raw_in_range, &raw_is_integral);

        // Runtime-produced integer values may use Perry's INT32 NaN-box. This
        // is the same tag test used by `js_array_get_index_or_string`; negative
        // payloads remain named-property keys and therefore take the fallback.
        let bits = blk.bitcast_double_to_i64(idx_double);
        let top16 = blk.lshr(I64, &bits, "48");
        let is_boxed_i32 = blk.icmp_eq(I64, &top16, crate::nanbox::INT32_TAG_TOP16_I64);
        let boxed_i32 = blk.trunc(I64, &bits, I32);
        let boxed_nonnegative = blk.icmp_sge(I32, &boxed_i32, "0");
        let boxed_is_canonical = blk.and(I1, &is_boxed_i32, &boxed_nonnegative);

        let canonical = blk.or(I1, &raw_is_canonical, &boxed_is_canonical);
        let value = blk.select(I1, &is_boxed_i32, I32, &boxed_i32, &raw_i32);
        (value, canonical)
    };
    ctx.block()
        .cond_br(&is_canonical_i32, &element_label, &runtime_label);

    ctx.current_block = element_idx;
    let element_value = lower_guarded_array_index_get(
        ctx,
        arr_box,
        &idx_i32,
        "aidx.dynamic",
        require_numeric_layout,
        coerce_numeric_fallback,
        receiver_slot,
    )?;
    let element_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = runtime_idx;
    let runtime_value = if preserve_claimed_receiver_fallback {
        // An erased Array declaration is a claim rather than a receiver-tag
        // proof. Keep the established SSO-string receiver arm for the exact
        // property-key fallback; only the guarded canonical tier may consume
        // the receiver as an array without first classifying it.
        lower_claimable_array_string_key_get(ctx, arr_box, idx_double)
    } else {
        lower_array_index_get_via_runtime_key(ctx, arr_box, idx_double, coerce_numeric_fallback)
    };
    let runtime_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[
            (&element_value, &element_end),
            (&runtime_value, &runtime_end),
        ],
    ))
}

/// Read a string-valued key from a receiver admitted by an erased Array type.
///
/// The ordinary array ABI takes an already-unboxed `ArrayHeader*`, which loses
/// an SSO String's tag/payload before the runtime can validate the claim. Keep
/// the receiver boxed until that immediate representation is separated; heap
/// Strings remain real pointers and are classified inside the array-key helper,
/// while every other value retains the established fallback.
pub(super) fn lower_claimable_array_string_key_get(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_double: &str,
) -> String {
    super::guarded_array::emit_read_cold(ctx, arr_box, "0", idx_double, "0", false, false, None)
}

pub(super) fn emit_claimable_array_key_body(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_double: &str,
) -> String {
    crate::expr::array_or_sso_index_get(ctx, arr_box, idx_double)
}
