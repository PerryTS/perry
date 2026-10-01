//! Shape-guarded own Symbol property loads.
use crate::expr::FnCtx;
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I1, I32, I64, PTR};

/// Emit a weak monomorphic IC for an exact own Symbol-keyed data property.
///
/// The cache stores raw bits, not roots.  Its epoch is advanced by every
/// Symbol-property mutation and completed GC, so a moved/reclaimed receiver or
/// value cannot hit and the cache cannot keep otherwise-dead objects alive.
pub(crate) fn lower_symbol_property_get_ic(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    sym_box: &str,
) -> String {
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = crate::expr::inline_cache_global_name(ctx, site_id);
    ctx.ic_globals.push(cache_name.clone());

    let probe_idx = ctx.new_block("symic.probe");
    let shape_idx = ctx.new_block("symic.shape");
    let shape_label = ctx.block_label(shape_idx);
    let hit_idx = ctx.new_block("symic.hit");
    let miss_idx = ctx.new_block("symic.miss");
    let merge_idx = ctx.new_block("symic.merge");
    let probe_label = ctx.block_label(probe_idx);
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);
    let merge_label = ctx.block_label(merge_idx);

    // #9708: the cache sits behind a pointer slot that the miss handler fills
    // on the first prime. The probe's three loads go through the pointer, so
    // an absent cache branches straight to the miss — the edge a fresh
    // (all-zero) global took anyway, since a zero epoch never matches.
    let ic_slot = crate::expr::emit_inline_cache_slot(ctx, &cache_name);
    let cache_ref = ic_slot.cache.clone();
    let cache_slot_ref = ic_slot.slot_ref.clone();
    ctx.block()
        .cond_br(&ic_slot.present, &probe_label, &miss_label);

    ctx.current_block = probe_idx;
    let epoch = ctx
        .block()
        .load_atomic_acquire(I64, "@PERRY_SYMBOL_PROPERTY_IC_EPOCH", 8);
    let cached_epoch_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "0")]);
    let cached_epoch = ctx.block().load_atomic_acquire(I64, &cached_epoch_ptr, 8);
    let epoch_matches = ctx.block().icmp_eq(I64, &epoch, &cached_epoch);
    let obj_bits = ctx.block().bitcast_double_to_i64(obj_box);
    let cached_obj_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "1")]);
    let cached_obj = ctx.block().load(I64, &cached_obj_ptr);
    let obj_matches = ctx.block().icmp_eq(I64, &obj_bits, &cached_obj);
    let sym_bits = ctx.block().bitcast_double_to_i64(sym_box);
    let cached_sym_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "2")]);
    let cached_sym = ctx.block().load(I64, &cached_sym_ptr);
    let sym_matches = ctx.block().icmp_eq(I64, &sym_bits, &cached_sym);
    let identity_matches = ctx.block().and(I1, &obj_matches, &sym_matches);
    let hit = ctx.block().and(I1, &epoch_matches, &identity_matches);
    ctx.block().cond_br(&hit, &shape_label, &miss_label);

    ctx.current_block = shape_idx;
    let raw = ctx.block().and(I64, &obj_bits, POINTER_MASK_I64);
    let shape_addr = ctx.block().add(I64, &raw, "4");
    let shape_ptr = ctx.block().inttoptr(I64, &shape_addr);
    let shape = ctx.block().load(I32, &shape_ptr);
    let shape = ctx.block().zext(I32, &shape, I64);
    let entry_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "3")]);
    let entry = ctx.block().load(I64, &entry_ptr);
    let cached_shape = ctx.block().lshr(I64, &entry, "32");
    let shape_matches = ctx.block().icmp_eq(I64, &shape, &cached_shape);
    ctx.block().cond_br(&shape_matches, &hit_label, &miss_label);

    ctx.current_block = hit_idx;
    let slot = ctx.block().and(I64, &entry, "4294967295");
    let offset = ctx.block().shl(I64, &slot, "3");
    let fields = ctx.block().add(I64, &raw, "16");
    let addr = ctx.block().add(I64, &fields, &offset);
    let cached_value_ptr = ctx.block().inttoptr(I64, &addr);
    let cached_value_bits = ctx.block().load(I64, &cached_value_ptr);
    let cached_value = ctx.block().bitcast_i64_to_double(&cached_value_bits);
    let hit_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = miss_idx;
    let miss_value = ctx.block().call(
        DOUBLE,
        "js_object_get_symbol_property_ic_miss",
        &[(DOUBLE, obj_box), (DOUBLE, sym_box), (PTR, &cache_slot_ref)],
    );
    let miss_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[(&cached_value, &hit_end), (&miss_value, &miss_end)],
    )
}
