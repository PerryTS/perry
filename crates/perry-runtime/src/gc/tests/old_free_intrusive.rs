//! #11505: the old-gen hole free list is threaded through the holes
//! themselves — each listed hole's first payload word links to the next hole
//! of its size — with only per-size heads outside the heap.
//!
//! What that shape newly owes, each pinned below against a real sweep:
//!
//! * the chains stay exact across sweeps: every swept hole listed once on its
//!   own size's chain, reuse unlinks exactly the hole it hands out, and the
//!   next sweep's rebuild lists what is still dead and nothing that was
//!   reused;
//! * a block reset unlinks that block's holes from a chain it SHARES with a
//!   surviving block — the chain runs through the recycled bytes, so a missed
//!   unlink corrupts the survivors' chain, not just one entry — at every
//!   reclaim entry point;
//! * a defrag-time take unlinks the first hole off the excluded pages and
//!   keeps the ones it steps over;
//! * a hole too small to carry its link is not listed — writing the link
//!   would land on the next object's header.

use super::super::*;
use super::support::*;

/// Total 48 bytes: a direct-indexed head.
const SMALL_PAYLOAD: usize = 40;
/// Total above `OLD_FREE_SMALL_MAX`: a head in the size -> head map.
const LARGE_PAYLOAD: usize = 3000;

fn begin() -> std::sync::MutexGuard<'static, ()> {
    let isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    old_free_reset_for_test();
    crate::arena::old_pages_begin_gc_cycle();
    isolation
}

fn end() {
    old_free_reset_for_test();
    clear_marks();
    remembered_set_clear();
}

fn old_string(payload: usize) -> usize {
    crate::arena::arena_alloc_gc_old(payload, 8, GC_TYPE_STRING) as usize
}

fn mark(users: &[usize]) {
    for &user in users {
        let (header, _) = old_test_header_and_size(user);
        unsafe { (*header).gc_flags |= GC_FLAG_MARKED };
    }
}

fn full_sweep() {
    crate::arena::old_pages_begin_gc_cycle();
    let _ = sweep_with_age_bump_and_old_reclaim(false, true);
}

/// Every listed hole, after checking the list against the heap: each hole is
/// listed once, still reads as dead, sits on its own size's chain, and lies
/// in an old block whose `old_free_holes` bit is set; the byte counter is the
/// listed total.
fn listed_consistent() -> std::collections::HashSet<usize> {
    let mut seen = std::collections::HashSet::new();
    let mut bytes = 0usize;
    for (user, size) in old_free_listed_for_test() {
        assert!(seen.insert(user), "hole {user:#x} is listed twice");
        let (header, header_size) = old_test_header_and_size(user);
        assert_eq!(
            unsafe { (*header).obj_type },
            0,
            "a listed hole must still read as dead to every heap walker"
        );
        assert_eq!(header_size, size, "a hole must sit on its own size's chain");
        match crate::arena::old_arena_block_for_test(user) {
            Some((_, flagged)) => assert!(
                flagged,
                "hole {user:#x}: a listed hole's block must be flagged, or its reset skips the unlink"
            ),
            None => panic!(
                "hole {user:#x} is listed but lies in no old block: its block was recycled \
                 without unlinking it"
            ),
        }
        bytes += size;
    }
    assert_eq!(
        old_free_bytes(),
        bytes,
        "the byte counter follows the chains"
    );
    seen
}

