//! Entry-owned chain validation is loads and comparisons. Keeping it in IR
//! avoids a new runtime-call safepoint on otherwise pure read/store hits.
use super::FnCtx;
use crate::types::{I32, I64, I8, PTR};
fn pointer_bytes(ctx: &FnCtx<'_>) -> usize {
    if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        4
    } else {
        8
    }
}

fn fields(ctx: &mut FnCtx<'_>, proof: &str) -> (String, String) {
    let pointer_size = pointer_bytes(ctx);
    let hops = ctx.block().load(PTR, proof);
    let len_ptr = ctx
        .block()
        .gep(I8, proof, &[(I64, &pointer_size.to_string())]);
    let len = if pointer_size == 8 {
        ctx.block().load(I64, &len_ptr)
    } else {
        let len = ctx.block().load(I32, &len_ptr);
        ctx.block().zext(I32, &len, I64)
    };
    (hops, len)
}

pub(super) fn guard(ctx: &mut FnCtx<'_>, proof: &str, miss: &str) {
    let (hops, len) = fields(ctx, proof);
    guard_hops(ctx, &hops, &len, miss);
}

/// Short store proofs share four comparisons instead of duplicating the
/// complete validator under the four-hop case. The entry roots every address;
/// comparing their words in reverse order has no observable effect.
pub(super) fn guard_store(ctx: &mut FnCtx<'_>, proof: &str, miss: &str) {
    let gate_idx = ctx.new_block("shape.chain.store");
    let gate_l = ctx.block_label(gate_idx);
    ctx.block().br(&gate_l);
    ctx.current_block = gate_idx;
    let (hops, len) = fields(ctx, proof);
    let four_idx = ctx.new_block("shape.chain.four");
    let general_idx = ctx.new_block("shape.chain.general");
    let end_idx = ctx.new_block("shape.chain.store.valid");
    let four_l = ctx.block_label(four_idx);
    let general_l = ctx.block_label(general_idx);
    let end_l = ctx.block_label(end_idx);
    let prefix: Vec<_> = (0..3)
        .map(|_| ctx.new_block("shape.chain.prefix"))
        .collect();
    let prefix_l: Vec<_> = prefix.iter().map(|&i| ctx.block_label(i)).collect();
    let four = ctx.block().icmp_eq(I64, &len, "4");
    ctx.block().cond_br(&four, &four_l, &general_l);
    ctx.current_block = general_idx;
    let deep_idx = ctx.new_block("shape.chain.deep");
    let deep_l = ctx.block_label(deep_idx);
    let mut dispatch = format!("switch i64 {len}, label %{deep_l} [ i64 0, label %{end_l}");
    for (n, target) in prefix_l.iter().enumerate() {
        dispatch.push_str(&format!(" i64 {}, label %{target}", n + 1));
    }
    dispatch.push_str(" ]");
    ctx.block().emit_raw(dispatch);
    ctx.block().mark_terminated();
    // Lengths above four compare the tail, then consume the shared prefix.
    // The induction variable is numeric SSA, with no per-site stack slot.
    ctx.current_block = deep_idx;
    let loop_idx = ctx.new_block("shape.chain.tail.loop");
    let check_idx = ctx.new_block("shape.chain.tail.check");
    let loop_l = ctx.block_label(loop_idx);
    let check_l = ctx.block_label(check_idx);
    ctx.block().br(&loop_l);
    ctx.current_block = loop_idx;
    let next = ctx.block().fresh_reg();
    let i = ctx.block().phi(I64, &[("4", &deep_l), (&next, &check_l)]);
    let more = ctx.block().icmp_ult(I64, &i, &len);
    ctx.block().cond_br(&more, &check_l, &four_l);
    ctx.current_block = check_idx;
    let offset = ctx.block().mul(
        I64,
        &i,
        &crate::runtime_abi::SHAPE_CHAIN_HOP_BYTES.to_string(),
    );
    let valid = hop_matches(ctx, &hops, &offset);
    ctx.block().emit_raw(format!("{next} = add i64 {i}, 1"));
    ctx.block().cond_br(&valid, &loop_l, miss);
    ctx.current_block = four_idx;
    let offset = (3 * crate::runtime_abi::SHAPE_CHAIN_HOP_BYTES).to_string();
    let valid = hop_matches(ctx, &hops, &offset);
    ctx.block().cond_br(&valid, &prefix_l[2], miss);
    for n in (0..3).rev() {
        ctx.current_block = prefix[n];
        let offset = (n * crate::runtime_abi::SHAPE_CHAIN_HOP_BYTES).to_string();
        let valid = hop_matches(ctx, &hops, &offset);
        let next_l = if n == 0 { &end_l } else { &prefix_l[n - 1] };
        ctx.block().cond_br(&valid, next_l, miss);
    }
    ctx.current_block = end_idx;
}

