//! `RootedValues` — a growable list of NaN-boxed JS values that the collector
//! can actually see.
//!
//! # The bug class this exists to close (#7949, third shape of #6949)
//!
//! A runtime helper that accumulates JS values into a plain `Vec<f64>` and then
//! keeps filling it across calls that can allocate has written a GC root the
//! collector does not know about:
//!
//! ```ignore
//! let mut out: Vec<(f64, f64)> = Vec::with_capacity(length);
//! for i in 0..length {
//!     let item = js_array_get_f64(raw, i as u32);
//!     let key = js_closure_call2(cb, item, i as f64); // runs user JS -> may evacuate
//!     out.push((key, item));                          // everything already in `out` is stale
//! }
//! ```
//!
//! The `Vec` lives on the Rust heap. It is not a shadow slot, not a temp root,
//! and not reachable from any registered scanner, so an evacuating minor can
//! neither keep those objects alive nor rewrite the addresses. Nothing faults
//! at the collection — the nursery recycles the bytes, the stale word reads a
//! valid *unrelated* object, and the program dies cycles later somewhere else
//! (`TypeError: value is not a function`). `scripts/gc_root_dominance_check.py`
//! is structurally blind to it: it reads emitted LLVM IR, and this container is
//! Rust-side.
//!
//! `RootedValues` stores each element as a [`RuntimeHandle`] instead of a raw
//! word. The handle stack is a registered *mutable* root scanner
//! (`MutableRootScannerSource::RuntimeHandles`), so every element is marked and
//! every element's slot is rewritten when its object moves. `get` re-reads the
//! slot, so a caller cannot accidentally name a pre-collection address unless
//! it deliberately stashes the returned word across a call.
//!
//! # What it does NOT do
//!
//! It is not a proof, for the same reason [`RuntimeHandle::across_mut`] is not:
//! Rust has no effect system to mark "this call may allocate". `get` hands back
//! a bare `f64`, and holding *that* across an allocating call is still wrong.
//! What this type buys is that the *container* stops being the hole, so the
//! remaining review question is local to a single expression.
//!
//! # Scope-nesting hazard
//!
//! [`RuntimeHandleScope::drop`] truncates the handle stack to the depth it was
//! created at. So **never push to an outer `RootedValues` while an inner
//! `RuntimeHandleScope` is alive** — the inner scope's drop would silently
//! truncate away the handles the outer container just took, and every later
//! `get` on them would panic (`runtime handle used after its scope was
//! dropped`) or, worse, alias a handle a later push reused. Either give the
//! whole helper one scope, or finish the inner scope before pushing again.

use super::*;

/// A `Vec<f64>` of NaN-boxed JS values that is a real GC root. See the module
/// docs for the bug class and the scope-nesting hazard.
pub struct RootedValues<'scope> {
    scope: &'scope RuntimeHandleScope,
    handles: Vec<RuntimeHandle<'scope>>,
}

impl<'scope> RootedValues<'scope> {
    #[inline]
    pub fn new(scope: &'scope RuntimeHandleScope) -> Self {
        Self {
            scope,
            handles: Vec::new(),
        }
    }

    #[inline]
    pub fn with_capacity(scope: &'scope RuntimeHandleScope, capacity: usize) -> Self {
        Self {
            scope,
            handles: Vec::with_capacity(capacity),
        }
    }

    /// Root `value` and append it. The element is a GC root from this point
    /// until the owning scope is dropped.
    #[inline]
    pub fn push(&mut self, value: f64) {
        let handle = self.scope.root_nanbox_f64(value);
        self.handles.push(handle);
    }

    /// Re-read element `index` **after** whatever has run since it was pushed.
    /// This is the only supported way to get the word back out: the returned
    /// `f64` is a copy and goes stale the moment anything else can allocate.
    #[inline]
    pub fn get(&self, index: usize) -> f64 {
        self.handles[index].get_nanbox_f64()
    }

    /// Overwrite element `index`, keeping it rooted.
    #[inline]
    pub fn set(&self, index: usize, value: f64) {
        self.handles[index].set_nanbox_f64(value);
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }

    /// Iterate the current values. Same caveat as [`Self::get`]: the yielded
    /// words are copies, so do not let one span an allocating call.
    pub fn iter(&self) -> impl Iterator<Item = f64> + '_ {
        self.handles.iter().map(RuntimeHandle::get_nanbox_f64)
    }
}
