//! #10182: block-granular reclamation in the synchronous full sweep.
//!
//! Every case plants an old-generation population on a fresh thread (so the
//! arenas start empty), runs one synchronous full collection, and asks the
//! sweep which blocks it reclaimed without entering
//! (`block_skip::sabotage::last_skipped_block_bases`). Each protective
//! assertion is paired with a sabotaged run that breaks exactly the fact it
//! depends on and shows the harm the assertion exists to catch.

use super::super::*;
use super::support::*;
use crate::gc::trace::block_skip::{block_skip_reclaimed_totals, sabotage};

const PLANT_BYTES: usize = 3 * crate::arena::BLOCK_SIZE + crate::arena::BLOCK_SIZE / 2;

fn run_isolated(test: fn()) {
    std::thread::spawn(move || {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        reset_global_roots();
        let _roots = ShadowAndGlobalRootResetGuard;
        test();
    })
    .join()
    .expect("block-skip test thread must not panic");
}

fn synchronous_full() {
    let _ = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
        GcTriggerKind::OldGenBytes,
    ));
}

/// Plant dead old-gen plain objects — strings, zero-field objects and small
/// arrays in rotation — until `bytes` are allocated. Returns every user address.
unsafe fn plant_plain(bytes: usize) -> Vec<usize> {
    let mut planted = Vec::new();
    let mut allocated = 0usize;
    let mut i = 0usize;
    while allocated < bytes {
        let user = match i % 3 {
            0 => crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize,
            1 => alloc_old_test_object(0).0 as usize,
            _ => alloc_old_test_array(2).0 as usize,
        };
        allocated += old_test_header_and_size(user).1;
        planted.push(user);
        i += 1;
    }
    planted
}

/// `data` of the arena block holding `user`.
fn block_base(user: usize) -> usize {
    crate::arena::classify_heap_space_in_range(user)
        .map(|(_, base, _)| base)
        .expect("planted object must be in a registered arena block")
}

/// Every walkable object in the block starting at `base`.
fn objects_in_block(base: usize) -> Vec<usize> {
    let snapshots = crate::arena::arena_block_snapshots();
    let Some(block_idx) = snapshots.iter().position(|s| s.data == base) else {
        return Vec::new();
    };
    let mut objects = Vec::new();
    crate::arena::arena_walk_objects_filtered(
        |idx| idx == block_idx,
        |header_ptr, _| objects.push(header_ptr as usize + GC_HEADER_SIZE),
    );
    objects
}

/// A block every walkable object of which is in `planted`, that is neither the
/// first nor the last planted block (so it is full, and not the old arena's
/// allocation block).
fn interior_planted_block(planted: &[usize]) -> (usize, Vec<usize>) {
    let set: std::collections::HashSet<usize> = planted.iter().copied().collect();
    let first = block_base(planted[0]);
    let last = block_base(*planted.last().unwrap());
    let mut seen = std::collections::HashSet::new();
    for &user in planted {
        let base = block_base(user);
        if base == first || base == last || !seen.insert(base) {
            continue;
        }
        let objects = objects_in_block(base);
        if !objects.is_empty() && objects.iter().all(|o| set.contains(o)) {
            return (base, objects);
        }
    }
    panic!("test premise: the planted population must fill an interior block");
}

fn skipped(base: usize) -> bool {
    sabotage::last_skipped_block_bases().contains(&base)
}

fn header_type(user: usize) -> u8 {
    unsafe { (*header_from_user_ptr(user as *const u8)).obj_type }
}

/// (a) A block holding only dead plain objects is reclaimed without the sweep
/// entering it, and the live-subject counters account every object in it.
#[test]
fn a_dead_block_of_plain_objects_is_reclaimed_without_visiting_it() {
    run_isolated(|| {
        let planted = unsafe { plant_plain(PLANT_BYTES) };
        let (base, objects) = interior_planted_block(&planted);
        let before = block_skip_reclaimed_totals();

        synchronous_full();

        let after = block_skip_reclaimed_totals();
        assert!(
            skipped(base),
            "the all-dead interior block {base:#x} must be reclaimed without a visit"
        );
        assert!(after.0 > before.0, "the skip counter must move");
        assert!(
            after.1 - before.1 >= objects.len() as u64,
            "every object of the skipped block must be accounted: {} < {}",
            after.1 - before.1,
            objects.len()
        );
        assert!(
            !crate::arena::pointer_in_old_gen(objects[0]),
            "the skipped block must still be released by the block cleanup"
        );
    });
}

