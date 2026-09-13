//! #10182: a full sweep applies old-generation page accounting once per page
//! (`arena::page_meta::sweep_tally`) instead of once per object.
//!
//! The case plants an old population whose liveness is known object by object
//! — live, pinned and dead objects of assorted sizes, objects spanning pages,
//! whole pages of dead objects inside live blocks, and more dead objects than
//! one page-index flush holds — runs one synchronous full, and compares every
//! planted page's accounting with an oracle computed from the plan: a page
//! any surviving object overlaps sums its live, pinned and dead overlaps; a
//! page no survivor overlaps is emptied by the page-index removal, which zeroes
//! its accounting. The sabotaged twin applies the tally after the page-index
//! flush instead of before and shows the comparison notices.

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
    .expect("sweep page-tally test thread must not panic");
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fate {
    Live,
    Pinned,
    Dead,
}

struct Planted {
    header: usize,
    size: usize,
    fate: Fate,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
struct PageExpect {
    live_bytes: usize,
    live_objects: usize,
    pinned_bytes: usize,
    pinned_objects: usize,
    dead_bytes: usize,
    dead_objects: usize,
}

const PAGE: usize = crate::arena::GENERATION_PAGE_SIZE;

/// Plant the population. Live objects are held by one rooted old array; pinned
/// objects are pinned. The plan ends with a run of dead objects covering whole
/// pages, so the last page the sweep visits holds no survivor.
unsafe fn plant() -> Vec<Planted> {
    const LIVE_SLOTS: u32 = 2048;
    let (holder, elements) = alloc_old_test_array(LIVE_SLOTS);
    let root: &'static mut u64 = Box::leak(Box::new(ptr_bits(holder as usize)));
    js_gc_register_global_root(root as *mut u64 as i64);
    let mut planted = vec![Planted {
        header: holder as usize - GC_HEADER_SIZE,
        size: old_test_header_and_size(holder as usize).1,
        fate: Fate::Live,
    }];
    let mut live_slot = 0u32;
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as usize
    };
    let mut allocated = 0usize;
    while allocated < 3 * crate::arena::BLOCK_SIZE {
        let roll = next();
        let payload = if roll % 29 == 0 {
            5000 + roll % 7000
        } else {
            8 + (roll >> 5) % 600
        };
        let fate = match (roll >> 12) % 64 {
            0 => Fate::Pinned,
            1..=7 if live_slot < LIVE_SLOTS => Fate::Live,
            _ => Fate::Dead,
        };
        let user = crate::arena::arena_alloc_gc_old(payload, 8, GC_TYPE_STRING) as usize;
        let size = old_test_header_and_size(user).1;
        match fate {
            Fate::Live => {
                *elements.add(live_slot as usize) = string_bits(user);
                live_slot += 1;
            }
            Fate::Pinned => crate::gc::pin_object(header_from_user_ptr(user as *const u8)),
            Fate::Dead => {}
        }
        planted.push(Planted {
            header: user - GC_HEADER_SIZE,
            size,
            fate,
        });
        allocated += size;
        // Every so often, a run of dead objects covering whole pages.
        if roll % 97 == 0 {
            let mut run = 0;
            while run < 3 * PAGE {
                let user = crate::arena::arena_alloc_gc_old(120, 8, GC_TYPE_STRING) as usize;
                let size = old_test_header_and_size(user).1;
                planted.push(Planted {
                    header: user - GC_HEADER_SIZE,
                    size,
                    fate: Fate::Dead,
                });
                run += size;
                allocated += size;
            }
        }
    }
    let mut run = 0;
    while run < 3 * PAGE {
        let user = crate::arena::arena_alloc_gc_old(120, 8, GC_TYPE_STRING) as usize;
        let size = old_test_header_and_size(user).1;
        planted.push(Planted {
            header: user - GC_HEADER_SIZE,
            size,
            fate: Fate::Dead,
        });
        run += size;
    }
    planted
}

