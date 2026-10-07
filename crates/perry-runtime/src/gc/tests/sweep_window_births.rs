//! #11842: objects the program allocates while a budgeted sweep is still
//! running.
//!
//! A budgeted cycle sweeps across mutator windows. A birth in one of them
//! carries no mark, so the sweep must never be able to reach it, and nothing
//! the sweep does in a later step may lose track of it. Two ways that broke,
//! both found in a compiled package manager whose cold installs failed 10-40%
//! of the time with the incremental collector on (wrong files, lost resolved
//! packages, a crash on a 1.4 GB "string"):
//!
//! * The old-gen hole list from the previous sweep stayed usable, so a
//!   same-size allocation could land in a hole inside a block the walk had not
//!   reached yet. The walk then freed that live, unmarked object.
//! * The block cleanup's last step moves Eden's current block back to the
//!   first empty one and repoints codegen's inline allocator there, without
//!   first storing the inline allocator's offset into the block it was filling.
//!   Everything it had placed in that block since the sweep began sat past the
//!   block's stored offset, and the block's next fill overwrote it.

use super::super::*;
use super::support::*;

/// Total 48 bytes, the same for every old object here, so a reuse is an
/// exact fit.
const PAYLOAD: usize = 40;

fn old_string() -> usize {
    crate::arena::arena_alloc_gc_old(PAYLOAD, 8, GC_TYPE_STRING) as usize
}

fn mark(users: &[usize]) {
    for &user in users {
        let (header, _) = old_test_header_and_size(user);
        unsafe { (*header).gc_flags |= GC_FLAG_MARKED };
    }
}

fn begin_old() -> std::sync::MutexGuard<'static, ()> {
    let isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    old_free_reset_for_test();
    crate::arena::old_pages_begin_gc_cycle();
    isolation
}

fn end_old() {
    old_free_reset_for_test();
    clear_marks();
    remembered_set_clear();
}

fn sweep_step(sweep: &mut IncrementalSweepState) -> bool {
    let _heap_change = crate::gc::heap_generation::HeapChange::begin(
        crate::gc::heap_generation::HeapChangeKind::Sweep,
    );
    sweep.step(1)
}

/// Two live old objects with a dead one between them, swept once so the dead
/// object's hole is listed. Returns the two live objects.
fn live_pair_around_a_listed_hole() -> [usize; 2] {
    let live_a = old_string();
    let dead = old_string();
    let live_b = old_string();
    mark(&[live_a, live_b]);
    crate::arena::old_pages_begin_gc_cycle();
    let _ = sweep_with_age_bump_and_old_reclaim(false, true);
    let size = old_test_header_and_size(live_a).1;
    assert!(
        old_free_listed_for_test()
            .iter()
            .any(|&(user, listed)| user == dead && listed == size),
        "SUBJECT-LIVE CHECK: the dead object's hole must be listed at the size the test allocates"
    );
    [live_a, live_b]
}

#[test]
fn an_old_object_born_while_a_full_sweep_walks_is_not_freed_by_it() {
    let _isolation = begin_old();
    let live = live_pair_around_a_listed_hole();

    // The next full sweep, stepped the way a budgeted cycle steps it: the
    // marks are final and the program runs between steps.
    mark(&live);
    crate::arena::old_pages_begin_gc_cycle();
    let mut sweep = IncrementalSweepState::new(false, true, None, false, false);
    assert!(!sweep_step(&mut sweep), "the sweep must still be walking");

    // ---- mutator window: no allocate-black during a sweep ----
    let born = old_string();
    let (born_header, _) = old_test_header_and_size(born);
    assert_eq!(
        unsafe { (*born_header).gc_flags } & GC_FLAG_MARKED,
        0,
        "SUBJECT-LIVE CHECK: a birth outside a mark phase carries no mark"
    );
    // ---- the sweep resumes ----

    let mut steps = 0;
    while !sweep_step(&mut sweep) {
        steps += 1;
        assert!(steps < 500_000, "sweep did not complete within step limit");
    }
    assert_eq!(
        unsafe { (*born_header).obj_type },
        GC_TYPE_STRING,
        "#11842: an object born while the sweep was walking was freed by that sweep"
    );
    assert!(
        !old_free_listed_for_test()
            .iter()
            .any(|&(user, _)| user == born || live.contains(&user)),
        "a live object must never be listed as a hole"
    );
    end_old();
}

