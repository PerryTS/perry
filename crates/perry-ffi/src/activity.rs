//! Balanced native handle references. A reference owns exactly one increment.
use std::sync::atomic::{AtomicUsize, Ordering};

/// Owns one reference in a host-maintained native liveness counter.
pub struct Reference {
    counter: &'static AtomicUsize,
    active: bool,
}

impl Reference {
    /// Create an active or inactive reference.
    pub fn new(counter: &'static AtomicUsize, active: bool) -> Self {
        let mut reference = Self {
            counter,
            active: false,
        };
        reference.set(active);
        reference
    }

    /// Change reference state idempotently; Drop releases an active reference.
    pub fn set(&mut self, active: bool) {
        if self.active == active {
            return;
        }
        self.active = active;
        if active {
            self.counter.fetch_add(1, Ordering::AcqRel);
        } else {
            let previous = self.counter.fetch_sub(1, Ordering::AcqRel);
            debug_assert!(previous > 0, "native handle reference underflow");
        }
    }
}

impl Drop for Reference {
    fn drop(&mut self) {
        self.set(false);
    }
}

