//! Logical array element addresses, including a lazy queue front offset.

use crate::block::LlBlock;
use crate::types::{I32, I64};

impl LlBlock {
    /// The caller has proved `array` is a live, non-forwarded GC_TYPE_ARRAY.
    /// Its backing ends at `array + GcHeader.size - GC_HEADER_SIZE`; capacity
    /// counts the slots remaining after the queue front. Match runtime
    /// `array::storage::array_elements_ptr` without a call or an extra header.
    pub(crate) fn array_elements_addr(&mut self, array: &str) -> String {
        let capacity_addr = self.add(I64, array, "4");
        let capacity_ptr = self.inttoptr(I64, &capacity_addr);
        let capacity = self.load(I32, &capacity_ptr);
        self.array_elements_addr_with_capacity(array, &capacity)
    }

    /// [`Self::array_elements_addr`] for a caller that has already loaded
    /// `capacity` (the i32 at payload `+4`) from the same live header, so the
    /// element base does not re-load it.
    pub(crate) fn array_elements_addr_with_capacity(
        &mut self,
        array: &str,
        capacity: &str,
    ) -> String {
        let size_addr = self.sub(I64, array, "4");
        let size_ptr = self.inttoptr(I64, &size_addr);
        let size = self.load(I32, &size_ptr);
        let size = self.zext(I32, &size, I64);
        let capacity = self.zext(I32, capacity, I64);
        let bytes = self.shl(I64, &capacity, "3");
        let end = self.add(I64, array, &size);
        let end = self.sub(I64, &end, "8");
        self.sub(I64, &end, &bytes)
    }

    /// Lower the array's dense prefix P (EK design §3.1) to at most `bound`
    /// (an i32): every inline store that lowers `length` keeps
    /// `P <= length`, as the runtime's `array_truncate_length` does. The
    /// caller has proved a live, non-forwarded GC_TYPE_ARRAY.
    pub(crate) fn array_lower_ek_prefix(&mut self, array: &str, bound: &str) {
        let offset = crate::target_layout::ARRAY_EK_PREFIX_OFFSET_BYTES.to_string();
        let prefix_addr = self.add(I64, array, &offset);
        let prefix_ptr = self.inttoptr(I64, &prefix_addr);
        let prefix = self.load(I32, &prefix_ptr);
        let above = self.icmp_ult(I32, bound, &prefix);
        let lowered = self.select(crate::types::I1, &above, I32, bound, &prefix);
        // GC_STORE_AUDIT(POINTER_FREE): the element-kind word is an integer, never an edge.
        self.store(I32, &lowered, &prefix_ptr);
    }

    /// Address one checked GET slot in the same canonical backing. The caller
    /// proves a live Array and `index < capacity`. In eight-byte slot units,
    /// `size/8 - capacity + index` lies in `[2, size/8)`, so this u32 arithmetic
    /// cannot overflow. Scale only the final displacement. This also matches
    /// runtime storage's rounded physical capacity for shifted/named arrays.
    /// Store and append builders remain unchanged.
    pub(crate) fn array_element_addr_with_capacity(
        &mut self,
        array: &str,
        capacity: &str,
        index: &str,
    ) -> String {
        let size_addr = self.sub(I64, array, "4");
        let size_ptr = self.inttoptr(I64, &size_addr);
        let size = self.load(I32, &size_ptr);
        let slots = self.lshr(I32, &size, "3");
        let slots = self.sub(I32, &slots, capacity);
        let slots = self.add(I32, &slots, index);
        let slots = self.zext(I32, &slots, I64);
        let bytes = self.shl(I64, &slots, "3");
        let tail = self.sub(I64, array, "8");
        self.add(I64, &tail, &bytes)
    }
}
