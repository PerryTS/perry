use super::*;

/// Allocate memory from the thread-local arena
/// This is very fast - just a pointer bump in the common case
///
/// Coexists with the inline allocator: every call here syncs the
/// inline state's offset back to the underlying block first (so we
/// don't overwrite inline-allocated memory), then allocates, then
/// resyncs the inline state to the post-alloc state of the block.
/// The two extra TLS reads cost ~5-10ns per call, which is fine
/// because non-inline allocations (`js_string_from_bytes`,
/// `js_closure_alloc`, etc.) are infrequent compared to the
/// per-class-instance hot path that uses the inline allocator.
#[inline]
pub fn arena_alloc(size: usize, align: usize) -> *mut u8 {
    INLINE_STATE.with(|inline_s| unsafe {
        let inline = &mut *inline_s.get();
        ARENA.with(|a| {
            let arena = &mut *(*a).get();
            // Sync inline → block before allocating, if the inline
            // state has been initialized.
            if !inline.data.is_null() {
                arena.blocks[arena.current].offset = inline.offset;
            }
            let ptr = arena.alloc(size, align);
            // Resync block → inline (may have advanced to a new block).
            if !inline.data.is_null() {
                let block = &arena.blocks[arena.current];
                inline.data = block.data;
                inline.offset = block.offset;
                inline.size = block.size;
            }
            ptr
        })
    })
}

/// Allocate from the longlived arena (issue #179). Unlike `arena_alloc`,
/// this never touches the inline allocator state — the longlived arena
/// is reserved for explicit-call allocations from cache builders
/// (`js_string_from_bytes_longlived`, `js_array_alloc_with_length_longlived`),
/// not hot-path `new ClassName()` bump allocations.
pub fn arena_alloc_longlived(size: usize, align: usize) -> *mut u8 {
    LONGLIVED_ARENA.with(|a| unsafe {
        let arena = &mut *a.get();
        arena.alloc(size, align)
    })
}

/// Allocate a GcHeader-prefixed object from the longlived arena (issue #179).
/// Same header layout as `arena_alloc_gc` so every walker, tracer, and
/// NaN-boxed-pointer resolver works unchanged — these objects are simply
/// not subject to block reset, so their backing storage is stable for the
/// lifetime of the thread.
///
/// No free-list reuse: longlived objects are never swept individually
/// (the cache's root scanner keeps them marked), so there's nothing to
/// re-add to the free list.
pub fn arena_alloc_gc_longlived(size: usize, align: usize, obj_type: u8) -> *mut u8 {
    use crate::gc::{GcHeader, GC_FLAG_ARENA, GC_HEADER_SIZE};

    // Same alignment-preservation rationale as `arena_alloc_gc`: pad
    // `total` to a multiple of `max(align, 8)` so the next caller's
    // bumped offset stays aligned. The codegen inline fast path
    // assumes this invariant.
    let pad = align.max(8);
    let total = (GC_HEADER_SIZE + size + pad - 1) & !(pad - 1);
    let raw = arena_alloc_longlived(total, align);

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA | crate::gc::gc_birth_extra_flags();
        (*header)._reserved = 0;
        (*header).size = total as u32;
    }
    unsafe { raw.add(GC_HEADER_SIZE) }
}

/// Allocate from the old-generation arena (gen-GC Phase B per
/// `docs/generational-gc-plan.md`). Reserved for objects PROMOTED
/// from the nursery (= the general `ARENA`) by Phase C's minor GC.
/// No caller in Phase B — the promotion path lands in Phase C.
/// Same layout as `arena_alloc_gc` so every walker/tracer/sweep
/// already covers it via the `arena_walk_*` family extensions
/// below.
///
/// Routes through a non-inline allocator path (no `INLINE_STATE`
/// touch) so codegen's hot bump-pointer loop on `new ClassName()`
/// stays exclusively pinned to the nursery.
pub fn arena_alloc_old(size: usize, align: usize) -> *mut u8 {
    OLD_ARENA.with(|a| unsafe {
        let arena = &mut *a.get();
        arena.alloc(size, align)
    })
}

pub(crate) fn arena_alloc_old_excluding_pages(
    size: usize,
    align: usize,
    excluded_pages: &crate::fast_hash::PtrHashSet<usize>,
) -> *mut u8 {
    OLD_ARENA.with(|a| unsafe {
        let arena = &mut *a.get();
        arena.alloc_excluding_pages(size, align, excluded_pages)
    })
}

