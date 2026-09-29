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
/// set onto `out`, clearing the summary of every walked block that has none.
///
/// `include_tenured` walks `Longlived` and `Old` blocks as well. A pass that
/// cannot act on a tenured object (a minor: old objects are black leaves and
/// their young children are remembered by the barrier) leaves it `false`.
pub(crate) fn collect_pinned_arena_headers(include_tenured: bool, out: &mut Vec<*mut GcHeader>) {
    sync_inline_arena_state();
    let walk = |arena: &mut Arena, out: &mut Vec<*mut GcHeader>| {
        for block in arena.blocks.iter_mut() {
            if !block.pinned_summary {
                continue;
            }
            let mut found = false;
            let mut offset = 0usize;
            while offset < block.offset {
                let aligned = (offset + 7) & !7;
                if aligned >= block.offset {
                    break;
                }
                // SAFETY: `aligned < block.offset`, so this is a parseable
                // header inside the block's bump-allocated prefix — the same
                // walk `arena_walk_objects_filtered` does.
                let header = unsafe { block.data.add(aligned) } as *mut GcHeader;
                let (total, flags, obj_type) = unsafe {
                    (
                        (*header).size as usize,
                        (*header).gc_flags,
                        (*header).obj_type,
                    )
                };
                if total == 0 || total > block.size {
                    break;
                }
                if flags & crate::gc::GC_FLAG_PINNED != 0
                    && crate::gc::gc_type_is_arena_walkable(obj_type)
                {
                    found = true;
                    out.push(header);
                }
                offset = aligned + total;
            }
            block.pinned_summary = found;
        }
    };
    ARENA.with(|a| walk(unsafe { &mut *a.get() }, out));
    SURVIVOR_ARENA_0.with(|a| walk(unsafe { &mut *a.get() }, out));
    SURVIVOR_ARENA_1.with(|a| walk(unsafe { &mut *a.get() }, out));
    if include_tenured {
        LONGLIVED_ARENA.with(|a| walk(unsafe { &mut *a.get() }, out));
        OLD_ARENA.with(|a| walk(unsafe { &mut *a.get() }, out));
    }
}
