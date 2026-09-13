//! #10182: an unbudgeted sweep walks each arena block in one pass
//! (`ArenaSweepObjectsState::sweep_whole_block`), keeping marked objects inline
//! and handing every other header to `process_object`.
//!
//! The same deterministic population is planted on two fresh threads — live,
//! pinned and dead old objects (the live ones rooted through one old array),
//! live and dead nursery objects — and one full collection runs on each: on one
//! thread in small work steps, so the sweep takes the per-object path, on the
//! other to completion, so it takes the whole-block path. The sweep statistics
//! and every planted header's fate must be identical. The sabotaged twin keeps
//! objects on the fast path without recording their block as live, and the
//! comparison sees the block reclaimed under them.

use super::super::*;
use super::support::*;
use crate::gc::trace::block_skip::sabotage;

#[derive(Debug, PartialEq, Eq)]
struct Result {
    /// `(freed, eden live, eden dead, arena live, from-space live, reset
    /// blocks)` from the sweep.
    stats: (u64, u64, u64, u64, u64, usize),
    /// `(obj_type, gc_flags)` of every planted header after the collection, in
    /// planting order.
    fates: Vec<(u8, u8)>,
    live_old_survivors: usize,
}

fn trace_snapshot() -> GcTriggerSnapshot {
    GcTriggerSnapshot {
        kind: GcTriggerKind::Manual,
        steps_before: Some(GcStepSnapshot::current()),
    }
}

unsafe fn plant_and_collect(stepped: bool, sabotaged: bool) -> Result {
    const LIVE_SLOTS: u32 = 32_000;
    let (holder, elements) = alloc_old_test_array(LIVE_SLOTS);
    let mut root = ptr_bits(holder as usize);
    js_gc_register_global_root(&mut root as *mut u64 as i64);
    let mut headers = vec![holder as usize - GC_HEADER_SIZE];
    let mut live_slot = 0u32;
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state as usize
    };
    let mut live_old = Vec::new();
    for i in 0..60_000usize {
        let roll = next();
        let young = i % 5 == 0;
        let user = if young {
            young_leaf()
        } else {
            crate::arena::arena_alloc_gc_old(8 + roll % 120, 8, GC_TYPE_STRING) as usize
        };
        headers.push(user - GC_HEADER_SIZE);
        // The first 40 000 objects (about three blocks) hold survivors and
        // garbage but nothing pinned, so whole blocks keep their survivors on
        // the fast path alone; the rest add pinned objects.
        let pinning_stretch = i >= 40_000;
        match roll % 16 {
            0..=9 if live_slot < LIVE_SLOTS && !pinning_stretch => {
                *elements.add(live_slot as usize) = string_bits(user);
                live_slot += 1;
                if !young {
                    live_old.push(user);
                }
            }
            10 if !young && pinning_stretch => {
                crate::gc::pin_object(header_from_user_ptr(user as *const u8))
            }
            _ => {}
        }
    }
    let _sabotage = sabotaged.then(|| sabotage::Guard::arm(sabotage::FORGET_WHOLE_BLOCK_LIVE));
    let mut cycle = GcCycleState::new_full(trace_snapshot());
    let outcome = if stepped {
        while !cycle.step(GcWorkBudget::bounded(64)).completed {}
        cycle
            .take_outcome()
            .expect("completed cycle has an outcome")
    } else {
        cycle.run_to_completion()
    };
    let sweep = outcome.trace.expect("trace requested").sweep;
    let fates = headers
        .iter()
        .map(|&h| {
            let header = h as *const GcHeader;
            ((*header).obj_type, (*header).gc_flags)
        })
        .collect();
    let live_old_survivors = live_old
        .iter()
        .filter(|&&u| crate::arena::pointer_in_old_gen(u))
        .count();
    Result {
        stats: (
            sweep.freed_bytes,
            sweep.eden_live_bytes,
            sweep.eden_dead_bytes,
            sweep.arena_live_bytes,
            sweep.arena_live_from_space_bytes,
            sweep.reset_blocks,
        ),
        fates,
        live_old_survivors,
    }
}

fn on_fresh_thread(stepped: bool, sabotaged: bool) -> Result {
    std::thread::spawn(move || {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        reset_global_roots();
        let _roots = ShadowAndGlobalRootResetGuard;
        unsafe { plant_and_collect(stepped, sabotaged) }
    })
    .join()
    .expect("whole-block sweep test thread must not panic")
}

#[test]
fn the_whole_block_sweep_matches_the_per_object_sweep() {
    let per_object = on_fresh_thread(true, false);
    let whole_block = on_fresh_thread(false, false);
    assert!(
        per_object.stats.0 > 0 && per_object.stats.3 > 0,
        "premise: the sweep both freed and kept: {:?}",
        per_object.stats
    );
    assert!(per_object.live_old_survivors > 0, "premise: old survivors");
    assert_eq!(per_object.stats, whole_block.stats);
    assert_eq!(
        per_object.live_old_survivors,
        whole_block.live_old_survivors
    );
    assert!(
        per_object.fates == whole_block.fates,
        "a planted header's fate differs"
    );
}

#[test]
fn sabotaged_whole_block_liveness_is_caught_by_the_comparison() {
    let per_object = on_fresh_thread(true, false);
    let sabotaged = on_fresh_thread(false, true);
    assert!(
        sabotaged.live_old_survivors < per_object.live_old_survivors,
        "keeping objects without marking their block live must release rooted \
         objects' blocks: {} vs {}",
        sabotaged.live_old_survivors,
        per_object.live_old_survivors
    );
}
