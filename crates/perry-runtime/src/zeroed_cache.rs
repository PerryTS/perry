//! Zero-allocated per-thread cache tables (#11507).
//!
//! The runtime's direct-mapped / open-addressed IC and plan caches used to be
//! built with `vec![EMPTY; N].into_boxed_slice()`. For a struct element that
//! is an element-by-element fill: every page of every table is written at
//! thread start, whether or not the program ever probes it (~1.9 MiB per
//! thread, paid by hello-world and by every `perry/thread` worker).
//!
//! `new_zeroed_cache` asks the allocator for zeroed memory instead. Fresh
//! allocator memory is already zero, so the allocator hands it back without a
//! fill and the OS maps each page on the first write that lands in it. That
//! only works if a zeroed entry IS the table's empty slot, which is the
//! contract [`ZeroEmpty`] states.

/// A cache entry whose all-zero bit pattern is both a valid value and the
/// owning table's empty (miss) encoding.
///
/// # Safety
/// Every field must be an integer, `bool`, or an array of those, so all-zero
/// bytes are a valid value; and the table must treat an all-zero entry
/// exactly as it treats a never-written slot. Each cache's module has a test
/// that a freshly created table reads as empty in every slot.
pub(crate) unsafe trait ZeroEmpty: Copy {}

/// A `len`-entry table whose every slot is empty, without writing it.
#[inline]
pub(crate) fn new_zeroed_cache<T: ZeroEmpty>(len: usize) -> Box<[T]> {
    // SAFETY: `T: ZeroEmpty` promises all-zero bytes are a valid `T`.
    unsafe { Box::<[T]>::new_zeroed_slice(len).assume_init() }
}
