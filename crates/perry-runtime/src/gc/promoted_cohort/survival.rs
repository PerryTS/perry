//! What a promoted-cohort full's own mark says about the blocks the nursery
//! minor at the same safepoint promoted (#10241).
//!
//! # The defect this answers
//!
//! A minor promotes in place, and may skip its trace, on the strength of the
//! PREVIOUS minor's young-survival ratio. An untraced run re-measures only when
//! its byte budget runs out, and every full resets that budget
//! (`note_full_collection_reclaimed_old_gen`). A cohort full every ~`live`
//! promoted bytes therefore keeps a workload that turned from building a live
//! set to churning on the untraced path indefinitely: nothing measures the
//! churn, every minor promotes it, and every cohort full marks the whole live
//! set to reclaim it (`14_grow_then_churn`: 13 cohort fulls, 0 copied objects,
//! wall +38 %).
//!
//! # Why the full's survival alone is not the answer
//!
//! The full measures exactly: of the bytes the last minor promoted, how many
//! are still reachable. On the probe that is 4‰. But a parse loop measures
//! 250–500‰ there (`records_array_20m:parse` 500, `records_array_8m:scan` 250):
//! each tree's top-level array is born old, the tree's young records stay
//! reachable through that array's remembered slots, and so every minor promotes
//! the dead previous tree(s) with the live current one. Feeding that 500‰ to
//! the predictor makes the next minor evacuate — and the evacuating minor
//! copies both trees (~95 ms on 20m), because to a minor the dead tree is live:
//! its parent is old, and a minor treats every old object as live.
//!
//! The predictor describes what a MINOR measures, so the full's figure may
//! replace it only when the two provably coincide. They differ by exactly the
//! young bytes a minor reaches through a remembered parent the full found dead.
//!
//! # The discriminator
//!
//! The minor notes its remembered set as its own dirty scan sees it (the dirty
//! old pages; `note_minor_remembered_parents`). After the full's mark and before
//! its sweep, every object on those pages that the mark left unmarked has its
//! slots on those pages read, exactly the slots the minor's dirty scan visits:
//! if one refers into the blocks the minor promoted, a minor would have found
//! young bytes live that the full found dead, and the measurement is not the
//! minor's (`MinorView::DeadParent`). If none does, a minor's reachability over
//! that young generation equals the full's — same safepoint, same roots, and
//! every old parent that could root a young object was itself proven live — so
//! the survival is exactly the ratio a traced minor would have measured
//! (`MinorView::Exact`). Parents the remembered set names by header rather than
//! by page (external slot pages, the fallback set) are not re-derived, so any
//! such entry makes the view `Unverifiable`.
//!
//! On a parse loop the dead previous tree's top array is such a parent — and
//! its dirty pages are exactly the slots written since the previous minor, so
//! the first slot read hits. On the probe the only remembered parent is the
//! 32-slot ring, which is live.
//!
//! # Exact or nothing
//!
//! The blocks are the ones the minor's promotion walk recorded for census
//! adoption (`trace::adopt_census`), keyed by data address with the bump extent
//! and header bytes they held at promotion. Each is accounted when the full's
//! sweep either walks it whole at the same extent (its live bytes are the
//! sweep's own `arena_live_bytes` delta) or reclaims it unwalked as a dead block
//! (live 0). A block the sweep reaches any other way, at another extent, or not
//! at all leaves the measurement unset.

use super::super::*;
use std::cell::RefCell;

/// Whether the full's survival is the survival a minor would have measured.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::gc) enum MinorView {
    /// No sweep-start check ran.
    Unchecked,
    /// No remembered parent of the minor that the full found dead refers into
    /// the promoted blocks.
    Exact,
    /// A dead remembered parent refers into the promoted blocks.
    DeadParent,
    /// The minor's remembered set was not noted, or names parents by header.
    Unverifiable,
}

impl MinorView {
    pub(in crate::gc) fn as_str(self) -> &'static str {
        match self {
            Self::Unchecked => "unchecked",
            Self::Exact => "exact",
            Self::DeadParent => "dead_parent",
            Self::Unverifiable => "unverifiable",
        }
    }
}

