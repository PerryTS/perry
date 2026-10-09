//! Apple backing: tagged anonymous mappings aligned to their block size.
//! Idle pages are released as reusable and must be reactivated before reuse.

/// One block per region. 2 MiB blocks raised arena capacity and peak
/// footprint on macOS with no huge-page benefit to pay for it.
pub(super) const ALIGN: usize = 1024 * 1024;

// VM_MEMORY_APPLICATION_SPECIFIC_2; mimalloc uses SPECIFIC_1 (240).
// MAP_ANON interprets the fd's high byte as VM_MAKE_TAG(tag).
pub(super) const VM_TAG: u32 = 241;

/// The owned extent of a `len`-byte region: whole alignment units.
pub(super) fn extent(len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    len.checked_add(ALIGN - 1).map(|n| n & !(ALIGN - 1))
}

/// Over-map and trim, so the mapping owns exactly `extent` aligned bytes.
pub(super) unsafe fn map(extent: usize) -> *mut u8 {
    let Some(reserved) = extent.checked_add(ALIGN) else {
        return std::ptr::null_mut();
    };
    let raw = libc::mmap(
        std::ptr::null_mut(),
        reserved,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
        (VM_TAG << 24) as libc::c_int,
        0,
    );
    if raw == libc::MAP_FAILED {
        return std::ptr::null_mut();
    }
    let address = raw as usize;
    let aligned = (address + ALIGN - 1) & !(ALIGN - 1);
    let prefix = aligned - address;
    let suffix = reserved - prefix - extent;
    if prefix != 0 {
        assert_eq!(libc::munmap(raw, prefix), 0);
    }
    if suffix != 0 {
        assert_eq!(libc::munmap((aligned + extent) as *mut _, suffix), 0);
    }
    aligned as *mut u8
}

pub(super) unsafe fn unmap(data: *mut u8, extent: usize) {
    assert_eq!(libc::munmap(data.cast(), extent), 0, "region unmap failed");
}

/// Take the pages out of the footprint; contents become undefined.
pub(super) unsafe fn release(data: *mut u8, len: usize) -> bool {
    libc::madvise(data.cast(), len, libc::MADV_FREE_REUSABLE) == 0
}

/// Must precede any write to pages released with [`release`].
pub(super) unsafe fn reuse(data: *mut u8, len: usize) {
    assert_eq!(
        libc::madvise(data.cast(), len, libc::MADV_FREE_REUSE),
        0,
        "region reuse failed"
    );
}
