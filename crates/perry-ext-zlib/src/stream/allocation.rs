//! Native codec allocation accounting. Brotli routes every allocation through
//! a counting allocator; zstd exposes sizeof; miniz's fixed workspace uses its
//! pinned backend's actual boxed sizes (including its separately boxed arrays).
use brotli::{Allocator, SliceWrapper};
use std::cell::Cell;
use std::rc::Rc;
#[derive(Clone, Default)]
pub(super) struct CountingAlloc(pub(super) Rc<Cell<usize>>);
impl<T: Default + Clone> Allocator<T> for CountingAlloc {
    type AllocatedMemory = <brotli::enc::StandardAlloc as Allocator<T>>::AllocatedMemory;
    fn alloc_cell(&mut self, len: usize) -> Self::AllocatedMemory {
        let memory = brotli::enc::StandardAlloc::default().alloc_cell(len);
        self.0
            .set(self.0.get() + memory.slice().len() * std::mem::size_of::<T>());
        memory
    }
    fn free_cell(&mut self, memory: Self::AllocatedMemory) {
        self.0
            .set(self.0.get() - memory.slice().len() * std::mem::size_of::<T>());
        drop(memory);
    }
}
impl brotli::enc::combined_alloc::BrotliAlloc for CountingAlloc {}

pub(super) fn inflate_bytes() -> usize {
    std::mem::size_of::<miniz_oxide::inflate::stream::InflateState>()
}
