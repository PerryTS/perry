//! `PayloadBuffer` for binding crates: a payload's raw working memory (#11919;
//! the runtime side is `perry-runtime/src/native_payload_buffer.rs`).
//!
//! A codec payload keeps one [`BufferOwner`] and allocates its working
//! buffers through it. The owner counts their exact bytes; add
//! [`BufferOwner::bytes`] into the external bytes the payload states (at
//! `alloc`, after each stream step, in `set_external_bytes`). Buffers free
//! themselves on drop, so `close` (a stream's `destroy()`) gives the bytes back
//! at once, and the sweep's drop of an unclosed payload is the backstop.
//!
//! Raw bytes only: never a JS value, a NaN-boxed word or a GC pointer.
//! An owner and its buffers stay on the payload's thread.
//!
//! [`BufferOwner::hook`] is the same allocator as a C codec's custom
//! allocator hook (`alloc(opaque, size)` / `free(opaque, ptr)`, brotli's
//! `CAllocator`), for a codec whose allocations the payload cannot see.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;

/// Every buffer's alignment.
pub const ALIGN: usize = 16;
const ABI_VERSION: u64 = 1;

extern "C" {
    fn js_perry_payload_buffer_abi() -> u64;
    fn js_perry_payload_buffer_alloc(
        owner: *const Cell<usize>,
        len: usize,
        out_len: *mut usize,
    ) -> *mut u8;
    fn js_perry_payload_buffer_grow(
        owner: *const Cell<usize>,
        ptr: *mut u8,
        len: usize,
        new_len: usize,
        out_len: *mut usize,
    ) -> *mut u8;
    fn js_perry_payload_buffer_shrink(
        owner: *const Cell<usize>,
        ptr: *mut u8,
        len: usize,
        new_len: usize,
        out_len: *mut usize,
    ) -> *mut u8;
    fn js_perry_payload_buffer_release(owner: *const Cell<usize>, ptr: *mut u8, len: usize);
    fn js_perry_payload_buffer_live_bytes() -> usize;
    fn js_perry_payload_buffer_hook_alloc(opaque: *mut c_void, size: usize) -> *mut c_void;
    fn js_perry_payload_buffer_hook_free(opaque: *mut c_void, ptr: *mut c_void);
}

/// The runtime speaks this file's buffer ABI.
pub fn abi_matches() -> bool {
    unsafe { js_perry_payload_buffer_abi() == ABI_VERSION | (ALIGN as u64) << 8 }
}

/// Process-wide bytes held by live payload buffers (tests, diagnostics).
pub fn live_bytes() -> usize {
    unsafe { js_perry_payload_buffer_live_bytes() }
}

/// One payload's buffer ledger: the runtime's `PayloadBufferOwner` (a
/// transparent byte counter) behind an `Rc`, so its address stays put while
/// the payload moves and every buffer and allocator clone can reach it.
#[derive(Clone)]
pub struct BufferOwner(Rc<Cell<usize>>);

impl Default for BufferOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl BufferOwner {
    /// Bytes of the owner's own heap cell (the `Rc` allocation), for callers
    /// that state the payload's full native size.
    pub const HEAP_BYTES: usize = 3 * std::mem::size_of::<usize>();

    /// A fresh owner with no buffers.
    pub fn new() -> Self {
        assert!(abi_matches(), "payload buffer ABI mismatch");
        Self(Rc::new(Cell::new(0)))
    }

    /// Exact bytes held by this owner's live buffers.
    pub fn bytes(&self) -> usize {
        self.0.get()
    }

    fn ledger(&self) -> *const Cell<usize> {
        Rc::as_ptr(&self.0)
    }

    /// The owner as a C allocator hook. `opaque` stays valid while any clone
    /// of this owner lives; keep one in the codec state that uses the hook.
    pub fn hook(&self) -> CAllocHook {
        CAllocHook {
            alloc_func: js_perry_payload_buffer_hook_alloc,
            free_func: js_perry_payload_buffer_hook_free,
            opaque: self.ledger() as *mut c_void,
        }
    }
}

/// A C codec's allocator hook triple (brotli's `CAllocator` layout).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CAllocHook {
    /// Allocate `size` zeroed bytes for `opaque`; null on failure.
    pub alloc_func: unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void,
    /// Free a block from `alloc_func` (null is a no-op).
    pub free_func: unsafe extern "C" fn(*mut c_void, *mut c_void),
    /// The owner's ledger.
    pub opaque: *mut c_void,
}

/// A zero-initialized, `ALIGN`-aligned byte buffer counted against its owner.
pub struct PayloadBuffer {
    ptr: NonNull<u8>,
    len: usize,
    owner: BufferOwner,
}

impl PayloadBuffer {
    /// `len` zeroed bytes; `None` when the backing cannot supply them.
    pub fn alloc(owner: &BufferOwner, len: usize) -> Option<Self> {
        let mut out = 0;
        let ptr = unsafe { js_perry_payload_buffer_alloc(owner.ledger(), len, &mut out) };
        Some(Self {
            ptr: NonNull::new(ptr)?,
            len: out,
            owner: owner.clone(),
        })
    }

    /// Grow to `new_len`, keeping the bytes (the tail is zero). `false` leaves
    /// the buffer unchanged.
    pub fn grow(&mut self, new_len: usize) -> bool {
        let mut out = self.len;
        let ptr = unsafe {
            js_perry_payload_buffer_grow(
                self.owner.ledger(),
                self.ptr.as_ptr(),
                self.len,
                new_len,
                &mut out,
            )
        };
        match NonNull::new(ptr) {
            Some(ptr) => {
                self.ptr = ptr;
                self.len = out;
                true
            }
            None => false,
        }
    }

    /// Shrink to `new_len`, keeping the first `new_len` bytes.
    pub fn shrink(&mut self, new_len: usize) {
        let mut out = self.len;
        let ptr = unsafe {
            js_perry_payload_buffer_shrink(
                self.owner.ledger(),
                self.ptr.as_ptr(),
                self.len,
                new_len,
                &mut out,
            )
        };
        if let Some(ptr) = NonNull::new(ptr) {
            self.ptr = ptr;
            self.len = out;
        }
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the length is zero.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The first byte.
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    /// The first byte, writable.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// The bytes.
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    /// The bytes, writable.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

impl Drop for PayloadBuffer {
    fn drop(&mut self) {
        unsafe {
            js_perry_payload_buffer_release(self.owner.ledger(), self.ptr.as_ptr(), self.len)
        };
    }
}
