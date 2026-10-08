//! `PayloadBuffer`: a native payload's raw working memory (#11919).
//!
//! A codec keeps large working buffers (brotli's ring buffer and hash tables,
//! an inflate window, an output scratch). They are plain bytes owned by the
//! payload `T`: never traced, never holding a JS value or a GC pointer.
//!
//! * [`PayloadBuffer::alloc`] / [`grow`](PayloadBuffer::grow) /
//!   [`shrink`](PayloadBuffer::shrink) / [`release`](PayloadBuffer::release)
//!   take the payload's [`PayloadBufferOwner`], which counts the exact bytes
//!   its live buffers hold. The family adds [`PayloadBufferOwner::bytes`] into
//!   the external bytes it states (`alloc`, `set_external_bytes`, a stream
//!   step's `StepOut::external_bytes`), so the buffers are paced like any
//!   other retained native memory, and the cell's release returns them.
//! * The payload frees its buffers in `Drop for T`. `close` (a stream's
//!   `destroy()`) drops `T` at once, so the bytes go back before any
//!   collection; the sweep (or worker teardown) is the backstop for a payload
//!   nobody closed.
//! * [`js_perry_payload_buffer_hook_alloc`] / [`js_perry_payload_buffer_hook_free`]
//!   are the same allocator in the shape of a C codec's custom allocator hook
//!   (`alloc(opaque, size)`, `free(opaque, ptr)`, brotli's `CAllocator`):
//!   `opaque` is the owner, and the block's size rides in a 16-byte prefix.
//!
//! Backing: every byte comes from [`backing`], the one seam the region
//! page-run allocator replaces. Today it is the global allocator (mimalloc on
//! 64-bit targets), with the requested length accounted exactly.
//!
//! Rules: an owner and its buffers belong to one thread (the payload's); a
//! buffer is released through the owner that allocated it; nothing here
//! allocates on the GC heap, calls JS or can start a collection, so a stream
//! step and a `Drop for T` may use it.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Every buffer is aligned to this (enough for any codec state struct).
pub const PAYLOAD_BUFFER_ALIGN: usize = 16;

/// Revision of the buffer C ABI. Bumped with any signature change.
pub const PERRY_PAYLOAD_BUFFER_ABI_VERSION: u64 = 1;

const HOOK_HEADER: usize = PAYLOAD_BUFFER_ALIGN;
const MAX_LEN: usize = isize::MAX as usize - PAYLOAD_BUFFER_ALIGN;

/// The exact bytes one payload's live buffers hold. Plain native data: it may
/// sit in the payload inline, or behind an `Rc` when a codec's allocator
/// instances need a stable `opaque` pointer.
#[repr(transparent)]
#[derive(Default, Debug)]
pub struct PayloadBufferOwner {
    bytes: Cell<usize>,
}

impl PayloadBufferOwner {
    pub const fn new() -> Self {
        Self {
            bytes: Cell::new(0),
        }
    }

    /// Bytes held by this owner's live buffers.
    #[inline]
    pub fn bytes(&self) -> usize {
        self.bytes.get()
    }

    #[inline]
    fn add(&self, bytes: usize) {
        self.bytes.set(self.bytes.get() + bytes);
        LIVE_BYTES.fetch_add(bytes, Ordering::Relaxed);
    }

    #[inline]
    fn sub(&self, bytes: usize) {
        let held = self.bytes.get();
        debug_assert!(held >= bytes, "buffer released by another owner");
        // Releasing more than the owner holds is a caller bug. Both counters
        // take the same clamped amount, so neither wraps and they stay equal
        // (the process count is the sum of the owner counts).
        let bytes = bytes.min(held);
        self.bytes.set(held - bytes);
        LIVE_BYTES.fetch_sub(bytes, Ordering::Relaxed);
    }
}

/// Bytes held by every live payload buffer in the process.
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Process-wide bytes held by live payload buffers.
pub fn live_bytes() -> usize {
    LIVE_BYTES.load(Ordering::Relaxed)
}

