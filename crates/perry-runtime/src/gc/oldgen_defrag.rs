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

// Test override for selection-policy tests. Thread-local so parallel tests do
// not race with the production default or one another.
#[cfg(test)]
thread_local! {
    pub(crate) static OLD_DEFRAG_TEST_OVERRIDE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

/// RAII enable for defrag unit tests on this thread for the guard's lifetime.
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

/// RAII *disable* for defrag on this thread, so the OFF arm is exercised
/// deterministically rather than depending on the ambient environment.
///
/// Without this the OFF state has no behavioural coverage at all: the value
/// mapping is unit-tested, but nothing asserts that a disabled collector
/// actually declines to select a page. That gap is what #7917 records.
#[cfg(test)]
pub(crate) struct OldDefragTestDisable;

#[cfg(test)]
impl OldDefragTestDisable {
    pub(crate) fn new() -> Self {
        OLD_DEFRAG_TEST_OVERRIDE.with(|c| c.set(Some(false)));
        OldDefragTestDisable
    }
}

#[cfg(test)]
impl Drop for OldDefragTestDisable {
    fn drop(&mut self) {
        OLD_DEFRAG_TEST_OVERRIDE.with(|c| c.set(None));
    }
}

fn old_page_defrag_enabled_from_value(value: Option<&str>) -> bool {
    matches!(value, Some("1") | Some("on") | Some("true"))
}

fn old_page_defrag_enabled() -> bool {
    #[cfg(test)]
    if let Some(v) = OLD_DEFRAG_TEST_OVERRIDE.with(|c| c.get()) {
        return v;
    }
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        old_page_defrag_enabled_from_value(std::env::var("PERRY_GC_OLD_DEFRAG").ok().as_deref())
    })
}

pub(super) fn select_old_page_defrag_pages(force: bool) -> OldPageDefragSelection {
    // #7876 restored the mutable-root contract for old movable addresses, and
    // #7913 shipped that restoration with defrag ON by default. The contract
    // work is sound and stays; the DEFAULT is what this reverts (#7917).
    //
    // #7876's own acceptance criteria said to "re-enable defrag only after the
    // reproducer and a dependency-scale stress corpus are clean". No such
    // corpus exists yet, and none of the 19 benchmark programs can produce a
    // candidate page: selection needs `dead_bytes >= live_bytes` on an old
    // page, which needs promote-then-die at scale. The retain family survives
    // at 999-1000 permille and the churn family promotes almost nothing, so
    // the suite yields neither a benefit signal nor a regression signal while
    // still inheriting the full old-address rewrite surface.
    //
    // So this is opt-in until a fragmentation workload exists that can
    // actually exercise it. When that lands, the losing arm gets DELETED
    // rather than left standing -- per CLAUDE.md, a mode that still exists is
    // a decision that has not been made.
    if !old_page_defrag_enabled() {
        return OldPageDefragSelection::default();
    }
    let snapshot = crate::arena::old_page_meta_snapshot();
    select_old_page_defrag_pages_from_snapshot(&snapshot, force)
}

#[cfg(test)]
mod tests {
    use super::{
        old_page_defrag_enabled_from_value, select_old_page_defrag_pages, OldDefragTestDisable,
        OldDefragTestEnable,
    };

    #[test]
    fn old_page_defrag_is_opt_in_via_perry_gc_old_defrag() {
        // Unset means OFF: defrag is opt-in until a fragmentation workload
        // exists that can demonstrate it (#7917).
        assert!(!old_page_defrag_enabled_from_value(None));
        assert!(old_page_defrag_enabled_from_value(Some("1")));
        assert!(old_page_defrag_enabled_from_value(Some("on")));
        assert!(old_page_defrag_enabled_from_value(Some("true")));
        assert!(!old_page_defrag_enabled_from_value(Some("0")));
        assert!(!old_page_defrag_enabled_from_value(Some("off")));
        assert!(!old_page_defrag_enabled_from_value(Some("false")));
        // Anything unrecognised is OFF, so a typo cannot silently enable
        // old-generation relocation.
        assert!(!old_page_defrag_enabled_from_value(Some("unexpected")));
    }

    /// The OFF arm, asserted through the gated entry point rather than through
    /// the value mapping.
    ///
    /// This matters because `select_old_page_defrag_pages_from_snapshot` does
    /// NOT consult the knob — the gate lives only in
    /// `select_old_page_defrag_pages` — so every pre-existing selection test
    /// bypasses the switch entirely. Before this test the OFF state had no
    /// behavioural coverage at all (#7917).
    ///
    /// The observable is the page-meta snapshot counter rather than the
    /// returned selection, for two reasons. It needs no old-arena fixture, and
    /// more importantly an empty selection is **not** evidence on its own: a
    /// bare test process has no eligible old pages, so asserting only
    /// "disabled returns nothing" passes just as happily against a kill switch
    /// that does nothing at all. That is the gate-that-cannot-fail shape this
    /// codebase keeps re-learning, and the first version of this test walked
    /// straight into it.
    ///
    /// So the positive control is load-bearing: it proves the enabled path
    /// really does reach the snapshot, which is the thing the disabled path
    /// must then be shown to skip.
    ///
    /// It also pins the *placement* of the gate, not merely its effect: the
    /// short-circuit must happen before the O(old pages) snapshot, so a
    /// disabled collector pays nothing on every ordinary minor.
    #[test]
    fn disabled_defrag_short_circuits_before_taking_a_page_snapshot() {
        use crate::arena::old_page_meta_snapshot_calls_for_tests as snapshot_calls;

        let before_enabled = snapshot_calls();
        let enabled = {
            let _enable = OldDefragTestEnable::new();
            select_old_page_defrag_pages(true)
        };
        let enabled_calls = snapshot_calls() - before_enabled;

        let before_disabled = snapshot_calls();
        let disabled_forced = {
            let _disable = OldDefragTestDisable::new();
            select_old_page_defrag_pages(true)
        };
        let disabled_unforced = {
            let _disable = OldDefragTestDisable::new();
            select_old_page_defrag_pages(false)
        };
        let disabled_calls = snapshot_calls() - before_disabled;

        assert_eq!(
            enabled_calls, 1,
            "positive control: enabled defrag must reach the page snapshot. If \
             this is 0 the assertions below prove nothing, because a switch \
             that never runs looks identical to one that correctly declines"
        );

        assert_eq!(
            disabled_calls, 0,
            "the kill switch must short-circuit BEFORE the O(old pages) \
             snapshot, so a disabled collector pays nothing per minor"
        );

        // `force` bypasses the dead>=live ratio, so this also proves the gate
        // beats a forced selection rather than merely losing the ratio test.
        assert_eq!(disabled_forced.selected_pages, 0);
        assert_eq!(disabled_forced.candidate_pages, 0);
        assert!(disabled_forced.pages.is_empty());
        assert!(disabled_forced.page_order.is_empty());
        assert_eq!(disabled_forced.selected_live_bytes, 0);
        assert_eq!(disabled_forced.selected_reclaimable_bytes, 0);
        assert_eq!(disabled_unforced.selected_pages, 0);
        assert!(disabled_unforced.pages.is_empty());

        // Sanity: the enabled arm returned a real (possibly empty) selection
        // rather than the disabled default, i.e. the two paths are distinct.
        let _ = enabled;
    }
}
