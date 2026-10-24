/// Address ranges of the live old-generation blocks, as
/// `(base, end_exclusive, global_block_index, size)` sorted by base.
///
/// Old-gen memory is released a BLOCK at a time (`old_arena_reclaim_*`), so a
/// pass that wants its bytes back has to reason in blocks. `OldPageMeta` is
/// page-granular and carries no block identity, which is why #9772's selection
/// could predict 44 MB of "releasable block bytes" from page granules and
/// release nothing.
#[cfg(test)]
pub(crate) fn old_arena_block_ranges() -> Vec<(usize, usize, usize, usize)> {
    old_arena_ranges(false)
}

/// Movable backing only, using the allocation class stored by the arena.
/// Oversized object/byte extents stay excluded even if their geometry changes.
pub(crate) fn old_arena_movable_block_ranges() -> Vec<(usize, usize, usize, usize)> {
    old_arena_ranges(true)
}

fn old_arena_ranges(movable_only: bool) -> Vec<(usize, usize, usize, usize)> {
    let old_block_start = longlived_end();
    OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        let mut out: Vec<(usize, usize, usize, usize)> = arena
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(i, block)| {
                if block.data.is_null() || block.size == 0 {
                    return None;
                }
                if movable_only
                    && (matches!(block.extent_kind, super::region::Kind::CollectingHolder)
                        || matches!(block.extent_kind, super::region::Kind::LargeObject))
                {
                    return None;
                }
                let base = block.data as usize;
                Some((base, base + block.size, old_block_start + i, block.size))
            })
            .collect();
        crate::cold_sort::sort_by_u64_key(&mut out, |r| r.0 as u64);
        out
    })
}

/// Index into an old-arena range snapshot for the block containing
/// `addr`, or `None` when the address is not in a live old-gen block.
pub(crate) fn old_arena_block_range_index(
    ranges: &[(usize, usize, usize, usize)],
    addr: usize,
) -> Option<usize> {
    let idx = ranges.partition_point(|r| r.0 <= addr).checked_sub(1)?;
    (addr < ranges[idx].1).then_some(idx)
}

pub(crate) fn old_arena_source_blocks_for_pages(
    selected_pages: &crate::fast_hash::PtrHashSet<usize>,
) -> OldArenaSourceBlockSelection {
    let mut selection = OldArenaSourceBlockSelection::default();
    if selected_pages.is_empty() {
        return selection;
    }

    let old_block_start = longlived_end();
    OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for (i, block) in arena.blocks.iter().enumerate() {
            if block.data.is_null() || block.size == 0 {
                continue;
            }
            let base = block.data as usize;
            let first_page = generation_page_for_addr(base);
            let last_page = generation_page_for_addr(base + block.size - 1);
            if !(first_page..=last_page).any(|page| selected_pages.contains(&page)) {
                continue;
            }

            selection.block_indices.insert(old_block_start + i);
            for page in first_page..=last_page {
                selection.pages.insert(page);
            }
        }
    });
    selection
}

pub(crate) fn old_arena_walk_objects_on_pages(
    pages: &crate::fast_hash::PtrHashSet<usize>,
    mut callback: impl FnMut(*mut u8),
) -> usize {
    let headers = old_arena_headers_on_pages(pages);
    let count = headers.len();
    for header_addr in headers {
        callback(header_addr as *mut u8);
    }
    count
}

