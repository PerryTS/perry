//! Sole, immovable record Region. ShapeIds never participate in address arithmetic.
//! Page ownership is publication/retirement bookkeeping, never a receiver probe.
use super::{PublishError, ShapeRecord};
use crate::codegen_abi::LAYOUT_ROW_LIMIT;
const RECORD: usize = core::mem::size_of::<ShapeRecord>();

pub(super) struct Region {
    base: *mut ShapeRecord,
    capacity: usize,
    pub(super) page: usize,
    // Number of live records intersecting each OS page. Grows only on publication.
    live: Vec<u32>,
}

#[cfg(unix)]
unsafe fn reserve(bytes: usize) -> *mut u8 {
    let p = libc::mmap(
        core::ptr::null_mut(),
        bytes,
        libc::PROT_NONE,
        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
        -1,
        0,
    );
    if p == libc::MAP_FAILED {
        core::ptr::null_mut()
    } else {
        p.cast()
    }
}
#[cfg(unix)]
unsafe fn commit(at: *mut u8, bytes: usize) -> bool {
    libc::mprotect(at.cast(), bytes, libc::PROT_READ | libc::PROT_WRITE) == 0
}
#[cfg(unix)]
unsafe fn discard(at: *mut u8, bytes: usize) -> bool {
    libc::madvise(at.cast(), bytes, libc::MADV_DONTNEED) == 0
        && libc::mprotect(at.cast(), bytes, libc::PROT_NONE) == 0
}
#[cfg(unix)]
unsafe fn unreserve(at: *mut u8, bytes: usize) {
    assert!(libc::munmap(at.cast(), bytes) == 0, "shape Region release failed");
}

#[cfg(windows)]
unsafe fn reserve(bytes: usize) -> *mut u8 {
    use windows_sys::Win32::System::Memory::*;
    VirtualAlloc(core::ptr::null(), bytes, MEM_RESERVE, PAGE_NOACCESS).cast()
}
#[cfg(windows)]
unsafe fn commit(at: *mut u8, bytes: usize) -> bool {
    use windows_sys::Win32::System::Memory::*;
    !VirtualAlloc(at.cast(), bytes, MEM_COMMIT, PAGE_READWRITE).is_null()
}
#[cfg(windows)]
unsafe fn discard(at: *mut u8, bytes: usize) -> bool {
    use windows_sys::Win32::System::Memory::*;
    VirtualFree(at.cast(), bytes, MEM_DECOMMIT) != 0
}
#[cfg(windows)]
unsafe fn unreserve(at: *mut u8, _bytes: usize) {
    use windows_sys::Win32::System::Memory::*;
    assert!(
        VirtualFree(at.cast(), 0, MEM_RELEASE) != 0,
        "shape Region release failed"
    );
}

// Wasm/limited-address-space targets keep a fixed, nonmoving allocation.
#[cfg(not(any(unix, windows)))]
unsafe fn reserve(bytes: usize) -> *mut u8 {
    std::alloc::alloc_zeroed(std::alloc::Layout::from_size_align(bytes, 4096).unwrap())
}
#[cfg(not(any(unix, windows)))]
unsafe fn commit(_at: *mut u8, _bytes: usize) -> bool {
    true
}
#[cfg(not(any(unix, windows)))]
unsafe fn discard(at: *mut u8, bytes: usize) -> bool {
    core::ptr::write_bytes(at, 0, bytes);
    true
}
#[cfg(not(any(unix, windows)))]
unsafe fn unreserve(at: *mut u8, bytes: usize) {
    std::alloc::dealloc(
        at,
        std::alloc::Layout::from_size_align(bytes, 4096).unwrap(),
    );
}

fn os_page() -> usize {
    #[cfg(unix)]
    {
        let p = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if p > 0 {
            return p as usize;
        }
    }
    #[cfg(windows)]
    {
        let mut info = unsafe { core::mem::zeroed() };
        unsafe { windows_sys::Win32::System::SystemInformation::GetSystemInfo(&mut info) };
        return info.dwPageSize as usize;
    }
    #[allow(unreachable_code)]
    4096
}
impl Region {
    pub(super) fn new() -> Self {
        let mut capacity = if cfg!(target_pointer_width = "64") {
            LAYOUT_ROW_LIMIT as usize
        } else {
            1 << 16
        };
        loop {
            let base = unsafe { reserve(capacity * RECORD) };
            if !base.is_null() {
                return Self {
                    base: base.cast(),
                    capacity,
                    page: os_page(),
                    live: Vec::new(),
                };
            }
            assert!(capacity > (1 << 12), "cannot reserve shape Region");
            capacity /= 2;
        }
    }
    pub(super) fn base(&self) -> *mut ShapeRecord {
        self.base
    }
    pub(super) fn capacity(&self) -> usize {
        self.capacity
    }
    fn pages(&self, row: usize) -> core::ops::RangeInclusive<usize> {
        row * RECORD / self.page..=((row + 1) * RECORD - 1) / self.page
    }
    /// Commit all intersecting pages BEFORE reading or initializing this row.
    pub(super) fn prepare(&mut self, row: usize) -> Result<*mut ShapeRecord, PublishError> {
        if row >= self.capacity {
            return Err(PublishError::Capacity);
        }
        let end = *self.pages(row).end();
        if self.live.len() <= end {
            self.live.resize(end + 1, 0);
        }
        for p in self.pages(row) {
            if self.live[p] == 0
                && !unsafe { commit(self.base.cast::<u8>().add(p * self.page), self.page) }
            {
                return Err(PublishError::Commit);
            }
        }
        Ok(unsafe { self.base.add(row) })
    }
    pub(super) fn published(&mut self, row: usize) {
        for p in self.pages(row) {
            self.live[p] += 1;
        }
    }
    /// Caller removed identity membership and zeroed the row after proving no carrier.
    pub(super) fn retired(&mut self, row: usize) {
        for p in self.pages(row) {
            assert!(self.live[p] != 0);
            self.live[p] -= 1;
            if self.live[p] == 0 {
                assert!(
                    unsafe { discard(self.base.cast::<u8>().add(p * self.page), self.page) },
                    "shape Region page decommit failed"
                );
            }
        }
    }
    pub(super) fn committed_bytes(&self) -> usize {
        self.live.iter().filter(|&&n| n != 0).count() * self.page
    }
}
impl Drop for Region {
    fn drop(&mut self) {
        unsafe { unreserve(self.base.cast(), self.capacity * RECORD) };
    }
}
