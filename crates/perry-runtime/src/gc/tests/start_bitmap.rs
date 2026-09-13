//! #10182: the census answers arena membership from one object-start bitmap
//! per censused block (sorted start lists for oversized blocks).
//!
//! Every case plants a population on a fresh thread (so the arenas start
//! empty), builds the production census, and compares `contains` and
//! `enclosing_object` against an oracle derived from an independent arena walk
//! for every address the census covers. The sabotaged twin shifts every bitmap
//! probe by one alignment unit and shows the comparison notices.

use super::super::*;
use super::support::*;
use crate::gc::trace::start_bitmap_sabotage;

fn run_isolated(test: fn()) {
    std::thread::spawn(move || {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        reset_global_roots();
        let _roots = ShadowAndGlobalRootResetGuard;
        test();
    })
    .join()
    .expect("start-bitmap test thread must not panic");
}

/// Plant a population that exercises the bitmap's edges: runs of minimal
/// objects (consecutive bits across word boundaries), objects of assorted and
/// odd sizes, a 600 KB object inside a 1 MB block (an interior pointer deep in
/// it floors across many zero words), an oversized block (sorted start list),
/// nursery leaves and nursery objects.
unsafe fn plant_population() -> Vec<usize> {
    let mut planted = Vec::new();
    for i in 0..9000usize {
        let payload = match i % 7 {
            0 | 1 | 2 => 0,
            3 => 8,
            4 => 13,
            5 => 40,
            _ => 200 + (i % 5) * 64,
        };
        planted.push(crate::arena::arena_alloc_gc_old(payload, 8, GC_TYPE_STRING) as usize);
    }
    planted.push(crate::arena::arena_alloc_gc_old(600 * 1024, 8, GC_TYPE_STRING) as usize);
    for _ in 0..200 {
        planted.push(crate::arena::arena_alloc_gc_old(24, 8, GC_TYPE_STRING) as usize);
    }
    planted.push(crate::arena::arena_alloc_gc_old(
        2 * crate::arena::BLOCK_SIZE + 4096,
        8,
        GC_TYPE_STRING,
    ) as usize);
    for _ in 0..3000 {
        planted.push(young_leaf());
        planted.push(alloc_nursery_test_object(3).0 as usize);
    }
    planted
}

/// `(user pointer, total size)` of every walkable arena object, ascending.
fn oracle_objects() -> Vec<(usize, usize)> {
    let mut cursor = crate::arena::ArenaObjectCursor::new(crate::arena::ArenaWalkOrder::Address);
    let mut objects = Vec::new();
    while let Some((header, _)) = cursor.next() {
        let size = unsafe { (*(header as *const GcHeader)).size as usize };
        objects.push((header as usize + GC_HEADER_SIZE, size));
    }
    objects
}

#[derive(Default, Debug)]
struct Comparison {
    queries: usize,
    contains_mismatches: usize,
    enclosing_mismatches: usize,
    enclosing_hits: usize,
    /// An enclosing hit whose start lies at least one bitmap word (64 units)
    /// below the query: the floor search had to cross zero words.
    enclosing_hits_across_words: usize,
    sorted_blocks: usize,
    bitmap_blocks: usize,
}

/// Query every address (step 4, so both aligned and unaligned ones) from each
/// census block's base to 64 bytes past its walked extent.
fn compare_with_oracle(valid: &ValidPointerSet, objects: &[(usize, usize)]) -> Comparison {
    let starts: std::collections::HashSet<usize> = objects.iter().map(|&(u, _)| u).collect();
    let mut cmp = Comparison::default();
    for block in &valid.arena_blocks {
        if block.sorted {
            cmp.sorted_blocks += 1;
        } else {
            cmp.bitmap_blocks += 1;
        }
        let mut addr = block.base;
        while addr < block.base + block.extent + 64 {
            cmp.queries += 1;
            if valid.contains(&addr) != starts.contains(&addr) {
                cmp.contains_mismatches += 1;
            }
            let floor = objects.partition_point(|&(u, _)| u <= addr);
            let expected = floor
                .checked_sub(1)
                .map(|i| objects[i])
                .filter(|&(u, size)| addr >= u && addr < u + size.saturating_sub(GC_HEADER_SIZE))
                .map(|(u, _)| u);
            let got = valid.enclosing_object(addr);
            if got != expected {
                cmp.enclosing_mismatches += 1;
            }
            if let Some(start) = got {
                cmp.enclosing_hits += 1;
                if addr - start >= 64 * 8 + GC_HEADER_SIZE {
                    cmp.enclosing_hits_across_words += 1;
                }
            }
            addr += 4;
        }
    }
    cmp
}