#[test]
fn swept_holes_are_reused_and_the_chains_stay_exact_across_sweeps() {
    let _isolation = begin();

    // Every dead object sits between two live ones, so each block that
    // holds a hole also holds a live object and is swept hole by hole.
    let mut live = vec![old_string(SMALL_PAYLOAD)];
    let mut small_dead = Vec::new();
    let mut large_dead = Vec::new();
    for i in 0..24 {
        if i % 8 == 0 {
            large_dead.push(old_string(LARGE_PAYLOAD));
        } else {
            small_dead.push(old_string(SMALL_PAYLOAD));
        }
        live.push(old_string(SMALL_PAYLOAD));
    }
    let small_total = old_test_header_and_size(small_dead[0]).1;
    let large_total = old_test_header_and_size(large_dead[0]).1;
    assert!(
        small_total <= OLD_FREE_SMALL_MAX && large_total > OLD_FREE_SMALL_MAX,
        "premise: one size per head kind"
    );

    mark(&live);
    full_sweep();
    let listed = listed_consistent();
    for &dead in small_dead.iter().chain(&large_dead) {
        assert!(
            listed.contains(&dead),
            "swept hole {dead:#x} must be listed"
        );
    }
    for &kept in &live {
        assert!(
            !listed.contains(&kept),
            "a live object must never be listed"
        );
    }

    // Reuse hands out distinct listed holes of exactly the requested size
    // and unlinks exactly those.
    let bytes_before = old_free_bytes();
    let mut reused = Vec::new();
    for _ in 0..5 {
        let user = old_string(SMALL_PAYLOAD);
        assert!(
            small_dead.contains(&user),
            "a same-size allocation must land in a hole"
        );
        assert!(!reused.contains(&user), "a hole must be handed out once");
        reused.push(user);
    }
    let large_reused = old_string(LARGE_PAYLOAD);
    assert!(
        large_dead.contains(&large_reused),
        "the size -> head map must serve an exact-size large hole"
    );
    reused.push(large_reused);
    assert_eq!(
        old_free_bytes(),
        bytes_before - 5 * small_total - large_total
    );
    let after_reuse = listed_consistent();
    for user in &reused {
        assert!(
            !after_reuse.contains(user),
            "a reused hole must be unlinked"
        );
    }
    assert_eq!(after_reuse.len(), listed.len() - reused.len());

    // The next sweep rebuilds the list from the heap: what stayed dead is
    // listed again, what was reused (and kept alive) is not.
    mark(&live);
    mark(&reused);
    full_sweep();
    let relisted = listed_consistent();
    for &dead in small_dead.iter().chain(&large_dead) {
        assert_eq!(
            relisted.contains(&dead),
            !reused.contains(&dead),
            "hole {dead:#x}: listed exactly when it was not reused"
        );
    }

    // Draining a chain yields each of its holes once and leaves the other
    // chains alone.
    let listed_of = |total: usize| -> Vec<usize> {
        old_free_listed_for_test()
            .into_iter()
            .filter(|&(_, size)| size == total)
            .map(|(user, _)| user)
            .collect()
    };
    let small_listed = listed_of(small_total).len();
    let large_listed = listed_of(large_total);
    let mut drained = std::collections::HashSet::new();
    while let Some(user) = old_free_take_exact(small_total, None) {
        assert!(
            drained.insert(user),
            "a drained chain must not repeat a hole"
        );
    }
    assert_eq!(drained.len(), small_listed);
    assert!(listed_of(small_total).is_empty());
    assert_eq!(listed_of(large_total), large_listed);
    listed_consistent();

    end();
}

/// The old-block reclaim entry points; each reaches one of the reset sites
/// that must unlink a recycled block's holes.
#[derive(Clone, Copy, Debug)]
enum Reclaim {
    Full,
    Selected,
    StepwiseFull,
    StepwiseSelected,
}

fn reclaim_one_block(how: Reclaim, block_has_live: &[bool], dying_idx: usize) {
    let _heap_change = crate::gc::heap_generation::HeapChange::begin(
        crate::gc::heap_generation::HeapChangeKind::Sweep,
    );
    let mut selected = crate::fast_hash::new_ptr_hash_set();
    selected.insert(dying_idx);
    let snapshots = crate::arena::arena_block_snapshots();
    let mut stepwise = match how {
        Reclaim::Full => {
            let _ = crate::arena::old_arena_reclaim_dead_blocks(block_has_live);
            return;
        }
        Reclaim::Selected => {
            let _ = crate::arena::old_arena_reclaim_selected_dead_blocks(block_has_live, &selected);
            return;
        }
        Reclaim::StepwiseFull => {
            crate::arena::OldArenaReclaimDeadBlocksState::new_full(block_has_live, &snapshots)
        }
        Reclaim::StepwiseSelected => crate::arena::OldArenaReclaimDeadBlocksState::new_selected(
            block_has_live,
            &snapshots,
            &selected,
        ),
    };
    while !stepwise.step(1) {}
}

