//! The accessor candidate reuses the one collecting property-read exit.
//! Validation and invocation live in the shared runtime holder consumer.
use super::super::FnCtx;
use crate::types::{I64, I8, PTR};

pub(super) fn emit_class_accessor_arm(
    ctx: &mut FnCtx<'_>,
    entry_idx: usize,
    cache_slot_ref: &str,
    slow_label: &str,
    front_label: &str,
) {
    ctx.current_block = entry_idx;
    // An empty MRU also describes cached absent/inherited data. Those reads
    // must keep their GC-leaf front. Inspect only the existing accessor kind;
    // the shared collecting exit still authenticates and invokes the entry.
    let kind_idx = ctx.new_block("pic.acc.kind");
    let kind_label = ctx.block_label(kind_idx);
    let cache = ctx.block().load(PTR, cache_slot_ref);
    let present = ctx.block().icmp_ne(PTR, &cache, "null");
    ctx.block().cond_br(&present, &kind_label, front_label);
    ctx.current_block = kind_idx;
    let kind_ptr = ctx.block().gep(
        I8,
        &cache,
        &[(I64, &(perry_abi::PIC_HOLDER_KIND_WORD * 8).to_string())],
    );
    let kind = ctx.block().load(I64, &kind_ptr);
    let accessor = ctx
        .block()
        .and(I64, &kind, &perry_abi::PIC_HOLDER_ACCESSOR_BIT.to_string());
    let is_accessor = ctx.block().icmp_ne(I64, &accessor, "0");
    ctx.block().cond_br(&is_accessor, slow_label, front_label);
}
