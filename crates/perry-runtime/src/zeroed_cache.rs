//! Zero-allocated per-thread cache tables (#11507).
//!
//! The runtime's direct-mapped / open-addressed IC and plan caches used to be
//! built with `vec![EMPTY; N].into_boxed_slice()`. For a struct element that
//! is an element-by-element fill: every page of every table is written at
//! thread start, whether or not the program ever probes it (~1.9 MiB per
//! thread, paid by hello-world and by every `perry/thread` worker).
//!
//! `new_zeroed_cache` asks the allocator for zeroed memory instead, which
//! skips the element-by-element fill but NOT the writes: mimalloc zero-fills a
//! block it cannot prove fresh (a `memset` of the whole table at thread start,
//! measured on Linux and macOS), so every page is still mapped and dirty.
//! [`ZeroPages`] takes a fixed table straight from the OS instead, on its
//! first use: a thread that never probes it maps nothing, and a page it never
//! writes stays unmapped. Both only work if a zeroed entry IS the table's
//! empty slot, which is the contract [`ZeroEmpty`] states.

/// A cache entry whose all-zero bit pattern is both a valid value and the
/// owning table's empty (miss) encoding.
///
/// # Safety
/// Every field must be an integer, `bool`, or an array of those, so all-zero
/// bytes are a valid value; and the table must treat an all-zero entry
/// exactly as it treats a never-written slot. Each cache's module has a test
/// that a freshly created table reads as empty in every slot.
pub(crate) unsafe trait ZeroEmpty: Copy {}

/// A fixed `N`-entry table on fresh zero pages, mapped by its first use
/// ([`Self::get`]). A scan or prune of a table no one used has nothing to
/// visit ([`Self::mapped`]).
pub(crate) struct ZeroPages<T: ZeroEmpty, const N: usize> {
    table: std::cell::Cell<*mut [T; N]>,
}

impl<T: ZeroEmpty, const N: usize> ZeroPages<T, N> {
    const BYTES: usize = std::mem::size_of::<[T; N]>();

    pub(crate) const fn new() -> Self {
        ZeroPages {
            table: std::cell::Cell::new(std::ptr::null_mut()),
        }
    }

    /// The table, mapped now if this is its first use.
    #[inline(always)]
    pub(crate) fn get(&self) -> *mut [T; N] {
        let table = self.table.get();
        if table.is_null() {
            self.map()
        } else {
            table
        }
    }

    /// The table if it was ever used.
    #[inline]
    pub(crate) fn mapped(&self) -> Option<*mut [T; N]> {
        let table = self.table.get();
        (!table.is_null()).then_some(table)
    }

    #[cold]
    #[inline(never)]
    fn map(&self) -> *mut [T; N] {
        const { assert!(std::mem::align_of::<T>() <= 4096) };
        // SAFETY: `T: ZeroEmpty`: the zero pages are N valid, empty entries.
        let table = unsafe { map_zero_pages(Self::BYTES) }.cast::<[T; N]>();
        if table.is_null() {
            std::alloc::handle_alloc_error(page_layout(Self::BYTES));
        }
        self.table.set(table);
        table
    }
}

impl<T: ZeroEmpty, const N: usize> Drop for ZeroPages<T, N> {
    fn drop(&mut self) {
        if let Some(table) = self.mapped() {
            // SAFETY: mapped by `map` with this size; no reference outlives
            // the owning state.
            unsafe { unmap_zero_pages(table.cast(), Self::BYTES) };
        }
    }
}

fn page_layout(bytes: usize) -> std::alloc::Layout {
    std::alloc::Layout::from_size_align(bytes, 4096).expect("zero-page table layout")
}

#[cfg(unix)]
unsafe fn map_zero_pages(bytes: usize) -> *mut u8 {
    let p = libc::mmap(
        std::ptr::null_mut(),
        bytes,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
        -1,
        0,
    );
    if p == libc::MAP_FAILED {
        std::ptr::null_mut()
    } else {
        p.cast()
    }
}
#[cfg(unix)]
unsafe fn unmap_zero_pages(at: *mut u8, bytes: usize) {
    libc::munmap(at.cast(), bytes);
}
#[cfg(windows)]
unsafe fn map_zero_pages(bytes: usize) -> *mut u8 {
    use windows_sys::Win32::System::Memory::*;
    VirtualAlloc(
        std::ptr::null(),
        bytes,
        MEM_RESERVE | MEM_COMMIT,
        PAGE_READWRITE,
    )
    .cast()
}
#[cfg(windows)]
unsafe fn unmap_zero_pages(at: *mut u8, _bytes: usize) {
    use windows_sys::Win32::System::Memory::*;
    VirtualFree(at.cast(), 0, MEM_RELEASE);
}
#[cfg(not(any(unix, windows)))]
unsafe fn map_zero_pages(bytes: usize) -> *mut u8 {
    std::alloc::alloc_zeroed(page_layout(bytes))
}
#[cfg(not(any(unix, windows)))]
unsafe fn unmap_zero_pages(at: *mut u8, bytes: usize) {
    std::alloc::dealloc(at, page_layout(bytes));
}

/// A `len`-entry table whose every slot is empty, without writing it.
#[inline]
pub(crate) fn new_zeroed_cache<T: ZeroEmpty>(len: usize) -> Box<[T]> {
    // SAFETY: `T: ZeroEmpty` promises all-zero bytes are a valid `T`.
    unsafe { Box::<[T]>::new_zeroed_slice(len).assume_init() }
}
