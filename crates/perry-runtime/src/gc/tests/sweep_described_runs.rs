//! #10182: a full collection no longer expands every described promoted page
//! run when it starts. The sweep expands a page's run only right before it
//! invalidates a dead header on that page, and a page on which every object
//! survives keeps its run.
//!
//! The population is real old-gen memory whose eager page index is replaced by
//! described runs, the way an untraced in-place promotion leaves a block. Each
//! case runs one synchronous full on a fresh thread and compares the page index
//! and the page accounting against the planted liveness. The sabotaged twin
//! invalidates dead headers without expanding their page first and shows the
//! page accounting keeps counting a freed object.

use super::super::*;
use super::support::*;
use crate::gc::trace::block_skip::sabotage;

fn run_isolated(test: fn()) {
    std::thread::spawn(move || {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        reset_global_roots();
        let _roots = ShadowAndGlobalRootResetGuard;
        test();
    })
    .join()
    .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

struct Planted {
    /// `(header, total size)` of every planted object, ascending.
    objects: Vec<(usize, usize)>,
    /// A page strictly inside the planted span.
    page: usize,
}

impl Planted {
    fn overlaps_page(&self, header: usize, size: usize) -> bool {
        let page_base = crate::arena::generation_page_base(self.page);
        header < page_base + crate::arena::GENERATION_PAGE_SIZE && header + size > page_base
    }

    fn on_page(&self) -> Vec<(usize, usize)> {
        self.objects
            .iter()
            .copied()
            .filter(|&(h, s)| self.overlaps_page(h, s))
            .collect()
    }
}

/// Allocate old strings over several pages, drop their eager page index and
/// DESCRIBE every page instead, exactly as `finish_in_place_promotion` does on
/// its untraced path.
unsafe fn plant_described() -> Planted {
    let mut objects = Vec::new();
    for _ in 0..600 {
        let user = crate::arena::arena_alloc_gc_old(56, 8, GC_TYPE_STRING) as usize;
        let (header, size) = old_test_header_and_size(user);
        objects.push((header as usize, size));
    }
    objects.sort_unstable();
    crate::arena::old_arena_page_index_clear_for_tests();
    let mut runs: Vec<(usize, usize, usize, usize, usize)> = Vec::new();
    for &(header, size) in &objects {
        let first = crate::arena::generation_page_for_addr(header);
        let last = crate::arena::generation_page_for_addr(header + size - 1);
        for page in first..=last {
            let base = crate::arena::generation_page_base(page);
            let overlap =
                (header + size).min(base + crate::arena::GENERATION_PAGE_SIZE) - header.max(base);
            match runs.last_mut() {
                Some(run) if run.0 == page => {
                    run.2 = header;
                    run.3 += 1;
                    run.4 += overlap;
                }
                _ => runs.push((page, header, header, 1, overlap)),
            }
        }
    }
    assert!(
        runs.len() >= 5,
        "premise: the population spans several pages"
    );
    for &(page, first, last, count, bytes) in &runs {
        crate::arena::register_promoted_page_run(page, first, last, count, bytes);
    }
    Planted {
        objects,
        page: runs[runs.len() / 2].0,
    }
}

fn root_all(headers: &[usize]) -> Vec<Box<u64>> {
    headers
        .iter()
        .map(|&header| {
            let mut slot = Box::new(string_bits(header + GC_HEADER_SIZE));
            js_gc_register_global_root(&mut *slot as *mut u64 as i64);
            slot
        })
        .collect()
}

fn page_headers(page: usize) -> Vec<usize> {
    let mut pages = crate::fast_hash::new_ptr_hash_set();
    pages.insert(page);
    let mut seen = Vec::new();
    crate::arena::old_arena_walk_objects_on_pages(&pages, |h| seen.push(h as usize));
    seen.sort_unstable();
    seen
}

fn synchronous_full() {
    let _ = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
        GcTriggerKind::OldGenBytes,
    ));
}

/// Every object overlapping the page survives: the full leaves the page's run
/// described, and the run still expands to exactly those objects.
#[test]
fn a_full_keeps_the_run_of_a_page_whose_objects_all_survive() {
    run_isolated(|| {
        let planted = unsafe { plant_described() };
        let live: Vec<usize> = planted.on_page().iter().map(|&(h, _)| h).collect();
        let _roots = root_all(&live);
        assert!(crate::arena::promoted_page_run_pending(planted.page));

        synchronous_full();

        assert!(
            planted
                .objects
                .iter()
                .any(|&(h, _)| unsafe { (*(h as *const GcHeader)).obj_type == 0 }),
            "premise: the full swept the unrooted objects on the other pages"
        );
        assert!(
            crate::arena::promoted_page_run_pending(planted.page),
            "a page on which nothing died is not reshaped, so its run must stay described"
        );
        assert_eq!(page_headers(planted.page), live);
    });
}

struct OneDead {
    objects_removed: usize,
    bytes_removed: usize,
    victim_bytes: usize,
    index: Vec<usize>,
    live: Vec<usize>,
}

fn one_dead_on_the_page(sabotaged: bool) -> OneDead {
    let planted = unsafe { plant_described() };
    let on_page = planted.on_page();
    // Kill one object in the middle of the page; root every other object
    // overlapping it.
    let victim = on_page[on_page.len() / 2];
    let live: Vec<usize> = on_page
        .iter()
        .map(|&(h, _)| h)
        .filter(|&h| h != victim.0)
        .collect();
    let _roots = root_all(&live);
    let before = crate::arena::old_page_meta_for_tests(planted.page).expect("page meta");
    {
        let _sabotage = sabotaged.then(|| sabotage::Guard::arm(sabotage::FORGET_RUN_EXPANSION));
        synchronous_full();
    }
    let after = crate::arena::old_page_meta_for_tests(planted.page).expect("page meta");
    assert_eq!(
        unsafe { (*(victim.0 as *const GcHeader)).obj_type },
        0,
        "premise: the unrooted object was swept"
    );
    assert!(!crate::arena::promoted_page_run_pending(planted.page));
    OneDead {
        objects_removed: before.object_count - after.object_count,
        bytes_removed: before.allocated_bytes - after.allocated_bytes,
        victim_bytes: victim.1,
        index: page_headers(planted.page),
        live,
    }
}

/// One object on the page dies: the sweep expands the page before it
/// invalidates the header, so the batched removal finds the object and the
/// page's accounting drops exactly that object.
#[test]
fn a_dead_object_on_a_described_page_leaves_the_page_accounting_exact() {
    run_isolated(|| {
        let r = one_dead_on_the_page(false);
        assert_eq!(
            r.objects_removed, 1,
            "the page must stop counting the freed object"
        );
        assert_eq!(r.bytes_removed, r.victim_bytes, "and exactly its bytes");
        assert_eq!(
            r.index, r.live,
            "the page index holds exactly the survivors"
        );
    });
}

// Debug builds detect the deliberately stale run before the release-only
// accounting observation below. Preserve and require that exact guard failure.
#[test]
#[cfg_attr(
    debug_assertions,
    should_panic(expected = "a promoted page run did not re-parse to the object count")
)]
fn sabotaged_expansion_order_keeps_counting_a_freed_object() {
    run_isolated(|| {
        let r = one_dead_on_the_page(true);
        assert_eq!(
            r.index, r.live,
            "the index still reads right: the harm is silent"
        );
        assert_eq!(
            r.objects_removed, 0,
            "expanded after the header was invalidated, the run no longer lists the \
             dead object, the removal misses it, and the page keeps counting it"
        );
    });
}
