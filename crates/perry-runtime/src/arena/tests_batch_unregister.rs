//! The batched old-object page unregister must be indistinguishable from the
//! per-object remover it replaces on the full sweep's hot path.
//!
//! Both are driven over the SAME population in one fresh arena: remove a subset
//! one object at a time and snapshot, restore the subset, remove it again in
//! one batch and snapshot, then require the two snapshots to be identical --
//! page membership as walked, and every touched page's metadata. The subset
//! mixes page-spanning objects, fully emptied pages and partial pages, because
//! those are the three shapes the grouping and the per-page reset/refresh
//! have to get right.

use super::page_meta::{
    old_arena_walk_objects_on_pages, old_object_page_overlaps, old_page_meta_for_tests,
    register_old_object_pages, unregister_old_object_pages, unregister_old_objects_batch,
    OldPageMeta, GENERATION_PAGE_SIZE,
};
use super::tests::old_header_and_size as header_and_size;
use super::*;
use crate::gc::GC_TYPE_STRING;

type Snapshot = (Vec<usize>, Vec<(usize, Option<OldPageMeta>)>);

fn snapshot(pages: &[usize]) -> Snapshot {
    let mut set = crate::fast_hash::new_ptr_hash_set();
    for &page in pages {
        set.insert(page);
    }
    let mut members = Vec::new();
    old_arena_walk_objects_on_pages(&set, |header| members.push(header as usize));
    members.sort_unstable();
    let metas = pages
        .iter()
        .map(|&page| (page, old_page_meta_for_tests(page)))
        .collect();
    (members, metas)
}

#[test]
fn batched_unregister_leaves_the_same_index_and_metadata_as_per_object_removal() {
    super::tests::run_with_fresh_arenas(|| {
        // Many small objects per page, plus objects wider than a page.
        let mut objects = Vec::new();
        for i in 0..600 {
            let size = if i % 97 == 0 {
                GENERATION_PAGE_SIZE + 512
            } else {
                72
            };
            let ptr = arena_alloc_gc_old(size, 8, GC_TYPE_STRING) as usize;
            objects.push(header_and_size(ptr));
        }
        let mut pages: Vec<usize> = objects
            .iter()
            .flat_map(|&(h, s)| old_object_page_overlaps(h, s).into_iter().map(|(p, _)| p))
            .collect();
        pages.sort_unstable();
        pages.dedup();

        // Every third object, every page-spanning object, and EVERY object on
        // one chosen page, so at least one page empties completely.
        let emptied_page = pages[pages.len() / 2];
        let subset: Vec<(usize, usize)> = objects
            .iter()
            .enumerate()
            .filter(|&(i, &(h, s))| {
                i % 3 == 0
                    || s > GENERATION_PAGE_SIZE
                    || old_object_page_overlaps(h, s)
                        .iter()
                        .any(|&(p, _)| p == emptied_page)
            })
            .map(|(_, &o)| o)
            .collect();
        assert!(subset.len() > 200 && subset.len() < objects.len());

        let before = snapshot(&pages);

        for &(h, s) in &subset {
            unregister_old_object_pages(h, s);
        }
        let per_object = snapshot(&pages);
        assert_ne!(per_object, before, "the subset must actually be removed");

        for &(h, s) in &subset {
            register_old_object_pages(h, s);
        }
        assert_eq!(
            snapshot(&pages).0,
            before.0,
            "restoring the subset must restore page membership"
        );

        let mut scratch = Vec::new();
        unregister_old_objects_batch(&subset, &mut scratch);
        let batched = snapshot(&pages);

        assert_eq!(
            batched.0, per_object.0,
            "batched removal must leave exactly the headers per-object removal leaves"
        );
        for ((page, a), (_, b)) in per_object.1.iter().zip(batched.1.iter()) {
            let fields = |m: &Option<OldPageMeta>| {
                m.map(|m| {
                    (
                        m.allocated_bytes,
                        m.object_count,
                        m.live_bytes,
                        m.dead_bytes,
                        m.live_object_count,
                        m.dead_object_count,
                        m.evacuation_eligible,
                    )
                })
            };
            assert_eq!(fields(a), fields(b), "page {page:#x} metadata diverged");
        }
        assert!(
            per_object.1.iter().any(|(p, m)| *p == emptied_page
                && m.map_or(true, |m| m.object_count == 0 && m.allocated_bytes == 0)),
            "the chosen page must be emptied, or the reset path went unexercised"
        );
    });
}
