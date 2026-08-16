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
    // #7469: both thread-locals come off the one cached hot-TLS base rather
    // than two `_tlv_get_addr` calls. The comment above about "two extra TLS
    // reads cost ~5-10ns" was measuring exactly that toll.
    unsafe {
        let inline_ptr = crate::arena::hot_inline_state();
        let arena_ptr = crate::arena::hot_arena();
        // Sync inline → block before allocating, if the inline
        // state has been initialized. Borrows are deliberately
        // short-lived: `arena_cell_alloc` runs the GC between two
        // disjoint borrows, and the collector mutates BOTH the arena
        // and `INLINE_STATE` (`Arena::resync_inline_to_current`). #7022.
        if !(*inline_ptr).data.is_null() {
            let offset = (*inline_ptr).offset;
            let arena = &mut *arena_ptr;
            let current = arena.current;
            arena.blocks[current].offset = offset;
        }
        let ptr = crate::arena::arena_cell_alloc(arena_ptr, size, align);
        // Resync block → inline (may have advanced to a new block).
        if !(*inline_ptr).data.is_null() {
            let (data, offset, block_size) = {
                let arena = &*arena_ptr;
                let block = &arena.blocks[arena.current];
                (block.data, block.offset, block.size)
            };
            let inline = &mut *inline_ptr;
            inline.data = data;
            inline.offset = offset;
            inline.size = block_size;
        }
        ptr
    }
}

/// [`arena_alloc_gc`] with its **collection point removed**: the request is
/// served by bumping the nursery block that is already open, or the call
/// returns null. It never runs `gc_check_trigger()`, never reserves a fresh
/// block and never births into old-gen.
///
/// ★ The value here is not the handful of instructions saved on the slow
/// branch — it is the *guarantee*. A runtime helper holding raw heap pointers
/// it has not rooted can allocate through this and, on a non-null return,
/// KNOW that nothing moved: the only collection point on the arena path is
/// precisely the one this refuses to reach. That turns "root every operand
/// into the transient handle stack, then re-read every one of them
/// afterwards" into "read them once", for the overwhelmingly common case
/// where a 1 MB block has room.
///
/// On null the caller MUST fall back: root its operands, re-issue through
/// [`arena_alloc_gc`], and re-read the operands from their handles. Nothing
/// has collected at that point either — a null is a refusal, not an event —
/// so the operands are still readable where the caller last saw them.
///
/// Deliberately written out rather than sharing a body with `arena_alloc_gc`:
/// that function is `#[inline(always)]` into every allocation site in the
/// program (including user IR, through the bitcode-link path), and it is not
/// worth risking its codegen to save twenty lines here. The two divergences
/// are both refusals — an oversized request and a non-empty hot free list
/// both return null instead of being served — so this can only ever hand back
/// memory `arena_alloc_gc` would have handed back identically.
#[inline(always)]
pub(crate) fn arena_alloc_gc_no_collect(size: usize, align: usize, obj_type: u8) -> *mut u8 {
    use crate::gc::{GcHeader, GC_FLAG_ARENA, GC_HEADER_SIZE};

    let total = gc_padded_total_size(size, align);
    // Old-gen birth walks page lists and can reserve — outside the contract.
    if crate::gc::is_large_object_total_size_for_type(total, obj_type) {
        return std::ptr::null_mut();
    }
    // The free-list arm of `arena_alloc_gc` cannot collect either, but nothing
    // in the tree ever sets this latch, so serving it here would be untested
    // code on a hot path. Refuse and let the caller take the rooted path.
    if crate::gc::hot_arena_free_list_nonempty().get() {
        return std::ptr::null_mut();
    }

    let raw = arena_alloc_no_collect(total, align);
    if raw.is_null() {
        return std::ptr::null_mut();
    }

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA | crate::gc::gc_birth_extra_flags();
        crate::gc::gc_note_black_birth(header);
        (*header)._reserved = 0;
        (*header).size = total as u32;
    }

    unsafe { raw.add(GC_HEADER_SIZE) }
}

/// [`arena_alloc`] minus its collection point: serve the request from the
/// block that is already open, or return null.
///
/// The inline-state sync/resync mirrors `arena_alloc`'s, so a successful
/// allocation is indistinguishable from one taken through it. A refusal
/// leaves every offset exactly where it was, so the caller's fallback through
/// `arena_alloc` behaves as if this had never been called.
#[inline(always)]
fn arena_alloc_no_collect(size: usize, align: usize) -> *mut u8 {
    unsafe {
        let inline_ptr = crate::arena::hot_inline_state();
        let arena_ptr = crate::arena::hot_arena();
        if !(*inline_ptr).data.is_null() {
            let offset = (*inline_ptr).offset;
            let arena = &mut *arena_ptr;
            let current = arena.current;
            arena.blocks[current].offset = offset;
        }
        let Some(ptr) = crate::arena::arena_cell_try_alloc_current(arena_ptr, size, align) else {
            return std::ptr::null_mut();
        };
        if !(*inline_ptr).data.is_null() {
            let (data, offset, block_size) = {
                let arena = &*arena_ptr;
                let block = &arena.blocks[arena.current];
                (block.data, block.offset, block.size)
            };
            let inline = &mut *inline_ptr;
            inline.data = data;
            inline.offset = offset;
            inline.size = block_size;
        }
        ptr
    }
}