/// The backing seam: allocate (`ptr` null), resize, or free (`new_len` 0) a
/// block of `PAYLOAD_BUFFER_ALIGN`-aligned bytes. Fresh and grown bytes are
/// zero. Null means the request could not be met; the old block is intact.
///
/// # Safety
/// A non-null `ptr` came from this function with length `old_len`.
unsafe fn backing(ptr: *mut u8, old_len: usize, new_len: usize) -> *mut u8 {
    use std::alloc::{alloc_zeroed, dealloc, realloc, Layout};
    if new_len > MAX_LEN {
        return std::ptr::null_mut();
    }
    if ptr.is_null() {
        return alloc_zeroed(Layout::from_size_align_unchecked(
            new_len,
            PAYLOAD_BUFFER_ALIGN,
        ));
    }
    let old = Layout::from_size_align_unchecked(old_len, PAYLOAD_BUFFER_ALIGN);
    if new_len == 0 {
        dealloc(ptr, old);
        return std::ptr::null_mut();
    }
    let resized = realloc(ptr, old, new_len);
    if !resized.is_null() && new_len > old_len {
        resized.add(old_len).write_bytes(0, new_len - old_len);
    }
    resized
}

#[inline]
fn empty() -> NonNull<u8> {
    // Aligned, never dereferenced: the pointer of every zero-length buffer.
    NonNull::new(PAYLOAD_BUFFER_ALIGN as *mut u8).unwrap()
}

/// The allocator's namespace. A buffer is `(ptr, len)`; the caller keeps both
/// and hands them back to [`grow`](Self::grow), [`shrink`](Self::shrink) and
/// [`release`](Self::release).
pub struct PayloadBuffer;

impl PayloadBuffer {
    /// `len` zeroed bytes counted against `owner`. `None` when the backing
    /// cannot supply them. A zero length allocates nothing.
    pub fn alloc(owner: &PayloadBufferOwner, len: usize) -> Option<(NonNull<u8>, usize)> {
        if len == 0 {
            return Some((empty(), 0));
        }
        let ptr = NonNull::new(unsafe { backing(std::ptr::null_mut(), 0, len) })?;
        owner.add(len);
        Some((ptr, len))
    }

    /// Grow a buffer to `new_len`, keeping its bytes; the new tail is zero.
    /// `None` leaves the old buffer intact.
    ///
    /// # Safety
    /// `(ptr, len)` is a live buffer of `owner`.
    pub unsafe fn grow(
        owner: &PayloadBufferOwner,
        ptr: NonNull<u8>,
        len: usize,
        new_len: usize,
    ) -> Option<(NonNull<u8>, usize)> {
        if new_len <= len {
            return Some((ptr, len));
        }
        if len == 0 {
            return Self::alloc(owner, new_len);
        }
        let grown = NonNull::new(backing(ptr.as_ptr(), len, new_len))?;
        owner.add(new_len - len);
        Some((grown, new_len))
    }

    /// Shrink a buffer to `new_len`, keeping its first `new_len` bytes. If the
    /// backing cannot move it, the old buffer is returned unchanged.
    ///
    /// # Safety
    /// `(ptr, len)` is a live buffer of `owner`.
    pub unsafe fn shrink(
        owner: &PayloadBufferOwner,
        ptr: NonNull<u8>,
        len: usize,
        new_len: usize,
    ) -> (NonNull<u8>, usize) {
        if new_len >= len {
            return (ptr, len);
        }
        if new_len == 0 {
            Self::release(owner, ptr, len);
            return (empty(), 0);
        }
        match NonNull::new(backing(ptr.as_ptr(), len, new_len)) {
            Some(shrunk) => {
                owner.sub(len - new_len);
                (shrunk, new_len)
            }
            None => (ptr, len),
        }
    }

