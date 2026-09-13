//! #10182: a full's hole-list rebuild (`old_free_rebuild_from_live_old_blocks`)
//! skips a live old block the census proved hole-free: every header parsed,
//! none invalidated, the block unchanged since, and nothing invalidated in it by
//! this sweep.
//!
//! The population fills several old blocks. One interior block keeps every
//! object rooted but carries a pre-existing hole (an invalidated header, the
//! shape a dead object leaves behind in a live block); another is rooted and
//! hole-free. The hole must still reach the free list and the hole-free block
//! must be skipped. The sabotaged twin makes the sweep believe the census saw
//! no hole, and the hole is lost.

use super::super::*;
use super::support::*;
use crate::gc::trace::block_skip::{hole_rebuild_blocks_skipped, sabotage};

fn run_isolated(test: fn()) {
    std::thread::spawn(move || {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        reset_global_roots();
        let _roots = ShadowAndGlobalRootResetGuard;
        test();
    })
    .join()
    .expect("hole-rebuild test thread must not panic");
}

fn block_base(user: usize) -> usize {
    crate::arena::classify_heap_space_in_range(user)
        .map(|(_, base, _)| base)
        .expect("planted object must be in a registered arena block")
}

struct Planted {
    /// Every planted user pointer except the hole, rooted.
    _roots: Vec<Box<u64>>,
    hole_user: usize,
    hole_size: usize,
}

/// Allocate 4.5 blocks of 64-byte old strings, root all of them, and turn one
/// object in the second block into a hole. Its size is unique in the old arena
/// so the free list can be asked for exactly that hole.
unsafe fn plant() -> Planted {
    let mut users = Vec::new();
    let mut bytes = 0usize;
    let mut hole_user = 0usize;
    while bytes < 4 * crate::arena::BLOCK_SIZE + crate::arena::BLOCK_SIZE / 2 {
        let payload = if hole_user == 0 && bytes > crate::arena::BLOCK_SIZE + 4096 {
            200
        } else {
            56
        };
        let user = crate::arena::arena_alloc_gc_old(payload, 8, GC_TYPE_STRING) as usize;
        let size = old_test_header_and_size(user).1;
        if payload == 200 {
            hole_user = user;
        } else {
            users.push(user);
        }
        bytes += size;
    }
    let (hole_header, hole_size) = old_test_header_and_size(hole_user);
    assert_ne!(
        block_base(hole_user),
        block_base(users[0]),
        "premise: interior block"
    );
    super::super::invalidate_dead_old_arena_header(hole_header, hole_size);
    let roots = users
        .iter()
        .map(|&user| {
            let mut slot = Box::new(string_bits(user));
            js_gc_register_global_root(&mut *slot as *mut u64 as i64);
            slot
        })
        .collect();
    Planted {
        _roots: roots,
        hole_user,
        hole_size,
    }
}

fn synchronous_full() {
    let _ = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
        GcTriggerKind::OldGenBytes,
    ));
}

fn hole_listed(planted: &Planted) -> bool {
    let taken = super::super::old_free::old_free_take_exact(planted.hole_size, None);
    if let Some(user) = taken {
        // Put it back so the sweep's accounting is left as found.
        super::super::old_free::old_free_push_for_test(user, planted.hole_size);
    }
    taken == Some(planted.hole_user)
}

#[test]
fn a_hole_in_a_live_block_reaches_the_free_list_and_hole_free_blocks_are_not_parsed() {
    run_isolated(|| {
        let planted = unsafe { plant() };
        let skipped_before = hole_rebuild_blocks_skipped();

        synchronous_full();

        assert!(
            hole_rebuild_blocks_skipped() > skipped_before,
            "the rooted hole-free blocks must be skipped by the rebuild"
        );
        assert!(
            hole_listed(&planted),
            "the pre-existing hole in a live block must still be on the free list"
        );
    });
}

#[test]
fn sabotaged_hole_census_loses_the_hole() {
    run_isolated(|| {
        let planted = unsafe { plant() };
        {
            let _sabotage = sabotage::Guard::arm(sabotage::FORGET_HOLES);
            synchronous_full();
        }
        assert!(
            !hole_listed(&planted),
            "a block believed hole-free is not parsed, so its hole never reaches the list"
        );
    });
}
