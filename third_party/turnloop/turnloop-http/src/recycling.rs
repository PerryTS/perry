//! Recycle brotli scratch allocations across complete bodies, including buffers
//! released between metablocks. All memory is initialized before handing it back.
use std::sync::{Arc, Mutex};
pub(crate) struct Memory<T>(Vec<T>);
impl<T> Default for Memory<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}
impl<T> brotli::SliceWrapper<T> for Memory<T> {
    fn slice(&self) -> &[T] {
        &self.0
    }
}
impl<T> brotli::SliceWrapperMut<T> for Memory<T> {
    fn slice_mut(&mut self) -> &mut [T] {
        &mut self.0
    }
}
pub(crate) struct Pool<T>(Arc<Mutex<Vec<Memory<T>>>>);
impl<T> Clone for Pool<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Default for Pool<T> {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Vec::new())))
    }
}
impl<T: Default + Clone> brotli::Allocator<T> for Pool<T> {
    type AllocatedMemory = Memory<T>;
    fn alloc_cell(&mut self, len: usize) -> Memory<T> {
        let mut pool = self.0.lock().expect("scratch pool poisoned");
        let mut result = pool
            .iter()
            .enumerate()
            .filter(|(_, m)| m.0.capacity() >= len)
            .min_by_key(|(_, m)| m.0.capacity())
            .map(|(i, _)| i)
            .map(|i| pool.swap_remove(i))
            .unwrap_or_default();
        result.0.resize(len, T::default());
        result.0.fill(T::default());
        result
    }
    fn free_cell(&mut self, mut memory: Memory<T>) {
        if memory.0.capacity() != 0 {
            memory.0.clear();
            self.0.lock().expect("scratch pool poisoned").push(memory);
        }
    }
}
