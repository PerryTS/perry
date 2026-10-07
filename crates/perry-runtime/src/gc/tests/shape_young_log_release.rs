//! #12098: a keys address whose Eden block is released must leave the shape
//! table together with its young-log entry.
//!
//! The young walk used to drop such an address from the log (it classifies
//! nowhere, so no minor can act on it) while its slot index and family stayed
//! in the table. No prune could remove them afterwards: every dead-owner
//! probe skips an address it cannot attribute to the heap. Once the block
//! came back as Eden, the entries were minor-relevant again but unlogged, and
//! the next minor's rule-2 re-derivation panicked.

use super::super::*;
use super::support::*;
use crate::arena::HeapSpace;

fn eden_block_index(addr: usize) -> Option<usize> {
    crate::arena::sync_inline_arena_state();
    crate::arena::ARENA.with(|arena| unsafe {
        let arena = &*arena.get();
        arena.blocks.iter().position(|block| {
            let base = block.data as usize;
            !block.data.is_null() && (base..base + block.size).contains(&addr)
        })
    })
}

/// Allocate unrooted filler arrays until one lands in Eden block `target` or
/// later. Returns that allocation's block index.
fn fill_eden_through_block(target: usize) -> usize {
    for _ in 0..4_000_000 {
        let filler = crate::array::js_array_alloc(64) as usize;
        if let Some(index) = eden_block_index(filler) {
            if index >= target {
                return index;
            }
        }
    }
    panic!("filler never reached Eden block {target}");
}

/// Seed a keys address in an Eden block, release the block, let
/// `first_walk` be the first shape walk that meets the released address, then
/// reuse the block as Eden and run a minor.
fn released_keys_block_round_trip(first_walk: fn()) {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::object::shapes::test_clear_shape_table();
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);

    // A keys array in an Eden block the post-minor reset does not keep: the
    // reset points allocation back at block 0, and an idle block outside the
    // recent window is released after two idle collections.
    fill_eden_through_block(2);
    let keys = crate::array::js_array_alloc(4) as usize;
    let keys_block = eden_block_index(keys).expect("the keys array must be in Eden");
    assert!(keys_block >= 2, "the keys array must sit past block 0");
    fill_eden_through_block(keys_block + 6);

    // A slot index and a family under the keys address, logged as rule 1
    // requires.
    crate::object::shapes::test_seed_shape_entry(keys);
    assert!(crate::object::shapes::test_shape_entry_exists(keys));
    assert!(!crate::object::shapes::test_shape_ids_for_keys(keys).is_empty());

    // The keys array dies in a way no prune can attribute. In the native
    // stream churn that found #12098, the array was tenured in place, died,
    // and its bytes were reused; the last young prune before the block was
    // released read a zeroed header there (`obj_type 0, flags 0`). Reproduce
    // that header directly.
    unsafe {
        let header = header_from_user_ptr(keys as *const u8);
        (*header).obj_type = 0;
        (*header).gc_flags = 0;
    }

    // Collect until the idle block is released and the address classifies
    // nowhere.
    let mut released_after = None;
    for minor in 0..8 {
        let _ = gc_collect_minor();
        if crate::arena::classify_heap_space(keys) == HeapSpace::Unknown {
            released_after = Some(minor);
            break;
        }
    }
    assert!(
        released_after.is_some(),
        "fixture: the keys array's Eden block must be released ({:?})",
        crate::arena::classify_heap_space(keys)
    );

    // The first walk to meet the released address.
    first_walk();

    // Reuse the block as Eden: the pool hands released blocks back first.
    let mut reused = false;
    for _ in 0..4_000_000 {
        let _ = crate::array::js_array_alloc(64);
        if crate::arena::classify_heap_space(keys) == HeapSpace::NurseryEden {
            reused = true;
            break;
        }
    }
    assert!(reused, "fixture: the released block must come back as Eden");

    // Rule 2 re-derives the relevant keys from the table on this minor. A
    // stale entry under the reused address panics here.
    let _ = gc_collect_minor();

    assert!(
        !crate::object::shapes::test_shape_entry_exists(keys),
        "the slot index under a released keys address must leave with its log entry"
    );
    assert!(
        crate::object::shapes::test_shape_ids_for_keys(keys).is_empty(),
        "the family under a released keys address must leave with its log entry"
    );
}

#[test]
fn shape_keys_in_a_released_eden_block_leave_the_table_with_their_log_entry() {
    // A minor's young walk meets the released address first.
    released_keys_block_round_trip(|| {
        let _ = gc_collect_minor();
    });
}

#[test]
fn a_full_walk_retires_a_logged_keys_address_whose_block_was_released() {
    // A full collection meets the released address first. Its walk rebuilds
    // the log from the tables, so it must settle what the log named first.
    released_keys_block_round_trip(|| {
        crate::gc::js_gc_collect();
    });
}
