//! Logical array element addresses, including a lazy queue front offset.

use crate::block::LlBlock;
use crate::types::{I32, I64};

impl LlBlock {
    /// The caller has proved `array` is a live, non-forwarded GC_TYPE_ARRAY.
    /// Its backing ends at `array + GcHeader.size - GC_HEADER_SIZE`; capacity
    /// counts the slots remaining after the queue front. Match runtime
    /// `array::storage::array_elements_ptr` without a call or an extra header.
    pub(crate) fn array_elements_addr(&mut self, array: &str) -> String {
        let size_addr = self.sub(I64, array, "4");
        let size_ptr = self.inttoptr(I64, &size_addr);
        let size = self.load(I32, &size_ptr);
        let size = self.zext(I32, &size, I64);
        let capacity_addr = self.add(I64, array, "4");
        let capacity_ptr = self.inttoptr(I64, &capacity_addr);
        let capacity = self.load(I32, &capacity_ptr);
        let capacity = self.zext(I32, &capacity, I64);
        let bytes = self.shl(I64, &capacity, "3");
        let end = self.add(I64, array, &size);
        let end = self.sub(I64, &end, "8");
        self.sub(I64, &end, &bytes)
    }
}