/// Place one object the way codegen's inline allocator does: by bumping the
/// inline state's own copy of the current block's offset, leaving the arena
/// block untouched. Returns `(start, end)`, or `None` when the block is full.
unsafe fn inline_birth() -> Option<(usize, usize)> {
    let state = crate::arena::js_inline_arena_state();
    let total = (GC_HEADER_SIZE + PAYLOAD + 7) & !7;
    let at = ((*state).offset + 7) & !7;
    if (*state).data.is_null() || at + total > (*state).size {
        return None;
    }
    let header = (*state).data.add(at) as *mut GcHeader;
    (*header).obj_type = GC_TYPE_STRING;
    (*header).gc_flags = GC_FLAG_ARENA;
    (*header)._reserved = 0;
    (*header).size = gc_header_size_word(total);
    (*state).offset = at + total;
    Some((header as usize, header as usize + total))
}

#[test]
fn inline_births_during_a_budgeted_block_cleanup_stay_inside_their_block() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_marks();
    clear_mark_seeds();

    // Several Eden blocks of garbage, so the cleanup resets blocks behind the
    // current one and then moves allocation back to the first of them.
    for _ in 0..8 * crate::arena::BLOCK_SIZE / (8 * 1024) {
        let _ = crate::arena::arena_alloc_gc(8 * 1024, 8, GC_TYPE_STRING);
    }
    let inline_block_before = unsafe { (*crate::arena::js_inline_arena_state()).data } as usize;

    // A budgeted non-moving sweep, with the program allocating inline
    // between every step.
    let mut sweep = IncrementalSweepState::new(false, false, None, false, true);
    let mut born = Vec::new();
    loop {
        if let Some(object) = unsafe { inline_birth() } {
            born.push(object);
        }
        if sweep_step(&mut sweep) {
            break;
        }
        assert!(
            born.len() < 500_000,
            "sweep did not complete within step limit"
        );
    }
    let inline_block_after = unsafe { (*crate::arena::js_inline_arena_state()).data } as usize;
    assert_ne!(
        inline_block_before, inline_block_after,
        "SUBJECT-LIVE CHECK: the cleanup must have moved the inline allocator to another block"
    );
    assert!(
        !born.is_empty(),
        "SUBJECT-LIVE CHECK: the program must have allocated inline"
    );

    // Every object the inline allocator placed must lie inside its block's
    // recorded fill, or the block's next fill writes over it.
    let blocks = crate::arena::arena_block_snapshots();
    for &(start, end) in &born {
        let block = blocks
            .iter()
            .find(|block| block.data != 0 && start >= block.data && start < block.data + block.size)
            .expect("an inline birth lies in an arena block");
        assert!(
            end <= block.data + block.offset,
            "#11842: an object the inline allocator placed during the cleanup ends at +{} but its \
             block records only {} bytes in use; the next fill of that block overwrites it",
            end - block.data,
            block.offset
        );
    }
    clear_marks();
}

#[test]
fn the_sweep_quarantine_poisons_a_swept_old_object_and_never_lists_its_hole() {
    let _isolation = begin_old();
    let _mode =
        crate::arena::OldSweepProtectionGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    let (retired_before, _) = crate::arena::old_sweep_quarantine_stats();

    let live_a = old_string();
    let dead = old_string();
    let live_b = old_string();
    mark(&[live_a, live_b]);
    crate::arena::old_pages_begin_gc_cycle();
    let _ = sweep_with_age_bump_and_old_reclaim(false, true);

    let (retired_after, _) = crate::arena::old_sweep_quarantine_stats();
    assert!(
        retired_after > retired_before,
        "the instrument must have retired the swept object"
    );
    assert!(
        old_free_listed_for_test().is_empty(),
        "under the instrument no hole is ever listed for reuse"
    );
    let first_word = unsafe { (dead as *const u64).read() };
    assert_eq!(
        first_word, 0x7FFD_0000_DEAD_0000,
        "a retired object's payload must read as the poison word"
    );
    assert_ne!(
        old_string(),
        dead,
        "a retired object's bytes were handed out again"
    );
    end_old();
}
