//! Which arena blocks may hold a pinned object.
//!
//! A pinned object is a GC root: something outside the heap the collector can
//! see (a native completion, a cross-thread queue, an AppKit string return)
//! holds its address. `GC_FLAG_PINNED` in the header is the authority on
//! whether an object is pinned. [`ArenaBlock::pinned_summary`] only tells the
//! root scan which blocks to walk for such headers, so a cycle never walks the
//! whole heap to find a handful of pins.
//!
//! The summary is set by the pin setters in `gc/pin.rs` (the only sanctioned
//! writers of the header bit, enforced by `scripts/gc_pin_sites.py`) and
//! cleared by [`collect_pinned_arena_headers`] when it walks a block and finds
//! no pinned header left in it. A pinned object is never swept, so a block
//! holding one is never reset; a set summary on a reset block only costs one
//! wasted walk.

use super::*;
use crate::gc::GcHeader;

/// Record that the arena object whose header is at `header_addr` is pinned.
/// Returns `false` when the address is in none of this thread's arena blocks.
pub(crate) fn note_pinned_arena_header(header_addr: usize) -> bool {
    let note = |arena: &mut Arena| -> bool {
        for block in arena.blocks.iter_mut() {
            let base = block.data as usize;
            if header_addr >= base && header_addr < base + block.size {
                block.pinned_summary = true;
                return true;
            }
        }
        false
    };
    ARENA.with(|a| note(unsafe { &mut *a.get() }))
        || SURVIVOR_ARENA_0.with(|a| note(unsafe { &mut *a.get() }))
        || SURVIVOR_ARENA_1.with(|a| note(unsafe { &mut *a.get() }))
        || LONGLIVED_ARENA.with(|a| note(unsafe { &mut *a.get() }))
        || OLD_ARENA.with(|a| note(unsafe { &mut *a.get() }))
}

/// Push every header carrying `GC_FLAG_PINNED` in a block whose summary is
/// set onto `out`, and set each walked block's summary to whether it holds one.
///
/// `include_tenured` walks `Longlived` and `Old` blocks as well. A pass that
/// cannot act on a tenured object (a minor: old objects are black leaves and
/// their young children are remembered by the barrier) leaves it `false`.
/// `place_tenured` walks EVERY tenured block, summary or not: a tenured pin
/// made through `gc::pin_object_non_young` is not placed in its block at pin
/// time (see `gc/pin.rs`), so this walk is what places it.
pub(crate) fn collect_pinned_arena_headers(
    include_tenured: bool,
    place_tenured: bool,
    out: &mut Vec<*mut GcHeader>,
) {
    sync_inline_arena_state();
    let walk = |arena: &mut Arena, every_block: bool, out: &mut Vec<*mut GcHeader>| {
        for block in arena.blocks.iter_mut() {
            if !every_block && !block.pinned_summary {
                continue;
            }
            let mut found = false;
            super::walk::for_each_block_header(block, |header| {
                // SAFETY: the walker yields parseable headers of this block.
                if unsafe { (*header).gc_flags } & crate::gc::GC_FLAG_PINNED != 0 {
                    found = true;
                    out.push(header);
                }
            });
            block.pinned_summary = found;
        }
    };
    ARENA.with(|a| walk(unsafe { &mut *a.get() }, false, out));
    SURVIVOR_ARENA_0.with(|a| walk(unsafe { &mut *a.get() }, false, out));
    SURVIVOR_ARENA_1.with(|a| walk(unsafe { &mut *a.get() }, false, out));
    if include_tenured {
        LONGLIVED_ARENA.with(|a| walk(unsafe { &mut *a.get() }, place_tenured, out));
        OLD_ARENA.with(|a| walk(unsafe { &mut *a.get() }, place_tenured, out));
    }
}