/// GcHeader-prefixed counterpart of `arena_alloc_old`. See
/// `arena_alloc_gc_longlived` for the same shape on the longlived
/// arena — only the backing region differs.
pub fn arena_alloc_gc_old(size: usize, align: usize, obj_type: u8) -> *mut u8 {
    use crate::gc::{GcHeader, GC_FLAG_ARENA, GC_HEADER_SIZE};

    // Same alignment-preservation rationale as `arena_alloc_gc`.
    let pad = align.max(8);
    let total = (GC_HEADER_SIZE + size + pad - 1) & !(pad - 1);
    let raw = arena_alloc_old(total, align);

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA | crate::gc::gc_birth_extra_flags();
        (*header)._reserved = 0;
        (*header).size = total as u32;
    }
    register_old_object_pages(raw as usize, total);

    unsafe { raw.add(GC_HEADER_SIZE) }
}

pub(crate) fn arena_alloc_gc_old_excluding_pages(
    size: usize,
    align: usize,
    obj_type: u8,
    excluded_pages: &crate::fast_hash::PtrHashSet<usize>,
) -> *mut u8 {
    use crate::gc::{GcHeader, GC_FLAG_ARENA, GC_HEADER_SIZE};

    let pad = align.max(8);
    let total = (GC_HEADER_SIZE + size + pad - 1) & !(pad - 1);
    let raw = arena_alloc_old_excluding_pages(total, align, excluded_pages);

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA | crate::gc::gc_birth_extra_flags();
        (*header)._reserved = 0;
        (*header).size = total as u32;
    }
    register_old_object_pages(raw as usize, total);

    unsafe { raw.add(GC_HEADER_SIZE) }
}

#[inline(always)]
fn gc_padded_total_size(size: usize, align: usize) -> usize {
    let pad = align.max(8);
    (crate::gc::GC_HEADER_SIZE + size + pad - 1) & !(pad - 1)
}

pub(crate) fn inactive_survivor_index() -> usize {
    ACTIVE_SURVIVOR.with(|active| 1 - active.get())
}

pub(crate) fn with_survivor_arena_mut<R>(idx: usize, f: impl FnOnce(&mut Arena) -> R) -> R {
    match idx {
        0 => SURVIVOR_ARENA_0.with(|a| unsafe { f(&mut *a.get()) }),
        1 => SURVIVOR_ARENA_1.with(|a| unsafe { f(&mut *a.get()) }),
        _ => unreachable!("invalid survivor arena index"),
    }
}

pub(crate) fn with_survivor_arena<R>(idx: usize, f: impl FnOnce(&Arena) -> R) -> R {
    match idx {
        0 => SURVIVOR_ARENA_0.with(|a| unsafe { f(&*a.get()) }),
        1 => SURVIVOR_ARENA_1.with(|a| unsafe { f(&*a.get()) }),
        _ => unreachable!("invalid survivor arena index"),
    }
}

/// Allocate into the inactive survivor semispace. The copying minor GC
/// resets this space before use and flips it active after from-space reset.
pub(crate) fn arena_alloc_gc_survivor(size: usize, align: usize, obj_type: u8) -> *mut u8 {
    use crate::gc::{GcHeader, GC_FLAG_ARENA, GC_HEADER_SIZE};

    let total = gc_padded_total_size(size, align);
    let idx = inactive_survivor_index();
    let raw = with_survivor_arena_mut(idx, |arena| arena.alloc(total, align));

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA | crate::gc::gc_birth_extra_flags();
        (*header)._reserved = 0;
        (*header).size = total as u32;
    }

    unsafe { raw.add(GC_HEADER_SIZE) }
}

