//! Recursion depth guard for `js_native_call_method`, preventing stack
//! overflow from circular module dependencies during initialization.
//!
//! The depth is a managed stack of frame marks (`crate::exception::marks`):
//! one entry per method frame in flight. A throw skips the guards' drops of
//! the frames it abandons; the catching landing pad pops their entries, so the
//! counter cannot leak one level per caught throw (#5591).

use std::cell::RefCell;

crate::perry_thread_local! {
    static CALL_METHOD_DEPTH: RefCell<crate::exception::CatchStack<()>> =
        const { RefCell::new(crate::exception::CatchStack::new()) };
}
const MAX_CALL_METHOD_DEPTH: usize = 512;

pub(super) struct CallMethodDepthGuard {
    depth_before: usize,
}
impl CallMethodDepthGuard {
    /// Enter one method frame, owned by the calling frame. `#[inline(always)]`
    /// is load-bearing: the mark must be read in the guard's owner.
    #[inline(always)]
    pub(super) fn enter(_method_name: &str) -> Option<Self> {
        Self::enter_at(crate::exception::frame_mark())
    }

    #[inline(always)]
    fn enter_at(mark: usize) -> Option<Self> {
        CALL_METHOD_DEPTH.with(|d| {
            let mut d = d.borrow_mut();
            let v = d.len();
            if v >= MAX_CALL_METHOD_DEPTH {
                // Silently return null object to prevent stack overflow
                None
            } else {
                d.push(mark, ());
                Some(CallMethodDepthGuard { depth_before: v })
            }
        })
    }
}
impl Drop for CallMethodDepthGuard {
    fn drop(&mut self) {
        CALL_METHOD_DEPTH.with(|d| {
            // A landing pad may already have popped this entry (its frame was
            // abandoned and a later system-unwinder cleanup runs this drop),
            // so the drop truncates to its own index: idempotent, never a
            // second decrement of an enclosing method's level.
            let mut d = d.borrow_mut();
            if d.len() > self.depth_before {
                d.truncate(self.depth_before);
            }
        });
    }
}

/// Landing pad at stack pointer `sp`: drop the levels of every method frame
/// the throw abandoned.
pub(crate) fn call_method_depth_unwind_to(sp: usize) {
    CALL_METHOD_DEPTH.with(|d| {
        d.borrow_mut().unwind_to(sp);
    });
}

#[cfg(any(test, target_os = "wasi"))]
pub(crate) fn call_method_depth_restore_depth(depth: usize) {
    CALL_METHOD_DEPTH.with(|d| {
        d.borrow_mut().restore_depth(depth);
    });
}

pub(crate) fn call_method_depth_check_marks(sp: usize) {
    CALL_METHOD_DEPTH
        .with(|d| crate::exception::marks::check_marks(d.borrow().marks(), sp, "call_method"));
}

/// The current `js_native_call_method` recursion depth.
#[cfg(any(test, target_os = "wasi"))]
pub(crate) fn call_method_depth() -> u32 {
    CALL_METHOD_DEPTH.with(|d| d.borrow().len() as u32)
}

#[cfg(test)]
pub(crate) fn test_enter_catch_method(mark: usize) {
    std::mem::forget(CallMethodDepthGuard::enter_at(mark).unwrap());
}
