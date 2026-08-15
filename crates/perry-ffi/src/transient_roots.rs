//! Transient GC roots for FFI code (#8082).
//!
//! Ext crates keep user closures and other heap values in handle-struct side
//! tables that registered mutable-root scanners mark AND rewrite on a moving
//! collection. A SNAPSHOT of those tables in a Rust local — a cloned listener
//! `Vec<i64>`, a pending-request struct drained from a channel — is a copy
//! the collector cannot see: after the first callback triggers a collection,
//! every remaining copied pointer is stale. The #8082 forced-moving gate
//! faulted on exactly that shape in the http server's pending-request pump.
//!
//! `TransientRootScope` parks such copies in the runtime's transient-handle
//! stack (the same slots `RuntimeHandleScope` uses, marked and rewritten by
//! the registered scanner) and re-reads the post-collection value at each
//! use. Scopes must strictly nest — the `Drop` truncates back to the entry
//! depth, and the runtime's JS-exception savepoints restore the stack across
//! throws that skip Rust frames.

extern "C" {
    fn js_ffi_root_scope_enter() -> usize;
    fn js_ffi_root_push_heap_addr(addr: u64) -> usize;
    fn js_ffi_root_get_heap_addr(index: usize) -> u64;
    fn js_ffi_root_push_nanbox(bits: u64) -> usize;
    fn js_ffi_root_get_nanbox(index: usize) -> u64;
    fn js_ffi_root_scope_exit(base: usize);
}

/// RAII scope over the runtime's transient-handle stack; see module docs.
pub struct TransientRootScope {
    base: usize,
}

impl TransientRootScope {
    /// Snapshot the current stack depth; `Drop` truncates back to it.
    pub fn enter() -> Self {
        Self {
            base: unsafe { js_ffi_root_scope_enter() },
        }
    }

    /// Root a raw heap address (an ext table's `i64` closure pointer). A zero
    /// address is accepted and reads back as zero.
    pub fn root_addr(&self, addr: i64) -> TransientRootedAddr {
        TransientRootedAddr {
            index: unsafe { js_ffi_root_push_heap_addr(addr as u64) },
        }
    }

    /// Root every address in `addrs`, preserving order.
    pub fn root_addrs(&self, addrs: &[i64]) -> Vec<TransientRootedAddr> {
        addrs.iter().map(|addr| self.root_addr(*addr)).collect()
    }

    /// Root a NaN-boxed value handed to callbacks (string/buffer/object).
    pub fn root_nanbox(&self, value: f64) -> TransientRootedNanbox {
        TransientRootedNanbox {
            index: unsafe { js_ffi_root_push_nanbox(value.to_bits()) },
        }
    }
}

impl Drop for TransientRootScope {
    fn drop(&mut self) {
        unsafe { js_ffi_root_scope_exit(self.base) }
    }
}

/// A rooted raw heap address; `get()` returns the post-collection value.
#[derive(Clone, Copy)]
pub struct TransientRootedAddr {
    index: usize,
}

impl TransientRootedAddr {
    /// The post-collection address. Re-read this at every use; never hold the
    /// returned value across another call that can run JS.
    pub fn get(&self) -> i64 {
        unsafe { js_ffi_root_get_heap_addr(self.index) as i64 }
    }
}

/// A rooted NaN-boxed value; `get()` returns the post-collection value.
#[derive(Clone, Copy)]
pub struct TransientRootedNanbox {
    index: usize,
}

impl TransientRootedNanbox {
    /// The post-collection value. Re-read at every use.
    pub fn get(&self) -> f64 {
        f64::from_bits(unsafe { js_ffi_root_get_nanbox(self.index) })
    }
}
