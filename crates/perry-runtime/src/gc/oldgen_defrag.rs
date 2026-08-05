//! Old-page defragmentation SELECTION: which old-gen pages are worth
//! evacuating, and the test/env gates around that choice. Split from
//! `oldgen.rs` (2000-line lint cap); the evacuation-policy decisions that
//! CONSUME a selection stay there.

#[derive(Default)]
pub(super) struct OldPageDefragSelection {
    pub(super) pages: crate::fast_hash::PtrHashSet<usize>,
    pub(super) page_order: Vec<usize>,
    pub(super) candidate_pages: usize,
    pub(super) selected_pages: usize,
    pub(super) selected_live_bytes: usize,
    pub(super) selected_reclaimable_bytes: usize,
    /// Page-granule bytes the selected pages would hand back once their
    /// movable live objects are evacuated: page size minus pinned bytes
    /// (selection skips pinned pages, so in practice the full granule).
    pub(super) selected_releasable_block_bytes: usize,
    pub(super) skipped_pinned_pages: usize,
}

#[inline]
pub(super) fn old_page_defrag_eligible(meta: crate::arena::OldPageMeta) -> bool {
    meta.allocated_bytes > 0 && meta.live_bytes > 0 && meta.dead_bytes > 0 && meta.pinned_bytes == 0
}

#[inline]
pub(super) fn old_page_defrag_skipped_for_pin(meta: crate::arena::OldPageMeta) -> bool {
    meta.allocated_bytes > 0 && meta.live_bytes > 0 && meta.dead_bytes > 0 && meta.pinned_bytes > 0
}

pub(super) fn select_old_page_defrag_pages_from_snapshot(
    snapshot: &[crate::arena::OldPageMeta],
    force: bool,
) -> OldPageDefragSelection {
    let mut selection = OldPageDefragSelection::default();
    let mut candidates = Vec::new();
    for &meta in snapshot {
        if old_page_defrag_skipped_for_pin(meta) {
            selection.skipped_pinned_pages = selection.skipped_pinned_pages.saturating_add(1);
            continue;
        }
        if !old_page_defrag_eligible(meta) {
            continue;
        }
        selection.candidate_pages = selection.candidate_pages.saturating_add(1);
        if force || meta.dead_bytes >= meta.live_bytes {
            candidates.push(meta);
        }
    }

    candidates.sort_unstable_by(|a, b| {
        let b_ratio = (b.dead_bytes as u128).saturating_mul(a.allocated_bytes as u128);
        let a_ratio = (a.dead_bytes as u128).saturating_mul(b.allocated_bytes as u128);
        b_ratio
            .cmp(&a_ratio)
            .then_with(|| a.live_bytes.cmp(&b.live_bytes))
            .then_with(|| a.page_base.cmp(&b.page_base))
    });

    for meta in candidates {
        let page = crate::arena::generation_page_for_addr(meta.page_base);
        if selection.pages.insert(page) {
            selection.page_order.push(page);
            selection.selected_pages = selection.selected_pages.saturating_add(1);
            selection.selected_live_bytes = selection
                .selected_live_bytes
                .saturating_add(meta.live_bytes);
            selection.selected_reclaimable_bytes = selection
                .selected_reclaimable_bytes
                .saturating_add(meta.dead_bytes);
            selection.selected_releasable_block_bytes =
                selection.selected_releasable_block_bytes.saturating_add(
                    (meta.page_end.saturating_sub(meta.page_base))
                        .saturating_sub(meta.pinned_bytes),
                );
        }
    }

    selection
}

// gh #6206 test hook: the defrag machinery's unit tests exercise the
// selection/copy/re-remember mechanics directly and must bypass the
// production off-gate below. Thread-local so parallel tests don't race.
#[cfg(test)]
thread_local! {
    pub(crate) static OLD_DEFRAG_TEST_OVERRIDE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

/// RAII enable for the defrag unit tests: forces the off-gate open on this
/// thread for the guard's lifetime.
#[cfg(test)]
pub(crate) struct OldDefragTestEnable;

#[cfg(test)]
impl OldDefragTestEnable {
    pub(crate) fn new() -> Self {
        OLD_DEFRAG_TEST_OVERRIDE.with(|c| c.set(Some(true)));
        OldDefragTestEnable
    }
}

#[cfg(test)]
impl Drop for OldDefragTestEnable {
    fn drop(&mut self) {
        OLD_DEFRAG_TEST_OVERRIDE.with(|c| c.set(None));
    }
}

fn old_page_defrag_enabled() -> bool {
    #[cfg(test)]
    if let Some(v) = OLD_DEFRAG_TEST_OVERRIDE.with(|c| c.get()) {
        return v;
    }
    use std::sync::OnceLock;
    static OPT_IN: OnceLock<bool> = OnceLock::new();
    *OPT_IN.get_or_init(|| {
        matches!(
            std::env::var("PERRY_GC_OLD_DEFRAG").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

pub(super) fn select_old_page_defrag_pages(force: bool) -> OldPageDefragSelection {
    // gh #6206: old-page defrag evacuation is OFF pending a rewrite-contract
    // fix. With defrag active, a reader can observe a pre-move address of a
    // defrag-moved old object long after the cycle (wild-pointer crash /
    // silently corrupt cached value); the reproducer corrupts 6/6 with defrag
    // enabled and is clean 6/6 with it disabled, on the same binary, while
    // every heap-payload slot (arrays in-length, object fields, Map entries)
    // verifies as correctly rewritten — the stale reference lives on a
    // non-heap path (address-keyed cache / IC / side table) the defrag
    // rewrite doesn't reach. Nursery evacuation and tenured promotion (the
    // reclaim-critical moving paths) are unaffected. Re-enable for
    // debugging/bisection with PERRY_GC_OLD_DEFRAG=1.
    if !old_page_defrag_enabled() {
        return OldPageDefragSelection::default();
    }
    let snapshot = crate::arena::old_page_meta_snapshot();
    select_old_page_defrag_pages_from_snapshot(&snapshot, force)
}
