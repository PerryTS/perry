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
    /// Allocate a record array ONCE at its estimated final size (#10123).
    ///
    /// The previous clamp of 16,384 slots is a 131,088-byte allocation -- 16
    /// bytes over the 131,072-byte pointer-bearing birth threshold -- so every
    /// large record array was born OLD on its very first allocation and then
    /// doubled twice more in old-gen (131 -> 262 -> 524 KB for 59,000 rows).
    /// An old array of young records keeps them alive through the remembered
    /// set after the document dies, and no full is ever scheduled for it.
    ///
    /// Sizing to the estimate removes the doubling chain. Up to the JSON
    /// young-birth ceiling the single allocation is kept in the nursery, so a
    /// minor reclaims the array and its records together once the document is
    /// dead; past it the array is still one old allocation rather than four.
    pub(super) unsafe fn presized_records(
        batch: &mut Option<crate::arena::ConstructionBatch>,
        estimated_len: usize,
    ) -> Self {
        let slot = std::mem::size_of::<crate::value::JSValue>();
        let header = std::mem::size_of::<ArrayHeader>() + crate::gc::GC_HEADER_SIZE;
        let young_slots = (crate::gc::LARGE_OBJECT_STORAGE_YOUNG_BIRTH_CEILING_BYTES
            .saturating_sub(header))
            / slot;
        let capacity = estimated_len.clamp(16, (u32::MAX / 2) as usize);
        if capacity <= young_slots {
            let _young = crate::gc::JsonWideBirthScope::arrays();
            return Self::new(batch, capacity as u32);
        }
        Self::new(batch, capacity as u32)
    }

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
            let slots = crate::array::array_elements_ptr(raw as *const ArrayHeader).cast::<u64>();
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
        let slot = crate::array::array_elements_ptr(self.ptr)
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
        let old_slots = crate::array::array_elements_ptr(self.ptr).cast::<JSValue>();
        debug_assert!(next.batched);
        let new_slots = crate::array::array_elements_ptr(next.ptr).cast::<JSValue>();
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
            let slots = crate::array::array_elements_ptr(self.ptr).cast::<u64>();
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
                let slots = crate::array::array_elements_ptr(self.ptr).cast::<u64>();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// #10123: a record array sized from the parser estimate is ONE allocation,
    /// born young when it fits the JSON young-birth ceiling and old past it.
    /// The previous 16,384-slot clamp was a 131,088-byte first allocation --
    /// 16 bytes over the pointer-bearing threshold -- so every large record
    /// array began life tenured and then doubled twice more in old-gen.
    #[test]
    fn presized_record_array_is_one_allocation_in_the_right_generation() {
        let slot = std::mem::size_of::<crate::value::JSValue>();
        let header = std::mem::size_of::<ArrayHeader>() + crate::gc::GC_HEADER_SIZE;
        let young_slots =
            (crate::gc::LARGE_OBJECT_STORAGE_YOUNG_BIRTH_CEILING_BYTES - header) / slot;
        unsafe {
            let _suppress = crate::gc::GcSuppressScope::new();

            // records_*_8m shape: 7.1 MB / 96 -> 74,145 slots, 593 KB.
            let mut batch = None;
            let young = ConstructionArray::presized_records(&mut batch, 74_145);
            assert!(74_145 <= young_slots, "fixture must sit under the ceiling");
            assert!(
                !crate::arena::pointer_in_old_gen(young.ptr as usize),
                "an estimate under the young-birth ceiling must be born young"
            );
            assert!((*young.ptr).capacity >= 74_145);

            // It must not need to grow for the rows it was sized for.
            let mut young = young;
            let before = young.ptr;
            for i in 0..59_000 {
                young.push(&mut batch, JSValue::number(i as f64));
            }
            assert_eq!(young.ptr, before, "a presized record array must not regrow");

            // records_*_20m shape: past the ceiling the single allocation is old.
            let mut batch = None;
            let old = ConstructionArray::presized_records(&mut batch, young_slots + 1_000);
            assert!(
                crate::arena::pointer_in_old_gen(old.ptr as usize),
                "an estimate past the ceiling keeps the old-gen birth"
            );
        }
    }
}