/// Allocate from the longlived arena (issue #179). Unlike `arena_alloc`,
/// this never touches the inline allocator state — the longlived arena
/// is reserved for explicit-call allocations from cache builders
/// (`js_string_from_bytes_longlived`, `js_array_alloc_with_length_longlived`),
/// not hot-path `new ClassName()` bump allocations.
pub fn arena_alloc_longlived(size: usize, align: usize) -> *mut u8 {
    LONGLIVED_ARENA.with(|a| unsafe { crate::arena::arena_cell_alloc(a.get(), size, align) })
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
        crate::gc::gc_note_black_birth(header);
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
    OLD_ARENA.with(|a| unsafe { crate::arena::arena_cell_alloc(a.get(), size, align) })
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
///
/// #7624: page registration is DEFERRED here (`defer_old_object_page_registration`
/// rather than `register_old_object_pages`). This is the per-object old-gen
/// birth path — since #7613's promote-on-first-copy it carries every promotion
/// a copying minor makes, ~113 MB per json_pipeline run — and eager
/// registration costs two `RefCell` borrows, two `Vec` allocations, and a
/// linear dedup scan that grows as the page fills. Allocation policy is
/// deliberately UNCHANGED: the `old_free_take_exact` hole probe below stays,
/// so this is a bookkeeping change only. See the flush discipline in
/// `arena/page_meta.rs`.
pub fn arena_alloc_gc_old(size: usize, align: usize, obj_type: u8) -> *mut u8 {
    use crate::gc::{GcHeader, GC_FLAG_ARENA, GC_HEADER_SIZE};

    // Same alignment-preservation rationale as `arena_alloc_gc`.
    let pad = align.max(8);
    let total = (GC_HEADER_SIZE + size + pad - 1) & !(pad - 1);
    // #7437: reuse a swept same-size hole before bumping — otherwise a
    // block with any live object never yields its dead bytes back and old
    // capacity only ever grows. Exact fit keeps `GcHeader::size` equal to
    // what per-object promotion accounting records for this allocation.
    if let Some(user_ptr) = crate::gc::old_free_take_exact(total, None) {
        let raw = (user_ptr - GC_HEADER_SIZE) as *mut u8;
        unsafe {
            let header = raw as *mut GcHeader;
            (*header).obj_type = obj_type;
            (*header).gc_flags = GC_FLAG_ARENA | crate::gc::gc_birth_extra_flags();
            crate::gc::gc_note_black_birth(header);
            (*header)._reserved = 0;
            (*header).size = total as u32;
        }
        defer_old_object_page_registration(raw as usize, total);
        return user_ptr as *mut u8;
    }
    let raw = arena_alloc_old(total, align);

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA | crate::gc::gc_birth_extra_flags();
        crate::gc::gc_note_black_birth(header);
        (*header)._reserved = 0;
        (*header).size = total as u32;
    }
    defer_old_object_page_registration(raw as usize, total);

    unsafe { raw.add(GC_HEADER_SIZE) }
}

/// The old-gen + born-tenured shape `arena_alloc_gc` hands a LARGE object, for
/// a caller that wants it on size-independent grounds.
///
/// #7539's `LazyArrayHeader` is the caller: it used to reach this arm by being
/// multi-megabyte (its tape was inline), and every caller outside `json_tape`
/// relies on the resulting header address being stable across allocations.
/// Moving the tape out shrank the header to ~88 bytes, which would have made
/// it nursery-resident and movable; asking for this shape explicitly keeps the
/// invariant those callers were already written against.
pub(crate) fn arena_alloc_gc_old_born_tenured(size: usize, align: usize, obj_type: u8) -> *mut u8 {
    use crate::gc::{GcHeader, GC_FLAG_TENURED, GC_HEADER_SIZE};

    let user_ptr = arena_alloc_gc_old(size, align, obj_type);
    unsafe {
        let header = user_ptr.sub(GC_HEADER_SIZE) as *mut GcHeader;
        (*header).gc_flags |= GC_FLAG_TENURED;
    }
    user_ptr
}