/// (c) One reachable object keeps its whole block on the per-object path: the
/// block is not skipped, the object survives, and its dead neighbours are
/// swept one by one (an old dead header is invalidated to `obj_type == 0`).
#[test]
fn a_live_neighbour_keeps_its_block_on_the_per_object_path() {
    run_isolated(|| {
        let planted = unsafe { plant_plain(PLANT_BYTES) };
        let (base, objects) = interior_planted_block(&planted);
        let live = *objects
            .iter()
            .find(|&&o| header_type(o) == GC_TYPE_OBJECT)
            .expect("interior block holds a planted object");
        let dead_neighbour = *objects.iter().find(|&&o| o != live).unwrap();
        let mut root = ptr_bits(live);
        js_gc_register_global_root(&mut root as *mut u64 as i64);

        synchronous_full();

        assert!(
            !skipped(base),
            "a block with a reachable object must be walked"
        );
        assert!(
            crate::arena::pointer_in_old_gen(live),
            "the reachable object's block must survive"
        );
        assert_eq!(header_type(live), GC_TYPE_OBJECT);
        assert_eq!(
            header_type(dead_neighbour),
            0,
            "the live block's dead neighbours are swept per object"
        );
        assert!(
            !sabotage::last_skipped_block_bases().is_empty(),
            "the other all-dead blocks are still skipped in the same sweep"
        );
    });
}

/// Sabotage for (c): with the census's reachability record broken, the same
/// population loses its reachable object's block — which is the assertion
/// above failing.
#[test]
fn sabotaged_reachability_reclaims_a_live_block() {
    run_isolated(|| {
        let planted = unsafe { plant_plain(PLANT_BYTES) };
        let (base, objects) = interior_planted_block(&planted);
        let live = *objects
            .iter()
            .find(|&&o| header_type(o) == GC_TYPE_OBJECT)
            .unwrap();
        let mut root = ptr_bits(live);
        js_gc_register_global_root(&mut root as *mut u64 as i64);

        {
            let _sabotage = sabotage::Guard::arm(sabotage::FORGET_REACHED);
            synchronous_full();
        }

        assert!(
            skipped(base),
            "sabotage premise: the live block is skipped once reachability is forgotten"
        );
        assert!(
            !crate::arena::pointer_in_old_gen(live),
            "and the reachable object's block is released under its root"
        );
        reset_global_roots();
    });
}

/// Plant a population with `special` allocated in the middle of it, and
/// return `(special, block base)` with the premise that its block is interior.
unsafe fn plant_around<T>(
    special: impl FnOnce() -> T,
    user_of: impl Fn(&T) -> usize,
) -> (T, usize) {
    let mut planted = plant_plain(PLANT_BYTES / 2);
    let value = special();
    let user = user_of(&value);
    planted.push(user);
    planted.extend(plant_plain(PLANT_BYTES / 2));
    let base = block_base(user);
    assert_ne!(base, block_base(planted[0]), "premise: not the first block");
    assert_ne!(
        base,
        block_base(*planted.last().unwrap()),
        "premise: not the allocation block"
    );
    (value, base)
}

/// (b) A dead promise is a finalize-hook object: its block keeps the
/// per-object path and `PromiseCleanup` drops its side-table entries.
#[test]
fn a_dead_promise_keeps_its_block_on_the_per_object_path() {
    run_isolated(|| {
        let (promise, base) = unsafe {
            plant_around(
                || {
                    let p = alloc_old_test_promise();
                    crate::promise::scanners::test_park_promise_side_table_entries(p);
                    p
                },
                |p| *p as usize,
            )
        };
        assert_eq!(
            crate::promise::scanners::test_promise_side_table_counts_for(promise as usize),
            (1, 1, 1),
            "premise: one entry parked in each table"
        );

        synchronous_full();

        assert!(!skipped(base), "a block holding a promise must be walked");
        assert_eq!(
            crate::promise::scanners::test_promise_side_table_counts_for(promise as usize),
            (0, 0, 0),
            "the dead promise's finalizer must have run"
        );
    });
}

/// Sabotage for (b): with obligations forgotten the promise's block is skipped
/// and its finalizer never runs.
#[test]
fn sabotaged_obligations_skip_a_promise_finalizer() {
    run_isolated(|| {
        let (promise, base) = unsafe {
            plant_around(
                || {
                    let p = alloc_old_test_promise();
                    crate::promise::scanners::test_park_promise_side_table_entries(p);
                    p
                },
                |p| *p as usize,
            )
        };

        {
            let _sabotage = sabotage::Guard::arm(sabotage::FORGET_OBLIGATIONS);
            synchronous_full();
        }

        assert!(
            skipped(base),
            "sabotage premise: the promise block is skipped"
        );
        assert_eq!(
            crate::promise::scanners::test_promise_side_table_counts_for(promise as usize),
            (1, 1, 1),
            "and the finalizer did not run — the assertion above fails"
        );
    });
}

/// (b) A dead Set keeps its block on the per-object path (its side allocation
/// is a finalize hook).
#[test]
fn a_dead_set_keeps_its_block_on_the_per_object_path() {
    run_isolated(|| {
        let ((set, elements, layout), base) =
            unsafe { plant_around(|| alloc_old_test_set(4), |(s, _, _)| *s as usize) };
        let neighbour = *objects_in_block(base)
            .iter()
            .find(|&&o| o != set as usize)
            .unwrap();

        synchronous_full();

        assert!(!skipped(base), "a block holding a Set must be walked");
        assert_eq!(
            header_type(neighbour),
            0,
            "its dead neighbours are swept per object"
        );
        unsafe { retire_old_test_set(set, elements, layout) };
    });
}