/// The measurement a probe produced.
pub(in crate::gc) struct PromotedSurvival {
    pub(in crate::gc) blocks: usize,
    pub(in crate::gc) promoted_bytes: usize,
    pub(in crate::gc) live_bytes: usize,
    pub(in crate::gc) minor_view: MinorView,
}

impl PromotedSurvival {
    pub(in crate::gc) fn permille(&self) -> Option<u64> {
        survival_permille(self.promoted_bytes, self.live_bytes)
    }
}

pub(in crate::gc) fn survival_permille(promoted_bytes: usize, live_bytes: usize) -> Option<u64> {
    (promoted_bytes > 0).then(|| {
        (live_bytes as u64)
            .saturating_mul(1000)
            .checked_div(promoted_bytes as u64)
            .unwrap_or(0)
            .min(1000)
    })
}

/// The minor's remembered set, as page keys: nothing here is an address the
/// full dereferences. Kept only when the remembered set names no parent by
/// header, so its dirty pages are exactly its dirty old pages.
pub(in crate::gc) struct RememberedParents {
    dirty_old_pages: crate::fast_hash::PtrHashSet<usize>,
}

struct SurvivalProbe {
    /// Recorded blocks not yet accounted: data address -> (extent, bytes).
    pending: crate::fast_hash::PtrHashMap<usize, (usize, u64)>,
    /// `(data, data + extent)` of every recorded block, sorted.
    ranges: Vec<(usize, usize)>,
    parents: Option<RememberedParents>,
    minor_view: MinorView,
    blocks: usize,
    promoted_bytes: u64,
    live_bytes: u64,
    exact: bool,
}

crate::perry_thread_local! {
    /// The remembered set of the minor recording for this safepoint's cohort
    /// full. Page keys and a count only.
    static MINOR_REMEMBERED_PARENTS: RefCell<Option<RememberedParents>> =
        const { RefCell::new(None) };
    /// The armed probe of the cohort full in progress. Block data addresses
    /// are identity keys and range bounds for pointer comparisons; nothing is
    /// read through them.
    static SURVIVAL_PROBE: RefCell<Option<SurvivalProbe>> = const { RefCell::new(None) };
}

/// A copying minor took its remembered-set snapshot. Kept only while the
/// promotion census is recording for a cohort full at this safepoint.
pub(in crate::gc) fn note_minor_remembered_parents(snapshot: &RememberedDirtySnapshot) {
    if !super::super::trace::adopt_census::recording() {
        return;
    }
    // Parents named by header are not re-derivable from page keys: leave the
    // record unset, and the view `Unverifiable`.
    let parents = (snapshot.external_dirty_entries.is_empty()
        && snapshot.fallback_headers.is_empty())
    .then(|| RememberedParents {
        dirty_old_pages: snapshot.dirty_old_pages.clone(),
    });
    MINOR_REMEMBERED_PARENTS.with(|p| *p.borrow_mut() = parents);
}

pub(in crate::gc) fn clear_minor_remembered_parents() {
    MINOR_REMEMBERED_PARENTS.with(|p| *p.borrow_mut() = None);
}

/// Arm the probe over the recorded `(data, extent, bytes)` blocks before the
/// cohort full, with the remembered set the same minor noted.
pub(in crate::gc) fn arm_survival_probe(blocks: Vec<(usize, usize, u64)>) {
    let parents = MINOR_REMEMBERED_PARENTS.with(|p| p.borrow_mut().take());
    if blocks.is_empty() {
        SURVIVAL_PROBE.with(|p| *p.borrow_mut() = None);
        return;
    }
    let mut pending = crate::fast_hash::new_ptr_hash_map();
    let mut ranges = Vec::with_capacity(blocks.len());
    let mut promoted_bytes = 0u64;
    for (data, extent, bytes) in blocks {
        promoted_bytes = promoted_bytes.saturating_add(bytes);
        pending.insert(data, (extent, bytes));
        ranges.push((data, data.saturating_add(extent)));
    }
    ranges.sort_unstable();
    let minor_view = if parents.is_some() {
        MinorView::Unchecked
    } else {
        MinorView::Unverifiable
    };
    let probe = SurvivalProbe {
        blocks: pending.len(),
        pending,
        ranges,
        parents,
        minor_view,
        promoted_bytes,
        live_bytes: 0,
        exact: true,
    };
    SURVIVAL_PROBE.with(|p| *p.borrow_mut() = Some(probe));
}

