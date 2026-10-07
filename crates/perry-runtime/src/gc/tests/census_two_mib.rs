//! Regression for the step-1 two-pointer census overflow at chunk 2.
use super::super::*;
use super::support::*;

#[test]
#[cfg(target_os = "linux")]
fn a_full_two_mib_block_is_censused_by_both_walks() {
    std::thread::spawn(|| {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        assert_eq!(crate::arena::BLOCK_SIZE, 2 * 1024 * 1024);
        let mut users = Vec::new();
        for _ in 0..crate::arena::BLOCK_SIZE / 64 + 1 {
            users.push(crate::arena::arena_alloc_gc_old(56, 8, GC_TYPE_STRING) as usize);
        }
        let whole = ValidPointerSetBuilder::new().finish();
        let mut builder = ValidPointerSetBuilder::new();
        while !builder.step(7) {}
        let stepped = builder.finish();
        let full = whole
            .arena_blocks
            .iter()
            .find(|b| b.extent == crate::arena::BLOCK_SIZE)
            .expect("LIVE SUBJECT: a full regular 2 MiB block");
        assert!(!full.sorted);
        assert_eq!(full.chunks.len(), 4);
        assert!(full.chunks.iter().all(|p| !p.is_null()));
        assert_eq!(whole.start_bitmap_chunks, stepped.start_bitmap_chunks);
        for user in users {
            assert!(whole.contains(&user), "whole census lost {user:#x}");
            assert!(stepped.contains(&user), "stepped census lost {user:#x}");
            assert!(!whole.contains(&(user + 1)));
        }
    })
    .join()
    .unwrap();
}
