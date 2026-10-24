/// A synthetic old-gen block plus `count` distinct in-range header addresses.
/// Registration never dereferences a header, so fabricated addresses exercise
/// the bookkeeping exactly as real ones do — and keep the test independent of
/// how many objects an allocator happens to fit in a page.
fn synthetic_old_headers(count: usize) -> Vec<usize> {
    let (base, min_size) = synthetic_old_block_range();
    let size = (count * 64)
        .next_multiple_of(GENERATION_PAGE_SIZE)
        .max(min_size);
    register_block_space(base, size, HeapGeneration::Old, HeapSpace::Old);
    (0..count).map(|i| base + i * 64).collect()
}

fn page_object_count(page: usize) -> usize {
    old_page_meta_for_tests(page)
        .map(|meta| meta.object_count)
        .unwrap_or(0)
}

#[test]
fn old_gen_birth_defers_its_page_registration() {
    run_with_fresh_arenas(|| {
        assert_eq!(deferred_old_page_registrations_len(), 0);
        let _old_ptr = arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
        assert!(
            deferred_old_page_registrations_len() > 0,
            "arena_alloc_gc_old must defer, not register eagerly — otherwise \
             the change is inert and every measurement of it is vacuous"
        );
    });
}

#[test]
fn cycle_start_flushes_deferred_registrations() {
    run_with_fresh_arenas(|| {
        let old_ptr = arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
        let (header_addr, total_size) = old_header_and_size(old_ptr);
        assert!(deferred_old_page_registrations_len() > 0);

        // The single flush point all three cycle constructors route through.
        old_pages_begin_gc_cycle();

        assert_eq!(
            deferred_old_page_registrations_len(),
            0,
            "old_pages_begin_gc_cycle must leave the deferral buffer empty"
        );
        let mut pages = crate::fast_hash::new_ptr_hash_set();
        for (page, _) in old_object_page_overlaps(header_addr, total_size) {
            pages.insert(page);
        }
        let mut visited = Vec::new();
        old_arena_walk_objects_on_pages(&pages, |header| visited.push(header as usize));
        assert_seen_headers("post-cycle-start walk", &visited, &[header_addr]);
    });
}

/// The other half of the cycle-constructor claim. `cycle_start_flushes_...`
/// proves `old_pages_begin_gc_cycle` flushes; this proves each of the three
/// constructors actually calls it, which is what makes "every collection begins
/// with a complete index" true rather than merely asserted in a comment.
#[test]
fn every_cycle_constructor_routes_through_the_flush_point() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gc");
    for (file, what) in [
        ("mod.rs", "non-moving / copying minor"),
        ("cycle.rs", "full mark-sweep (GcCycleState::new_full)"),
        ("policy.rs", "budgeted minor"),
    ] {
        let src = std::fs::read_to_string(root.join(file))
            .unwrap_or_else(|e| panic!("cannot read gc/{file}: {e}"));
        assert!(
            src.contains("old_pages_begin_gc_cycle()"),
            "the {what} constructor in gc/{file} no longer calls \
             old_pages_begin_gc_cycle(); deferred old-page registrations would \
             survive into the cycle unflushed (#7624)"
        );
    }
}

#[test]
fn deferral_buffer_flushes_at_its_size_cap() {
    run_with_fresh_arenas(|| {
        let headers = synthetic_old_headers(DEFERRED_OLD_PAGE_REGISTRATION_CAP);
        for &header in &headers {
            defer_old_object_page_registration(header, 64);
        }
        assert_eq!(
            deferred_old_page_registrations_len(),
            0,
            "the buffer must self-flush at DEFERRED_OLD_PAGE_REGISTRATION_CAP \
             so it cannot grow without bound between collections"
        );
        // And the cap flush is a real registration, not a discard.
        assert!(page_object_count(generation_page_for_addr(headers[0])) > 0);
    });
}