fn oracle(planted: &[Planted]) -> std::collections::BTreeMap<usize, (PageExpect, bool)> {
    let mut pages: std::collections::BTreeMap<usize, (PageExpect, bool)> = Default::default();
    for object in planted {
        let end = object.header + object.size;
        let mut page_base = object.header & !(PAGE - 1);
        while page_base < end {
            let overlap = end.min(page_base + PAGE) - object.header.max(page_base);
            let (expect, survivor) = pages.entry(page_base).or_default();
            match object.fate {
                Fate::Live | Fate::Pinned => {
                    *survivor = true;
                    expect.live_bytes += overlap;
                    expect.live_objects += 1;
                    if object.fate == Fate::Pinned {
                        expect.pinned_bytes += overlap;
                        expect.pinned_objects += 1;
                    }
                }
                Fate::Dead => {
                    expect.dead_bytes += overlap;
                    expect.dead_objects += 1;
                }
            }
            page_base += PAGE;
        }
    }
    pages
}

#[derive(Debug, Default)]
struct Comparison {
    pages: usize,
    survivor_pages_with_dead: usize,
    emptied_pages: usize,
    multi_page_objects: usize,
    dead_objects: usize,
    mismatches: usize,
    first_mismatch: Option<(usize, PageExpect, PageExpect)>,
}

fn compare(planted: &[Planted]) -> Comparison {
    let meta: std::collections::HashMap<usize, crate::arena::OldPageMeta> =
        crate::arena::old_page_meta_snapshot()
            .into_iter()
            .map(|m| (m.page_base, m))
            .collect();
    let mut cmp = Comparison {
        multi_page_objects: planted
            .iter()
            .filter(|o| o.header / PAGE != (o.header + o.size - 1) / PAGE)
            .count(),
        dead_objects: planted.iter().filter(|o| o.fate == Fate::Dead).count(),
        ..Comparison::default()
    };
    for (page_base, (expect, survivor)) in oracle(planted) {
        cmp.pages += 1;
        let expect = if survivor {
            if expect.dead_objects > 0 {
                cmp.survivor_pages_with_dead += 1;
            }
            expect
        } else {
            cmp.emptied_pages += 1;
            PageExpect::default()
        };
        let got = meta
            .get(&page_base)
            .map_or(PageExpect::default(), |m| PageExpect {
                live_bytes: m.live_bytes,
                live_objects: m.live_object_count,
                pinned_bytes: m.pinned_bytes,
                pinned_objects: m.pinned_object_count,
                dead_bytes: m.dead_bytes,
                dead_objects: m.dead_object_count,
            });
        let eligible_ok = meta.get(&page_base).is_none_or(|m| {
            m.evacuation_eligible
                == (m.allocated_bytes > 0
                    && m.live_bytes > 0
                    && m.dead_bytes > 0
                    && m.pinned_bytes == 0)
        });
        if got != expect || !eligible_ok {
            cmp.mismatches += 1;
            cmp.first_mismatch.get_or_insert((page_base, expect, got));
        }
    }
    cmp
}

fn full() {
    let _ = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
        GcTriggerKind::OldGenBytes,
    ));
}

#[test]
fn full_sweep_page_accounting_matches_the_planted_liveness() {
    run_isolated(|| {
        let planted = unsafe { plant() };
        full();
        let cmp = compare(&planted);
        assert!(
            cmp.emptied_pages >= 3,
            "premise: pages with no survivor: {cmp:?}"
        );
        assert!(cmp.survivor_pages_with_dead >= 100, "premise: {cmp:?}");
        assert!(cmp.multi_page_objects >= 10, "premise: {cmp:?}");
        assert!(
            cmp.dead_objects > 4096,
            "premise: more than one page-index flush: {cmp:?}"
        );
        assert_eq!(cmp.mismatches, 0, "{cmp:?}");
    });
}

#[test]
fn sabotaged_tally_order_is_caught_by_the_oracle() {
    run_isolated(|| {
        let planted = unsafe { plant() };
        {
            let _sabotage = sabotage::Guard::arm(sabotage::FORGET_PAGE_TALLY_ORDER);
            full();
        }
        let cmp = compare(&planted);
        assert!(
            cmp.mismatches > 0,
            "a tally applied after the page-index flush must leave an emptied page's accounting: {cmp:?}"
        );
    });
}
