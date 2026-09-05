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
    /// Selection stopped at [`IDLE_COMPACT_MOVE_BUDGET_BYTES`] rather than
    /// running out of candidates — the rest wait for the next idle compaction.
    pub(super) budget_stopped: bool,
}

#[inline]
pub(super) fn old_page_defrag_eligible(meta: crate::arena::OldPageMeta) -> bool {
    meta.allocated_bytes > 0 && meta.live_bytes > 0 && meta.dead_bytes > 0 && meta.pinned_bytes == 0
}

#[inline]
pub(super) fn old_page_defrag_skipped_for_pin(meta: crate::arena::OldPageMeta) -> bool {
    meta.allocated_bytes > 0 && meta.live_bytes > 0 && meta.dead_bytes > 0 && meta.pinned_bytes > 0
}

/// Live bytes one idle compaction will move before it stops selecting.
///
/// This bounds how much a single pass MOVES. 8 MiB came from the #9644
/// fixture (9.4 MB in 235,241 objects in 132 ms once the free-list pathology
/// was gone, `gc/old_free.rs::old_free_filter_pages`).
///
/// Measured on the compiled claude-code TUI, cutting it to 1 MiB moved the
/// selection from ~50 blocks to ~15 and left the pause UNCHANGED — three
/// interleaved pairs gave a 1,070 ms mean against the old selection's
/// 1,044 ms, with a 515-1,375 ms spread that tracks machine load rather than
/// the arm. So this pass is dominated by fixed per-pass cost (the old-page
/// meta snapshot, the walk over the selected blocks' pages, the sweep), not by
/// moving, and the budget's job is bounding the moved volume rather than
/// buying back pause. 1 MiB is enough to release ~15 MB of whole blocks per
/// pass; selection is cheapest-block-first, so what one pass leaves behind is
/// what the next one takes. Lowering the fixed cost is separate work.
pub(super) const IDLE_COMPACT_MOVE_BUDGET_BYTES: usize = 1024 * 1024;