/// Allocate from arena with a GcHeader prepended.
/// Returns pointer to usable memory AFTER the GcHeader.
/// The object is NOT added to any tracking list — arena objects are discovered
/// by walking arena blocks linearly.
///
/// `#[inline(always)]` so the bitcode-link path can fully inline
/// this into user IR — the bump-pointer pattern is small enough
/// (~10 instructions on the fast path) that inlining is a clear win
/// and the slow path (free-list walk + new arena block) is gated
/// behind a cold branch.
#[inline(always)]
pub fn arena_alloc_gc(size: usize, align: usize, obj_type: u8) -> *mut u8 {
    use crate::gc::{GcHeader, GC_FLAG_ARENA, GC_FLAG_TENURED, GC_HEADER_SIZE};

    // Large arena-backed GC objects are born directly in non-moving old
    // generation. The threshold applies to the actual bytes a copying nursery
    // would otherwise move: GcHeader + payload + alignment padding.
    let total = gc_padded_total_size(size, align);
    if crate::gc::is_large_object_total_size(total) {
        let user_ptr = arena_alloc_gc_old(size, align, obj_type);
        unsafe {
            let header = user_ptr.sub(GC_HEADER_SIZE) as *mut GcHeader;
            (*header).gc_flags |= GC_FLAG_TENURED;
        }
        return user_ptr;
    }

    // Hot path: bump-allocate from the current arena block, skipping the
    // free-list walk entirely. The free-list-nonempty `Cell` is a single
    // unboxed load (no `RefCell::borrow_mut` cost) and is `false` for the
    // first GC cycle of every benchmark — which is when allocation-heavy
    // micro-benchmarks like object_create / binary_trees run their tight
    // loops. Walking an empty Vec was costing ~10ns per alloc (borrow,
    // iterate, drop) for nothing; this `Cell` check is ~1ns.
    let reused = if crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.get()) {
        crate::gc::ARENA_FREE_LIST.with(|fl| {
            let mut fl = fl.borrow_mut();
            // Find a slot that fits (exact or slightly larger)
            let mut best_idx = None;
            let mut best_waste = usize::MAX;
            for (idx, &(_, slot_size)) in fl.iter().enumerate() {
                if slot_size >= size && slot_size - size < best_waste {
                    best_waste = slot_size - size;
                    best_idx = Some(idx);
                    if best_waste == 0 {
                        break; // Perfect fit
                    }
                }
            }
            if let Some(idx) = best_idx {
                let (ptr, _slot_size) = fl.swap_remove(idx);
                if fl.is_empty() {
                    crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.set(false));
                }
                Some(ptr)
            } else {
                None
            }
        })
    } else {
        None
    };

    if let Some(user_ptr) = reused {
        // Reusing a free-list slot: the GcHeader is already in place (before user_ptr)
        // Just update it
        unsafe {
            let header = user_ptr.sub(GC_HEADER_SIZE) as *mut GcHeader;
            (*header).obj_type = obj_type;
            (*header).gc_flags = GC_FLAG_ARENA | crate::gc::gc_birth_extra_flags();
            (*header)._reserved = 0;
            // size field already set from original allocation
        }
        return user_ptr;
    }

    // Pad `total` up to a multiple of 8 so the arena's offset stays
    // 8-aligned after each GC alloc. The codegen inline bump-allocator
    // fast path in `crates/perry-codegen/src/lower_call.rs` reads the
    // current offset, adds `total_size`, and stores back without
    // re-aligning — its "every allocation is a multiple of 8"
    // invariant is only valid if every `arena_alloc_gc` caller
    // honors it. Strings (`StringHeader=20` bytes + N-byte payload)
    // routinely allocate odd sizes, which left the offset misaligned
    // for the next inline class allocation. Symptoms: `new World()`
    // returned a misaligned user_ptr; `arena_walk_objects` (which
    // walks at 8-aligned positions) skipped the World object;
    // `build_valid_pointer_set` therefore never inserted World;
    // `try_mark_value` rejected the World pointer found in the
    // shadow stack; mark phase missed every reachable Map / Array
    // hanging off World; sweep freed the archetype's componentData
    // entries buffer; the next allocation reused that slab and the
    // first componentData key drifted to a denormal (~1.086e-311),
    // throwing "Component type 1 is not in this archetype" on the
    // next query.
    let raw = arena_alloc(total, align);

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA | crate::gc::gc_birth_extra_flags();
        (*header)._reserved = 0;
        (*header).size = total as u32;
    }

    unsafe { raw.add(GC_HEADER_SIZE) }
}

/// Allocate an object of known size from the arena
/// Returns a properly aligned pointer
#[no_mangle]
pub extern "C" fn js_arena_alloc(size: u32) -> *mut u8 {
    arena_alloc(size as usize, 8)
}
