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
    pub size: usize,   // offset 16 — permitted inline bump bound (0 during black births)
    /// Stable address of THIS thread's live birth flags, never a cached value.
    /// Generated code reads the byte again after every raw allocation merge.
    pub birth_flags: *const u8, // offset 24 on the 64-bit inline-allocation ABI
    /// Existing per-thread mark-seed queue, not its reallocating backing buffer.
    /// Resolving TLS here keeps the unpublished birth's seeding call GC-leaf.
    pub birth_seeds: *mut std::ffi::c_void, // offset 32 on LP64
}

pub const INLINE_BIRTH_FLAGS_OFFSET_LP64: usize = 24;
pub const INLINE_BIRTH_SEEDS_OFFSET_LP64: usize = 32;
#[cfg(target_pointer_width = "64")]
const _: () =
    assert!(std::mem::offset_of!(InlineArenaState, birth_flags) == INLINE_BIRTH_FLAGS_OFFSET_LP64);
#[cfg(target_pointer_width = "64")]
const _: () =
    assert!(std::mem::offset_of!(InlineArenaState, birth_seeds) == INLINE_BIRTH_SEEDS_OFFSET_LP64);

/// Generic ThreadHeap entry: resolve this thread's cursor, proving and
/// initializing it when necessary. Runtime callers can use it before gc_init.
/// Compiled ELF executable entries take the same storage's native TLS address
/// after gc_init; other output kinds and targets retain this generic entry.
/// The address is stable for the lifetime of the thread, across refills and GC.
#[no_mangle]
pub extern "C" fn js_inline_arena_state() -> *mut InlineArenaState {
    // SAFETY: `hot_inline_state`/`hot_arena` return this thread's own
    // `INLINE_STATE`/`ARENA` storage — the same addresses `.with()` would hand
    // out, resolved once per thread instead of once per call.
    unsafe {
        let ptr = super::block::inline_state_hot_addr().cast::<InlineArenaState>();
        if prove_inline_arena_state(ptr) {
            return ptr;
        }
        let state = &mut *ptr;
        if state.birth_flags.is_null() {
            state.birth_seeds = crate::gc::mark_seed_queue_address();
            state.birth_flags = crate::gc::gc_birth_flags_address();
        }
        if state.data.is_null() {
            // Lazy init: copy from underlying ARENA's current block.
            let arena = &*super::block::hot_arena();
            let block = &arena.blocks[arena.current];
            state.data = block.data;
            state.offset = block.offset;
            state.size = super::inline::allocation_limit(block.offset, block.size);
        }
        state as *mut InlineArenaState
    }
}

