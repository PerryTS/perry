//! Return physical pages inside an empty block while keeping its allocation
//! available for reuse. Allocator-owned bytes in either boundary page must
//! never be discarded: arena allocations are only guaranteed 16-byte alignment.

#[cfg(unix)]
fn page_size() -> Option<usize> {
    static SIZE: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    *SIZE.get_or_init(|| {
        let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        (size > 0).then_some(size as usize)
    })
}

/// Offset and length of complete pages contained in the owned byte range.
#[cfg(any(unix, test))]
fn interior_pages(address: usize, size: usize, page: usize) -> Option<(usize, usize)> {
    if page == 0 {
        return None;
    }
    let skip = (page - address % page) % page;
    let length = size.checked_sub(skip)? / page * page;
    (length > 0 && address.checked_add(size).is_some()).then_some((skip, length))
}

/// The caller owns `size` bytes at `data`, and no contents remain live. Return
/// the bytes accepted by the kernel, so tests can distinguish success from a
/// silently rejected (e.g. unaligned) advice call.
pub(super) unsafe fn release(data: *mut u8, size: usize) -> usize {
    #[cfg(unix)]
    {
        let Some(page) = page_size() else { return 0 };
        let Some((skip, length)) = interior_pages(data as usize, size, page) else {
            return 0;
        };
        // Linux's MADV_FREE retains pages in RSS until pressure. DONTNEED
        // drops the empty private pages now; bump allocation initializes the
        // payload again on reuse. Other Unix targets retain their FREE policy.
        #[cfg(target_os = "linux")]
        let advice = libc::MADV_DONTNEED;
        #[cfg(not(target_os = "linux"))]
        let advice = libc::MADV_FREE;
        if libc::madvise(data.add(skip).cast(), length, advice) == 0 {
            return length;
        }
    }
    #[cfg(not(unix))]
    let _ = (data, size);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_complete_owned_pages_are_eligible() {
        assert_eq!(interior_pages(16, 4 * 4096, 4096), Some((4080, 3 * 4096)));
        assert_eq!(interior_pages(4096, 4096, 4096), Some((0, 4096)));
        assert_eq!(interior_pages(16, 4096, 4096), None);
        assert_eq!(interior_pages(0, 0, 4096), None);
        assert_eq!(interior_pages(0, 4096, 0), None);
        assert_eq!(interior_pages(usize::MAX - 15, 4096, 4096), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn kernel_releases_interior_pages_without_touching_either_neighbor() {
        unsafe {
            let page = page_size().unwrap();
            let size = 5 * page;
            let mapping = libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            );
            assert_ne!(mapping, libc::MAP_FAILED);
            let data = mapping.cast::<u8>();
            std::ptr::write_bytes(data, 0xa5, size);
            let mut resident = [0u8; 5];
            assert_eq!(libc::mincore(mapping, size, resident.as_mut_ptr()), 0);
            assert!(
                resident.iter().all(|v| v & 1 != 0),
                "touch must fault in every page"
            );
            // Model a malloc allocation sharing both boundary pages.
            assert_eq!(release(data.add(16), size - 32), 3 * page);
            assert_eq!(libc::mincore(mapping, size, resident.as_mut_ptr()), 0);
            assert_eq!(resident.map(|v| v & 1), [1, 0, 0, 0, 1]);
            for i in 0..page {
                assert_eq!(*data.add(i), 0xa5);
                assert_eq!(*data.add(4 * page + i), 0xa5);
            }
            // Pages remain mapped and writable for the next arena owner.
            std::ptr::write_bytes(data.add(page), 0x5a, 3 * page);
            assert_eq!(*data.add(2 * page), 0x5a);
            assert_eq!(libc::munmap(mapping, size), 0);
        }
    }
}
