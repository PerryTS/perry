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

use crate::types::{DOUBLE, I1, I32, I64, I8};

use super::{lower_guarded_array_index_get, lower_inline_dyn_typed_array_get, unbox_to_i64, FnCtx};

pub(super) fn lower_array_index_get_via_runtime_key(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_double: &str,
    coerce_numeric_fallback: bool,
) -> String {
    let boxed = crate::expr::array_or_sso_index_get(ctx, arr_box, idx_double);
    if coerce_numeric_fallback {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &boxed)])
    } else {
        boxed
    }
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
    let element_value = if preserve_claimed_receiver_fallback {
        // An erased Array declaration admits object-backed Array subclasses
        // (`class Archetype extends Array` — wolf-ecs `packed[sparse[x]]`) and
        // typed arrays as readily as plain Arrays. The guarded plain-array
        // tier rejects those on its `GC_TYPE_ARRAY` brand and its feedback
        // fallback then classifies the receiver out of line on every read.
        // Read the brand once here: a plain Array keeps the guarded tier,
        // every other heap pointer takes the receiver-unknown numeric tiers
        // (inline typed-array read, dense-subclass `arrlike.ic`, complete
        // dispatcher) that the runtime-key arm already uses for the same
        // receivers. Non-pointers keep the guarded tier's unchanged fallback.
        let brand_idx = ctx.new_block("aidx.claimed.brand");
        let array_idx = ctx.new_block("aidx.claimed.array");
        let other_idx = ctx.new_block("aidx.claimed.other");
        let claimed_merge_idx = ctx.new_block("aidx.claimed.merge");
        let brand_label = ctx.block_label(brand_idx);
        let array_label = ctx.block_label(array_idx);
        let other_label = ctx.block_label(other_idx);
        let claimed_merge_label = ctx.block_label(claimed_merge_idx);
        {
            let blk = ctx.block();
            let arr_bits = blk.bitcast_double_to_i64(arr_box);
            let arr_handle = blk.and(I64, &arr_bits, crate::nanbox::POINTER_MASK_I64);
            let tag = blk.lshr(I64, &arr_bits, "48");
            let is_pointer = blk.icmp_eq(I64, &tag, "32765"); // POINTER_TAG
                                                              // The same heap band the receiver-unknown tiers dereference in.
            let above_handle_band = blk.icmp_ugt(I64, &arr_handle, "1048575");
            let below_heap_limit = blk.icmp_ult(I64, &arr_handle, "140737488355328");
            let in_heap = blk.and(I1, &above_handle_band, &below_heap_limit);
            let heap_candidate = blk.and(I1, &is_pointer, &in_heap);
            blk.cond_br(&heap_candidate, &brand_label, &array_label);
        }
        ctx.current_block = brand_idx;
        {
            let blk = ctx.block();
            let arr_bits = blk.bitcast_double_to_i64(arr_box);
            let arr_handle = blk.and(I64, &arr_bits, crate::nanbox::POINTER_MASK_I64);
            let gc_type_addr = blk.sub(I64, &arr_handle, "8");
            let gc_type_ptr = blk.inttoptr(I64, &gc_type_addr);
            let gc_type = blk.load(I8, &gc_type_ptr);
            let is_array = blk.icmp_eq(I8, &gc_type, "1"); // GC_TYPE_ARRAY
            blk.cond_br(&is_array, &array_label, &other_label);
        }
        ctx.current_block = array_idx;
        let array_value = lower_guarded_array_index_get(
            ctx,
            arr_box,
            &idx_i32,
            "aidx.dynamic",
            require_numeric_layout,
            coerce_numeric_fallback,
            receiver_slot,
        )?;
        let array_end = ctx.block().label.clone();
        ctx.block().br(&claimed_merge_label);
        ctx.current_block = other_idx;
        let other_value =
            lower_inline_dyn_typed_array_get(ctx, arr_box, idx_double, coerce_numeric_fallback);
        let other_end = ctx.block().label.clone();
        ctx.block().br(&claimed_merge_label);
        ctx.current_block = claimed_merge_idx;
        ctx.block().phi(
            DOUBLE,
            &[(&array_value, &array_end), (&other_value, &other_end)],
        )
    } else {
        lower_guarded_array_index_get(
            ctx,
            arr_box,
            &idx_i32,
            "aidx.dynamic",
            require_numeric_layout,
            coerce_numeric_fallback,
            receiver_slot,
        )?
    };
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
    let string_idx = ctx.new_block("aidxkey.sso");
    let array_idx = ctx.new_block("aidxkey.raw");
    let merge_idx = ctx.new_block("aidxkey.merge");
    let string_label = ctx.block_label(string_idx);
    let array_label = ctx.block_label(array_idx);
    let merge_label = ctx.block_label(merge_idx);

    let bits = ctx.block().bitcast_double_to_i64(arr_box);
    let top16 = ctx.block().lshr(I64, &bits, "48");
    let is_sso_string = ctx.block().icmp_eq(I64, &top16, "32761"); // SHORT_STRING_TAG
    ctx.block()
        .cond_br(&is_sso_string, &string_label, &array_label);

    ctx.current_block = string_idx;
    let string_value = ctx.block().call(
        DOUBLE,
        "js_string_index_get_boxed",
        &[(DOUBLE, arr_box), (DOUBLE, idx_double)],
    );
    let string_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // A dynamic key that is an integer-valued double in `[0, 2^32)` IS an
    // array index, so the receiver-unknown numeric tiers apply to it exactly
    // as they do to a statically proven index: the inline typed-array read,
    // then the dense Array-subclass `arrlike.ic` shape cache, then the
    // complete `js_packed_arraylike_index_get` → `js_dyn_index_get`
    // dispatcher. Before this, an `Any`-typed key (`packed[sparse[x]]` in the
    // wolf-ecs SparseSet, `a[b[i]]` in general) always took the out-of-line
    // `js_array_get_index_or_string` route below. Fractional, negative, NaN
    // and out-of-range keys keep that route unchanged; `-0` round-trips to
    // index 0, which is what ToPropertyKey gives it too.
    let int_idx = ctx.new_block("aidxkey.int");
    let int_label = ctx.block_label(int_idx);
    let generic_idx = ctx.new_block("aidxkey.generic");
    let generic_label = ctx.block_label(generic_idx);
    ctx.current_block = array_idx;
    {
        let blk = ctx.block();
        let nonnegative = blk.fcmp("oge", idx_double, "0.0");
        let below_limit = blk.fcmp("olt", idx_double, "4294967296.0");
        let in_range = blk.and(I1, &nonnegative, &below_limit);
        blk.cond_br(&in_range, &int_label, &generic_label);
    }
    ctx.current_block = int_idx;
    let int_label_checked = ctx.new_block("aidxkey.int.exact");
    let int_label_checked_label = ctx.block_label(int_label_checked);
    {
        let blk = ctx.block();
        // In range, so `fptosi` is well-defined; the round trip rejects
        // fractional keys.
        let idx_i64 = blk.fptosi(DOUBLE, idx_double, I64);
        let idx_back = blk.sitofp(I64, &idx_i64, DOUBLE);
        let is_integer = blk.fcmp("oeq", &idx_back, idx_double);
        blk.cond_br(&is_integer, &int_label_checked_label, &generic_label);
    }
    ctx.current_block = int_label_checked;
    let index_value = lower_inline_dyn_typed_array_get(ctx, arr_box, idx_double, false);
    let index_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = generic_idx;
    let arr_handle = unbox_to_i64(ctx.block(), arr_box);
    let array_value = ctx.block().call(
        DOUBLE,
        "js_array_get_index_or_string",
        &[(I64, &arr_handle), (DOUBLE, idx_double)],
    );
    let array_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (&string_value, &string_end),
            (&index_value, &index_end),
            (&array_value, &array_end),
        ],
    )
}
