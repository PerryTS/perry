//! Measurement prototype: OS backing beneath the existing per-agent block pool.
//! No pointer registry, cache, collector callback or object-layout change.
//! Production in-band descriptors and byte-store ownership are in REGION-DESIGN.

use super::HeapGeneration;
use std::collections::BTreeMap;
use std::sync::RwLock;

/// Every live region's extent (start -> end), process-wide. `map` and `unmap`
/// are the only OS backing of arena blocks, nursery, old and large alike, so
/// this answers "is this word inside Perry's GC memory" for any thread
/// without per-thread metadata. Pooled blocks stay mapped and stay listed.
static REGIONS: RwLock<BTreeMap<usize, usize>> = RwLock::new(BTreeMap::new());

fn extent_len(len: usize) -> usize {
    #[cfg(target_os = "linux")]
    {
        mapped_len(len).expect("invalid region extent")
    }
    #[cfg(not(target_os = "linux"))]
    {
        len
    }
}

/// Does one live region hold all of `[addr, addr + len)`?
pub(crate) fn contains(addr: usize, len: usize) -> bool {
    let Some(end) = addr.checked_add(len) else {
        return false;
    };
    let regions = REGIONS.read().unwrap_or_else(|e| e.into_inner());
    regions
        .range(..=addr)
        .next_back()
        .is_some_and(|(_, &region_end)| end <= region_end)
}

pub(super) const ALIGN: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    NurseryBlock,
    OldBlock,
    LargeObject,
}

pub(super) fn kind_for(generation: HeapGeneration, len: usize) -> Kind {
    match generation {
        HeapGeneration::Old | HeapGeneration::Longlived if len > super::BLOCK_SIZE => {
            Kind::LargeObject
        }
        HeapGeneration::Old | HeapGeneration::Longlived => Kind::OldBlock,
        _ => Kind::NurseryBlock,
    }
}

fn mapped_len(len: usize) -> Option<usize> {
    if len == 0 || len % 4096 != 0 {
        return None;
    }
    len.checked_add(ALIGN - 1).map(|n| n & !(ALIGN - 1))
}

/// Charge the pool for the owned OS extent, including rounded tails. Other
/// targets retain the allocator backend and its exact requested length.
pub(super) fn backing_len(len: usize) -> usize {
    #[cfg(target_os = "linux")]
    {
        mapped_len(len).expect("invalid region extent")
    }
    #[cfg(not(target_os = "linux"))]
    {
        len
    }
}