/// Snapshot the unique owners on these pages before any visitor mutates the
/// heap. Collectors may use its length to bound their own coverage bookkeeping.
pub(crate) fn old_arena_headers_on_pages(
    pages: &crate::fast_hash::PtrHashSet<usize>,
) -> Vec<usize> {
    if pages.is_empty() {
        return Vec::new();
    }

    // #7624 READER: promotions land in old-gen mid-cycle (a copying minor's
    // root scan runs before the remembered-set walk), so this cannot rely on
    // the cycle-start flush alone.
    flush_deferred_old_page_registrations();
    // RUN READER: a promoted page's list is built on demand.
    materialize_promoted_page_runs(pages.iter().copied());

    // The index contains an object once on EVERY page its body overlaps.
    // Visit selected pages in address order: if a header starts on or before
    // the preceding selected page, this object was already emitted there.
    // This removes the per-object exact set and its rehashes. The page index
    // remains the single authority, including for objects spanning clean gaps.
    let mut ordered_pages: Vec<_> = pages.iter().copied().collect();
    ordered_pages.sort_unstable();
    let mut headers = Vec::new();
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let index = index.borrow();
        let mut previous_page = None;
        for page in ordered_pages {
            if let Some(page_headers) = index.get(&page) {
                for header_addr in page_headers.iter(page) {
                    if previous_page
                        .is_none_or(|previous| generation_page_for_addr(header_addr) > previous)
                    {
                        headers.push(header_addr);
                    }
                }
            }
            previous_page = Some(page);
        }
    });

    headers
}

pub(crate) struct OldArenaPageObjectCursor {
    pages: Vec<usize>,
    page_cursor: usize,
    header_cursor: usize,
}

impl OldArenaPageObjectCursor {
    pub(crate) fn new(pages: &crate::fast_hash::PtrHashSet<usize>) -> Self {
        // #7624 READER: same obligation as `old_arena_walk_objects_on_pages`.
        // The cursor is stepped incrementally by the budgeted cycle, which
        // marks but never allocates into old-gen, so nothing can accumulate
        // between `new` and the last `next`; `next` debug-asserts that rather
        // than paying a thread-local check per object.
        flush_deferred_old_page_registrations();
        // RUN READER: same obligation, same window — the stepping window
        // promotes nothing, so expanding every page's run once here covers
        // every `next`.
        materialize_promoted_page_runs(pages.iter().copied());
        Self {
            pages: pages.iter().copied().collect(),
            page_cursor: 0,
            header_cursor: 0,
        }
    }

    pub(crate) fn next(&mut self) -> Option<usize> {
        // #7624: `new` flushed; the stepping window must not re-dirty the
        // buffer, or this reader would be walking a stale index. Debug-only so
        // the per-object read costs nothing in a shipped collector.
        debug_assert!(
            DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| buf.borrow().is_empty()),
            "an old-gen birth happened while a page-object cursor was stepping; \
             this reader is now walking a stale index (#7624)"
        );
        loop {
            let page = *self.pages.get(self.page_cursor)?;
            let header = OLD_GEN_PAGE_OBJECTS.with(|index| {
                index
                    .borrow()
                    .get(&page)
                    .and_then(|headers| headers.get(page, self.header_cursor))
            });
            if let Some(header) = header {
                self.header_cursor += 1;
                return Some(header);
            }
            self.page_cursor += 1;
            self.header_cursor = 0;
        }
    }

    pub(crate) fn is_done(&self) -> bool {
        self.page_cursor >= self.pages.len()
    }
}

pub(crate) fn old_arena_page_index_remove_object(header_addr: usize, total_size: usize) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    // #7624 REMOVER: see `unregister_old_block_pages`.
    flush_deferred_old_page_registrations();
    let overlaps = old_object_page_overlaps(header_addr, total_size);
    if overlaps.is_empty() {
        return;
    }
    // RUN REMOVER: a removal against a page whose list is still described
    // would silently no-op, and the later expansion would resurrect the
    // object. Expand first, then remove from the real list.
    materialize_promoted_page_runs(overlaps.iter().map(|&(page, _)| page));
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        for (page, _) in overlaps {
            let mut remove_page = false;
            if let Some(headers) = index.get_mut(&page) {
                headers.retain(page, |addr| addr != header_addr);
                remove_page = headers.is_empty();
            }
            if remove_page {
                index.remove(&page);
            }
        }
    });
}

