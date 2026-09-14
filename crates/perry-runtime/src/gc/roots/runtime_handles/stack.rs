use super::{RuntimeHandleSlot, RUNTIME_HANDLE_STACK_HOT_GUARD};
use std::alloc::{alloc, dealloc, handle_alloc_error, realloc, Layout};
use std::cell::Cell;

/// Non-dropping metadata: ELF can resolve this TLS without a destructor-init
/// check. The separate hot guard owns the allocation and clears these cells
/// before freeing it. Scopes can therefore cache a reference to the metadata,
/// including in a TLS destructor that runs after the buffer has been freed.
/// Cell makes the scope and its borrowed handles thread-bound.
pub(super) struct RuntimeHandleStack {
    data: Cell<*mut RuntimeHandleSlot>,
    top: Cell<usize>,
    capacity: Cell<usize>,
}

pub(super) type StackRef = &'static RuntimeHandleStack;

impl RuntimeHandleStack {
    pub(super) const fn new() -> Self {
        Self {
            data: Cell::new(std::ptr::null_mut()),
            top: Cell::new(0),
            capacity: Cell::new(0),
        }
    }

    #[inline(always)]
    pub(super) fn len(&self) -> usize {
        self.top.get()
    }

    #[inline(always)]
    pub(super) fn truncate(&self, base: usize) {
        // Restore can precede a surviving scope's Drop. Never grow the live
        // prefix back to that scope's now-obsolete base (Vec::truncate semantics).
        self.top.set(self.top.get().min(base));
    }

    #[inline(always)]
    pub(super) fn push(&self, slot: RuntimeHandleSlot) -> usize {
        let index = self.top.get();
        if index == self.capacity.get() {
            self.grow();
        }
        // SAFETY: grow guarantees capacity > index. Slots are Copy, so a
        // truncated slot needs no drop before reuse. Publish only after write.
        unsafe { self.data.get().add(index).write(slot) };
        self.top.set(index + 1);
        index
    }

    #[inline(always)]
    pub(super) fn get(&self, index: usize) -> Option<RuntimeHandleSlot> {
        if index >= self.top.get() {
            return None;
        }
        // SAFETY: precisely [0, top) is initialized. No reference into the
        // buffer escapes; a callback may grow or scan the stack after this copy.
        Some(unsafe { self.data.get().add(index).read() })
    }

    #[inline(always)]
    pub(super) fn set(&self, index: usize, slot: RuntimeHandleSlot) {
        if index >= self.top.get() {
            super::handle_used_after_scope();
        }
        // SAFETY: the checked live index is initialized. Reload the base: a
        // visitor called between get and set may have reallocated the buffer.
        unsafe { self.data.get().add(index).write(slot) };
    }

    #[cold]
    #[inline(never)]
    fn grow(&self) {
        // Register the owner before allocating. During late TLS teardown this
        // also rejects resurrection after its destructor has already run.
        RUNTIME_HANDLE_STACK_HOT_GUARD.with(|_| {});
        let old_capacity = self.capacity.get();
        let capacity = old_capacity
            .checked_mul(2)
            .expect("runtime handle stack capacity overflow")
            .max(4);
        let layout = Layout::array::<RuntimeHandleSlot>(capacity)
            .expect("runtime handle stack allocation overflow");
        // SAFETY: data belongs exclusively to this thread and was allocated
        // with the matching old layout. Rust allocation does not run Perry GC.
        let data = unsafe {
            if old_capacity == 0 {
                alloc(layout)
            } else {
                realloc(
                    self.data.get().cast(),
                    Layout::array::<RuntimeHandleSlot>(old_capacity).unwrap(),
                    layout.size(),
                )
            }
        };
        if data.is_null() {
            handle_alloc_error(layout);
        }
        self.data.set(data.cast());
        self.capacity.set(capacity);
    }

    pub(super) fn release(&self) {
        let capacity = self.capacity.replace(0);
        let data = self.data.replace(std::ptr::null_mut());
        self.top.set(0);
        if capacity != 0 {
            // SAFETY: the guard releases its one allocation once, after
            // unpublishing the cache. No references into the buffer exist.
            unsafe {
                dealloc(
                    data.cast(),
                    Layout::array::<RuntimeHandleSlot>(capacity).unwrap(),
                );
            }
        }
    }

    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.capacity.get()
    }
}

const _: () = assert!(!std::mem::needs_drop::<RuntimeHandleStack>());
