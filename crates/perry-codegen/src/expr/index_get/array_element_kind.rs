//! Ordinary array element admission from its immutable named layout record.
//! A null holder has plain elements. A fixed ordinary holder with kind zero
//! proves the same fact; overflow, unknown ids and descriptor kinds decline.
use super::FnCtx;
use crate::types::{I1, I32, I64, I8, PTR};

pub(super) fn has_descriptors(ctx: &mut FnCtx<'_>, array: &str) -> String {
    if !crate::expr::array_proto_guard::program_writes_element_descriptors() {
        return "false".into();
    }
    if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        return crate::expr::array_proto_guard::emit_array_element_descriptors(ctx.block(), array);
    }
    let named_at = ctx.block().add(I64, array, "8");
    let named_ptr = ctx.block().inttoptr(I64, &named_at);
    let named = ctx.block().load(I64, &named_ptr);
    let empty = ctx.block().icmp_eq(I64, &named, "0");
    let entry_l = ctx.block().label.clone();
    let lookup = ctx.new_block("array.elements.layout");
    let record = ctx.new_block("array.elements.kind");
    let reject = ctx.new_block("array.elements.unknown");
    let done = ctx.new_block("array.elements.checked");
    let lookup_l = ctx.block_label(lookup);
    let record_l = ctx.block_label(record);
    let reject_l = ctx.block_label(reject);
    let done_l = ctx.block_label(done);
    ctx.block().cond_br(&empty, &done_l, &lookup_l);
    ctx.current_block = lookup;
    let word_at = ctx.block().add(I64, &named, "4");
    let word_ptr = ctx.block().inttoptr(I64, &word_at);
    let word = ctx.block().load(I32, &word_ptr);
    // A holder's overflow can add descriptors without changing its fixed
    // prefix. Its shadow bit sends this access to complete indexed semantics.
    let shadow = ctx.block().and(
        I32,
        &word,
        &(crate::runtime_abi::LAYOUT_SHADOW_BIT as i32).to_string(),
    );
    let fixed = ctx.block().icmp_eq(I32, &shadow, "0");
    let relative = ctx.block().sub(
        I32,
        &word,
        &(crate::runtime_abi::SHAPE_ID_BASE as i32).to_string(),
    );
    let relative = ctx.block().zext(I32, &relative, I64);
    let dir = crate::expr::agent_ptr::emit_agent_ptr_or(
        ctx,
        crate::runtime_abi::AGENT_PTR_SHAPE_DIR,
        "@PERRY_EMPTY_SHAPE_DIR",
    );
    let base_at = ctx.block().gep(
        I8,
        &dir,
        &[(I64, &crate::runtime_abi::SHAPE_DIR_FLAT_BASE.to_string())],
    );
    let base = ctx.block().load(PTR, &base_at);
    let bound_at = ctx.block().gep(
        I8,
        &dir,
        &[(
            I64,
            &(crate::runtime_abi::SHAPE_DIR_FLAT_BASE + 8).to_string(),
        )],
    );
    let bound = ctx.block().load(I64, &bound_at);
    let inside = ctx.block().icmp_ult(I64, &relative, &bound);
    let valid = ctx.block().and(I1, &inside, &fixed);
    ctx.block().cond_br(&valid, &record_l, &reject_l);
    ctx.current_block = record;
    let offset = ctx.block().mul(
        I64,
        &relative,
        &crate::runtime_abi::SHAPE_RECORD_SIZE.to_string(),
    );
    let record_ptr = ctx.block().gep(I8, &base, &[(I64, &offset)]);
    let flags_at = ctx.block().gep(
        I8,
        &record_ptr,
        &[(
            I64,
            &crate::runtime_abi::SHAPE_RECORD_FLAGS_KIND.to_string(),
        )],
    );
    let flags = ctx.block().load(I32, &flags_at);
    // Present + Ordinary, the same packed test as admits_plain_birth.
    let flags = ctx.block().and(I32, &flags, "3841");
    let ordinary = ctx.block().icmp_eq(I32, &flags, "1");
    let kind_at = ctx.block().gep(
        I8,
        &record_ptr,
        &[(
            I64,
            &crate::runtime_abi::SHAPE_RECORD_ELEMENT_KIND.to_string(),
        )],
    );
    let kind = ctx.block().load(I64, &kind_at);
    let plain = ctx.block().icmp_eq(I64, &kind, "0");
    let plain = ctx.block().and(I1, &plain, &ordinary);
    let descriptors = ctx.block().xor(I1, &plain, "true");
    let record_end = ctx.block().label.clone();
    ctx.block().br(&done_l);
    ctx.current_block = reject;
    ctx.block().br(&done_l);
    ctx.current_block = done;
    ctx.block().phi(
        I1,
        &[
            ("false", &entry_l),
            (&descriptors, &record_end),
            ("true", &reject_l),
        ],
    )
}