    /// Give a buffer back.
    ///
    /// # Safety
    /// `(ptr, len)` is a live buffer of `owner`; it is not used afterwards.
    pub unsafe fn release(owner: &PayloadBufferOwner, ptr: NonNull<u8>, len: usize) {
        if len == 0 {
            return;
        }
        backing(ptr.as_ptr(), len, 0);
        owner.sub(len);
    }
}

// ---- C ABI (perry-ffi `native_payload::buffer`) -------------------------

/// The buffer ABI revision and alignment, checked by perry-ffi.
#[no_mangle]
pub extern "C" fn js_perry_payload_buffer_abi() -> u64 {
    PERRY_PAYLOAD_BUFFER_ABI_VERSION | (PAYLOAD_BUFFER_ALIGN as u64) << 8
}

/// `len` zeroed bytes for `owner`; null on failure. `*out_len` is the length.
///
/// # Safety
/// `owner` is a live owner on this thread; `out_len` is writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_buffer_alloc(
    owner: *const PayloadBufferOwner,
    len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    match PayloadBuffer::alloc(&*owner, len) {
        Some((ptr, len)) => {
            out_len.write(len);
            ptr.as_ptr()
        }
        None => std::ptr::null_mut(),
    }
}

/// Grow; null on failure (the old buffer stays valid).
///
/// # Safety
/// As [`PayloadBuffer::grow`]; `out_len` is writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_buffer_grow(
    owner: *const PayloadBufferOwner,
    ptr: *mut u8,
    len: usize,
    new_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    let Some(ptr) = NonNull::new(ptr) else {
        return std::ptr::null_mut();
    };
    match PayloadBuffer::grow(&*owner, ptr, len, new_len) {
        Some((ptr, len)) => {
            out_len.write(len);
            ptr.as_ptr()
        }
        None => std::ptr::null_mut(),
    }
}

/// Shrink; never fails (an unmovable buffer keeps its old length).
///
/// # Safety
/// As [`PayloadBuffer::shrink`]; `out_len` is writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_buffer_shrink(
    owner: *const PayloadBufferOwner,
    ptr: *mut u8,
    len: usize,
    new_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    let Some(ptr) = NonNull::new(ptr) else {
        out_len.write(len);
        return ptr;
    };
    let (ptr, len) = PayloadBuffer::shrink(&*owner, ptr, len, new_len);
    out_len.write(len);
    ptr.as_ptr()
}

/// # Safety
/// As [`PayloadBuffer::release`].
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_buffer_release(
    owner: *const PayloadBufferOwner,
    ptr: *mut u8,
    len: usize,
) {
    if let Some(ptr) = NonNull::new(ptr) {
        PayloadBuffer::release(&*owner, ptr, len);
    }
}

/// Process-wide live payload-buffer bytes (tests, diagnostics).
#[no_mangle]
pub extern "C" fn js_perry_payload_buffer_live_bytes() -> usize {
    live_bytes()
}

/// A C codec's allocator hook (`brotli_alloc_func`): `opaque` is the owner.
/// The block is zeroed; its size is kept in a 16-byte prefix for the free.
///
/// # Safety
/// `opaque` is null or a live [`PayloadBufferOwner`] on this thread.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_buffer_hook_alloc(
    opaque: *mut c_void,
    size: usize,
) -> *mut c_void {
    if opaque.is_null() || size == 0 {
        return std::ptr::null_mut();
    }
    let Some(total) = size.checked_add(HOOK_HEADER) else {
        return std::ptr::null_mut();
    };
    let owner = &*(opaque as *const PayloadBufferOwner);
    match PayloadBuffer::alloc(owner, total) {
        Some((ptr, _)) => {
            (ptr.as_ptr() as *mut usize).write(total);
            ptr.as_ptr().add(HOOK_HEADER).cast()
        }
        None => std::ptr::null_mut(),
    }
}

