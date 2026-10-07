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

/// miniz_oxide 0.9.1 (pinned) boxes the deflate dictionary (32 KiB plus 258
/// lookahead bytes), its 32 Ki-entry `next` and `hash` u16 chains, and three
/// 288-symbol Huffman tables (u16 counts, u16 codes, u8 sizes) outside
/// `CompressorOxide` itself.
pub(super) fn deflate_bytes() -> usize {
    const DICT: usize = 32_768 + 258;
    const CHAINS: usize = 2 * 32_768 * std::mem::size_of::<u16>();
    const HUFFMAN: usize = 3 * 288 * (2 + 2 + 1);
    std::mem::size_of::<miniz_oxide::deflate::core::CompressorOxide>() + DICT + CHAINS + HUFFMAN
}