#[test]
fn a_reset_block_is_unlinked_from_a_chain_it_shares_with_a_surviving_block() {
    // A fresh thread per entry point: fresh arenas, so each run's first old
    // block is its own. Every entry point runs, so a failure names each site
    // that skipped its unlink.
    let failed: Vec<Reclaim> = [
        Reclaim::Full,
        Reclaim::Selected,
        Reclaim::StepwiseFull,
        Reclaim::StepwiseSelected,
    ]
    .into_iter()
    .filter(|&how| {
        std::thread::spawn(move || reset_block_is_unlinked(how))
            .join()
            .is_err()
    })
    .collect();
    assert!(
        failed.is_empty(),
        "reset sites that left holes linked: {failed:?}"
    );
}

fn reset_block_is_unlinked(how: Reclaim) {
    let _isolation = begin();

    // Live/dead pairs of one size across more than one old block, so the
    // size's single chain threads holes of several blocks.
    let mut live = Vec::new();
    let mut dead = Vec::new();
    while dead.len() < 8
        || crate::arena::old_arena_block_for_test(dead[0]).map(|(idx, _)| idx)
            == crate::arena::old_arena_block_for_test(*dead.last().unwrap()).map(|(idx, _)| idx)
    {
        live.push(old_string(SMALL_PAYLOAD));
        dead.push(old_string(SMALL_PAYLOAD));
    }
    live.push(old_string(SMALL_PAYLOAD));
    let total = old_test_header_and_size(dead[0]).1;
    let (dying_idx, _) = crate::arena::old_arena_block_for_test(dead[0]).unwrap();
    let block_of = |user: usize| crate::arena::old_arena_block_for_test(user).map(|(idx, _)| idx);
    let (in_dying, surviving): (Vec<usize>, Vec<usize>) = dead
        .iter()
        .partition(|&&user| block_of(user) == Some(dying_idx));
    assert!(!in_dying.is_empty() && !surviving.is_empty());

    mark(&live);
    full_sweep();
    let listed = listed_consistent();
    assert!(
        in_dying
            .iter()
            .chain(&surviving)
            .all(|user| listed.contains(user)),
        "premise: both blocks' holes are on the shared chain"
    );

    // Reclaim only the first block, as the full sweep does for a block whose
    // last live object died: its bytes are recycled wholesale.
    let mut block_has_live = vec![true; crate::arena::arena_block_count()];
    block_has_live[dying_idx] = false;
    reclaim_one_block(how, &block_has_live, dying_idx);
    assert_eq!(
        crate::arena::old_arena_block_for_test(in_dying[0]),
        None,
        "{how:?}: premise: the dying block was reclaimed"
    );

    let after = listed_consistent();
    for user in &in_dying {
        assert!(
            !after.contains(user),
            "{how:?}: hole {user:#x} in the reset block #{dying_idx} is still linked"
        );
    }
    for user in &surviving {
        assert!(
            after.contains(user),
            "{how:?}: hole {user:#x} in the surviving block must stay reachable"
        );
    }
    let mut drained = Vec::new();
    while let Some(user) = old_free_take_exact(total, None) {
        assert!(
            !in_dying.contains(&user),
            "a recycled block's hole was handed out"
        );
        drained.push(user);
    }
    assert!(
        surviving.iter().all(|user| drained.contains(user)),
        "the chain must still reach every surviving hole past the unlinked ones"
    );

    end();
}

