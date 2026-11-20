//! Copying-minor reclamation of young holders that own native memory.
//!
//! A Map or Set store, or an AsyncLocalStorage context snapshot holder, is an
//! ordinary young, movable GC object whose payload also owns native buffers.
//! The copying minor runs no per-object finalizer, so after the trace the
//! holders that died in from-space are found through the exact object-start
//! bitmap the blocks already carry (these kinds record their starts), and
//! their native buffers are released. No owner registry is consulted: the
//! bitmap is a fact of the block, and liveness is each holder's own
//! MARKED/FORWARDED state. A start whose holder was already released (an
//! earlier sweep, a growth that retired the store) finalizes as a no-op, and a
//! start reused by another kind is rejected by its type.
//!
//! Two per-thread booleans let programs without young holders skip the bitmap
//! scan. They summarize where holders may be; they never decide liveness.
use super::*;
use crate::gc::{GcHeader, GC_HEADER_SIZE};

thread_local! {
    /// A holder may sit in Eden or the active survivor half.
    static YOUNG_STORES_PRESENT: Cell<bool> = const { Cell::new(false) };
    /// The running copying minor moved a holder into the inactive survivor
    /// half; after the flip that half is from-space for the next minor.
    static YOUNG_STORES_SURVIVED: Cell<bool> = const { Cell::new(false) };
}

/// A holder was just allocated at `user`; note it when it was born young.
#[inline]
pub(crate) fn note_young_owned_store(user: *const u8) {
    // SAFETY: `user` is the payload of an allocation this thread just made.
    let header = unsafe { crate::value::addr_class::honest_header(user as usize) };
    if header.gc_flags & crate::gc::GC_FLAG_TENURED != 0 {
        return;
    }
    YOUNG_STORES_PRESENT.with(|present| present.set(true));
}

/// Called from a holder's move hook: a non-tenured copy lands in the survivor
/// half that becomes from-space after this minor's flip.
#[inline]
pub(crate) fn note_owned_store_copied(new_user: usize) {
    // SAFETY: `new_user` is the copy the collector just made of the holder.
    let header = unsafe { crate::value::addr_class::honest_header(new_user) };
    if header.gc_flags & crate::gc::GC_FLAG_TENURED == 0 {
        YOUNG_STORES_SURVIVED.with(|survived| survived.set(true));
    }
}

/// May a holder with native buffers still be young (Eden or a survivor half)?
/// `false` is exact; `true` may be stale until the next copying minor's walk.
#[inline]
pub(crate) fn young_owned_stores_may_exist() -> bool {
    YOUNG_STORES_PRESENT.with(Cell::get) || YOUNG_STORES_SURVIVED.with(Cell::get)
}

#[inline]
fn is_walked_type(obj_type: u8) -> bool {
    matches!(
        obj_type,
        crate::gc::GC_TYPE_MAP_STORE
            | crate::gc::GC_TYPE_SET_STORE
            | crate::gc::GC_TYPE_ASYNC_SNAPSHOT
    )
}

/// Visit every store and snapshot-holder start in Eden and the active
/// survivor half, with the space of its block (classified once per block).
/// Callbacks must not allocate or collect; the arena blocks are borrowed.
fn walk_from_space_owners(mut visit: impl FnMut(*mut GcHeader, HeapSpace)) {
    sync_inline_arena_state();
    let mut walk = |blocks: &[ArenaBlock]| {
        for block in blocks {
            if block.data.is_null() || block.offset == 0 {
                continue;
            }
            let space = classify_heap_space(block.data as usize);
            if space != HeapSpace::NurseryEden && space != active_survivor_space() {
                continue;
            }
            walk_block_owners(block, &mut |header| visit(header, space));
        }
    };
    ARENA.with(|arena| unsafe {
        walk(&(*arena.get()).blocks);
    });
    if active_survivor_space() == HeapSpace::Survivor0 {
        SURVIVOR_ARENA_0.with(|arena| unsafe {
            walk(&(*arena.get()).blocks);
        });
    } else {
        SURVIVOR_ARENA_1.with(|arena| unsafe {
            walk(&(*arena.get()).blocks);
        });
    }
}

fn walk_block_owners(block: &ArenaBlock, visit: &mut impl FnMut(*mut GcHeader)) {
    let words = block.offset.div_ceil(64 << OBJECT_START_SHIFT);
    for (word_idx, &word) in block.object_starts.iter().take(words).enumerate() {
        let mut bits = word;
        while bits != 0 {
            let slot = word_idx * 64 + bits.trailing_zeros() as usize;
            bits &= bits - 1;
            let offset = slot << OBJECT_START_SHIFT;
            if offset + GC_HEADER_SIZE > block.offset {
                continue;
            }
            unsafe {
                let header = block.data.add(offset).cast::<GcHeader>();
                // A recycled block can keep a bit for an older tenant: the
                // header's own type and size decide.
                if is_walked_type((*header).obj_type)
                    && (*header).size as usize <= block.offset - offset
                {
                    visit(header);
                }
            }
        }
    }
}

/// Release the native buffers of every holder that died in the copying minor
/// running now. Runs after the trace and rewrite, before from-space reset.
pub(crate) fn finalize_dead_copied_minor_from_space_owned_stores() -> usize {
    let survived = YOUNG_STORES_SURVIVED.with(|survived| survived.replace(false));
    let present = YOUNG_STORES_PRESENT.with(|present| present.replace(survived));
    if !present {
        return 0;
    }
    let mut finalized = 0usize;
    let mut still_young = false;
    walk_from_space_owners(|header, space| unsafe {
        let user = header.cast::<u8>().add(GC_HEADER_SIZE);
        if !crate::gc::minor_side_owner_is_dead(&*header, space) {
            if (*header).gc_flags & (crate::gc::GC_FLAG_FORWARDED | crate::gc::GC_FLAG_TENURED) == 0
            {
                // A pinned or otherwise in-place young survivor stays where
                // the next copying minor must look again.
                still_young = true;
            }
            return;
        }
        match (*header).obj_type {
            crate::gc::GC_TYPE_MAP_STORE => crate::map::finalize_owned_store(user, true),
            crate::gc::GC_TYPE_SET_STORE => crate::set::finalize_owned_store(user, true),
            _ => crate::async_context::finalize_snapshot_holder(user.cast()),
        }
        finalized += 1;
    });
    if still_young {
        YOUNG_STORES_PRESENT.with(|present| present.set(true));
    }
    finalized
}