/// Shared ThreadHeap proof for the generic initializer and contract tests.
/// Compiled ELF entries use the same TLS storage after gc_init has established
/// this fact on each thread; they cache its address, never its mutable contents.
pub(crate) unsafe fn prove_inline_arena_state(state: *const InlineArenaState) -> bool {
    !(*state).data.is_null() && !(*state).birth_flags.is_null() && !(*state).birth_seeds.is_null()
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
#[cfg(not(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu")))]
#[no_mangle]
pub extern "C" fn js_inline_arena_slow_alloc(
    state: *mut InlineArenaState,
    size: usize,
    align: usize,
) -> *mut u8 {
    js_inline_arena_slow_alloc_body(state, size, align)
}

/// x86-64 Linux: an allocation slow path JS calls directly, entered through a
/// shim that hands its caller's frame to the collections it can run
/// (`gc::roots::walk_origin`). Allocation runs no JS, which is what lets it
/// offer.
#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
#[unsafe(naked)]
#[no_mangle]
pub extern "C" fn js_inline_arena_slow_alloc(
    state: *mut InlineArenaState,
    size: usize,
    align: usize,
) -> *mut u8 {
    crate::gc::offering_entry_asm!(3, js_inline_arena_slow_alloc_from)
}

#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
extern "C" fn js_inline_arena_slow_alloc_from(
    state: *mut InlineArenaState,
    size: usize,
    align: usize,
    pc: usize,
    sp: usize,
    fp: usize,
) -> *mut u8 {
    let origin = crate::gc::WalkOrigin { pc, sp, fp };
    let _origin = crate::gc::EntryWalkOrigin::enter(&origin);
    js_inline_arena_slow_alloc_body(state, size, align)
}

fn js_inline_arena_slow_alloc_body(
    state: *mut InlineArenaState,
    size: usize,
    align: usize,
) -> *mut u8 {
    // The nursery arena through the hot cache (`hot_arena`), not
    // `ARENA.with`: std's `LocalKey::with` reads the arena through the key's
    // accessor *pointer*, and wherever rustc leaves that instantiation out of
    // line (prod's 16 codegen units do) the GC call-effects analyzer sees an
    // indirect call that may reach JS — this AllocOnly entry and every
    // generated allocation site would pay statepoints for it.
    let arena_ptr = super::block::hot_arena();
    unsafe {
        // Sync inline-state offset back to underlying block (so
        // arena_walk_objects and the slow-path GC trigger see the
        // post-burst offset). Borrows stay short-lived: the GC inside
        // `arena_cell_alloc` mutates this same arena (#7022).
        let offset = (*state).offset;
        {
            let arena = &mut *arena_ptr;
            let current = arena.current;
            arena.note_allocation(offset.saturating_sub(arena.blocks[current].offset), false);
            super::alloc_sample::note_inline_sync(arena.blocks[current].offset, offset);
            arena.blocks[current].offset = offset;
        }
        // Allocate via existing path (may push a new block + run GC).
        let ptr = crate::arena::arena_cell_alloc(arena_ptr, size, align);
        // Resync inline state to the (possibly new) current block.
        let (data, block_offset, block_size) = {
            let arena = &*arena_ptr;
            let block = &arena.blocks[arena.current];
            (block.data, block.offset, block.size)
        };
        let state_ref = &mut *state;
        state_ref.data = data;
        state_ref.offset = block_offset;
        state_ref.size = super::inline::allocation_limit(block_offset, block_size);
        ptr
    }
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
                let current = arena.current;
                arena.note_allocation(
                    state.offset.saturating_sub(arena.blocks[current].offset),
                    false,
                );
                super::alloc_sample::note_inline_sync(arena.blocks[current].offset, state.offset);
                arena.blocks[current].offset = state.offset;
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
                let current = arena.current;
                arena.note_allocation(
                    inline.offset.saturating_sub(arena.blocks[current].offset),
                    false,
                );
                super::alloc_sample::note_inline_sync(arena.blocks[current].offset, inline.offset);
                arena.blocks[current].offset = inline.offset;
            }
            if arena.blocks[arena.current].offset < FRESH_GENERAL_BLOCK_MIN_USED_BYTES {
                return;
            }
            arena.install_fresh_block(BLOCK_SIZE);
            if !inline.data.is_null() {
                let block = &arena.blocks[arena.current];
                inline.data = block.data;
                inline.offset = block.offset;
                inline.size = super::inline::allocation_limit(block.offset, block.size);
            }
        });
    });
}

/// The canonical permission bound, not the physical block extent. Complete
/// generated births may omit their color test because no positive-size bump
/// fits during allocate-black. Runtime refills retain the physical ArenaBlock
/// extent and the existing coloring/seeding path.
#[inline(always)]
pub(crate) fn allocation_limit(offset: usize, block_size: usize) -> usize {
    if crate::gc::gc_birth_extra_flags() != 0 {
        0
    } else {
        super::alloc_sample::inline_limit(offset, block_size)
    }
}

/// Update the existing bump bound at a birth-color transition. Never
/// initialize an unused arena here, allocate, or synchronize a stale offset.
pub(crate) fn refresh_inline_allocation_limit() {
    // A dropped incremental cycle can run during TLS teardown, after the
    // arena has gone. There is no mutator then and no bound to reopen.
    let _ = INLINE_STATE.try_with(|s| unsafe {
        let state = &mut *s.get();
        if !state.data.is_null() {
            let _ = ARENA.try_with(|a| {
                let arena = &*(*a).get();
                state.size = allocation_limit(state.offset, arena.blocks[arena.current].size);
            });
        }
    });
}
