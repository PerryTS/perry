//! Windows backing: one private reservation per block, committed whole.
//! An empty block's pages are decommitted, which lowers the commit charge
//! (process private bytes) and not only the working set. Decommitted pages
//! fault on read as well as write, so they must be committed again before
//! any access; see [`super::reuse`].

use windows_sys::Win32::System::Memory::{
    VirtualAlloc, VirtualFree, MEM_COMMIT, MEM_DECOMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};

/// Commit granularity. Reservations themselves start on the 64 KiB
/// allocation granularity.
const PAGE: usize = 4096;

/// The owned extent of a `len`-byte region: whole pages.
pub(super) fn extent(len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    len.checked_add(PAGE - 1).map(|n| n & !(PAGE - 1))
}

/// A fresh reservation of `extent` bytes, committed (demand-zero), or null
/// when the system refuses the reservation or the commit charge.
pub(super) unsafe fn map(extent: usize) -> *mut u8 {
    VirtualAlloc(
        std::ptr::null(),
        extent,
        MEM_RESERVE | MEM_COMMIT,
        PAGE_READWRITE,
    )
    .cast()
}

/// Release the whole reservation `data` starts, committed or not.
pub(super) unsafe fn unmap(data: *mut u8) {
    assert_ne!(
        VirtualFree(data.cast(), 0, MEM_RELEASE),
        0,
        "region unmap failed: {}",
        std::io::Error::last_os_error()
    );
}

/// Decommit the pages; the reservation stays. Contents are lost.
pub(super) unsafe fn release(data: *mut u8, extent: usize) -> bool {
    VirtualFree(data.cast(), extent, MEM_DECOMMIT) != 0
}

/// Commit the pages again (demand-zero). Committing pages that are already
/// committed leaves them and their contents unchanged.
pub(super) unsafe fn reuse(data: *mut u8, extent: usize) {
    let committed = VirtualAlloc(data.cast(), extent, MEM_COMMIT, PAGE_READWRITE);
    assert!(
        !committed.is_null(),
        "arena region reuse failed (commit of {extent} bytes refused): {}",
        std::io::Error::last_os_error()
    );
}
