//! Promoted-block page runs: the page-object list for a whole-block promotion
//! is DESCRIBED (first/last header + count) and expanded only on demand.
//!
//! Two obligations are pinned here, in the same shape #7624 pins its own:
//!
//! * **behaviour** — a run must expand to exactly the header set the eager
//!   per-object list used to hold, and the tests must prove the run path was
//!   the one that ran (`pending_promoted_page_runs() > 0` before the read),
//!   not merely that a read returned something;
//! * **coverage** — every reader and remover of `OLD_GEN_PAGE_OBJECTS` must
//!   expand first, enumerated from the source so a reader added later cannot
//!   silently skip it.

use super::page_meta::{
    materialize_all_promoted_page_runs, register_promoted_page_run, unregister_old_block_pages,
};
use super::*;
use crate::gc::{GcHeader, GC_HEADER_SIZE, GC_TYPE_STRING};

const OBJ: usize = 64;

/// A real, parseable old-gen region: `count` back-to-back `GC_TYPE_STRING`
/// objects of `OBJ` bytes each, registered as an old block.
///
/// Real memory, not the synthetic addresses `tests.rs` uses — a run is
/// expanded by PARSING the block, so a fake address would not survive the hop.
struct PromotedRegion {
    _backing: Vec<u8>,
    base: usize,
    headers: Vec<usize>,
}

fn promoted_region(count: usize) -> PromotedRegion {
    let size = (count * OBJ).next_multiple_of(GENERATION_PAGE_SIZE);
    let mut backing = vec![0u8; size + GENERATION_PAGE_SIZE];
    // Page-align so the run's page geometry is the same one a promoted arena
    // block has.
    let base = (backing.as_mut_ptr() as usize).next_multiple_of(GENERATION_PAGE_SIZE);
    let headers: Vec<usize> = (0..count).map(|i| base + i * OBJ).collect();
    for &header in &headers {
        unsafe {
            *(header as *mut GcHeader) = GcHeader {
                obj_type: GC_TYPE_STRING,
                gc_flags: 0,
                _reserved: 0,
                size: OBJ as u32,
            };
        }
    }
    register_block_space(base, size, HeapGeneration::Old, HeapSpace::Old);
    PromotedRegion {
        _backing: backing,
        base,
        headers,
    }
}

/// Register every page the region spans as one promoted run, the way
/// `finish_in_place_promotion`'s walk does.
fn register_region_runs(region: &PromotedRegion) -> crate::fast_hash::PtrHashSet<usize> {
    let mut pages = crate::fast_hash::new_ptr_hash_set();
    let mut page_first: Vec<(usize, usize, usize, usize)> = Vec::new();
    for &header in &region.headers {
        let page = generation_page_for_addr(header);
        match page_first.last_mut() {
            Some(entry) if entry.0 == page => {
                entry.2 = header;
                entry.3 += 1;
            }
            _ => page_first.push((page, header, header, 1)),
        }
    }
    for (page, first, last, count) in page_first {
        register_promoted_page_run(page, first, last, count, count * OBJ);
        pages.insert(page);
    }
    pages
}

fn walk(pages: &crate::fast_hash::PtrHashSet<usize>) -> Vec<usize> {
    let mut seen = Vec::new();
    old_arena_walk_objects_on_pages(pages, |h| seen.push(h as usize));
    seen.sort_unstable();
    seen
}

#[test]
fn a_promoted_run_expands_to_exactly_the_eager_header_list() {
    super::tests::run_with_fresh_arenas(|| {
        let region = promoted_region(200);
        let pages = register_region_runs(&region);

        // SUBJECT-LIVE: the eager list must NOT have been built. Without this
        // the test would pass just as well against the per-object path it
        // replaces, which is the #7024 shape.
        assert!(
            pending_promoted_page_runs() > 0,
            "registration must DESCRIBE the run, not store the header list — \
             otherwise this test proves nothing about the new path"
        );

        let mut expected = region.headers.clone();
        expected.sort_unstable();
        assert_eq!(
            walk(&pages),
            expected,
            "an expanded run must be exactly the header set the per-object \
             list held; a short parse is a page whose objects a dirty scan \
             would never visit — a missed old->young edge"
        );
        assert_eq!(
            pending_promoted_page_runs(),
            0,
            "reading a page must consume its run, so the expansion is paid once"
        );
        // Idempotent: a second read must not duplicate.
        assert_eq!(walk(&pages), expected);
    });
}

#[test]
fn page_meta_accounting_matches_the_object_count_without_expanding() {
    super::tests::run_with_fresh_arenas(|| {
        let region = promoted_region(200);
        register_region_runs(&region);
        assert!(pending_promoted_page_runs() > 0);
        let summary = old_page_summary();
        assert_eq!(
            summary.object_count, 200,
            "the run records its count eagerly: defrag page selection reads \
             object_count/live_bytes and must not have to expand to see them"
        );
        assert_eq!(summary.live_object_count, 200);
    });
}

#[test]
fn a_full_cycle_expands_every_pending_run_before_it_can_sweep() {
    super::tests::run_with_fresh_arenas(|| {
        let region = promoted_region(200);
        let pages = register_region_runs(&region);
        assert!(pending_promoted_page_runs() > 0);

        // What `GcCycleState::new_full` calls. A run's bounds are addresses
        // captured at promotion; once the sweep can free objects inside the
        // block and `old_free` can refill the holes, those bounds stop being
        // object boundaries.
        materialize_all_promoted_page_runs();

        assert_eq!(pending_promoted_page_runs(), 0);
        let mut expected = region.headers.clone();
        expected.sort_unstable();
        assert_eq!(walk(&pages), expected);
    });
}