/// Each reader of the two tables, one obligation per assertion. Delete any one
/// `flush_deferred_old_page_registrations()` in `page_meta.rs` and exactly one
/// of these goes red.
#[test]
fn every_index_reader_flushes_before_reading() {
    run_with_fresh_arenas(|| {
        let headers = synthetic_old_headers(4);
        let page = generation_page_for_addr(headers[0]);
        let mut pages = crate::fast_hash::new_ptr_hash_set();
        pages.insert(page);

        // 1. old_arena_walk_objects_on_pages
        defer_old_object_page_registration(headers[0], 64);
        let mut visited = Vec::new();
        old_arena_walk_objects_on_pages(&pages, |h| visited.push(h as usize));
        assert_seen_headers("old_arena_walk_objects_on_pages", &visited, &[headers[0]]);
        assert_eq!(deferred_old_page_registrations_len(), 0);

        // 2. OldArenaPageObjectCursor — same index, incremental reader.
        defer_old_object_page_registration(headers[1], 64);
        let mut cursor = OldArenaPageObjectCursor::new(&pages);
        assert_eq!(
            deferred_old_page_registrations_len(),
            0,
            "OldArenaPageObjectCursor::new must flush before it starts stepping"
        );
        let mut seen = Vec::new();
        while let Some(h) = cursor.next() {
            seen.push(h);
        }
        assert_seen_headers("OldArenaPageObjectCursor", &seen, &headers[..2]);

        // 3. old_page_summary (OLD_GEN_PAGE_META)
        let before = old_page_summary().object_count;
        defer_old_object_page_registration(headers[2], 64);
        assert_eq!(
            old_page_summary().object_count,
            before + 1,
            "old_page_summary must flush; a mid-cycle promotion burst would \
             otherwise be missing from allocated_bytes/object_count"
        );

        // 4. old_page_meta_snapshot — drives defrag page selection.
        defer_old_object_page_registration(headers[3], 64);
        let snapshot = old_page_meta_snapshot();
        assert_eq!(deferred_old_page_registrations_len(), 0);
        let page_base = generation_page_base(page);
        let meta = snapshot
            .iter()
            .find(|m| m.page_base == page_base)
            .expect("snapshot should carry the page");
        assert_eq!(meta.object_count, 4);
    });
}

/// The remover obligation, and the one that is easiest to get wrong: a removal
/// that runs while the object is still only DEFERRED is a no-op, and the later
/// flush then puts the dead object back. Registration ORDER, not just eventual
/// visibility, is what the flush-before-remove rule buys.
#[test]
fn removing_a_deferred_object_does_not_resurrect_it() {
    run_with_fresh_arenas(|| {
        let headers = synthetic_old_headers(3);
        let mut pages = crate::fast_hash::new_ptr_hash_set();
        pages.insert(generation_page_for_addr(headers[0]));

        let visited_now = |pages: &crate::fast_hash::PtrHashSet<usize>| {
            let mut visited = Vec::new();
            old_arena_walk_objects_on_pages(pages, |h| visited.push(h as usize));
            visited
        };

        // 1. unregister_old_object_pages
        defer_old_object_page_registration(headers[0], 64);
        unregister_old_object_pages(headers[0], 64);
        assert!(
            !visited_now(&pages).contains(&headers[0]),
            "a deferred entry removed before its flush was resurrected by the \
             flush — unregister_old_object_pages must flush first (#7624)"
        );

        // 2. old_arena_page_index_remove_object
        defer_old_object_page_registration(headers[1], 64);
        old_arena_page_index_remove_object(headers[1], 64);
        assert!(
            !visited_now(&pages).contains(&headers[1]),
            "old_arena_page_index_remove_object must flush first (#7624)"
        );

        // 3. unregister_old_block_pages — the whole page goes away, and a
        //    later flush must not recreate it pointing into a recycled block.
        defer_old_object_page_registration(headers[2], 64);
        unregister_old_block_pages(&[generation_page_for_addr(headers[2])]);
        assert!(
            !visited_now(&pages).contains(&headers[2]),
            "unregister_old_block_pages must flush first (#7624)"
        );
    });
}

/// A copying minor's promotion owes its pages the live (and pinned) accounting
/// as well as the registration; both ride one deferred entry. The flush must
/// count every promoted object live on every page it overlaps — a
/// re-registered (hole-reused) address included, whose registration the dedup
/// skips — and leave plain old-gen births out of the live counts.
#[test]
fn promoted_deferral_applies_live_and_pinned_page_accounting() {
    run_with_fresh_arenas(|| {
        let headers = synthetic_old_headers(64);
        let page = generation_page_for_addr(headers[0]);
        register_old_object_pages(headers[0], 64);

        defer_promoted_old_object_page_registration(headers[0], 64, false);
        defer_promoted_old_object_page_registration(headers[1], 64, true);
        defer_promoted_old_object_page_registration(headers[2], 64, false);
        defer_old_object_page_registration(headers[3], 64);
        let meta = old_page_meta_for_tests(page).expect("page registered");
        assert_eq!(
            meta.object_count, 4,
            "registration is unchanged by the marks"
        );
        assert_eq!(meta.allocated_bytes, 4 * 64);
        assert_eq!(
            (meta.live_object_count, meta.live_bytes),
            (3, 3 * 64),
            "every promoted entry, the re-registered one included, counts live; \
             the plain birth does not"
        );
        assert_eq!((meta.pinned_object_count, meta.pinned_bytes), (1, 64));

        // An object spanning a page boundary counts live on both pages.
        let last = *headers.last().unwrap();
        let next_page_start =
            generation_page_base(generation_page_for_addr(last)) + GENERATION_PAGE_SIZE;
        let straddler = next_page_start - 32;
        defer_promoted_old_object_page_registration(straddler, 64, false);
        let lower = old_page_meta_for_tests(generation_page_for_addr(straddler)).unwrap();
        let upper = old_page_meta_for_tests(generation_page_for_addr(next_page_start));
        assert!(lower.live_bytes >= 32);
        assert_eq!(
            upper.map(|m| (m.live_object_count, m.live_bytes)),
            Some((1, 32))
        );
    });
}