/// Complete payload units are dense backing in every generation, including
/// refilled nursery blocks. Sub-unit extents and partial tails stay on base
/// pages. Advice precedes writes; neither prefault nor collapse.
pub(super) unsafe fn advise(data: *mut u8, len: usize, kind: Kind) {
    #[cfg(target_os = "linux")]
    {
        let mapped = mapped_len(len).expect("invalid region extent");
        let _ = kind;
        let dense = len / ALIGN * ALIGN;
        if dense != 0 {
            assert_eq!(libc::madvise(data.cast(), dense, libc::MADV_HUGEPAGE), 0);
        }
        if dense < mapped {
            assert_eq!(
                libc::madvise(
                    data.add(dense).cast(),
                    mapped - dense,
                    libc::MADV_NOHUGEPAGE
                ),
                0
            );
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (data, len, kind);
}

/// Fresh private anonymous backing, or null on refusal. This entry never
/// collects. The existing caller owns emergency reclaim outside arena borrows.
pub(super) unsafe fn map(kind: Kind, len: usize) -> *mut u8 {
    #[cfg(target_os = "linux")]
    {
        let Some(mapped) = mapped_len(len) else {
            return std::ptr::null_mut();
        };
        let Some(reserved) = mapped.checked_add(ALIGN) else {
            return std::ptr::null_mut();
        };
        let raw = libc::mmap(
            std::ptr::null_mut(),
            reserved,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        if raw == libc::MAP_FAILED {
            return std::ptr::null_mut();
        }
        let address = raw as usize;
        let aligned = (address + ALIGN - 1) & !(ALIGN - 1);
        let prefix = aligned - address;
        let suffix = reserved - prefix - mapped;
        if prefix != 0 {
            assert_eq!(libc::munmap(raw, prefix), 0);
        }
        if suffix != 0 {
            assert_eq!(libc::munmap((aligned + mapped) as *mut _, suffix), 0);
        }
        let data = aligned as *mut u8;
        advise(data, len, kind);
        register(data, len);
        data
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = kind;
        let data = std::alloc::alloc(std::alloc::Layout::from_size_align(len, 16).unwrap());
        if !data.is_null() {
            register(data, len);
        }
        data
    }
}

fn register(data: *mut u8, len: usize) {
    let start = data as usize;
    REGIONS
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .insert(start, start + extent_len(len));
}

/// Physical destruction, used by pool overflow, pool drain and TLS teardown.
/// Callers remove page metadata and external owners before destruction.
pub(super) unsafe fn unmap(data: *mut u8, len: usize) {
    REGIONS
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&(data as usize));
    #[cfg(target_os = "linux")]
    assert_eq!(
        libc::munmap(data.cast(), mapped_len(len).expect("invalid extent")),
        0,
        "region unmap failed"
    );
    #[cfg(not(target_os = "linux"))]
    std::alloc::dealloc(data, std::alloc::Layout::from_size_align(len, 16).unwrap());
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn flags_for(data: *mut u8) -> String {
        let smaps = std::fs::read_to_string("/proc/self/smaps").unwrap();
        let mut found = false;
        for line in smaps.lines() {
            if let Some(range) = line.split_whitespace().next().filter(|v| v.contains('-')) {
                let (start, end) = range.split_once('-').unwrap();
                if let (Ok(start), Ok(end)) = (
                    usize::from_str_radix(start, 16),
                    usize::from_str_radix(end, 16),
                ) {
                    found = (start..end).contains(&(data as usize));
                }
            }
            if found && line.starts_with("VmFlags:") {
                return line.to_owned();
            }
        }
        panic!("region missing from smaps");
    }

    #[test]
    fn aligned_os_backing_has_whole_unit_thp_and_releases_pages() {
        unsafe {
            for kind in [Kind::NurseryBlock, Kind::OldBlock, Kind::LargeObject] {
                let data = map(kind, 2 * ALIGN);
                assert!(!data.is_null());
                assert_eq!(data as usize & (ALIGN - 1), 0);
                let flags = flags_for(data);
                let expected = "hg";
                assert!(flags.split_whitespace().any(|v| v == expected), "{flags}");
                std::ptr::write_bytes(data, 0xa5, 2 * ALIGN);
                let mut resident = vec![0u8; 2 * ALIGN / 4096];
                assert_eq!(
                    libc::mincore(data.cast(), 2 * ALIGN, resident.as_mut_ptr()),
                    0
                );
                assert!(resident.iter().all(|v| v & 1 == 1));
                assert_eq!(
                    libc::madvise(data.cast(), 2 * ALIGN, libc::MADV_DONTNEED),
                    0
                );
                assert_eq!(
                    libc::mincore(data.cast(), 2 * ALIGN, resident.as_mut_ptr()),
                    0
                );
                assert!(resident.iter().all(|v| v & 1 == 0));
                *data = 42;
                assert_eq!(*data, 42);
                unmap(data, 2 * ALIGN);
                assert_eq!(libc::mincore(data.cast(), 4096, resident.as_mut_ptr()), -1);
            }
        }
    }

    #[test]
    fn reused_whole_region_keeps_dense_advice_in_nursery() {
        unsafe {
            let data = map(Kind::OldBlock, ALIGN);
            advise(data, ALIGN, Kind::NurseryBlock);
            assert!(flags_for(data).split_whitespace().any(|v| v == "hg"));
            unmap(data, ALIGN);
        }
    }

    #[test]
    fn invalid_extents_are_refused_without_collection() {
        unsafe {
            assert!(map(Kind::NurseryBlock, 0).is_null());
            assert!(map(Kind::NurseryBlock, ALIGN - 1).is_null());
            assert!(map(Kind::NurseryBlock, usize::MAX & !(ALIGN - 1)).is_null());
        }
    }

    #[test]
    fn dense_nursery_advice_survives_speculative_and_committed_promotion() {
        crate::arena::tests::run_with_fresh_arenas(|| {
            let user = crate::arena::arena_alloc_gc(24, 8, crate::gc::GC_TYPE_STRING);
            let base = (user as usize & !(ALIGN - 1)) as *mut u8;
            assert!(flags_for(base).split_whitespace().any(|v| v == "hg"));
            let speculative = crate::arena::retag_young_for_in_place_promotion(true);
            assert!(
                !speculative.is_empty(),
                "LIVE SUBJECT: retagged nursery block"
            );
            assert!(flags_for(base).split_whitespace().any(|v| v == "hg"));
            crate::arena::undo_in_place_promotion_retag(&speculative);
            assert!(flags_for(base).split_whitespace().any(|v| v == "hg"));
            let promotion = crate::arena::retag_young_for_in_place_promotion(false);
            let stats = crate::arena::finish_in_place_promotion(
                promotion,
                crate::arena::PromotionLiveness::AssumeAllLive,
            );
            assert!(stats.objects > 0);
            assert!(crate::arena::pointer_in_old_gen(user as usize));
            assert!(flags_for(base).split_whitespace().any(|v| v == "hg"));
        });
    }

    #[test]
    fn mature_complete_units_keep_partial_tails_on_base_pages() {
        unsafe {
            let len = ALIGN + 4096;
            let data = map(Kind::LargeObject, len);
            assert!(!data.is_null());
            assert!(flags_for(data).split_whitespace().any(|v| v == "hg"));
            assert!(flags_for(data.add(ALIGN))
                .split_whitespace()
                .any(|v| v == "nh"));
            unmap(data, len);
        }
    }

    #[test]
    fn nursery_whole_units_are_huge_but_small_extents_and_tails_are_not() {
        unsafe {
            for len in [4096, ALIGN - 4096, ALIGN, ALIGN + 4096, 2 * ALIGN + 4096] {
                let data = map(Kind::NurseryBlock, len);
                assert!(!data.is_null(), "LIVE SUBJECT: mapped nursery backing");
                let dense = len / ALIGN * ALIGN;
                for offset in (0..backing_len(len)).step_by(4096) {
                    // The kernel's VMA advice is the subject, independently of
                    // whether THP allocation succeeds on this machine.
                    let flags = flags_for(data.add(offset));
                    let expected = if offset < dense { "hg" } else { "nh" };
                    assert!(
                        flags.split_whitespace().any(|v| v == expected),
                        "len={len} offset={offset} expected={expected}: {flags}"
                    );
                }
                unmap(data, len);
            }
        }
    }
}