#[test]
fn dropping_a_block_discards_its_run_instead_of_parsing_freed_pages() {
    super::tests::run_with_fresh_arenas(|| {
        let region = promoted_region(200);
        let pages = register_region_runs(&region);
        let page_list: Vec<usize> = pages.iter().copied().collect();
        assert!(pending_promoted_page_runs() > 0);

        unregister_old_block_pages(&page_list);

        assert_eq!(
            pending_promoted_page_runs(),
            0,
            "the block is gone; a surviving run would have a later read parse \
             memory this arena no longer owns"
        );
        assert!(
            walk(&pages).is_empty(),
            "an unregistered page must stay empty — a run must not resurrect it"
        );
        drop(region);
    });
}

/// A TRACED promotion may NOT be described: its liveness lives in marks that
/// `clear_marks` destroys before any expansion could read them, so the eager
/// list is the only record. This pins that the two paths stayed separate.
#[test]
fn a_traced_promotion_still_stores_its_header_list() {
    super::tests::run_with_fresh_arenas(|| {
        let region = promoted_region(64);
        let page = generation_page_for_addr(region.base);
        let mut pages = crate::fast_hash::new_ptr_hash_set();
        pages.insert(page);

        super::page_meta::register_promoted_page_headers(
            page,
            &region.headers,
            region.headers.len() * OBJ,
        );

        assert_eq!(
            pending_promoted_page_runs(),
            0,
            "the Marked path must store, not describe — a described page would \
             re-parse to ALL walkable objects between the bounds, including the \
             unmarked ones the trace proved dead"
        );
        let mut expected = region.headers.clone();
        expected.sort_unstable();
        assert_eq!(walk(&pages), expected);
    });
}

/// The coverage half. Both tables are thread-locals private to `page_meta.rs`,
/// so the set of functions that touch `OLD_GEN_PAGE_OBJECTS` is enumerable from
/// the source: each must expand pending runs first, or carry a written reason.
///
/// A name in `EXEMPT` that no longer touches the table also fails, so a fix
/// cannot leave a stale exemption behind.
#[test]
fn every_page_object_reader_expands_promoted_runs() {
    const EXEMPT: &[(&str, &str)] = &[
        ("expand_promoted_run", "the expansion itself"),
        (
            "unregister_old_block_pages",
            "DISCARDS the run rather than expanding it — the backing block is \
             going away, so its bounds no longer address arena memory",
        ),
        (
            "old_arena_page_index_clear_for_tests",
            "DISCARDS, for the same reason it discards the deferral buffer",
        ),
        (
            "register_old_object_pages",
            "APPENDS a born-old object beyond the promoted run's bounds. The \
             two populations are disjoint and expansion unions them, so an \
             append needs no expansion; its `contains` dedup only guards \
             addresses it registered itself",
        ),
        (
            "flush_deferred_old_page_registrations_batch",
            "as register_old_object_pages — appends beyond the run",
        ),
        (
            "register_promoted_page_headers",
            "the TRACED promotion's eager path. It authors a page a described \
             run never covers (one promotion per page), and in the \
             two-blocks-share-a-page case the two populations are disjoint and \
             expansion unions them — the register_old_object_pages argument",
        ),
        (
            "next",
            "OldArenaPageObjectCursor::next. `new` expands every page it will \
             step, and the budgeted stepping window marks without promoting, \
             so no run can appear mid-walk — the same window argument #7624 \
             makes for the deferral buffer, and `next` already debug-asserts it",
        ),
    ];

    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/arena/page_meta.rs"),
    )
    .expect("page_meta.rs must be readable");

    let mut bodies: Vec<(String, String)> = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed
            .strip_prefix("pub(crate) fn ")
            .or_else(|| trimmed.strip_prefix("pub fn "))
            .or_else(|| trimmed.strip_prefix("fn "))
        {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            bodies.push((name, String::new()));
        }
        if let Some(last) = bodies.last_mut() {
            last.1.push_str(line);
            last.1.push('\n');
        }
    }

    let exempt_names: Vec<&str> = EXEMPT.iter().map(|(n, _)| *n).collect();
    let mut offenders = Vec::new();
    let mut touching = std::collections::BTreeSet::new();
    for (name, body) in &bodies {
        if !body.contains("OLD_GEN_PAGE_OBJECTS.with") {
            continue;
        }
        touching.insert(name.as_str());
        if body.contains("materialize_promoted_page_runs(")
            || body.contains("materialize_all_promoted_page_runs()")
        {
            continue;
        }
        if exempt_names.contains(&name.as_str()) {
            continue;
        }
        offenders.push(name.clone());
    }

    assert!(
        offenders.is_empty(),
        "these functions in arena/page_meta.rs read or mutate \
         OLD_GEN_PAGE_OBJECTS without first expanding pending promoted page \
         runs: {offenders:?}.\n\
         A promoted page's object list is DESCRIBED until someone asks for it, \
         so a reader that skips the expansion sees an EMPTY page — and the \
         dirty scan that reader feeds would then never visit those objects, \
         losing every old->young edge out of them. Call \
         materialize_promoted_page_runs(pages), or add the function to EXEMPT \
         with the argument for why the description cannot be observed there."
    );

    let stale: Vec<&str> = exempt_names
        .iter()
        .copied()
        .filter(|name| !touching.contains(name))
        .collect();
    assert!(
        stale.is_empty(),
        "these EXEMPT entries no longer touch OLD_GEN_PAGE_OBJECTS: {stale:?}. \
         Delete them — a stale exemption is an unexamined claim that would \
         cover a future function of the same name."
    );
}
