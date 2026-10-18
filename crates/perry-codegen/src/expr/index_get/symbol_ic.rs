//! Symbol reads share the computed-key holder record and its roots.
use crate::expr::receiver_range::{emit_fused_receiver_test, emit_handle};
use crate::expr::FnCtx;
use crate::types::{DOUBLE, I1, I64, PTR};

/// The first keyed way's own inline entry is an emitted shape/key guard.
/// Inherited, spill, absent and refused receivers use the same runtime front.
pub(crate) fn lower_symbol_property_get_ic(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    sym_box: &str,
) -> String {
    if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        return keyed_slow(ctx, obj_box, sym_box, "0", true);
    }

    // Object model step 5 (§3.7 case 3): the receiver's own layout answers
    // the symbol key (`perry_probe_get_keyed`); a decline takes the complete
    // keyed read.
    emit_keyed_probe(ctx, obj_box, sym_box, |ctx| {
        keyed_slow(ctx, obj_box, sym_box, "0", false)
    })
}

/// Separate property-key tags before the numeric array guards. Both branches
/// retain the original dynamic dispatch when their shape proof fails.
pub(super) fn lower_unknown_key_get(ctx: &mut FnCtx<'_>, obj: &str, key: &str) -> String {
    let bits = ctx.block().bitcast_double_to_i64(key);
    let tag = ctx.block().lshr(I64, &bits, "48");
    let ptr = ctx.block().icmp_eq(I64, &tag, "32765");
    let heap_string = ctx.block().icmp_eq(I64, &tag, "32767");
    let short_string = ctx.block().icmp_eq(I64, &tag, "32761");
    let property = ctx.block().or(I1, &ptr, &heap_string);
    let property = ctx.block().or(I1, &property, &short_string);
    let named = ctx.new_block("dynkey.property");
    let numeric = ctx.new_block("dynkey.numeric");
    let merge = ctx.new_block("dynkey.merge");
    let named_l = ctx.block_label(named);
    let numeric_l = ctx.block_label(numeric);
    let merge_l = ctx.block_label(merge);
    ctx.block().cond_br(&property, &named_l, &numeric_l);
    ctx.current_block = named;
    // Object model step 5 (§3.7 case 3): the receiver's own layout answers a
    // string or symbol key through the key's atom (`perry_probe_get_keyed`);
    // only a decline calls the complete keyed read.
    let named_value = emit_keyed_probe(ctx, obj, key, |ctx| keyed_slow(ctx, obj, key, "0", true));
    let named_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = numeric;
    let numeric_value = super::lower_inline_dyn_typed_array_get(ctx, obj, key, false);
    let numeric_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = merge;
    ctx.block().phi(
        DOUBLE,
        &[(&named_value, &named_end), (&numeric_value, &numeric_end)],
    )
}

/// `obj[key]` through the unknown-receiver probe, then `slow` on a decline
/// (`TAG_HOLE`). Returns the merged value; the current block is the merge.
pub(crate) fn emit_keyed_probe(
    ctx: &mut FnCtx<'_>,
    obj: &str,
    key: &str,
    slow: impl FnOnce(&mut FnCtx<'_>) -> String,
) -> String {
    let dir = crate::expr::agent_ptr::emit_agent_ptr_or(
        ctx,
        crate::runtime_abi::AGENT_PTR_SHAPE_DIR,
        "@PERRY_EMPTY_SHAPE_DIR",
    );
    let obj_bits = ctx.block().bitcast_double_to_i64(obj);
    let key_bits = ctx.block().bitcast_double_to_i64(key);
    let probed = ctx.block().call(
        DOUBLE,
        "perry_probe_get_keyed",
        &[(PTR, &dir), (I64, &obj_bits), (I64, &key_bits)],
    );
    let probed_bits = ctx.block().bitcast_double_to_i64(&probed);
    let served = ctx
        .block()
        .icmp_ne(I64, &probed_bits, crate::nanbox::TAG_HOLE_I64);
    let slow_idx = ctx.new_block("keyprobe.slow");
    let merge_idx = ctx.new_block("keyprobe.merge");
    let slow_l = ctx.block_label(slow_idx);
    let merge_l = ctx.block_label(merge_idx);
    let probe_end = ctx.block().label.clone();
    ctx.block().cond_br(&served, &merge_l, &slow_l);
    ctx.current_block = slow_idx;
    let slow_value = slow(ctx);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(&probed, &probe_end), (&slow_value, &slow_end)])
}

/// The complete keyed read after the probe declined (`js_object_get_keyed_slow`).
pub(crate) fn keyed_slow(
    ctx: &mut FnCtx<'_>,
    obj: &str,
    key: &str,
    site_id: &str,
    dynamic: bool,
) -> String {
    let _decision = crate::fallback_diag::decision(
        "keyed layout probe declined; complete keyed lookup required",
        "key coercion/receiver layout/presence",
    );
    ctx.block().call(
        DOUBLE,
        "js_object_get_keyed_slow",
        &[
            (DOUBLE, obj),
            (DOUBLE, key),
            (I64, site_id),
            (crate::types::I32, if dynamic { "1" } else { "0" }),
        ],
    )
}
