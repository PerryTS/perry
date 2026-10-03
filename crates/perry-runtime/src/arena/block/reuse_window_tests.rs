use super::*;

#[test]
fn reused_blocks_restart_the_window_and_cold_blocks_are_advised_once() {
    crate::arena::tests::run_with_fresh_arenas(|| unsafe {
        let size = BLOCK_SIZE;
        let raw = alloc(Layout::from_size_align(size, 16).unwrap());
        assert!(!raw.is_null());
        assert!(block_pool_put(raw, size));
        assert_eq!(advance_block_pool_reuse_window(), 0);
        assert_eq!(block_pool_take(size), Some(raw));
        assert!(block_pool_put(raw, size));
        assert_eq!(advance_block_pool_reuse_window(), 0);
        let first_release = advance_block_pool_reuse_window();
        #[cfg(unix)]
        assert!(first_release > 0, "cold owned pages must be advised");
        #[cfg(not(unix))]
        assert_eq!(first_release, 0);
        assert_eq!(advance_block_pool_reuse_window(), 0, "no repeated advice");
        assert_eq!(block_pool_take(size), Some(raw), "mapping stays reusable");
        dealloc_for_test(raw, size);
    });
}

unsafe fn dealloc_for_test(raw: *mut u8, size: usize) {
    std::alloc::dealloc(raw, Layout::from_size_align(size, 16).unwrap());
}

#[cfg(target_os = "linux")]
unsafe fn resident_pages(raw: *mut u8, size: usize) -> usize {
    let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
    let address = raw as usize;
    let skip = (page - address % page) % page;
    let length = (size - skip) / page * page;
    let mut pages = vec![0u8; length / page];
    assert_eq!(
        libc::mincore(raw.add(skip).cast(), length, pages.as_mut_ptr()),
        0
    );
    pages.iter().filter(|&&p| p & 1 != 0).count()
}

#[cfg(target_os = "linux")]
#[test]
fn real_collection_publication_keeps_warm_pages_and_releases_unused_pages() {
    crate::arena::tests::run_with_fresh_arenas(|| unsafe {
        // Initialize GC/arena providers before adding a deliberately different
        // size: unrelated initial 1 MiB arenas cannot consume this 2 MiB entry.
        crate::gc::js_gc_collect();
        let size = 2 * BLOCK_SIZE;
        let raw = alloc(Layout::from_size_align(size, 16).unwrap());
        assert!(!raw.is_null());
        std::ptr::write_bytes(raw, 0xa5, size);
        let resident = resident_pages(raw, size);
        assert!(resident > 100, "LIVE SUBJECT: pages were faulted in");
        assert!(block_pool_put(raw, size));
        assert_eq!(
            resident_pages(raw, size),
            resident,
            "put must retain hot pages"
        );

        crate::gc::js_gc_collect();
        assert_eq!(
            resident_pages(raw, size),
            resident,
            "first publication keeps warm pages"
        );
        crate::gc::js_gc_collect();
        assert_eq!(resident_pages(raw, size), 0, "unused pages leave RSS");

        assert_eq!(block_pool_take(size), Some(raw));
        std::ptr::write_bytes(raw, 0x5a, size);
        assert_eq!(
            resident_pages(raw, size),
            resident,
            "cold mapping remains writable"
        );
        dealloc_for_test(raw, size);
    });
}