/// Stamp `page`'s metadata dirty. Returns whether a metadata entry existed to
/// stamp: #7187 Phase B's "already dirty" cache may only remember a page whose
/// recording is complete in BOTH the modbuf and here, so it has to know.
pub(crate) fn old_page_mark_dirty(page: usize) -> bool {
    OLD_GEN_PAGE_META.with(|meta| {
        if let Some(page_meta) = meta.borrow_mut().get_mut(&page) {
            page_meta.dirty = true;
            true
        } else {
            false
        }
    })
}

pub(crate) fn old_page_clear_dirty(page: usize) {
    OLD_GEN_PAGE_META.with(|meta| {
        if let Some(page_meta) = meta.borrow_mut().get_mut(&page) {
            page_meta.dirty = false;
        }
    });
    // #7187 Phase B: one of the two places a page's `dirty` stamp can go false,
    // so one of the places the barrier's cached page can stop being a complete
    // recording. Invalidating here rather than at the callers covers the GC's
    // own clear loop and the tests that reach for this directly.
    crate::gc::dirty_page_cache_invalidate();
}

#[cfg(test)]
pub(crate) fn old_arena_page_index_clear_for_tests() {
    // Wiping page metadata makes real old-arena objects unclassifiable —
    // stand the #6179 differential verifier down for this thread's test.
    crate::gc::CLASSIFIER_VERIFY_SUPPRESSED.with(|c| c.set(true));
    // #7624: DISCARD rather than flush — a caller asking for an empty index
    // would get a repopulated one if the pending burst were folded in first.
    DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| buf.borrow_mut().clear());
    OLD_GEN_PAGE_OBJECTS.with(|index| index.borrow_mut().clear());
    // DISCARD pending runs for the same reason the deferral buffer is
    // discarded: a caller asking for an empty index must not get a
    // repopulated one at the next read.
    OLD_GEN_PAGE_PROMOTED_RUNS.with(|runs| runs.borrow_mut().clear());
    OLD_GEN_PAGE_PROMOTED_RUNS_NONEMPTY.with(|flag| flag.set(false));
}

#[cfg(test)]
pub(crate) fn old_page_meta_for_tests(page: usize) -> Option<OldPageMeta> {
    // #7624 READER: same rule as `old_page_summary`/`old_page_meta_snapshot`,
    // so a test that allocates and then inspects a page sees what eager
    // registration would have left.
    flush_deferred_old_page_registrations();
    let current_epoch = old_gen_page_dirty_epoch();
    OLD_GEN_PAGE_META.with(|meta| {
        meta.borrow()
            .get(&page)
            .copied()
            .map(|page_meta| page_meta.snapshot(page, current_epoch))
    })
}

/// `PERRY_GC_CENSUS`: estimated bytes held by the per-page side tables.
pub(crate) fn page_meta_census() -> Vec<crate::gc::census::SideTableRow> {
    use crate::gc::census::hash_table_bytes;
    let mut rows = Vec::new();
    with_page_generations(|m| {
        let m = m.borrow();
        rows.push((
            "arena.page_generations",
            m.len(),
            hash_table_bytes(
                m.capacity(),
                std::mem::size_of::<(usize, PageGenerationSlot)>(),
            ),
        ));
    });
    OLD_GEN_PAGE_OBJECTS.with(|m| {
        let m = m.borrow();
        let inner: usize = m.values().map(PageObjects::heap_bytes).sum();
        rows.push((
            "arena.old_gen_page_objects",
            m.len(),
            m.allocated_bytes() + inner,
        ));
    });
    OLD_GEN_PAGE_META.with(|m| {
        let m = m.borrow();
        rows.push(("arena.old_gen_page_meta", m.len(), m.allocated_bytes()));
    });
    OLD_GEN_PAGE_PROMOTED_RUNS.with(|m| {
        let m = m.borrow();
        rows.push((
            "arena.old_gen_page_promoted_runs",
            m.len(),
            hash_table_bytes(
                m.capacity(),
                std::mem::size_of::<(usize, PromotedPageRun)>(),
            ),
        ));
    });
    rows
}
