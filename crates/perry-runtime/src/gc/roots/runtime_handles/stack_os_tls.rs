//! Android and HarmonyOS use OS-backed TLS: even a non-Drop value's allocation is freed
//! during teardown. Preserve the original scoped Vec access on these targets.
//! Scopes and handles carry a thread-bound token, never an escaped TLS address.

use super::{handle_used_after_scope, RuntimeHandleSlot, RUNTIME_HANDLE_STACK};
use std::cell::RefCell;
use std::marker::PhantomData;

pub(super) struct RuntimeHandleStack(RefCell<Vec<RuntimeHandleSlot>>);

impl RuntimeHandleStack {
    pub(super) const fn new() -> Self {
        Self(RefCell::new(Vec::new()))
    }
}

#[derive(Clone, Copy)]
pub(super) struct StackRef(PhantomData<*mut ()>);

impl StackRef {
    pub(super) const fn new() -> Self {
        Self(PhantomData)
    }

    #[inline]
    pub(super) fn len(self) -> usize {
        RUNTIME_HANDLE_STACK.with(|stack| stack.0.borrow().len())
    }

    #[inline]
    pub(super) fn truncate(self, base: usize) {
        RUNTIME_HANDLE_STACK.with(|stack| stack.0.borrow_mut().truncate(base));
    }

    #[inline]
    pub(super) fn push(self, slot: RuntimeHandleSlot) -> usize {
        RUNTIME_HANDLE_STACK.with(|stack| {
            let mut slots = stack.0.borrow_mut();
            let index = slots.len();
            slots.push(slot);
            index
        })
    }

    #[inline]
    pub(super) fn get(self, index: usize) -> Option<RuntimeHandleSlot> {
        RUNTIME_HANDLE_STACK.with(|stack| stack.0.borrow().get(index).copied())
    }

    #[inline]
    pub(super) fn set(self, index: usize, slot: RuntimeHandleSlot) {
        RUNTIME_HANDLE_STACK.with(|stack| {
            let mut slots = stack.0.borrow_mut();
            let Some(target) = slots.get_mut(index) else {
                handle_used_after_scope();
            };
            *target = slot;
        });
    }

    #[cfg(test)]
    pub(super) fn capacity(self) -> usize {
        RUNTIME_HANDLE_STACK.with(|stack| stack.0.borrow().capacity())
    }
}
