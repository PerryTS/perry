//! The ordinary band's records at FIXED addresses: record `rel` (a ShapeId
//! minus `SHAPE_ID_BASE`) lives at `base + rel` in one per-agent virtual
//! reservation (64-bit targets).
//!
//! This is what makes "layout word to layout record" ONE address computation
//! (OBJECT_MODEL_2026-10-13.md §3.7 case 3): the unknown-receiver probe
//! (`object/layout_probe.rs`) and every by-id reader load the record base
//! once and index it with the id, instead of walking a page and a chunk
//! directory. The paged directory stays as the slab's bookkeeping (which runs
//! are allocated, release, enumeration); on these targets its chunks are
//! windows of the reservation rather than boxes, so both views name the same
//! cells.
//!
//! The reservation is address space only. A directory page's id range is
//! committed (made readable) when the directory grows to it, so every id below
//! [`Region::committed_ids`] reads a record: an untouched one is all zero,
//! which is `ShapeRecord::EMPTY` (not present). Released chunks are zero
//! (their records were removed) and give their whole OS pages back.
use super::ShapeRecord;

/// Ordinary-band ids: `[SHAPE_ID_BASE, SITE_SHAPE_ID_END)`.
pub(super) const ORDINARY_IDS: usize = (super::SITE_SHAPE_ID_END - super::SHAPE_ID_BASE) as usize;
/// Agent creation seeds the native birth layouts in the first dynamic plain
/// granule. The reservation must include that entire directory page, not just
/// the preceding static/kind granules.
const PAGE_IDS: usize = super::CHUNK_LEN * super::PAGE_LEN;
const FIRST_NATIVE_END: usize =
    (crate::codegen_abi::WEAKSET_BIRTH_LAYOUT - super::SHAPE_ID_BASE) as usize + 1;
const MIN_IDS: usize = FIRST_NATIVE_END.div_ceil(PAGE_IDS) * PAGE_IDS;

pub(super) struct Region {
    base: *mut ShapeRecord,
    reserved_ids: usize,
    committed_ids: usize,
}

const RECORD: usize = std::mem::size_of::<ShapeRecord>();