/// The matching free (`brotli_free_func`). Null is a no-op.
///
/// # Safety
/// `ptr` is null or a live block from [`js_perry_payload_buffer_hook_alloc`]
/// with this `opaque`, not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_buffer_hook_free(opaque: *mut c_void, ptr: *mut c_void) {
    if opaque.is_null() || ptr.is_null() {
        return;
    }
    let base = (ptr as *mut u8).sub(HOOK_HEADER);
    let total = (base as *const usize).read();
    PayloadBuffer::release(
        &*(opaque as *const PayloadBufferOwner),
        NonNull::new_unchecked(base),
        total,
    );
}

#[cfg(feature = "keepalive-anchors")]
mod keepalive {
    use super::*;
    #[used(compiler)]
    static ABI: extern "C" fn() -> u64 = js_perry_payload_buffer_abi;
    #[used(compiler)]
    static ALLOC: unsafe extern "C" fn(*const PayloadBufferOwner, usize, *mut usize) -> *mut u8 =
        js_perry_payload_buffer_alloc;
    #[used(compiler)]
    static GROW: unsafe extern "C" fn(
        *const PayloadBufferOwner,
        *mut u8,
        usize,
        usize,
        *mut usize,
    ) -> *mut u8 = js_perry_payload_buffer_grow;
    #[used(compiler)]
    static SHRINK: unsafe extern "C" fn(
        *const PayloadBufferOwner,
        *mut u8,
        usize,
        usize,
        *mut usize,
    ) -> *mut u8 = js_perry_payload_buffer_shrink;
    #[used(compiler)]
    static RELEASE: unsafe extern "C" fn(*const PayloadBufferOwner, *mut u8, usize) =
        js_perry_payload_buffer_release;
    #[used(compiler)]
    static LIVE: extern "C" fn() -> usize = js_perry_payload_buffer_live_bytes;
    #[used(compiler)]
    static HOOK_ALLOC: unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void =
        js_perry_payload_buffer_hook_alloc;
    #[used(compiler)]
    static HOOK_FREE: unsafe extern "C" fn(*mut c_void, *mut c_void) =
        js_perry_payload_buffer_hook_free;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_grow_shrink_release_account_exactly_and_zero_fill() {
        let owner = PayloadBufferOwner::new();
        let (ptr, len) = PayloadBuffer::alloc(&owner, 1000).unwrap();
        assert_eq!(len, 1000);
        assert_eq!(owner.bytes(), 1000);
        assert_eq!(ptr.as_ptr() as usize % PAYLOAD_BUFFER_ALIGN, 0);
        unsafe {
            let bytes = std::slice::from_raw_parts_mut(ptr.as_ptr(), len);
            assert!(bytes.iter().all(|b| *b == 0));
            bytes.fill(7);
            let (ptr, len) = PayloadBuffer::grow(&owner, ptr, len, 70_000).unwrap();
            assert_eq!((len, owner.bytes()), (70_000, 70_000));
            let bytes = std::slice::from_raw_parts(ptr.as_ptr(), len);
            assert!(bytes[..1000].iter().all(|b| *b == 7));
            assert!(bytes[1000..].iter().all(|b| *b == 0));
            let (ptr, len) = PayloadBuffer::shrink(&owner, ptr, len, 10);
            assert_eq!((len, owner.bytes()), (10, 10));
            assert!(std::slice::from_raw_parts(ptr.as_ptr(), len)
                .iter()
                .all(|b| *b == 7));
            PayloadBuffer::release(&owner, ptr, len);
        }
        assert_eq!(owner.bytes(), 0);
        let (ptr, len) = PayloadBuffer::alloc(&owner, 0).unwrap();
        assert_eq!((len, owner.bytes()), (0, 0));
        unsafe { PayloadBuffer::release(&owner, ptr, len) };
        assert!(PayloadBuffer::alloc(&owner, usize::MAX).is_none());
        assert_eq!(owner.bytes(), 0);
    }