fn guard_hops(ctx: &mut FnCtx<'_>, hops: &str, len: &str, miss: &str) {
    let index = ctx.func.alloca_entry(I64);
    let loop_idx = ctx.new_block("shape.chain.loop");
    let check_idx = ctx.new_block("shape.chain.check");
    let end_idx = ctx.new_block("shape.chain.valid");
    let loop_l = ctx.block_label(loop_idx);
    let check_l = ctx.block_label(check_idx);
    let end_l = ctx.block_label(end_idx);
    // The common one-to-three-hop chains need no induction variable. Every
    // load remains behind its length check; deeper chains retain the loop.
    for n in 0..3 {
        let load_idx = ctx.new_block("shape.chain.prefix");
        let next_idx = ctx.new_block("shape.chain.next");
        let load_l = ctx.block_label(load_idx);
        let next_l = ctx.block_label(next_idx);
        let more = ctx.block().icmp_ult(I64, &n.to_string(), len);
        ctx.block().cond_br(&more, &load_l, &end_l);
        ctx.current_block = load_idx;
        let offset = (n * crate::runtime_abi::SHAPE_CHAIN_HOP_BYTES).to_string();
        let valid = hop_matches(ctx, hops, &offset);
        ctx.block().cond_br(&valid, &next_l, miss);
        ctx.current_block = next_idx;
    }
    ctx.block().store(I64, "3", &index);
    ctx.block().br(&loop_l);
    ctx.current_block = loop_idx;
    let i = ctx.block().load(I64, &index);
    let more = ctx.block().icmp_ult(I64, &i, len);
    ctx.block().cond_br(&more, &check_l, &end_l);
    ctx.current_block = check_idx;
    let offset = ctx.block().mul(
        I64,
        &i,
        &crate::runtime_abi::SHAPE_CHAIN_HOP_BYTES.to_string(),
    );
    let valid = hop_matches(ctx, hops, &offset);
    let next = ctx.block().add(I64, &i, "1");
    ctx.block().store(I64, &next, &index);
    ctx.block().cond_br(&valid, &loop_l, miss);
    ctx.current_block = end_idx;
}

fn hop_matches(ctx: &mut FnCtx<'_>, hops: &str, offset: &str) -> String {
    let hop = ctx.block().gep(I8, hops, &[(I64, offset)]);
    let addr = ctx.block().load(PTR, &hop);
    let expected_ptr = ctx.block().gep(I64, &hop, &[(I64, "1")]);
    let expected = ctx.block().load(I64, &expected_ptr);
    let actual = ctx.block().load(I64, &addr);
    ctx.block().icmp_eq(I64, &actual, &expected)
}

pub(super) fn holder_value(ctx: &mut FnCtx<'_>, proof: &str, miss: &str) -> String {
    let pointer_size = pointer_bytes(ctx);
    let holder_p = ctx
        .block()
        .gep(I8, proof, &[(I64, &(2 * pointer_size).to_string())]);
    let holder = ctx.block().load(PTR, &holder_p);
    let slot_p = ctx
        .block()
        .gep(I8, proof, &[(I64, &(3 * pointer_size).to_string())]);
    let slot = ctx.block().load(I32, &slot_p);
    let index = ctx.block().and(I32, &slot, "2147483647");
    let index = ctx.block().zext(I32, &index, I64);
    let spill = ctx.block().icmp_slt(I32, &slot, "0");
    let inline_idx = ctx.new_block("shape.chain.inline");
    let spill_idx = ctx.new_block("shape.chain.spill");
    let load_idx = ctx.new_block("shape.chain.spill.load");
    let merge_idx = ctx.new_block("shape.chain.value");
    let inline_l = ctx.block_label(inline_idx);
    let spill_l = ctx.block_label(spill_idx);
    let load_l = ctx.block_label(load_idx);
    let merge_l = ctx.block_label(merge_idx);
    ctx.block().cond_br(&spill, &spill_l, &inline_l);
    ctx.current_block = inline_idx;
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let fields = ctx.block().gep(I8, &holder, &[(I64, &header)]);
    let field = ctx.block().gep(I64, &fields, &[(I64, &index)]);
    let inline = ctx.block().load(I64, &field);
    ctx.block().br(&merge_l);
    ctx.current_block = spill_idx;
    let meta_offset = crate::target_layout::object_meta_slot_offset_bytes(ctx.target_triple);
    let spill_offset = crate::runtime_abi::OBJECT_META_SPILL_OFFSET;
    let meta_p = ctx
        .block()
        .gep(I8, &holder, &[(I64, &meta_offset.to_string())]);
    let meta = ctx.block().load(PTR, &meta_p);
    // Priming only publishes an existing spill slot; the holder shape keeps
    // its storage and position. Bounds remain a conservative per-use guard.
    let spill_p = ctx
        .block()
        .gep(I8, &meta, &[(I64, &spill_offset.to_string())]);
    let buffer = ctx.block().load(PTR, &spill_p);
    let len = ctx.block().load(I32, &buffer);
    let len = ctx.block().zext(I32, &len, I64);
    let in_bounds = ctx.block().icmp_ult(I64, &index, &len);
    ctx.block().cond_br(&in_bounds, &load_l, miss);
    ctx.current_block = load_idx;
    let header = crate::runtime_abi::ARRAY_HEADER_SIZE;
    let elements = ctx.block().gep(I8, &buffer, &[(I64, &header.to_string())]);
    let element = ctx.block().gep(I64, &elements, &[(I64, &index)]);
    let spilled = ctx.block().load(I64, &element);
    ctx.block().br(&merge_l);
    ctx.current_block = merge_idx;
    ctx.block()
        .phi(I64, &[(&inline, &inline_l), (&spilled, &load_l)])
}