#[cfg(unix)]
unsafe fn reserve(bytes: usize) -> *mut u8 {
    let p = libc::mmap(
        std::ptr::null_mut(),
        bytes,
        libc::PROT_NONE,
        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
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
unsafe fn commit(at: *mut u8, bytes: usize) -> bool {
    libc::mprotect(at.cast(), bytes, libc::PROT_READ | libc::PROT_WRITE) == 0
}

#[cfg(unix)]
unsafe fn discard(at: *mut u8, bytes: usize) {
    // The pages hold only zero records already; giving them back keeps a
    // long-running agent's RSS at its live records, as releasing a boxed
    // chunk did.
    libc::madvise(at.cast(), bytes, libc::MADV_DONTNEED);
}

#[cfg(unix)]
unsafe fn unreserve(at: *mut u8, bytes: usize) {
    libc::munmap(at.cast(), bytes);
}

#[cfg(windows)]
unsafe fn reserve(bytes: usize) -> *mut u8 {
    use windows_sys::Win32::System::Memory::{VirtualAlloc, MEM_RESERVE, PAGE_NOACCESS};
    VirtualAlloc(std::ptr::null(), bytes, MEM_RESERVE, PAGE_NOACCESS).cast()
}

#[cfg(windows)]
unsafe fn commit(at: *mut u8, bytes: usize) -> bool {
    use windows_sys::Win32::System::Memory::{VirtualAlloc, MEM_COMMIT, PAGE_READWRITE};
    !VirtualAlloc(at.cast(), bytes, MEM_COMMIT, PAGE_READWRITE).is_null()
}

#[cfg(windows)]
unsafe fn discard(_at: *mut u8, _bytes: usize) {}

#[cfg(windows)]
unsafe fn unreserve(at: *mut u8, _bytes: usize) {
    use windows_sys::Win32::System::Memory::{VirtualFree, MEM_RELEASE};
    VirtualFree(at.cast(), 0, MEM_RELEASE);
}

fn os_page() -> usize {
    #[cfg(unix)]
    {
        // SAFETY: sysconf has no preconditions.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page > 0 {
            return page as usize;
        }
    }
    4096
}

fn reserve_with(mut try_reserve: impl FnMut(usize) -> *mut u8) -> (*mut ShapeRecord, usize) {
    let mut ids = ORDINARY_IDS;
    loop {
        let base = try_reserve(ids * RECORD);
        if !base.is_null() {
            return (base.cast(), ids);
        }
        assert!(
            ids > MIN_IDS,
            "Perry internal error: cannot reserve address space for the shape records"
        );
        // ORDINARY_IDS is three powers of two, so repeated halving skips
        // past the minimum. Try the exact usable floor before giving up.
        ids = (ids / 2).max(MIN_IDS);
    }
}

impl Region {
    /// Reserve the whole ordinary band or successively smaller shares, ending
    /// at the smallest reservation that contains the native birth page.
    pub(super) fn new() -> Region {
        let (base, reserved_ids) = reserve_with(|bytes| {
            // SAFETY: a fresh anonymous reservation.
            unsafe { reserve(bytes) }
        });
        Region {
            base,
            reserved_ids,
            committed_ids: 0,
        }
    }

    #[inline(always)]
    pub(super) fn base(&self) -> *mut ShapeRecord {
        self.base
    }

    /// Ids below this read a record.
    #[inline(always)]
    pub(super) fn committed_ids(&self) -> usize {
        self.committed_ids
    }

    /// Make ids `[0, ids)` readable. Returns the new committed bound.
    pub(super) fn ensure(&mut self, ids: usize) -> usize {
        if ids <= self.committed_ids {
            return self.committed_ids;
        }
        assert!(
            ids <= self.reserved_ids,
            "Perry internal error: ShapeId {ids:#x} past the shape-record reservation"
        );
        let page = os_page();
        let from = (self.committed_ids * RECORD) & !(page - 1);
        let to = ((ids * RECORD) + page - 1) & !(page - 1);
        let to = to.min(self.reserved_ids * RECORD);
        // SAFETY: inside this reservation.
        let ok = unsafe { commit((self.base as *mut u8).add(from), to - from) };
        assert!(ok, "Perry internal error: cannot commit shape-record pages");
        self.committed_ids = (to / RECORD).min(self.reserved_ids);
        self.committed_ids
    }

    /// The cells of slab chunk `index` (`chunk_len` records from
    /// `index * chunk_len`). The caller ensured they are committed.
    #[inline]
    pub(super) fn chunk_cells(&self, index: usize, chunk_len: usize) -> *mut ShapeRecord {
        debug_assert!((index + 1) * chunk_len <= self.committed_ids);
        // SAFETY: inside the committed prefix.
        unsafe { self.base.add(index * chunk_len) }
    }

    /// Give back the OS pages wholly inside ids `[from, to)`, whose records
    /// are all absent (zero).
    pub(super) fn discard_ids(&self, from: usize, to: usize) {
        let page = os_page();
        let start = ((from * RECORD) + page - 1) & !(page - 1);
        let end = (to.min(self.committed_ids) * RECORD) & !(page - 1);
        if end > start {
            // SAFETY: inside the committed prefix; every record there is EMPTY.
            unsafe { discard((self.base as *mut u8).add(start), end - start) };
        }
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        // SAFETY: the reservation made in `new`; nothing reads it after the
        // slab (its only owner) is dropped.
        unsafe { unreserve(self.base.cast(), self.reserved_ids * RECORD) };
    }
}

#[cfg(test)]
mod reservation_floor_tests {
    use super::*;

    #[test]
    fn limited_address_space_still_contains_the_native_birth_page() {
        let native_index = (crate::codegen_abi::WEAKSET_BIRTH_LAYOUT
            - crate::object::shapes::SHAPE_ID_BASE) as usize;
        let native_page_end = (native_index / PAGE_IDS + 1) * PAGE_IDS;
        let available = native_page_end * RECORD;
        let mut attempts = Vec::new();
        let (_, ids) = reserve_with(|bytes| {
            attempts.push(bytes);
            if bytes <= available {
                // No Region is constructed and this token is never dereferenced.
                std::ptr::NonNull::<ShapeRecord>::dangling().as_ptr().cast()
            } else {
                std::ptr::null_mut()
            }
        });
        assert_eq!(ids, native_page_end);
        assert_eq!(attempts.last(), Some(&available));
        assert!(attempts[..attempts.len() - 1]
            .iter()
            .all(|bytes| *bytes > available));
    }

    #[test]
    #[should_panic(expected = "cannot reserve address space for the shape records")]
    fn an_unusable_smaller_reservation_is_never_accepted() {
        let native_index = (crate::codegen_abi::WEAKSET_BIRTH_LAYOUT
            - crate::object::shapes::SHAPE_ID_BASE) as usize;
        let needed = (native_index / PAGE_IDS + 1) * PAGE_IDS * RECORD;
        reserve_with(|bytes| {
            if bytes < needed {
                std::ptr::NonNull::<ShapeRecord>::dangling().as_ptr().cast()
            } else {
                std::ptr::null_mut()
            }
        });
    }
}
