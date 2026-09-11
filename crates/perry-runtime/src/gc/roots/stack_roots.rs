//! Fixed-size roots for native callback loops. The existing shadow scanner
//! marks and rewrites their bound stack cells, so reading a current value does
//! not require a TLS lookup or indexing a growable handle buffer.

use super::shadow_stack::{js_shadow_frame_pop, js_shadow_frame_push, js_shadow_slot_bind};
use std::cell::UnsafeCell;

/// Run with fixed stack cells containing tagged JSValues or bare object-start
/// addresses (the shadow-stack root-word contract). The cells stay at the same
/// address until `f` returns; only the collector can change their contents.
///
/// Normal return and Rust unwind pop the frame before the cells die. A JS
/// throw restores the shadow savepoint before longjmp, removing the bindings
/// into the abandoned native frame even though Rust destructors do not run.
pub(crate) fn with_stack_roots<const N: usize, R>(
    values: [u64; N],
    f: impl FnOnce(&StackRoots<N>) -> R,
) -> R {
    let roots = StackRoots {
        cells: UnsafeCell::new(values),
    };
    let frame = Frame(js_shadow_frame_push(
        u32::try_from(N).expect("too many stack roots"),
    ));
    for index in 0..N {
        // UnsafeCell permits collector writes through the registered pointer
        // while `f` holds a shared reference. No Rust reference to a cell's
        // contents escapes, and the callback cannot move `roots` itself.
        js_shadow_slot_bind(index as u32, unsafe {
            roots.cells.get().cast::<u64>().add(index)
        });
    }
    let result = f(&roots);
    drop(frame);
    result
}

pub(crate) struct StackRoots<const N: usize> {
    cells: UnsafeCell<[u64; N]>,
}

impl<const N: usize> StackRoots<N> {
    #[inline(always)]
    pub(crate) fn get(&self, index: usize) -> u64 {
        assert!(index < N);
        // Copy only the current word; a later callback can rewrite this cell.
        unsafe { self.cells.get().cast::<u64>().add(index).read() }
    }
}

struct Frame(u64);

impl Drop for Frame {
    fn drop(&mut self) {
        js_shadow_frame_pop(self.0);
    }
}