/// Read once per synchronous full sweep.
pub(in crate::gc) fn survival_probe_armed() -> bool {
    SURVIVAL_PROBE.with(|p| p.borrow().is_some())
}

/// A synchronous full's marks are final and nothing is swept: decide whether a
/// remembered parent of the minor that the mark left unmarked refers into the
/// promoted blocks.
pub(in crate::gc) fn check_minor_view_at_full_sweep_start() {
    SURVIVAL_PROBE.with(|p| {
        let mut probe = p.borrow_mut();
        let Some(probe) = probe.as_mut() else {
            return;
        };
        if probe.minor_view != MinorView::Unchecked {
            return;
        }
        let Some(parents) = probe.parents.take() else {
            probe.minor_view = MinorView::Unverifiable;
            return;
        };
        #[cfg(test)]
        if sabotage::skipping_parent_check() {
            probe.minor_view = MinorView::Exact;
            return;
        }
        let ranges = &probe.ranges;
        let mut dead_parent = false;
        let pages = &parents.dirty_old_pages;
        crate::arena::old_arena_walk_objects_on_pages(pages, |header| {
            if !dead_parent {
                // SAFETY: the old page index names headers of live arena
                // allocations; the full has not swept anything yet.
                dead_parent =
                    unsafe { dead_parent_refers_into(header as *mut GcHeader, pages, ranges) };
            }
        });
        probe.minor_view = if dead_parent {
            MinorView::DeadParent
        } else {
            MinorView::Exact
        };
    });
}

/// Does `header`, unmarked by the full, hold on a dirty page a slot that
/// refers into `ranges`? Visits exactly the slots the minor's dirty scan
/// (`scan_dirty_object_slots`) visits, without its accounting.
///
/// # Safety
/// `header` is an arena header from the old page index, before the sweep.
unsafe fn dead_parent_refers_into(
    header: *mut GcHeader,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    ranges: &[(usize, usize)],
) -> bool {
    if !plausible_gc_header(header, true) || (*header).gc_flags & GC_FLAG_MARKED != 0 {
        return false;
    }
    let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
    if !matches!(
        crate::arena::classify_heap_generation(user),
        crate::arena::HeapGeneration::Old
    ) {
        return false;
    }
    let mut hit = false;
    let mut scratch = RememberedSetTraceStats::default();
    visit_gc_rewrite_slot_descriptors(header, |descriptor| unsafe {
        if hit {
            return;
        }
        match descriptor {
            GcMutableSlotDescriptor::Slot(slot) => {
                if !crate::weakref::is_weak_target_trace_slot(header, slot.slot)
                    && dirty_pages_contains_addr(dirty_pages, slot.slot as usize)
                {
                    hit = bits_refer_into(*slot.slot, ranges);
                }
            }
            GcMutableSlotDescriptor::Range { range, .. } => {
                let weak = crate::weakref::header_may_hold_weak_target_slots(header);
                // Newest slots first: the ones written since the last minor.
                for (start, end) in dirty_slot_ranges_for(range, dirty_pages, &mut scratch)
                    .into_iter()
                    .rev()
                {
                    for i in (start..end).rev() {
                        let slot = range.slot(i);
                        if weak && crate::weakref::is_weak_target_trace_slot(header, slot) {
                            continue;
                        }
                        if bits_refer_into(*slot, ranges) {
                            hit = true;
                            return;
                        }
                    }
                }
            }
            GcMutableSlotDescriptor::PointerFreeRange(_) => {}
        }
    });
    hit
}

