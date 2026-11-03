#[cfg(loom)]
pub(crate) use loom::cell::UnsafeCell;
#[cfg(loom)]
pub(crate) use loom::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[cfg(not(loom))]
pub(crate) use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[cfg(not(loom))]
pub(crate) struct UnsafeCell<T>(std::cell::UnsafeCell<T>);
#[cfg(not(loom))]
impl<T> UnsafeCell<T> {
    // The queue's slots are allocated zeroed rather than constructed one at a
    // time, so nothing in the non-loom build calls this any more. Kept because
    // it is half of this shim's parity with loom's `UnsafeCell`, and a shim
    // that only sometimes mirrors its subject is worse than an unused fn.
    #[allow(dead_code)]
    pub fn new(v: T) -> Self {
        Self(std::cell::UnsafeCell::new(v))
    }
    pub fn with_mut<R>(&self, f: impl FnOnce(*mut T) -> R) -> R {
        f(self.0.get())
    }
}