/// A dead weak TARGET's block is walked, and the `WeakRef` clears.
///
/// Not because the sweep owes a weak target anything — no per-object sweep
/// work is weak-specific; weak processing clears holders from mark bits before
/// the sweep. The block is walked because that processing asks the census
/// whether the target is a valid object, and a census hit records the block as
/// reached. That is conservative (it only costs the skip), so it is pinned
/// here rather than special-cased.
#[test]
fn a_dead_weak_target_block_is_walked_and_the_weak_ref_clears() {
    run_isolated(|| {
        let (target, base) =
            unsafe { plant_around(|| alloc_old_test_object(0).0 as usize, |t| *t) };
        let holder = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(target)));
        let mut root = ptr_bits(holder as usize);
        js_gc_register_global_root(&mut root as *mut u64 as i64);

        synchronous_full();

        assert!(
            !skipped(base),
            "weak processing's census lookup marks the target's block reached"
        );
        let deref = crate::weakref::js_weakref_deref(f64::from_bits(root));
        assert_eq!(
            deref.to_bits(),
            crate::value::TAG_UNDEFINED,
            "the WeakRef must be cleared"
        );
        assert!(
            !sabotage::last_skipped_block_bases().is_empty(),
            "the all-dead blocks around it are still skipped"
        );
    });
}

/// (b) An address-keyed side-table entry the dead-owner fan-out does not prune
/// — the legacy overflow table — makes object blocks obligations while it has
/// entries, and the per-object path drops the dead owner's entry.
#[test]
fn a_legacy_overflow_entry_keeps_object_blocks_on_the_per_object_path() {
    run_isolated(|| {
        let (owner, base) = unsafe { plant_around(|| alloc_old_test_object(0).0 as usize, |t| *t) };
        crate::state::state()
            .object_hot
            .overflow_fields
            .borrow_mut()
            .insert(owner, vec![crate::value::TAG_UNDEFINED]);

        synchronous_full();

        assert!(
            !skipped(base),
            "object blocks are obligations while the table is live"
        );
        assert!(
            !crate::state::state()
                .object_hot
                .overflow_fields
                .borrow()
                .contains_key(&owner),
            "the dead owner's overflow entry must be dropped"
        );
    });
}

/// Sabotage for the overflow case: forgetting obligations leaves a stale entry
/// that a new object at the recycled address would inherit.
#[test]
fn sabotaged_obligations_leave_a_stale_overflow_entry() {
    run_isolated(|| {
        let (owner, base) = unsafe { plant_around(|| alloc_old_test_object(0).0 as usize, |t| *t) };
        crate::state::state()
            .object_hot
            .overflow_fields
            .borrow_mut()
            .insert(owner, vec![crate::value::TAG_UNDEFINED]);

        {
            let _sabotage = sabotage::Guard::arm(sabotage::FORGET_OBLIGATIONS);
            synchronous_full();
        }

        assert!(
            skipped(base),
            "sabotage premise: the owner's block is skipped"
        );
        assert!(
            crate::state::state()
                .object_hot
                .overflow_fields
                .borrow()
                .contains_key(&owner),
            "and the stale entry survives — the assertion above fails"
        );
        crate::state::state()
            .object_hot
            .overflow_fields
            .borrow_mut()
            .clear();
    });
}

/// (b) A side-table entry the fan-out DOES prune — an element-shape record on
/// a dead array — is dropped even though its block is skipped.
#[test]
fn a_pruned_side_table_entry_is_dropped_from_a_skipped_block() {
    run_isolated(|| {
        let (arr, base) = unsafe { plant_around(|| alloc_old_test_array(2).0 as usize, |a| *a) };
        crate::array::test_seed_element_shape_record(arr);
        assert!(crate::array::test_element_shape_record_exists(arr));

        synchronous_full();

        assert!(
            skipped(base),
            "an element-shape record is not an obligation"
        );
        assert!(
            !crate::array::test_element_shape_record_exists(arr),
            "the dead owner's record must be pruned by the post-trace fan-out"
        );
    });
}

/// Only synchronous full cycles skip: a budgeted full resolves membership
/// through the page classifier and records nothing.
#[test]
fn a_budgeted_full_never_skips() {
    run_isolated(|| {
        let planted = unsafe { plant_plain(PLANT_BYTES) };
        let (base, _) = interior_planted_block(&planted);
        sabotage::record_skipped_block_bases(vec![base]);
        let before = block_skip_reclaimed_totals();

        let mut state =
            GcCycleState::new_full(GcTriggerSnapshot::capture(GcTriggerKind::OldGenBytes));
        state.set_progress_kind(GcProgressKind::NormalIncremental);
        let _ = state.run_to_completion();

        assert_eq!(
            block_skip_reclaimed_totals(),
            before,
            "a budgeted full must not skip a block"
        );
        assert!(
            sabotage::last_skipped_block_bases() == vec![base],
            "a budgeted sweep does not even consult the census"
        );
    });
}