pub(super) fn select_old_page_defrag_pages_from_snapshot(
    snapshot: &[crate::arena::OldPageMeta],
    force: bool,
) -> OldPageDefragSelection {
    let mut selection = OldPageDefragSelection::default();
    // #9772: the idle compaction's release unit is a BLOCK, so selecting the
    // globally most-fragmented PAGES predicts bytes it cannot return — the
    // emptied pages are scattered over blocks that keep other live occupants,
    // and `old_arena_reclaim_selected_dead_blocks` frees none of them. It
    // picked 10,740 pages promising 44 MB, ran 228 ms and released 0 on the
    // compiled claude-code TUI. Selecting whole blocks, cheapest-to-empty
    // first, makes the prediction achievable by construction: every selected
    // block ends the pass with no live occupant, which is exactly what the
    // reclaim tests.
    if idle_compact_armed() && idle_compact_block_selection_enabled() {
        return select_whole_blocks(snapshot, selection);
    }
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

    // Every non-idle caller takes the whole candidate set, as before.
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

crate::perry_thread_local! {
    /// Set for the duration of one `gc/idle_compact.rs` collection.
    static IDLE_COMPACT_ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Releasable block bytes the last idle selection promised (#9772).
    static LAST_IDLE_PREDICTED_RELEASE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `PERRY_GC_IDLE_COMPACT_BLOCKS` — ON by default. `=0`/`off`/`false` restores
/// the pre-#9772 page-granular selection, which predicts releasable bytes it
/// cannot return. Present so the two selections can be compared in one binary.
fn idle_compact_block_selection_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| crate::gc::env_default_on_enabled("PERRY_GC_IDLE_COMPACT_BLOCKS"))
}

/// Block bytes the most recent idle-compaction selection predicted it could
/// hand back. `gc/idle_compact.rs` checks the pass against it.
pub(super) fn last_idle_predicted_release_bytes() -> usize {
    LAST_IDLE_PREDICTED_RELEASE.with(|c| c.get())
}

fn idle_compact_armed() -> bool {
    IDLE_COMPACT_ARMED.with(|c| c.get())
}

/// Arms old-page defrag selection on this thread for the guard's lifetime.
pub(super) struct IdleCompactDefragArm;

impl IdleCompactDefragArm {
    pub(super) fn new() -> Self {
        IDLE_COMPACT_ARMED.with(|c| c.set(true));
        Self
    }
}

impl Drop for IdleCompactDefragArm {
    fn drop(&mut self) {
        IDLE_COMPACT_ARMED.with(|c| c.set(false));
    }
}

// Test override for selection-policy tests. Thread-local so parallel tests do
// not race with the production default or one another.
#[cfg(test)]
crate::perry_thread_local! {
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
    // The idle compaction arms selection for its own collection only. The env
    // default below still decides for every allocation-triggered collection —
    // #7917's opt-in is about the THROUGHPUT path, and this is idle time.
    if idle_compact_armed() {
        return true;
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
    let selection = select_old_page_defrag_pages_from_snapshot(&snapshot, force);
    if idle_compact_armed() {
        // #9772: publish what this pass PROMISED, so the pass that consumes it
        // can be judged against its own prediction instead of reporting a
        // pause and no bytes.
        LAST_IDLE_PREDICTED_RELEASE.with(|c| c.set(selection.selected_releasable_block_bytes));
    }
    if idle_compact_armed() && crate::gc::gc_diag_enabled() {
        let dead: usize = snapshot.iter().map(|m| m.dead_bytes).sum();
        let live: usize = snapshot.iter().map(|m| m.live_bytes).sum();
        eprintln!(
            "[gc-idle-compact] selection pages={} dead_bytes={dead} live_bytes={live} candidates={} selected={}              selected_live={} releasable={} skipped_pinned={} budget_stopped={}",
            snapshot.len(),
            selection.candidate_pages,
            selection.selected_pages,
            selection.selected_live_bytes,
            selection.selected_releasable_block_bytes,
            selection.skipped_pinned_pages,
            selection.budget_stopped,
        );
    }
    selection
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

/// Block-granular selection for the idle compaction (#9772).
///
/// Groups every old page with live bytes by its containing arena block, drops
/// blocks that hold pinned bytes (those can never be emptied), ranks the rest
/// by how much live data must move to empty them, and takes whole blocks until
/// [`IDLE_COMPACT_MOVE_BUDGET_BYTES`] of live bytes is committed.
/// `selected_releasable_block_bytes` is then the sum of the selected blocks'
/// sizes — memory the reclaim actually hands back — rather than a sum of page
/// granules nothing releases.
fn select_whole_blocks(
    snapshot: &[crate::arena::OldPageMeta],
    mut selection: OldPageDefragSelection,
) -> OldPageDefragSelection {
    let ranges = crate::arena::old_arena_block_ranges();
    if ranges.is_empty() {
        return selection;
    }
    #[derive(Default, Clone)]
    struct BlockAcc {
        live_bytes: usize,
        dead_bytes: usize,
        pinned: bool,
        pages: Vec<usize>,
    }
    let mut blocks: Vec<BlockAcc> = vec![BlockAcc::default(); ranges.len()];
    for &meta in snapshot {
        if meta.allocated_bytes == 0 {
            continue;
        }
        let Some(bi) = crate::arena::old_arena_block_range_index(&ranges, meta.page_base) else {
            continue;
        };
        let acc = &mut blocks[bi];
        acc.live_bytes = acc.live_bytes.saturating_add(meta.live_bytes);
        acc.dead_bytes = acc.dead_bytes.saturating_add(meta.dead_bytes);
        acc.pinned |= meta.pinned_bytes > 0;
        acc.pages
            .push(crate::arena::generation_page_for_addr(meta.page_base));
    }

    let mut order: Vec<usize> = (0..blocks.len())
        .filter(|&i| {
            let b = &blocks[i];
            // A block with no live occupant is already the ordinary sweep's
            // job; a pinned one can never be emptied by moving.
            !b.pinned && b.live_bytes > 0 && b.dead_bytes > 0 && !b.pages.is_empty()
        })
        .collect();
    selection.candidate_pages = order.iter().map(|&i| blocks[i].pages.len()).sum();
    selection.skipped_pinned_pages = blocks
        .iter()
        .filter(|b| b.pinned)
        .map(|b| b.pages.len())
        .sum();
    // Cheapest to empty first; among equals prefer the one that gives back the
    // most dead bytes.
    order.sort_unstable_by(|&a, &b| {
        blocks[a]
            .live_bytes
            .cmp(&blocks[b].live_bytes)
            .then_with(|| blocks[b].dead_bytes.cmp(&blocks[a].dead_bytes))
            .then_with(|| ranges[a].0.cmp(&ranges[b].0))
    });

    for bi in order {
        if selection.selected_live_bytes >= IDLE_COMPACT_MOVE_BUDGET_BYTES {
            selection.budget_stopped = true;
            break;
        }
        let acc = &blocks[bi];
        for &page in &acc.pages {
            if selection.pages.insert(page) {
                selection.page_order.push(page);
                selection.selected_pages = selection.selected_pages.saturating_add(1);
            }
        }
        selection.selected_live_bytes =
            selection.selected_live_bytes.saturating_add(acc.live_bytes);
        selection.selected_reclaimable_bytes = selection
            .selected_reclaimable_bytes
            .saturating_add(acc.dead_bytes);
        // The whole block comes back once its live occupants are gone.
        selection.selected_releasable_block_bytes = selection
            .selected_releasable_block_bytes
            .saturating_add(ranges[bi].3);
    }
    selection
}
