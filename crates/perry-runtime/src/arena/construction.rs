//! Allocation state for a synchronous, collection-suppressed construction.
//!
//! This borrows the ordinary nursery's inline cursor; it does not reserve a
//! slab, create a second heap, or change individual object lifetimes. Regular
//! allocations can interleave because they synchronize this same cursor.

use super::InlineArenaState;

pub(crate) struct ConstructionBatch {
    inline: *mut InlineArenaState,
    free_list_nonempty: *const std::cell::Cell<bool>,
}

impl ConstructionBatch {
    /// Publish old-to-young edges from completed JSON slots. The existing
    /// remembered set records pages, so one ordinary barrier for a young edge
    /// on each slot page covers every other inline slot on that page. A page
    /// with only old/longlived children is left clean. No new collector API or
    /// remembered-set representation is introduced.
    ///
    /// Slots must contain only parser-produced JSON values: heap children are
    /// arena strings, arrays or plain objects, never foreign/malloc cells.
    pub(crate) unsafe fn finish_json_slots(&self, parent: *mut u8, slots: *const u64, len: usize) {
        if !super::pointer_in_old_gen(parent as usize) {
            return;
        }
        let mut index = 0;
        while index < len {
            let addr = slots.add(index) as usize;
            let page_end = (addr | (super::page_meta::GENERATION_PAGE_SIZE - 1)) + 1;
            let end = (index + (page_end - addr) / 8).min(len);
            while index < end {
                let bits = *slots.add(index);
                let tag = bits & crate::value::TAG_MASK;
                if (tag == crate::value::POINTER_TAG || tag == crate::value::STRING_TAG)
                    && super::pointer_in_nursery((bits & crate::value::POINTER_MASK) as usize)
                {
                    crate::gc::runtime_write_barrier_slot(
                        parent as usize,
                        slots.add(index) as usize,
                        bits,
                    );
                    break;
                }
                index += 1;
            }
            index = end;
        }
    }

    /// The caller must keep collection suppressed, permit no user callbacks,
    /// and finish every payload before leaving that window. Refuse active
    /// allocate-black phases: their ordinary birth seeding remains mandatory.
    pub(crate) unsafe fn new() -> Option<Self> {
        assert!(crate::gc::gc_is_suppressed());
        if crate::gc::gc_birth_extra_flags() != 0
            || !crate::gc::incremental_mark_barrier_globally_idle()
        {
            return None;
        }
        Some(Self {
            inline: super::js_inline_arena_state(),
            free_list_nonempty: crate::gc::hot_arena_free_list_nonempty(),
        })
    }

    /// Allocate a final, ordinary nursery object with eight-byte alignment.
    /// Null requests the unchanged allocator (block refill, large-object
    /// placement or free-list reuse). This successful arm has no TLS lookup,
    /// GC trigger, arena borrow, registration vector or page-table operation.
    ///
    /// Only JSON cell types are admitted: Map needs an exact-start bitmap.
    #[inline]
    pub(crate) unsafe fn try_alloc(&mut self, size: usize, obj_type: u8) -> *mut u8 {
        use crate::gc::{GcHeader, GC_FLAG_ARENA, GC_HEADER_SIZE};
        debug_assert!(matches!(
            obj_type,
            crate::gc::GC_TYPE_OBJECT | crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_STRING
        ));
        let Some(total) = size.checked_add(GC_HEADER_SIZE + 7).map(|n| n & !7) else {
            return std::ptr::null_mut();
        };
        if crate::gc::is_large_object_total_size_for_type(total, obj_type)
            || (*self.free_list_nonempty).get()
        {
            return std::ptr::null_mut();
        }
        // Re-read the shared cursor, never cache a block pointer across an
        // ordinary allocation. A key-cache miss may have filled that block.
        let state = &mut *self.inline;
        let offset = (state.offset + 7) & !7;
        if total > state.size.saturating_sub(offset) {
            return std::ptr::null_mut();
        }
        let raw = state.data.add(offset);
        state.offset = offset + total;
        // GC_STORE_AUDIT(INIT): initialize a fresh GC allocation header.
        raw.cast::<GcHeader>().write(GcHeader {
            obj_type,
            gc_flags: GC_FLAG_ARENA,
            _reserved: 0,
            size: total as u32,
        });
        raw.add(GC_HEADER_SIZE)
    }
}