    #[test]
    fn c_hook_counts_its_prefix_and_frees_without_a_size() {
        let owner = PayloadBufferOwner::new();
        let opaque = &owner as *const PayloadBufferOwner as *mut c_void;
        unsafe {
            let a = js_perry_payload_buffer_hook_alloc(opaque, 100);
            let b = js_perry_payload_buffer_hook_alloc(opaque, 1 << 20);
            assert!(!a.is_null() && !b.is_null());
            assert_eq!(a as usize % PAYLOAD_BUFFER_ALIGN, 0);
            assert_eq!(owner.bytes(), 100 + (1 << 20) + 2 * HOOK_HEADER);
            js_perry_payload_buffer_hook_free(opaque, a);
            assert_eq!(owner.bytes(), (1 << 20) + HOOK_HEADER);
            js_perry_payload_buffer_hook_free(opaque, b);
            js_perry_payload_buffer_hook_free(opaque, std::ptr::null_mut());
            assert_eq!(owner.bytes(), 0);
            // A refused request is a null, as brotli's hook contract expects.
            assert!(js_perry_payload_buffer_hook_alloc(opaque, usize::MAX).is_null());
            assert!(js_perry_payload_buffer_hook_alloc(opaque, MAX_LEN).is_null());
            assert_eq!(owner.bytes(), 0);
        }
    }

    #[test]
    fn grow_failure_leaves_the_buffer_and_the_count_unchanged() {
        let owner = PayloadBufferOwner::new();
        let (ptr, len) = PayloadBuffer::alloc(&owner, 4096).unwrap();
        unsafe {
            std::ptr::write_bytes(ptr.as_ptr(), 0x5a, len);
            assert!(PayloadBuffer::grow(&owner, ptr, len, usize::MAX).is_none());
            assert!(PayloadBuffer::grow(&owner, ptr, len, MAX_LEN + 1).is_none());
            assert_eq!(owner.bytes(), 4096);
            let bytes = std::slice::from_raw_parts(ptr.as_ptr(), len);
            assert!(bytes.iter().all(|b| *b == 0x5a), "old bytes intact");
            PayloadBuffer::release(&owner, ptr, len);
        }
        assert_eq!(owner.bytes(), 0);
    }

    #[test]
    fn small_grow_and_grow_from_empty_keep_bytes_and_count() {
        let owner = PayloadBufferOwner::new();
        let (ptr, len) = PayloadBuffer::alloc(&owner, 1000).unwrap();
        unsafe {
            std::ptr::write_bytes(ptr.as_ptr(), 3, len);
            // Inside the backing's size class: typically the same block.
            let (ptr, len) = PayloadBuffer::grow(&owner, ptr, len, 1008).unwrap();
            assert_eq!((len, owner.bytes()), (1008, 1008));
            let bytes = std::slice::from_raw_parts(ptr.as_ptr(), len);
            assert!(bytes[..1000].iter().all(|b| *b == 3));
            assert!(bytes[1000..].iter().all(|b| *b == 0));
            PayloadBuffer::release(&owner, ptr, len);
            let (empty, zero) = PayloadBuffer::alloc(&owner, 0).unwrap();
            let (ptr, len) = PayloadBuffer::grow(&owner, empty, zero, 64).unwrap();
            assert_eq!((len, owner.bytes()), (64, 64));
            assert!(std::slice::from_raw_parts(ptr.as_ptr(), len)
                .iter()
                .all(|b| *b == 0));
            PayloadBuffer::release(&owner, ptr, len);
        }
        assert_eq!(owner.bytes(), 0);
    }

    #[test]
    fn over_release_clamps_both_counters_alike() {
        if cfg!(debug_assertions) {
            return; // the debug assertion is the intended failure there
        }
        let owner = PayloadBufferOwner::new();
        owner.add(100);
        owner.sub(1000);
        assert_eq!(owner.bytes(), 0);
        // Only 100 came off the process count; wrapping would read as ~2^64
        // (a bound, not an equality, so parallel tests cannot disturb it).
        assert!(
            live_bytes() < usize::MAX / 2,
            "the process count did not wrap"
        );
    }
}
