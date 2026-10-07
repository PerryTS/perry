//! Measurement prototype: OS backing beneath the existing per-agent block pool.
//! No pointer registry, cache, collector callback or object-layout change.
//! Production in-band descriptors and byte-store ownership are in REGION-DESIGN.

use super::HeapGeneration;

pub(super) const ALIGN: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) enum Kind {
    NurseryBlock,
    OldBlock,
    LargeObject,
}

pub(super) fn kind_for(generation: HeapGeneration, len: usize) -> Kind {
    match generation {
        HeapGeneration::Old | HeapGeneration::Longlived if len > ALIGN => Kind::LargeObject,
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

/// Mature complete payload units are THP eligible. Nursery and sparse tail
/// units are not. Advice precedes writes; neither prefault nor collapse.
pub(super) unsafe fn advise(data: *mut u8, len: usize, kind: Kind) {
    #[cfg(target_os = "linux")]
    {
        let mapped = mapped_len(len).expect("invalid region extent");
        assert_eq!(libc::madvise(data.cast(), mapped, libc::MADV_NOHUGEPAGE), 0);
        if !matches!(kind, Kind::NurseryBlock) {
            let dense = len / ALIGN * ALIGN;
            if dense != 0 {
                assert_eq!(libc::madvise(data.cast(), dense, libc::MADV_HUGEPAGE), 0);
            }
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
        data
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = kind;
        std::alloc::alloc(std::alloc::Layout::from_size_align(len, 16).unwrap())
    }
}

/// Physical destruction, used by pool overflow, pool drain and TLS teardown.
/// Callers remove page metadata and external owners before destruction.
pub(super) unsafe fn unmap(data: *mut u8, len: usize) {
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
    fn aligned_os_backing_has_kind_specific_thp_and_releases_pages() {
        unsafe {
            for kind in [Kind::NurseryBlock, Kind::OldBlock, Kind::LargeObject] {
                let data = map(kind, 2 * ALIGN);
                assert!(!data.is_null());
                assert_eq!(data as usize & (ALIGN - 1), 0);
                let flags = flags_for(data);
                let expected = if matches!(kind, Kind::NurseryBlock) {
                    "nh"
                } else {
                    "hg"
                };
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
    fn reused_region_changes_advice_before_new_birth() {
        unsafe {
            let data = map(Kind::OldBlock, ALIGN);
            advise(data, ALIGN, Kind::NurseryBlock);
            assert!(flags_for(data).split_whitespace().any(|v| v == "nh"));
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
}
