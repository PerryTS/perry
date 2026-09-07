//! An unpublished array under the parser's existing GC suppression window.
//! Aggregate layout facts in native state; install the final facts once.
use crate::{array::ArrayHeader, JSValue};

pub(super) struct ConstructionArray {
    ptr: *mut ArrayHeader,
    batched: bool,
    any_pointer: bool,
    all_pointers: bool,
    all_numbers: bool,
}

impl ConstructionArray {
    pub(super) unsafe fn new(
        batch: &mut Option<crate::arena::ConstructionBatch>,
        capacity: u32,
    ) -> Self {
        let raw = batch.as_mut().map_or(std::ptr::null_mut(), |b| {
            b.try_alloc(
                std::mem::size_of::<ArrayHeader>() + capacity as usize * 8,
                crate::gc::GC_TYPE_ARRAY,
            )
        });
        let ptr = if raw.is_null() {
            let ptr = crate::array::js_array_alloc_with_length_exact(capacity);
            (*ptr).length = 0;
            crate::array::set_array_numeric_layout(ptr, crate::array::NumericArrayLayout::RawF64);
            if batch.is_some() {
                crate::array::clear_array_numeric_layout(ptr);
            }
            ptr
        } else {
            let ptr = raw.cast::<ArrayHeader>();
            (*ptr).length = 0;
            (*ptr).capacity = capacity;
            let slots = raw.add(std::mem::size_of::<ArrayHeader>()).cast::<u64>();
            for index in 0..capacity as usize {
                // GC_STORE_AUDIT(INIT): initialize all physical slack for the
                // whole-heap verifier; live length advances only after writes.
                slots.add(index).write(crate::value::TAG_HOLE);
            }
            crate::gc::layout_init_pointer_free(raw);
            ptr
        };
        Self {
            ptr,
            batched: batch.is_some(),
            any_pointer: false,
            all_pointers: true,
            all_numbers: true,
        }
    }

    #[inline]
    pub(super) unsafe fn push(
        &mut self,
        batch: &mut Option<crate::arena::ConstructionBatch>,
        value: JSValue,
    ) {
        if (*self.ptr).length == (*self.ptr).capacity {
            self.grow(batch);
        }
        if !self.batched {
            let index = (*self.ptr).length as usize;
            if index < (*self.ptr).capacity as usize {
                crate::array::note_array_slot_layout_only(self.ptr, index, value.bits());
                (*self.ptr).length += 1;
            } else {
                self.ptr = crate::js_array_push(self.ptr, value);
            }
            return;
        }
        let length = (*self.ptr).length as usize;
        let slot = self
            .ptr
            .cast::<u8>()
            .add(std::mem::size_of::<ArrayHeader>())
            .cast::<JSValue>()
            .add(length);
        // GC_STORE_AUDIT(INIT): no marking phase or callbacks;
        // finish_layout installs the aggregate before publication or growth.
        slot.write(value);
        let tag = value.bits() & crate::value::TAG_MASK;
        let pointer = tag == crate::value::POINTER_TAG || tag == crate::value::STRING_TAG;
        self.any_pointer |= pointer;
        self.all_pointers &= pointer;
        self.all_numbers &= value.is_number();
        (*self.ptr).length += 1;
    }

    #[cold]
    unsafe fn grow(&mut self, batch: &mut Option<crate::arena::ConstructionBatch>) {
        if !self.batched {
            return; // The ordinary push owns its existing growth protocol.
        }
        self.finish_layout(batch);
        let capacity = (*self.ptr).capacity.saturating_mul(2).max(16);
        let mut next = Self::new(batch, capacity);
        let length = (*self.ptr).length as usize;
        let old_slots = self
            .ptr
            .cast::<u8>()
            .add(std::mem::size_of::<ArrayHeader>())
            .cast::<JSValue>();
        debug_assert!(next.batched);
        let new_slots = next
            .ptr
            .cast::<u8>()
            .add(std::mem::size_of::<ArrayHeader>())
            .cast::<JSValue>();
        // GC_STORE_AUDIT(INIT): completed nursery or old destination; aggregate
        // layout and page-level remembering are installed before publication.
        std::ptr::copy_nonoverlapping(old_slots, new_slots, length);
        (*next.ptr).length = length as u32;
        next.any_pointer = self.any_pointer;
        next.all_pointers = self.all_pointers;
        next.all_numbers = self.all_numbers;
        // No external alias exists during construction. The discarded prefix
        // is an ordinary, fully initialized unreachable array, collectible on
        // its own; it is not a root or an owner of the new allocation.
        *self = next;
    }

    unsafe fn finish_layout(&mut self, batch: &Option<crate::arena::ConstructionBatch>) {
        if !self.batched {
            return;
        }
        if self.all_pointers && (*self.ptr).length != 0 {
            crate::gc::layout_init_all_pointer_slots(self.ptr.cast());
        } else if self.any_pointer {
            // Preserve selective tracing for mixed arrays. Build its mask in
            // one pass over the completed payload, rather than updating a
            // per-object table for every element while parsing.
            let slots = self
                .ptr
                .cast::<u8>()
                .add(std::mem::size_of::<ArrayHeader>())
                .cast::<u64>();
            crate::gc::layout_rebuild_from_slots(
                self.ptr.cast(),
                slots,
                (*self.ptr).length as usize,
            );
        }
        if self.all_numbers {
            crate::array::set_array_numeric_layout(
                self.ptr,
                crate::array::NumericArrayLayout::RawF64,
            );
        }
        if self.any_pointer {
            if let Some(batch) = batch {
                let slots = self
                    .ptr
                    .cast::<u8>()
                    .add(std::mem::size_of::<ArrayHeader>())
                    .cast::<u64>();
                batch.finish_json_slots(self.ptr.cast(), slots, (*self.ptr).length as usize);
            }
        }
    }

    pub(super) unsafe fn finish(
        mut self,
        batch: &Option<crate::arena::ConstructionBatch>,
    ) -> *mut ArrayHeader {
        self.finish_layout(batch);
        self.ptr
    }
}
