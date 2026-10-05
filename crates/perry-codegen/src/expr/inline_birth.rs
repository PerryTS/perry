//! Inline allocations use the runtime's live birth color and seed protocol.
use super::FnCtx;
use crate::types::{I64, I8, PTR};

pub(super) const BIRTH_FLAGS_OFFSET: &str = "24";
pub(super) const BIRTH_SEEDS_OFFSET: &str = "32";

/// Read the cell VALUE at every allocation, after the potentially collecting
/// slow arm. Only the thread-local cell's stable address is cached in state.
pub(crate) fn flags(ctx: &mut FnCtx<'_>, state: &str) -> String {
    let blk = ctx.block();
    let address_field = blk.gep(I8, state, &[(I64, BIRTH_FLAGS_OFFSET)]);
    let address = blk.load(PTR, &address_field);
    let flags = blk.next_reg();
    blk.emit_raw(format!("{flags} = load volatile i8, ptr {address}"));
    blk.zext(I8, &flags, I64)
}

/// Include live flags in the SAME packed header store, not a later patch.
pub(crate) fn header(ctx: &mut FnCtx<'_>, packed: &str, flags: &str) -> String {
    let blk = ctx.block();
    let shifted = blk.shl(I64, flags, "8");
    blk.or(I64, packed, &shifted)
}

/// All slots must be initialized before this GC-leaf call and before any
/// collecting call. The runtime's type metadata decides whether to seed.
/// Idle/sweep births pay no call; unlike the insertion-barrier counter this
/// gate also covers BuildValidPointerSet's barrier-disabled mutator windows.
pub(crate) fn finish(ctx: &mut FnCtx<'_>, raw: &str, flags: &str, state: &str) {
    let active = ctx.block().icmp_ne(I64, flags, "0");
    let seed_idx = ctx.new_block("birth.seed");
    let done_idx = ctx.new_block("birth.ready");
    let seed_label = ctx.block_label(seed_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block().cond_br(&active, &seed_label, &done_label);
    ctx.current_block = seed_idx;
    let seed_field = ctx.block().gep(I8, state, &[(I64, BIRTH_SEEDS_OFFSET)]);
    let seeds = ctx.block().load(PTR, &seed_field);
    ctx.block()
        .call_void("js_gc_note_black_birth", &[(PTR, raw), (PTR, &seeds)]);
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
}

#[cfg(test)]
mod tests {
    #[test]
    fn inline_birth_cell_offset_matches_runtime() {
        let runtime = include_str!("../../../perry-runtime/src/arena/inline.rs");
        assert!(runtime.contains(&format!(
            "pub const INLINE_BIRTH_FLAGS_OFFSET_LP64: usize = {};",
            super::BIRTH_FLAGS_OFFSET
        )));
        assert!(runtime.contains(&format!(
            "pub const INLINE_BIRTH_SEEDS_OFFSET_LP64: usize = {};",
            super::BIRTH_SEEDS_OFFSET
        )));
    }
}