/// Does a slot's value (NaN-boxed or a raw address) point into `ranges`?
fn bits_refer_into(bits: u64, ranges: &[(usize, usize)]) -> bool {
    let addr = match bits >> 48 {
        0x7FFA | 0x7FFD | 0x7FFF => (bits & 0x0000_FFFF_FFFF_FFFF) as usize,
        0 => bits as usize,
        _ => return false,
    };
    if addr == 0 {
        return false;
    }
    let idx = ranges.partition_point(|&(start, _)| start <= addr);
    idx > 0 && addr < ranges[idx - 1].1
}

/// The sweep walked the block at `data` whole, to `extent`, and kept `live`
/// bytes of it.
pub(in crate::gc) fn note_probe_block_swept(data: usize, extent: usize, live: u64) {
    account_probe_block(data, extent, live);
}

/// The sweep reclaimed the block at `data` without walking it: nothing in it
/// was reached.
pub(in crate::gc) fn note_probe_block_skipped(data: usize, extent: usize) {
    account_probe_block(data, extent, 0);
}

fn account_probe_block(data: usize, extent: usize, live: u64) {
    SURVIVAL_PROBE.with(|p| {
        let mut probe = p.borrow_mut();
        let Some(probe) = probe.as_mut() else {
            return;
        };
        let Some((recorded_extent, _)) = probe.pending.remove(&data) else {
            return;
        };
        if recorded_extent != extent {
            probe.exact = false;
        }
        probe.live_bytes = probe.live_bytes.saturating_add(live);
    });
}

/// Disarm the probe; the measurement when every recorded block was accounted.
pub(in crate::gc) fn take_survival_probe() -> Option<PromotedSurvival> {
    let probe = SURVIVAL_PROBE.with(|p| p.borrow_mut().take())?;
    let survival = (probe.exact && probe.pending.is_empty()).then(|| PromotedSurvival {
        blocks: probe.blocks,
        promoted_bytes: usize::try_from(probe.promoted_bytes).unwrap_or(usize::MAX),
        live_bytes: usize::try_from(probe.live_bytes).unwrap_or(usize::MAX),
        minor_view: probe.minor_view,
    });
    #[cfg(test)]
    LAST_SURVIVAL_FOR_TESTS.with(|c| {
        *c.borrow_mut() = survival
            .as_ref()
            .map(|s| (s.blocks, s.promoted_bytes, s.live_bytes, s.minor_view));
    });
    survival
}

#[cfg(test)]
thread_local! {
    static LAST_SURVIVAL_FOR_TESTS: RefCell<Option<(usize, usize, usize, MinorView)>> =
        const { RefCell::new(None) };
}

/// `(blocks, promoted bytes, live bytes, minor view)` of the last probe taken,
/// when it measured (tests only).
#[cfg(test)]
pub(in crate::gc) fn last_survival_for_tests() -> Option<(usize, usize, usize, MinorView)> {
    LAST_SURVIVAL_FOR_TESTS.with(|c| *c.borrow())
}

/// Sabotage switches for the survival tests. Test builds only.
#[cfg(test)]
pub(in crate::gc) mod sabotage {
    use std::cell::Cell;

    thread_local! {
        static UNFED: Cell<bool> = const { Cell::new(false) };
        static SKIP_PARENT_CHECK: Cell<bool> = const { Cell::new(false) };
    }

    /// The cohort full measures but does not feed the predictor.
    pub(in crate::gc) fn unfed() -> bool {
        UNFED.with(Cell::get)
    }

    /// Every measurement counts as the minor's view.
    pub(super) fn skipping_parent_check() -> bool {
        SKIP_PARENT_CHECK.with(Cell::get)
    }

    pub(in crate::gc) struct Guard {
        flag: &'static std::thread::LocalKey<Cell<bool>>,
        previous: bool,
    }

    impl Guard {
        pub(in crate::gc) fn unfed() -> Self {
            Self {
                flag: &UNFED,
                previous: UNFED.with(|s| s.replace(true)),
            }
        }

        pub(in crate::gc) fn skip_parent_check() -> Self {
            Self {
                flag: &SKIP_PARENT_CHECK,
                previous: SKIP_PARENT_CHECK.with(|s| s.replace(true)),
            }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            self.flag.with(|s| s.set(self.previous));
        }
    }
}