#[test]
fn an_excluding_take_skips_and_keeps_holes_on_excluded_pages() {
    let _isolation = begin();

    // Live/dead pairs of one size until the holes span more than one
    // generation page, with at least two on the last page. The last hole
    // walked is the chain head, so excluding its page makes the take step
    // over the front of the chain.
    let page_of = |user: usize| crate::arena::generation_page_for_addr(user - GC_HEADER_SIZE);
    let mut live = Vec::new();
    let mut dead: Vec<usize> = Vec::new();
    while dead.len() < 8
        || page_of(dead[0]) == page_of(*dead.last().unwrap())
        || dead
            .iter()
            .filter(|&&user| page_of(user) == page_of(*dead.last().unwrap()))
            .count()
            < 2
    {
        live.push(old_string(SMALL_PAYLOAD));
        dead.push(old_string(SMALL_PAYLOAD));
    }
    live.push(old_string(SMALL_PAYLOAD));
    let total = old_test_header_and_size(dead[0]).1;

    mark(&live);
    full_sweep();
    let listed = listed_consistent();
    assert!(
        dead.iter().all(|user| listed.contains(user)),
        "premise: every hole is listed"
    );

    let mut excluded = crate::fast_hash::new_ptr_hash_set();
    excluded.insert(page_of(*dead.last().unwrap()));
    let on_excluded = |user: usize| {
        let header = user - GC_HEADER_SIZE;
        (page_of(user)..=crate::arena::generation_page_for_addr(header + total - 1))
            .any(|page| excluded.contains(&page))
    };
    let (skipped, usable): (Vec<usize>, Vec<usize>) =
        dead.iter().partition(|&&user| on_excluded(user));
    assert!(!skipped.is_empty() && !usable.is_empty());

    let mut taken = Vec::new();
    while let Some(user) = old_free_take_exact(total, Some(&excluded)) {
        assert!(
            !on_excluded(user),
            "an excluding take must never hand out a hole on an excluded page"
        );
        taken.push(user);
    }
    assert!(
        usable.iter().all(|user| taken.contains(user)),
        "every hole off the excluded pages must stay reachable past the skipped ones"
    );
    let after = listed_consistent();
    assert!(
        skipped.iter().all(|user| after.contains(user)),
        "a skipped hole must stay listed"
    );
    let mut rest = Vec::new();
    while let Some(user) = old_free_take_exact(total, None) {
        rest.push(user);
    }
    assert!(
        skipped.iter().all(|user| rest.contains(user)),
        "an unrestricted take must still reach the skipped holes"
    );

    end();
}

#[test]
fn a_hole_too_small_for_its_link_is_not_listed() {
    let _isolation = begin();

    let before = old_string(SMALL_PAYLOAD);
    let tiny = old_string(0);
    let neighbor = old_string(SMALL_PAYLOAD);
    let listed_dead = old_string(SMALL_PAYLOAD);
    let after = old_string(SMALL_PAYLOAD);
    let (tiny_header, tiny_total) = old_test_header_and_size(tiny);
    let (neighbor_header, neighbor_total) = old_test_header_and_size(neighbor);
    assert_eq!(tiny_total, GC_HEADER_SIZE, "premise: a header-only object");
    assert_eq!(
        neighbor_header as usize,
        tiny_header as usize + tiny_total,
        "premise: the neighbor's header is the word a link would take"
    );

    mark(&[before, neighbor, after]);
    full_sweep();

    assert_eq!(
        unsafe {
            (
                (*neighbor_header).obj_type,
                (*neighbor_header).size as usize,
            )
        },
        (GC_TYPE_STRING, neighbor_total),
        "listing the header-only hole would have written its link over this header"
    );
    let listed = listed_consistent();
    assert!(
        !listed.contains(&tiny),
        "a header-only hole has no room for its link"
    );
    assert!(
        listed.contains(&listed_dead),
        "the rebuild must step past the unlisted hole"
    );

    end();
}
