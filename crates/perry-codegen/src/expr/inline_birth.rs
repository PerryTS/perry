//! Inline allocations use the runtime's live birth color and seed protocol.
use super::FnCtx;
use crate::types::{I64, I8, PTR};

mod shared;
pub(crate) use shared::{class, empty_array};
mod fixed_alloc;
pub(crate) use fixed_alloc::{closure, layout, layout_with};
mod fixed;
pub(crate) use fixed::{compact_slots, fixed_class};

/// A codegen ABI fact, shared by definitions, split declarations and calls.
/// PreserveMost moves register saves into the shared slow/color routine.
pub(crate) fn calling_convention(name: &str) -> Option<&'static str> {
    matches!(
        name,
        "perry_birth_class" | "perry_birth_empty_array" | shared::PUSHED | shared::SIZED
    )
    .then_some("preserve_mostcc")
}

/// The slots the `[]` of `let id = []` is born with beyond the plain empty
/// birth, 0 for none: what the facts reserve for the list (`wpfacts
/// list_caps`), else sixteen for a binding a later loop of its body pushes
/// into (`collectors::loop_pushed_arrays`, all main knows with the facts
/// off or silent).
pub(crate) fn list_slots(ctx: &FnCtx<'_>, id: u32) -> u64 {
    let facts = (!ctx.wpfacts_off && !ctx.wpfacts.interp_guard)
        .then(|| ctx.wpfacts.list_capacities.get(&id).copied())
        .flatten();
    match facts {
        Some(slots) => u64::from(slots),
        None if ctx.loop_pushed_arrays.reserve.contains(&id) => {
            crate::target_layout::INLINE_PUSHED_ARRAY_SLOTS
        }
        None => 0,
    }
}

pub(super) const BIRTH_FLAGS_OFFSET: &str = "24";
#[cfg(test)]
const BIRTH_SEEDS_OFFSET: &str = "32";

/// One `usize` word of the arena cell (`offset`, `size`) at `at`, as an i64:
/// the field address and the widened value.
pub(crate) fn arena_load(
    blk: &mut crate::block::LlBlock,
    arena: &crate::target_layout::InlineArenaLayout,
    state: &str,
    at: u64,
) -> (String, String) {
    let field = blk.gep(I8, state, &[(I64, &at.to_string())]);
    let value = blk.load(arena.word, &field);
    if arena.word == I64 {
        (field, value)
    } else {
        let wide = blk.zext(arena.word, &value, I64);
        (field, wide)
    }
}

/// Store the bumped i64 `value` into the arena word at `field`.
pub(crate) fn arena_store(
    blk: &mut crate::block::LlBlock,
    arena: &crate::target_layout::InlineArenaLayout,
    value: &str,
    field: &str,
) {
    if arena.word == I64 {
        // GC_STORE_AUDIT(INIT): arena bump offset is allocator metadata.
        blk.store(I64, value, field);
    } else {
        let narrow = blk.trunc(I64, value, arena.word);
        // GC_STORE_AUDIT(INIT): arena bump offset is allocator metadata.
        blk.store(arena.word, &narrow, field);
    }
}

/// Read the cell VALUE at every allocation, after the potentially collecting
/// slow arm. Only the thread-local cell's stable address is cached in state.
pub(crate) fn flags(ctx: &mut FnCtx<'_>, state: &str) -> String {
    let at = crate::target_layout::inline_arena_layout(ctx.target_triple)
        .flags
        .to_string();
    let blk = ctx.block();
    let address_field = blk.gep(I8, state, &[(I64, &at)]);
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
    let at = crate::target_layout::inline_arena_layout(ctx.target_triple)
        .seeds
        .to_string();
    let seed_field = ctx.block().gep(I8, state, &[(I64, &at)]);
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
        let lp64 = crate::target_layout::inline_arena_layout("x86_64-unknown-linux-gnu");
        assert_eq!(lp64.flags.to_string(), super::BIRTH_FLAGS_OFFSET);
        assert_eq!(lp64.seeds.to_string(), super::BIRTH_SEEDS_OFFSET);
        let ilp32 = crate::target_layout::inline_arena_layout("wasm32-wasip2");
        for (name, value) in [
            ("OFFSET", ilp32.offset),
            ("SIZE", ilp32.limit),
            ("FLAGS", ilp32.flags),
            ("SEEDS", ilp32.seeds),
        ] {
            assert!(
                runtime.contains(&format!(
                    "pub const INLINE_{name}_OFFSET_ILP32: usize = {value};"
                )),
                "{name}"
            );
        }
        assert_eq!(ilp32.word, crate::types::I32);
    }
}

#[cfg(test)]
mod fixed_alloc_tests;
