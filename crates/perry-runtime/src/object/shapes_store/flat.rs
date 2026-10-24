//! Sole, immovable record Region. ShapeIds never participate in address arithmetic.
//! Page ownership is publication/retirement bookkeeping, never a receiver probe.
use super::{PublishError, ShapeRecord};
use crate::codegen_abi::LAYOUT_ROW_LIMIT;
const RECORD: usize = core::mem::size_of::<ShapeRecord>();

pub(super) struct Region {
    base: *mut ShapeRecord,
    capacity: usize,
    pub(super) page: usize,
    // Number of live records intersecting each OS page, in sparse leaves: the
    // bookkeeping is proportional to the pages this agent holds, never to the
    // highest page index (other agents' chunks lie between ours).
    /// Two levels: a small top vector indexed by leaf number (one slot per
    /// 512 pages) of lazily boxed leaves, so a lookup is two loads and the
    /// size follows the agent's own pages, not the process's chunk history.
    live: Vec<Option<Box<Leaf>>>,
}

/// Pages per leaf of the live-count map.
const LEAF_PAGES: usize = 512;
struct Leaf {
    /// Pages of this leaf with a nonzero count; the leaf is dropped at zero.
    used: u32,
    counts: [u32; LEAF_PAGES],
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
/// A Region smaller than the fixed dynamic base plus a few chunks cannot hold
/// the process-wide chunk space.
pub(super) const MIN_CAPACITY: usize = 2 * super::DYNAMIC_BASE_ROWS as usize;
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
            assert!(capacity > MIN_CAPACITY, "cannot reserve shape Region");
            capacity /= 2;
        }
    }
    pub(super) fn base(&self) -> *mut ShapeRecord {
        self.base
    }
    pub(super) fn capacity(&self) -> usize {
        self.capacity
    }
    /// Leaves held: the bookkeeping size, for tests.
    #[cfg(test)]
    pub(super) fn leaf_count(&self) -> usize {
        self.live.iter().flatten().count()
    }
    fn pages(&self, row: usize) -> core::ops::RangeInclusive<usize> {
        row * RECORD / self.page..=((row + 1) * RECORD - 1) / self.page
    }
    /// Commit all intersecting pages BEFORE reading or initializing this row.
    pub(super) fn prepare(&mut self, row: usize) -> Result<*mut ShapeRecord, PublishError> {
        if row >= self.capacity {
            return Err(PublishError::Capacity);
        }
        for p in self.pages(row) {
            if self.count(p) == 0
                && !unsafe { commit(self.base.cast::<u8>().add(p * self.page), self.page) }
            {
                return Err(PublishError::Commit);
            }
        }
        Ok(unsafe { self.base.add(row) })
    }
    fn count(&self, page: usize) -> u32 {
        match self.live.get(page / LEAF_PAGES) {
            Some(Some(leaf)) => leaf.counts[page % LEAF_PAGES],
            _ => 0,
        }
    }
    pub(super) fn published(&mut self, row: usize) {
        for p in self.pages(row) {
            let key = p / LEAF_PAGES;
            if self.live.len() <= key {
                self.live.resize_with(key + 1, || None);
            }
            let leaf = self.live[key].get_or_insert_with(|| {
                Box::new(Leaf {
                    used: 0,
                    counts: [0; LEAF_PAGES],
                })
            });
            let n = &mut leaf.counts[p % LEAF_PAGES];
            leaf.used += u32::from(*n == 0);
            *n += 1;
        }
    }
    /// Caller removed identity membership and zeroed the row after proving no carrier.
    pub(super) fn retired(&mut self, row: usize) {
        for p in self.pages(row) {
            let key = p / LEAF_PAGES;
            let leaf = self
                .live
                .get_mut(key)
                .and_then(|l| l.as_deref_mut())
                .expect("retired row on a live page");
            let n = &mut leaf.counts[p % LEAF_PAGES];
            assert!(*n != 0);
            *n -= 1;
            if *n == 0 {
                leaf.used -= 1;
                if leaf.used == 0 {
                    self.live[key] = None;
                }
                assert!(
                    unsafe { discard(self.base.cast::<u8>().add(p * self.page), self.page) },
                    "shape Region page decommit failed"
                );
            }
        }
    }
    /// True when every OS page of  is committed in THIS Region.
    pub(super) fn row_committed(&self, row: usize) -> bool {
        self.pages(row).all(|p| self.count(p) != 0)
    }
    pub(super) fn committed_bytes(&self) -> usize {
        self.live.iter().flatten().map(|leaf| leaf.used as usize).sum::<usize>() * self.page
    }
}
impl Drop for Region {
    fn drop(&mut self) {
        unsafe { unreserve(self.base.cast(), self.capacity * RECORD) };
    }
}
