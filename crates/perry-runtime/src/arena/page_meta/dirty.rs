use super::*;

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
