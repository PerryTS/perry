//! Symbol reads share the computed-key holder record and its roots.
use crate::expr::receiver_range::{emit_fused_receiver_test, emit_handle};
use crate::expr::FnCtx;
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

/// The first keyed way's own inline entry is an emitted shape/key guard.
/// Inherited, spill, absent and refused receivers use the same runtime front.
pub(crate) fn lower_symbol_property_get_ic(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    sym_box: &str,
) -> String {
    let slot = super::keyed_slot(ctx);
    let cache = ctx.block().load(PTR, &slot);
    let present = ctx.block().icmp_ne(PTR, &cache, "null");
    let bits = ctx.block().bitcast_double_to_i64(obj_box);
    let receiver = emit_fused_receiver_test(ctx.block(), &bits);
    let raw = emit_handle(ctx.block(), &receiver.biased);
    let workers = ctx
        .block()
        .load_atomic_seq_cst(I8, "@PERRY_METHOD_SITE_WORKERS_PRESENT", 1);
    let primary = ctx.block().icmp_eq(I8, &workers, "0");
    let ok = ctx.block().and(I1, &present, &receiver.is_object_pointer);
    let ok = ctx.block().and(I1, &ok, &primary);
    let probe = ctx.new_block("symic.probe");
    let hit = ctx.new_block("symic.hit");
    let miss = ctx.new_block("symic.miss");
    let merge = ctx.new_block("symic.merge");
    let probe_l = ctx.block_label(probe);
    let hit_l = ctx.block_label(hit);
    let miss_l = ctx.block_label(miss);
    let merge_l = ctx.block_label(merge);
    ctx.block().cond_br(&ok, &probe_l, &miss_l);
    ctx.current_block = probe;
    let shape_addr = ctx.block().add(I64, &raw, "4");
    let shape_ptr = ctx.block().inttoptr(I64, &shape_addr);
    let shape = ctx.block().load(I32, &shape_ptr);
    let shape = ctx.block().zext(I32, &shape, I64);
    let token = ctx.block().or(I64, &shape, "4611686018427387904");
    // HolderEntry starts at its receiver word; KEY follows its eight words.
    let word = |ctx: &mut FnCtx<'_>, i: usize| {
        let ptr = ctx.block().gep(I64, &cache, &[(I64, &i.to_string())]);
        ctx.block().load(I64, &ptr)
    };
    let base = perry_abi::PIC_HOLDER_RECV_WORD;
    let recv = word(ctx, perry_abi::PIC_HOLDER_RECV_WORD - base);
    let holder = word(ctx, perry_abi::PIC_HOLDER_OBJ_WORD - base);
    let kind = word(ctx, perry_abi::PIC_HOLDER_KIND_WORD - base);
    let key = word(ctx, perry_abi::PIC_HOLDER_STATE_WORD - base);
    let sym_bits = ctx.block().bitcast_double_to_i64(sym_box);
    let same_shape = ctx.block().icmp_eq(I64, &token, &recv);
    let same_key = ctx.block().icmp_eq(I64, &sym_bits, &key);
    let own = ctx.block().icmp_eq(I64, &holder, "0");
    let inline = ctx.block().icmp_ult(I64, &kind, "2147483648");
    let ok = ctx.block().and(I1, &same_shape, &same_key);
    let ok = ctx.block().and(I1, &ok, &own);
    let ok = ctx.block().and(I1, &ok, &inline);
    ctx.block().cond_br(&ok, &hit_l, &miss_l);
    ctx.current_block = hit;
    let offset = ctx.block().shl(I64, &kind, "3");
    let fields = ctx.block().add(I64, &raw, "16");
    let addr = ctx.block().add(I64, &fields, &offset);
    let ptr = ctx.block().inttoptr(I64, &addr);
    let value = ctx.block().load(DOUBLE, &ptr);
    let hit_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = miss;
    let value_miss = ctx.block().call(
        DOUBLE,
        "js_object_get_field_by_key_site",
        &[
            (PTR, &slot),
            (I64, "0"),
            (I64, &raw),
            (DOUBLE, sym_box),
            (DOUBLE, obj_box),
        ],
    );
    let miss_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = merge;
    ctx.block()
        .phi(DOUBLE, &[(&value, &hit_end), (&value_miss, &miss_end)])
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
    let slot = super::keyed_slot(ctx);
    let named_value = ctx.block().call(
        DOUBLE,
        "js_dyn_index_get_site",
        &[(PTR, &slot), (DOUBLE, obj), (DOUBLE, key)],
    );
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