/// #7624: registration stays EAGER here, unlike `arena_alloc_gc_old`. This is
/// old-page defrag's relocation allocator (`gc/oldgen.rs`'s
/// `evacuate_selected_old_pages_collecting`), which runs from INSIDE
/// `old_arena_walk_objects_on_pages`' callback — i.e. downstream of that
/// reader's own flush. Deferring would be sound (the walk snapshots its header
/// list before invoking the callback, and the flush discipline covers the
/// rest), but it buys nothing: defrag is a rare, per-cycle pass whose
/// per-object cost is dominated by the `copy_nonoverlapping` beside it, and
/// keeping it eager keeps the deferral's proof obligation to the one path that
/// measurably needs it.
pub(crate) fn arena_alloc_gc_old_excluding_pages(
    size: usize,
    align: usize,
    obj_type: u8,
    excluded_pages: &crate::fast_hash::PtrHashSet<usize>,
) -> *mut u8 {
    use crate::gc::{GcHeader, GC_FLAG_ARENA, GC_HEADER_SIZE};

    let pad = align.max(8);
    let total = (GC_HEADER_SIZE + size + pad - 1) & !(pad - 1);
    // #7437: same hole reuse as `arena_alloc_gc_old`, but never into a page
    // this defrag pass is evacuating.
    if let Some(user_ptr) = crate::gc::old_free_take_exact(total, Some(excluded_pages)) {
        let raw = (user_ptr - GC_HEADER_SIZE) as *mut u8;
        unsafe {
            let header = raw as *mut GcHeader;
            (*header).obj_type = obj_type;
            (*header).gc_flags = GC_FLAG_ARENA | crate::gc::gc_birth_extra_flags();
            crate::gc::gc_note_black_birth(header);
            (*header)._reserved = 0;
            (*header).size = total as u32;
        }
        register_old_object_pages(raw as usize, total);
        return user_ptr as *mut u8;
    }
    let raw = arena_alloc_old_excluding_pages(total, align, excluded_pages);

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA | crate::gc::gc_birth_extra_flags();
        crate::gc::gc_note_black_birth(header);
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

/// Raw-pointer counterpart of [`with_survivor_arena_mut`] for callers that must
/// not hold an `&mut Arena` across a GC trigger (#7022).
pub(crate) fn with_survivor_arena_cell<R>(idx: usize, f: impl FnOnce(*mut Arena) -> R) -> R {
    match idx {
        0 => SURVIVOR_ARENA_0.with(|a| f(a.get())),
        1 => SURVIVOR_ARENA_1.with(|a| f(a.get())),
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
    let raw = with_survivor_arena_cell(idx, |cell| unsafe {
        crate::arena::arena_cell_alloc(cell, total, align)
    });

    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = GC_FLAG_ARENA | crate::gc::gc_birth_extra_flags();
        crate::gc::gc_note_black_birth(header);
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
    //
    // It is TYPE-DEPENDENT, because the price of crossing it is: this object is
    // also stamped `GC_FLAG_TENURED`, and a minor never sweeps old-gen — so for
    // a POINTER-BEARING object the cost is not its own bytes, it is every
    // object it can reach, held live through the remembered set by a container
    // nothing refers to any more. See
    // `gc::LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES` for the measurement
    // (`shapes.ts` sat 16 bytes over the flat 16 KB line and re-marked 118 006
    // slots per minor because of it).
    let total = gc_padded_total_size(size, align);
    if crate::gc::is_large_object_total_size_for_type(total, obj_type) {
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
    let reused = if crate::gc::hot_arena_free_list_nonempty().get() {
        {
            let mut fl = crate::gc::hot_arena_free_list().borrow_mut();
            // Exact-fit only: a best-fit reuse into a larger slot would
            // leave `GcHeader.size` equal to the SLOT size (set below),
            // but the arena block walker steps by `size`, so a mismatch
            // would either skip the padding and misalign the walk, or —
            // if we kept the stale larger size — break the fixed-layout
            // invariant `plausible_gc_header` relies on. Exact match
            // makes both correct: `size == total`, and the walker steps
            // to the next real object. Mirrors `arena_alloc_gc_old`'s
            // `old_free_take_exact`.
            let mut found_idx = None;
            for (idx, &(_, slot_size)) in fl.iter().enumerate() {
                if slot_size == total {
                    found_idx = Some(idx);
                    break;
                }
            }
            if let Some(idx) = found_idx {
                let (ptr, _slot_size) = fl.swap_remove(idx);
                if fl.is_empty() {
                    crate::gc::hot_arena_free_list_nonempty().set(false);
                }
                Some(ptr)
            } else {
                None
            }
        }
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
            crate::gc::gc_note_black_birth(header);
            (*header)._reserved = 0;
            (*header).size = total as u32;
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
        crate::gc::gc_note_black_birth(header);
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
