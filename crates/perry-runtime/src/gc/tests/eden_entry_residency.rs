//! Real full-GC publication/entry subject, without manually aging blocks.
use super::super::*;
use super::support::*;

unsafe fn resident_interior_pages(address: usize, size: usize) -> usize {
    let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
    let begin = address.next_multiple_of(page);
    let end = (address + size) / page * page;
    let mut bits = vec![0u8; (end - begin) / page];
    assert_eq!(
        libc::mincore(begin as *mut _, end - begin, bits.as_mut_ptr()),
        0
    );
    bits.iter().filter(|&&b| b & 1 != 0).count()
}

#[test]
fn real_full_collection_gives_eden_a_reuse_interval_then_discards_idle_pages() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _scan = ConservativeScanDisabledGuard::new();
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        register_runtime_handle_root_scanner_for_tests();
        let bytes = [b'x'; 700];
        let mut last = std::ptr::null();
        // These are complete, real nursery strings, not bare arena buffers.
        for _ in 0..(8 * crate::arena::BLOCK_SIZE / bytes.len()) {
            last = crate::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        }
        assert!(crate::arena::pointer_in_nursery(last as usize));
        let roots = RuntimeHandleScope::new();
        let live = roots.root_string_ptr(last);
        let collect = || {
            gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
                GcTriggerKind::OldGenBytes,
            ))
        };
        collect();
        let candidates = crate::arena::ARENA.with(|cell| {
            let arena = &*cell.get();
            assert!(
                arena.blocks.len() >= 8,
                "fixture allocated several Eden blocks"
            );
            let keep_low = arena.current.saturating_sub(4);
            arena
                .blocks
                .iter()
                .enumerate()
                .filter_map(|(i, block)| {
                    (!block.data.is_null()
                        && block.offset == 0
                        && block.dead_cycles == 1
                        && !(keep_low..=arena.current).contains(&i))
                    .then_some((block.data as usize, block.size))
                })
                .collect::<Vec<_>>()
        });
        assert!(
            candidates.len() >= 2,
            "real first publication resets several blocks"
        );
        for &(address, size) in &candidates {
            assert!(
                resident_interior_pages(address, size) > 100,
                "first reset gives warm pages an actual reuse interval"
            );
        }
        // Real intervening mutator allocation uses the recent block. The
        // older reset blocks remain unused; no idle counts are written here.
        let extra = crate::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let extra_root = roots.root_string_ptr(extra);
        collect();
        for &(address, size) in &candidates {
            assert_eq!(
                resident_interior_pages(address, size),
                0,
                "next real collection entry discards unused interior pages"
            );
        }
        for root in [live, extra_root] {
            // The closure only reads the string; nothing here allocates.
            root.with_const_ptr(|string: *const crate::string::StringHeader| {
                assert_eq!((*string).byte_len, bytes.len() as u32);
                assert_eq!(
                    std::slice::from_raw_parts(crate::string::string_data(string), bytes.len()),
                    bytes
                );
            });
        }
    })
    .join()
    .expect("real GC entry subject must pass");
}
