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

#[cfg(target_os = "linux")]
#[test]
fn collection_entry_discards_only_previously_idle_eden_pages_and_keeps_reuse_safe() {
    crate::arena::tests::run_with_fresh_arenas(|| unsafe {
        for _ in 0..8 {
            crate::arena::arena_alloc(BLOCK_SIZE, 8);
        }
        super::super::sync_inline_arena_state();
        let (mapping, size, bitmap, recent) = ARENA.with(|cell| {
            let arena = &mut *cell.get();
            assert!(arena.current >= 7);
            let recent = arena.current;
            for i in [0, 1, 2, recent - 4, recent] {
                let block = &arena.blocks[i];
                std::ptr::write_bytes(block.data, 0xa5, block.size);
                assert!(resident_pages(block.data, block.size) > 100);
            }
            // Previous publication reset slot 0; slot 1 is a fresh reset with
            // no elapsed interval. Slot 2 was reused after its previous reset.
            for i in [0, 1, recent - 4, recent] {
                arena.blocks[i].clear_object_starts();
                arena.blocks[i].offset = 0;
                arena.blocks[i].dead_cycles = u32::from(i != 1);
            }
            arena.blocks[2].dead_cycles = 1;
            arena.resync_inline_to_current();
            let block = &arena.blocks[0];
            (block.data, block.size, block.object_starts_ptr(), recent)
        });
        assert!(crate::arena::discard_previously_idle_eden_pages() > 0);
        assert_eq!(
            resident_pages(mapping, size),
            0,
            "aged empty pages leave RSS"
        );
        assert_eq!(
            crate::arena::discard_previously_idle_eden_pages(),
            0,
            "an unchanged idle interval requires no repeated advice"
        );
        ARENA.with(|cell| {
            let arena = &mut *cell.get();
            assert!(arena.blocks[0].idle_pages_discarded);
            arena.blocks[0].clear_object_starts();
            assert!(
                arena.blocks[0].idle_pages_discarded,
                "resetting an already-empty block preserves advice state"
            );
            assert_eq!(arena.blocks[0].data, mapping);
            assert_eq!(arena.blocks[0].object_starts_ptr(), bitmap);
            assert_eq!(arena.blocks[0].dead_cycles, 1, "advice does not age blocks");
            for i in [1, 2, recent - 4, recent] {
                let block = &arena.blocks[i];
                assert!(resident_pages(block.data, block.size) > 100);
                assert_eq!(*block.data.add(4096), 0xa5);
            }
            let reused = arena.try_block_alloc(0, BLOCK_SIZE, 8).unwrap();
            assert_eq!(reused, mapping);
            std::ptr::write_bytes(reused, 0x5a, BLOCK_SIZE);
        });
        crate::arena::discard_previously_idle_eden_pages();
        assert!(resident_pages(mapping, size) > 100);
        assert_eq!(*mapping.add(4096), 0x5a, "reused data remains intact");
        ARENA.with(|cell| {
            let arena = &mut *cell.get();
            let block = &mut arena.blocks[0];
            assert_eq!(block.offset, BLOCK_SIZE);
            block.clear_object_starts();
            assert!(
                !block.idle_pages_discarded,
                "resetting actual reuse starts a new idle interval"
            );
            block.offset = 0;
        });
        assert!(
            crate::arena::discard_previously_idle_eden_pages() > 0,
            "reused pages are discarded again in the next eligible interval"
        );
        assert_eq!(resident_pages(mapping, size), 0);
    });
}

#[cfg(target_pointer_width = "64")]
#[test]
fn idle_advice_state_fits_existing_arena_block_padding() {
    assert_eq!(std::mem::size_of::<ArenaBlock>(), 48);
}
