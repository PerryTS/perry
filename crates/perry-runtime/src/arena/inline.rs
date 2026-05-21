use super::*;

/// Inline bump-allocator state. The codegen emits inline LLVM IR that
/// reads `data` and `offset`, computes `aligned + size`, checks against
/// `size`, stores the new offset, and returns `data + aligned`. The
/// underlying thread-local Arena is the source of truth between
/// inline-alloc bursts; this state is the source of truth during them.
///
/// Field offsets are load-bearing — the codegen GEPs into this struct
/// at hard-coded byte offsets (0/8/16). Do not reorder.
#[repr(C)]
pub struct InlineArenaState {
    pub data: *mut u8, // offset  0  — current block's data pointer
    pub offset: usize, // offset  8  — bump pointer (mutated inline)
    pub size: usize,   // offset 16  — current block's size
}

/// Get the per-thread inline arena state pointer. Called once per JS
/// function entry; the codegen caches the result in a stack slot and
/// reuses it for every `new ClassName()` in that function. The address
/// is stable for the lifetime of the thread, so caching is safe.
///
/// First call on each thread lazy-syncs from the underlying ARENA.
#[no_mangle]
pub extern "C" fn js_inline_arena_state() -> *mut InlineArenaState {
    INLINE_STATE.with(|s| {
        let state = unsafe { &mut *s.get() };
        if state.data.is_null() {
            // Lazy init: copy from underlying ARENA's current block.
            ARENA.with(|a| unsafe {
                let arena = &*a.get();
                let block = &arena.blocks[arena.current];
                state.data = block.data;
                state.offset = block.offset;
                state.size = block.size;
            });
        }
        state as *mut InlineArenaState
    })
}

/// Slow path for inline bump alloc. Called from emitted IR when the
/// fast-path bump check fails (would overflow the current block).
///
/// Sequence:
///   1. Sync inline state's offset back to the underlying ARENA block
///      (so the alloc that's about to push a new block sees the right
///      "current" offset, and so any concurrent GC walk sees all live
///      objects from the inline-alloc burst).
///   2. Allocate via the existing `Arena::alloc` path — handles new
///      block + GC trigger via `alloc_slow`.
///   3. Resync inline state to point at whichever block the alloc
///      landed in (may be the same block if there was leftover space,
///      or a fresh block from `alloc_slow`).
///
/// Returns the raw pointer (the codegen writes the GcHeader at this
/// address and the ObjectHeader at +8 — same layout the inline path
/// produces).
#[no_mangle]
pub extern "C" fn js_inline_arena_slow_alloc(
    state: *mut InlineArenaState,
    size: usize,
    align: usize,
) -> *mut u8 {
    let state_ref = unsafe { &mut *state };
    ARENA.with(|a| unsafe {
        let arena = &mut *a.get();
        // Sync inline-state offset back to underlying block (so
        // arena_walk_objects and the slow-path GC trigger see the
        // post-burst offset).
        arena.blocks[arena.current].offset = state_ref.offset;
        // Allocate via existing path (may push a new block + run GC).
        let ptr = arena.alloc(size, align);
        // Resync inline state to the (possibly new) current block.
        let block = &arena.blocks[arena.current];
        state_ref.data = block.data;
        state_ref.offset = block.offset;
        state_ref.size = block.size;
        ptr
    })
}

/// Sync the inline arena state's offset back to the underlying arena
/// block. Call before any code path that walks the arena (GC scan,
/// `arena_walk_objects`, allocation accounting) so the block's offset
/// reflects the inline-burst's true high-water mark.
///
/// Cheap when no inline allocs have happened yet (state.data is null);
/// otherwise it's a thread-local read + a single store.
pub fn sync_inline_arena_state() {
    INLINE_STATE.with(|s| unsafe {
        let state = &*s.get();
        if !state.data.is_null() {
            ARENA.with(|a| {
                let arena = &mut *(*a).get();
                arena.blocks[arena.current].offset = state.offset;
            });
        }
    });
}

/// Move subsequent general-arena allocations onto a fresh block when the
/// active block is occupied enough that phase mixing would pin meaningful RSS.
///
/// This is intentionally a phase-boundary tool, not an allocation fast path.
/// The non-generational collector cannot compact a block that mixes a live
/// JSON source string with dead parse/build objects, so JSON.parse uses this
/// to keep source-building, parse, and post-parse allocation phases from
/// sharing a busy 1 MB block under full mark-sweep fallback. Tiny parse loops
/// with explicit GCs often return to an almost-empty current block; forcing a
/// fresh block there only raises the process RSS high-water mark.
pub fn arena_start_fresh_general_block() {
    INLINE_STATE.with(|inline_s| unsafe {
        let inline = &mut *inline_s.get();
        ARENA.with(|a| {
            let arena = &mut *(*a).get();
            if !inline.data.is_null() {
                arena.blocks[arena.current].offset = inline.offset;
            }
            if arena.blocks[arena.current].offset < FRESH_GENERAL_BLOCK_MIN_USED_BYTES {
                return;
            }
            arena.install_fresh_block(BLOCK_SIZE);
            if !inline.data.is_null() {
                let block = &arena.blocks[arena.current];
                inline.data = block.data;
                inline.offset = block.offset;
                inline.size = block.size;
            }
        });
    });
}