/// The batched flush skips the dedup scan over entries added within the same
/// batch. That is only sound if it still catches the case the dedup exists for:
/// hole reuse handing back an address registered BEFORE the batch.
#[test]
fn batched_flush_matches_eager_registration() {
    run_with_fresh_arenas(|| {
        let headers = synthetic_old_headers(64);
        let page = generation_page_for_addr(headers[0]);

        // A pre-existing (pre-batch) registration, as hole reuse would leave.
        register_old_object_pages(headers[0], 64);
        assert_eq!(page_object_count(page), 1);

        // Now defer the whole set INCLUDING the already-registered address.
        for &header in &headers {
            defer_old_object_page_registration(header, 64);
        }
        let mut pages = crate::fast_hash::new_ptr_hash_set();
        pages.insert(page);
        let mut visited = Vec::new();
        old_arena_walk_objects_on_pages(&pages, |h| visited.push(h as usize));

        visited.sort_unstable();
        let mut expected = headers.clone();
        expected.sort_unstable();
        assert_eq!(
            visited, expected,
            "batched flush must produce exactly the eager index — no duplicate \
             for the re-registered address, no dropped entry"
        );
        assert_eq!(
            page_object_count(page),
            headers.len(),
            "page object_count must match the eager path's, counting the \
             re-registered address exactly once"
        );
    });
}

// ---------------------------------------------------------------------------
// #7912: `arena_alloc_gc_no_collect` — the "allocate without a collection
// point" entry point.
//
// Its whole value is a guarantee, not a speed: a caller holding raw heap
// pointers it has not rooted may allocate through it and, on a non-null
// return, KNOW nothing moved. That is only true if it REFUSES rather than
// reaching `gc_check_trigger()` when the open block cannot serve the request,
// so that is what these tests pin.
//
// ★ An earlier cut of this coverage asserted only "a small concat reached no
// trigger", which is vacuous: a small allocation into a block with room does
// not reach the trigger through `arena_alloc` either. Replacing the entry's
// body with the COLLECTING `arena_alloc` left that test green. These two
// drive the block to the point where the two entries must diverge.
// ---------------------------------------------------------------------------

#[test]
fn no_collect_alloc_refuses_a_full_block_instead_of_collecting() {
    run_with_fresh_arenas(|| {
        reset_gc_trigger_arena_probe();
        // Comfortably under LARGE_OBJECT_THRESHOLD_BYTES, so every request
        // takes the nursery bump path rather than old-gen birth.
        let chunk = LARGE_OBJECT_THRESHOLD_BYTES / 4;
        let bound = 8 * BLOCK_SIZE / chunk;
        let mut served = 0usize;
        let mut refused = false;
        for _ in 0..bound {
            if arena_alloc_gc_no_collect(chunk, 8, GC_TYPE_STRING).is_null() {
                refused = true;
                break;
            }
            served += 1;
        }
        assert!(
            refused,
            "the no-collect entry must REFUSE once the open block is full — it \
             served {served} chunks of {chunk} B without ever declining, which \
             means it reached the block-reservation/collection path it exists \
             to avoid"
        );
        assert!(
            served > 0,
            "test premise: the entry must serve from an open block at all"
        );
        assert_eq!(
            gc_trigger_arena_calls(),
            0,
            "the no-collect entry reached the allocation-point GC trigger; \
             every raw pointer a caller read before it is now potentially \
             from-space"
        );
        // A refusal is a refusal, not damage: the same request through the
        // collecting entry still works, which is the caller's fallback.
        assert!(
            !arena_alloc_gc(chunk, 8, GC_TYPE_STRING).is_null(),
            "the collecting fallback must still serve after a refusal"
        );
    });
}

#[test]
fn no_collect_alloc_refuses_an_oversized_request() {
    run_with_fresh_arenas(|| {
        reset_gc_trigger_arena_probe();
        // Old-gen birth walks page lists and can reserve, so it is outside the
        // contract even though it is not itself `gc_check_trigger`.
        assert!(
            arena_alloc_gc_no_collect(LARGE_OBJECT_THRESHOLD_BYTES * 2, 8, GC_TYPE_STRING)
                .is_null(),
            "a large-object request must be refused, not born tenured"
        );
        assert_eq!(gc_trigger_arena_calls(), 0);
    });
}
