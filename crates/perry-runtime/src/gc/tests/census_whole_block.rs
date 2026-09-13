//! #10182: an unbudgeted census walks each arena block in one pass
//! (`ValidPointerSetBuilder::census_whole_block`) instead of one cursor call
//! per object. It must build exactly the set the per-object walk builds.
//!
//! The population mixes everything the census records per object: plain
//! strings of odd sizes across bitmap words, an oversized block (sorted start
//! list), nursery objects stamped tenured (tenured-nursery bytes), and every
//! per-object obligation the block-skip sweep reads — a promise and a Set
//! (finalize hooks), a pinned object, an already-marked header, an array with
//! raw-f64 layout bits — plus an invalidated header the walk must step over.
//! A census built with a small work budget takes the per-object path; the
//! unbudgeted one takes the whole-block path; every recorded fact is compared.
//! The sabotaged twin drops the whole-block walk's start bits.

use super::super::*;
use super::support::*;
use crate::gc::trace::whole_block_census_sabotage;

fn run_isolated(test: fn()) {
    std::thread::spawn(move || {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        reset_global_roots();
        let _roots = ShadowAndGlobalRootResetGuard;
        test();
    })
    .join()
    .expect("whole-block census test thread must not panic");
}

struct Population {
    marked: usize,
}

unsafe fn plant() -> Population {
    for i in 0..40_000usize {
        let payload = [0usize, 8, 13, 40, 200][i % 5];
        crate::arena::arena_alloc_gc_old(payload, 8, GC_TYPE_STRING);
    }
    let _promise = alloc_old_test_promise();
    let (_set, _elements, _layout) = alloc_old_test_set(4);
    let pinned = crate::arena::arena_alloc_gc_old(24, 8, GC_TYPE_STRING) as usize;
    crate::gc::pin_object(header_from_user_ptr(pinned as *const u8) as *mut GcHeader);
    let marked = crate::arena::arena_alloc_gc_old(24, 8, GC_TYPE_STRING) as usize;
    (*(header_from_user_ptr(marked as *const u8) as *mut GcHeader)).gc_flags |= GC_FLAG_MARKED;
    let (raw_array, _) = alloc_old_test_array(4);
    (*(header_from_user_ptr(raw_array as *const u8) as *mut GcHeader))._reserved |=
        GC_ARRAY_RAW_F64_LAYOUT;
    let hole = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    (*(header_from_user_ptr(hole as *const u8) as *mut GcHeader)).obj_type = 0;
    for _ in 0..3000usize {
        crate::arena::arena_alloc_gc_old(16, 8, GC_TYPE_STRING);
    }
    crate::arena::arena_alloc_gc_old(2 * crate::arena::BLOCK_SIZE + 4096, 8, GC_TYPE_STRING);
    for i in 0..2000usize {
        let user = young_leaf();
        if i % 3 == 0 {
            (*(header_from_user_ptr(user as *const u8) as *mut GcHeader)).gc_flags |=
                GC_FLAG_TENURED;
        }
        alloc_nursery_test_object(2);
    }
    Population { marked }
}

fn stepped_census() -> ValidPointerSet {
    let mut builder = ValidPointerSetBuilder::new();
    while !builder.step(7) {}
    builder.finish()
}

#[derive(Debug, Default)]
struct Diff {
    blocks_compared: usize,
    sorted_blocks: usize,
    obligation_blocks: usize,
    mismatches: Vec<String>,
}

fn compare(stepped: &ValidPointerSet, whole: &ValidPointerSet) -> Diff {
    let mut diff = Diff::default();
    let mut note = |what: String| diff.mismatches.push(what);
    if stepped.arena_count != whole.arena_count {
        note(format!(
            "arena_count {} vs {}",
            stepped.arena_count, whole.arena_count
        ));
    }
    if (stepped.range_min, stepped.range_max) != (whole.range_min, whole.range_max) {
        note("pointer range".into());
    }
    if stepped.tenured_nursery_bytes() != whole.tenured_nursery_bytes() {
        note(format!(
            "tenured nursery bytes {} vs {}",
            stepped.tenured_nursery_bytes(),
            whole.tenured_nursery_bytes()
        ));
    }
    if stepped.arena_block_bases != whole.arena_block_bases {
        note("block fences".into());
    }
    if stepped.start_bitmap_chunks != whole.start_bitmap_chunks {
        note("start bitmap contents".into());
    }
    if stepped.large_starts != whole.large_starts {
        note("sorted start lists".into());
    }
    for (a, b) in stepped.arena_blocks.iter().zip(&whole.arena_blocks) {
        if (a.base, a.extent, a.block_idx, a.first, a.len, a.sorted)
            != (b.base, b.extent, b.block_idx, b.first, b.len, b.sorted)
        {
            note(format!("census block {:#x}", a.base));
        }
    }
    for block_idx in 0..crate::arena::arena_block_count() {
        let (a, b) = (
            stepped.block_census.block(block_idx),
            whole.block_census.block(block_idx),
        );
        match (a, b) {
            (None, None) => {}
            (Some(a), Some(b)) => {
                diff.blocks_compared += 1;
                if a.obligation {
                    diff.obligation_blocks += 1;
                }
                if (a.data, a.end, a.objects, a.bytes, a.obligation, a.premarked)
                    != (b.data, b.end, b.objects, b.bytes, b.obligation, b.premarked)
                {
                    diff.mismatches
                        .push(format!("block facts {block_idx}: {a:?} vs {b:?}"));
                }
            }
            _ => diff
                .mismatches
                .push(format!("block {block_idx} censused by one walk only")),
        }
    }
    diff.sorted_blocks = whole.arena_blocks.iter().filter(|b| b.sorted).count();
    diff
}

#[test]
fn the_whole_block_census_records_exactly_what_the_per_object_census_records() {
    run_isolated(|| {
        let population = unsafe { plant() };
        let stepped = stepped_census();
        let whole = ValidPointerSetBuilder::new().finish();
        let diff = compare(&stepped, &whole);
        assert!(
            diff.blocks_compared >= 4,
            "premise: several census blocks: {diff:?}"
        );
        assert!(
            diff.sorted_blocks >= 1,
            "premise: an oversized block: {diff:?}"
        );
        assert!(
            diff.obligation_blocks >= 1,
            "premise: the obligation objects were censused: {diff:?}"
        );
        assert!(
            stepped.tenured_nursery_bytes() > 0,
            "premise: tenured nursery bytes were recorded"
        );
        assert!(diff.mismatches.is_empty(), "{:?}", diff.mismatches);
        unsafe {
            (*(header_from_user_ptr(population.marked as *const u8) as *mut GcHeader)).gc_flags &=
                !GC_FLAG_MARKED;
        }
    });
}

#[test]
fn sabotaged_whole_block_census_is_caught_by_the_comparison() {
    run_isolated(|| {
        let population = unsafe { plant() };
        let stepped = stepped_census();
        let whole = {
            let _sabotage = whole_block_census_sabotage::Guard::arm();
            ValidPointerSetBuilder::new().finish()
        };
        let diff = compare(&stepped, &whole);
        assert!(
            diff.mismatches.iter().any(|m| m.contains("bitmap")),
            "a whole-block walk that drops start bits must disagree: {:?}",
            diff.mismatches
        );
        unsafe {
            (*(header_from_user_ptr(population.marked as *const u8) as *mut GcHeader)).gc_flags &=
                !GC_FLAG_MARKED;
        }
    });
}
