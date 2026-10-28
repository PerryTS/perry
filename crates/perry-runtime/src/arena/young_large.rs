//! Pointer-free young allocations with individually owned, non-moving backing.
//! Ownership lives in the existing Eden/old block vectors, never in a root list.
use super::region::Kind;
use super::*;

pub(super) fn allocate(total: usize, align: usize) -> *mut u8 {
    // Existing collecting allocation contract; never hold an arena borrow
    // while finishing a previously started non-moving cycle.
    crate::gc::allocation_pacing::finish_cycle_before_large_birth();
    let size = total.checked_add(4095).expect("large leaf extent overflow") & !4095;
    let data = super::block::block_pool_take(size).unwrap_or_else(|| unsafe {
        let data = super::region::map(Kind::LargeObject, size);
        assert!(!data.is_null(), "large leaf backing exhausted");
        data
    });
    unsafe { super::region::advise(data, size, Kind::LargeObject) };
    let mut block = ArenaBlock {
        extent_kind: Kind::LargeObject,
        data,
        size,
        offset: 0,
        object_starts: new_object_start_bitmap(size),
        dead_cycles: 0,
        old_free_holes: false,
        pinned_summary: false,
        idle_pages_discarded: false,
    };
    let raw = block
        .alloc(total, align)
        .expect("large leaf alignment exceeds backing");
    super::page_meta::register_block_with_kind(
        data as usize,
        size,
        HeapGeneration::Nursery,
        HeapSpace::NurseryEden,
        block.object_starts_ptr(),
        Kind::LargeObject,
    );
    // Allocation cannot select this block as the ordinary bump target.
    ARENA.with(|arena| unsafe {
        let arena = &mut *arena.get();
        arena.note_allocation(total, false); // process debt, without old-large burst debt
        install(arena, block);
    });
    ARENA_TOTAL_BYTES.with(|bytes| bytes.set(bytes.get().saturating_add(size)));
    super::from_space::invalidate_sealed_young_bytes();
    crate::gc::trigger_watermark::retire_trigger_watermark();
    raw
}

fn install(arena: &mut Arena, block: ArenaBlock) {
    if let Some(slot) = arena.blocks.iter_mut().find(|b| b.data.is_null()) {
        *slot = block;
    } else {
        arena.blocks.push(block);
    }
}

pub(crate) fn young_large_in_use_bytes() -> usize {
    ARENA.with(|arena| unsafe {
        (&*arena.get())
            .blocks
            .iter()
            .filter(|b| b.extent_kind == Kind::LargeObject)
            .map(|b| b.offset)
            .sum()
    })
}

/// The nursery budget includes large ownership. The copying/reset quantity
/// excludes it, so it cannot skew small-object survival or survivor influx.
pub(crate) fn moving_from_space_in_use_bytes() -> usize {
    copying_from_space_in_use_bytes().saturating_sub(young_large_in_use_bytes())
}

pub(crate) fn is_large_storage(addr: usize) -> bool {
    classify_heap_storage_in_range(addr).is_some_and(|(_, _, _, large)| large)
}

/// Finish a synchronous copying trace, after weak processing and before any
/// from-space reset or quarantine. Survivors retain their mark for clear_marks.
pub(crate) fn finish_copied_young_large() -> ArenaResetStats {
    finish(None, None)
}

/// A sliced/non-moving sweep has already finalized its dead headers and cleared
/// survivor marks. Its snapshot and live verdict, never current arena contents,
/// select eligible owners; post-snapshot births remain young for the next cycle.
pub(crate) fn finish_swept_young_large(
    snapshots: &[ArenaBlockSnapshot],
    live: &[bool],
) -> ArenaResetStats {
    finish(Some(snapshots), Some(live))
}

fn finish(snapshots: Option<&[ArenaBlockSnapshot]>, live: Option<&[bool]>) -> ArenaResetStats {
    crate::gc::heap_generation::debug_assert_heap_change_open();
    let mut stats = ArenaResetStats::default();
    ARENA.with(|arena| unsafe {
        let arena = &mut *arena.get();
        for index in 0..arena.blocks.len() {
            let block = &arena.blocks[index];
            if block.data.is_null() || block.extent_kind != Kind::LargeObject {
                continue;
            }
            if let Some(snapshots) = snapshots {
                if !snapshots.get(index).is_some_and(|s| {
                    s.data == block.data as usize
                        && s.size == block.size
                        && s.offset == block.offset
                }) {
                    continue;
                }
            }
            let header = block.data.cast::<crate::gc::GcHeader>();
            let alive = live.map_or_else(
                || {
                    (*header).gc_flags & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_PINNED)
                        != 0
                },
                |v| v.get(index).copied().unwrap_or(false),
            );
            let mut block = std::mem::replace(
                &mut arena.blocks[index],
                ArenaBlock {
                    extent_kind: Kind::NurseryBlock,
                    data: std::ptr::null_mut(),
                    size: 0,
                    offset: 0,
                    object_starts: Box::new([]),
                    dead_cycles: 0,
                    old_free_holes: false,
                    pinned_summary: false,
                    idle_pages_discarded: false,
                },
            );
            let (base, size, used) = (block.data as usize, block.size, block.offset);
            if alive {
                (*header).gc_flags |= crate::gc::GC_FLAG_TENURED;
                super::page_meta::retag_block_space(
                    base,
                    size,
                    HeapGeneration::Old,
                    HeapSpace::Old,
                );
                super::page_meta::register_old_object_pages(base, used);
                super::block::old_gen_in_use_bytes_add(used);
                OLD_ARENA.with(|old| install(&mut *old.get(), block));
            } else {
                if live.is_none() {
                    crate::gc::finalize_dead_arena_payload(
                        header,
                        block.data.add(crate::gc::GC_HEADER_SIZE),
                        false,
                    );
                }
                // No old-page facts were invented for a young allocation.
                unregister_block_generation(base, size);
                block.clear_object_starts();
                let disposition = release_arena_block(block.data, size);
                ARENA_TOTAL_BYTES.with(|bytes| bytes.set(bytes.get().saturating_sub(size)));
                stats.reusable_bytes = stats.reusable_bytes.saturating_add(used);
                stats.record_block_release(size, disposition);
            }
        }
    });
    stats
}
