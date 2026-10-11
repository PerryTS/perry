//! One own-positive shape compare; all other presence work is shared.
use super::FnCtx;
use crate::nanbox::TAG_TRUE_I64;
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

pub(crate) fn lower_in_presence_ic(ctx: &mut FnCtx<'_>, obj_box: &str, key_box: &str) -> String {
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = super::inline_cache_global_name(ctx, site_id);
    ctx.ic_globals.push(cache_name.clone());
    let slot_ref = format!("@{cache_name}");
    if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        return ctx.block().call(
            DOUBLE,
            "js_in_operator_presence_ic",
            &[(DOUBLE, obj_box), (DOUBLE, key_box), (PTR, &slot_ref)],
        );
    }
    let guard = ctx.new_block("in.pic.guard");
    let hit = ctx.new_block("in.pic.hit");
    let miss = ctx.new_block("in.pic.miss");
    let merge = ctx.new_block("in.pic.merge");
    let guard_label = ctx.block_label(guard);
    let hit_label = ctx.block_label(hit);
    let miss_label = ctx.block_label(miss);
    let merge_label = ctx.block_label(merge);
    let slot = super::emit_inline_cache_slot(ctx, &cache_name);
    let bits = ctx.block().bitcast_double_to_i64(obj_box);
    let recv = super::receiver_range::emit_fused_receiver_test(ctx.block(), &bits);
    let eligible = ctx.block().and(I1, &recv.is_object_pointer, &slot.present);
    ctx.block().cond_br(&eligible, &guard_label, &miss_label);
    ctx.current_block = guard;
    let shape_ptr = super::receiver_range::emit_field_ptr(ctx.block(), &recv.biased, 4);
    let shape = ctx.block().load(I32, &shape_ptr);
    let shape_word = ctx.block().zext(I32, &shape, I64);
    let token_ptr = ctx.block().gep(
        I8,
        &slot.cache,
        &[(I64, &perry_abi::KEYED_HOLDER_TOKEN_OFFSET.to_string())],
    );
    let token = ctx.block().load(I64, &token_ptr);
    // The producer reserves way zero for OWN PRESENT. The existing token
    // proves key membership, descriptors, storage and receiver kind; an
    // empty token cannot match. No value lane or holder chain is read here.
    let cached_shape = ctx.block().xor(I64, &token, "4611686018427387904");
    let matches = ctx.block().icmp_eq(I64, &shape_word, &cached_shape);
    ctx.block().cond_br(&matches, &hit_label, &miss_label);
    ctx.current_block = hit;
    let yes = ctx.block().bitcast_i64_to_double(TAG_TRUE_I64);
    ctx.block().br(&merge_label);
    ctx.current_block = miss;
    let slow = ctx.block().call(
        DOUBLE,
        "js_in_operator_presence_ic",
        &[(DOUBLE, obj_box), (DOUBLE, key_box), (PTR, &slot_ref)],
    );
    ctx.block().br(&merge_label);
    ctx.current_block = merge;
    ctx.block()
        .phi(DOUBLE, &[(&yes, &hit_label), (&slow, &miss_label)])
}