#[test]
fn start_bitmap_membership_and_floors_match_an_independent_arena_walk() {
    run_isolated(|| {
        let _planted = unsafe { plant_population() };
        let objects = oracle_objects();
        let valid = ValidPointerSetBuilder::new().finish();

        assert_eq!(
            valid.arena_count,
            objects.len(),
            "the census must record exactly the walkable objects"
        );
        let cmp = compare_with_oracle(&valid, &objects);
        assert!(
            cmp.sorted_blocks >= 1,
            "premise: an oversized block: {cmp:?}"
        );
        assert!(
            cmp.bitmap_blocks >= 2,
            "premise: several bitmap blocks: {cmp:?}"
        );
        assert!(
            cmp.enclosing_hits_across_words > 0,
            "premise: some floor search must cross bitmap words: {cmp:?}"
        );
        assert_eq!(cmp.contains_mismatches, 0, "{cmp:?}");
        assert_eq!(cmp.enclosing_mismatches, 0, "{cmp:?}");
        // Index size is the point of the change: well under the 8 B/object the
        // start runs cost.
        assert!(
            valid.arena_index_bytes() < objects.len() * 8,
            "bitmap index {} B for {} objects",
            valid.arena_index_bytes(),
            objects.len()
        );
    });
}

#[test]
fn sabotaged_bitmap_probe_is_caught_by_the_oracle() {
    run_isolated(|| {
        let _planted = unsafe { plant_population() };
        let objects = oracle_objects();
        let valid = ValidPointerSetBuilder::new().finish();
        let cmp = {
            let _sabotage = start_bitmap_sabotage::Guard::arm();
            compare_with_oracle(&valid, &objects)
        };
        assert!(
            cmp.contains_mismatches > 0,
            "a bitmap probing the neighbouring unit must disagree with the walk: {cmp:?}"
        );
    });
}

/// A real full collection over the planted population keeps a rooted object in
/// a bitmap block and the rooted oversized object in the sorted block, and
/// reclaims a dead neighbour of the small one per object.
#[test]
fn a_full_collection_marks_through_the_bitmap_and_the_sorted_list() {
    run_isolated(|| {
        let planted = unsafe { plant_population() };
        let big = *planted
            .iter()
            .find(|&&u| {
                let size = unsafe { (*header_from_user_ptr(u as *const u8)).size as usize };
                size > crate::arena::BLOCK_SIZE
            })
            .expect("premise: an oversized object");
        let small = planted[4321];
        let mut small_root = string_bits(small);
        let mut big_root = string_bits(big);
        js_gc_register_global_root(&mut small_root as *mut u64 as i64);
        js_gc_register_global_root(&mut big_root as *mut u64 as i64);
        let _ = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
            GcTriggerKind::OldGenBytes,
        ));
        for user in [small, big] {
            let header = unsafe { header_from_user_ptr(user as *const u8) };
            assert_eq!(
                unsafe { (*header).obj_type },
                GC_TYPE_STRING,
                "rooted object {user:#x} must survive the full"
            );
            assert!(crate::arena::pointer_in_old_gen(user));
        }
        assert_eq!(
            unsafe { (*header_from_user_ptr(planted[4322] as *const u8)).obj_type },
            0,
            "the unrooted neighbour of the rooted small object is swept"
        );
    });
}
