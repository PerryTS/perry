use super::*;

pub(super) const MIN_TENURED_NURSERY_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MIN_CANDIDATE_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MIN_CANDIDATE_RATIO_PCT: u64 = 25;
pub(super) const RSS_PRESSURE_BYTES: u64 = 192 * 1024 * 1024;
pub(super) const RSS_HARD_PRESSURE_BYTES: u64 = 256 * 1024 * 1024;
pub(super) const MAX_PREVIOUS_PAUSE_US: u64 = 20_000;
pub(super) const EVACUATION_POLICY_DISABLED_REASON: &str = "disabled";
pub(super) const EVACUATION_POLICY_LOW_PAUSE_NON_MOVING_REASON: &str = "low_pause_non_moving";

#[derive(Clone, Copy, Default)]
pub(super) struct EvacuationPolicySnapshot {
    pub(super) tenured_still_in_nursery_bytes: usize,
    pub(super) candidate_bytes: usize,
    pub(super) candidate_objects: usize,
    pub(super) reclaimable_candidate_bytes: usize,
    pub(super) reclaimable_candidate_objects: usize,
    pub(super) old_page_candidate_pages: usize,
    pub(super) old_page_selected_pages: usize,
    pub(super) old_page_selected_live_bytes: usize,
    pub(super) old_page_reclaimable_bytes: usize,
    pub(super) old_page_skipped_pinned_pages: usize,
    /// Block/page-granule bytes a defrag would actually release: the
    /// full size of every nursery block whose reset is blocked ONLY by
    /// movable candidates, plus the page granule of every selected old
    /// page. `reclaimable_candidate_bytes` counts the candidate
    /// OBJECTS' own bytes — but memory returns to the OS in whole
    /// blocks/pages, so 500 blocks each pinned by a few hundred bytes
    /// of scattered tenured survivors are ~500 MB of releasable RSS
    /// that the object-bytes metric reports as <1 MB. The policy gate
    /// passes when EITHER metric clears `MIN_CANDIDATE_BYTES`.
    pub(super) releasable_block_bytes: usize,
    pub(super) retained_forwarded_stub_bytes: usize,
    pub(super) retained_forwarded_stub_objects: usize,
    pub(super) conservative_pinned_bytes: usize,
    pub(super) rss_bytes: u64,
    pub(super) previous_pause_us: u64,
    pub(super) pre_evac_pause_us: u64,
}

impl EvacuationPolicySnapshot {
    #[inline]
    pub(super) fn candidate_ratio_pct(self) -> u64 {
        if self.tenured_still_in_nursery_bytes == 0 {
            return 0;
        }
        ((self.candidate_bytes as u128 * 100) / self.tenured_still_in_nursery_bytes as u128) as u64
    }

    #[inline]
    pub(super) fn reclaimable_candidate_ratio_pct(self) -> u64 {
        if self.tenured_still_in_nursery_bytes == 0 {
            return 0;
        }
        ((self.reclaimable_candidate_bytes as u128 * 100)
            / self.tenured_still_in_nursery_bytes as u128) as u64
    }

    #[inline]
    pub(super) fn effective_candidate_bytes(self) -> usize {
        self.candidate_bytes
            .saturating_add(self.old_page_selected_live_bytes)
    }

    #[inline]
    pub(super) fn effective_reclaimable_candidate_bytes(self) -> usize {
        self.reclaimable_candidate_bytes
            .saturating_add(self.old_page_reclaimable_bytes)
    }

    #[inline]
    pub(super) fn effective_reclaimable_candidate_ratio_pct(self) -> u64 {
        let denominator = self
            .tenured_still_in_nursery_bytes
            .saturating_add(self.old_page_selected_live_bytes)
            .saturating_add(self.old_page_reclaimable_bytes);
        if denominator == 0 {
            return 0;
        }
        ((self.effective_reclaimable_candidate_bytes() as u128 * 100) / denominator as u128) as u64
    }
}

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

#[derive(Clone, Copy)]
pub(super) struct EvacuationPolicyDecision {
    pub(super) allowed: bool,
    pub(super) considered: bool,
    pub(super) force: bool,
    pub(super) enabled: bool,
    pub(super) reason: &'static str,
    pub(super) snapshot: EvacuationPolicySnapshot,
}

impl Default for EvacuationPolicyDecision {
    fn default() -> Self {
        Self {
            allowed: true,
            considered: false,
            force: false,
            enabled: false,
            reason: "not_evaluated",
            snapshot: EvacuationPolicySnapshot::default(),
        }
    }
}

#[derive(Clone, Copy, Default)]
#[cfg_attr(not(feature = "diagnostics"), allow(dead_code))]
pub(super) struct SweepTraceStats {
    pub(super) dead_bytes: u64,
    // Compatibility alias for dead_bytes.
    pub(super) freed_bytes: u64,
    pub(super) reusable_bytes: usize,
    pub(super) returned_bytes: usize,
    pub(super) reset_blocks: usize,
    pub(super) deallocated_blocks: usize,
    // Compatibility alias for returned_bytes.
    pub(super) deallocated_bytes: usize,
    pub(super) retained_forwarded_stub_objects: usize,
    pub(super) retained_forwarded_stub_bytes: usize,
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

pub(super) fn select_old_page_defrag_pages(force: bool) -> OldPageDefragSelection {
    let snapshot = crate::arena::old_page_meta_snapshot();
    select_old_page_defrag_pages_from_snapshot(&snapshot, force)
}

pub(super) fn evacuation_policy_initial_decision(
    tenured_still_in_nursery_bytes: usize,
    rss_bytes: u64,
    previous_pause_us: u64,
    pre_evac_pause_us: u64,
    allowed: bool,
    force: bool,
    disabled_reason: &'static str,
    old_to_young_tracking_complete: bool,
    old_page_selected_pages: usize,
) -> EvacuationPolicyDecision {
    let snapshot = EvacuationPolicySnapshot {
        tenured_still_in_nursery_bytes,
        rss_bytes,
        previous_pause_us,
        pre_evac_pause_us,
        ..EvacuationPolicySnapshot::default()
    };
    if !allowed {
        return EvacuationPolicyDecision {
            allowed,
            force,
            reason: disabled_reason,
            snapshot,
            ..EvacuationPolicyDecision::default()
        };
    }
    if !old_to_young_tracking_complete {
        return EvacuationPolicyDecision {
            allowed,
            force,
            reason: "barriers_inactive",
            snapshot,
            ..EvacuationPolicyDecision::default()
        };
    }
    if force {
        return EvacuationPolicyDecision {
            allowed,
            considered: true,
            force,
            reason: "force_considered",
            snapshot,
            ..EvacuationPolicyDecision::default()
        };
    }
    if tenured_still_in_nursery_bytes >= MIN_TENURED_NURSERY_BYTES {
        return EvacuationPolicyDecision {
            allowed,
            considered: true,
            force,
            reason: "nursery_pressure",
            snapshot,
            ..EvacuationPolicyDecision::default()
        };
    }
    if rss_bytes >= gc_rss_pressure_dyn_bytes() {
        return EvacuationPolicyDecision {
            allowed,
            considered: true,
            force,
            reason: "rss_pressure",
            snapshot,
            ..EvacuationPolicyDecision::default()
        };
    }
    if old_page_selected_pages > 0 {
        return EvacuationPolicyDecision {
            allowed,
            considered: true,
            force,
            reason: "old_page_fragmentation",
            snapshot,
            ..EvacuationPolicyDecision::default()
        };
    }
    EvacuationPolicyDecision {
        allowed,
        force,
        reason: "low_pressure",
        snapshot,
        ..EvacuationPolicyDecision::default()
    }
}

pub(super) fn evacuation_policy_snapshot_after_mark(
    mut snapshot: EvacuationPolicySnapshot,
    force: bool,
    pre_evac_pause_us: u64,
    old_page_selection: &OldPageDefragSelection,
) -> EvacuationPolicySnapshot {
    #[derive(Clone, Copy, Default)]
    struct BlockCandidateState {
        candidate_bytes: usize,
        candidate_objects: usize,
        retained_live: bool,
    }

    snapshot.tenured_still_in_nursery_bytes = 0;
    snapshot.candidate_bytes = 0;
    snapshot.candidate_objects = 0;
    snapshot.reclaimable_candidate_bytes = 0;
    snapshot.reclaimable_candidate_objects = 0;
    snapshot.retained_forwarded_stub_bytes = 0;
    snapshot.retained_forwarded_stub_objects = 0;
    snapshot.conservative_pinned_bytes = 0;
    snapshot.pre_evac_pause_us = pre_evac_pause_us;
    snapshot.old_page_candidate_pages = old_page_selection.candidate_pages;
    snapshot.old_page_selected_pages = old_page_selection.selected_pages;
    snapshot.old_page_selected_live_bytes = old_page_selection.selected_live_bytes;
    snapshot.old_page_reclaimable_bytes = old_page_selection.selected_reclaimable_bytes;
    snapshot.old_page_skipped_pinned_pages = old_page_selection.skipped_pinned_pages;
    snapshot.releasable_block_bytes = old_page_selection.selected_releasable_block_bytes;

    let n_blocks = crate::arena::arena_block_count();
    let general_n = crate::arena::general_block_count();
    let mut blocks = vec![BlockCandidateState::default(); n_blocks];

    crate::arena::arena_walk_objects_with_block_index(|header_ptr, block_idx| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            if !crate::arena::pointer_in_nursery(user_ptr as usize) {
                return;
            }
            let flags = (*header).gc_flags;
            let total = (*header).size as usize;
            if flags & GC_FLAG_FORWARDED != 0 {
                if block_idx < general_n {
                    snapshot.retained_forwarded_stub_objects += 1;
                    snapshot.retained_forwarded_stub_bytes += total;
                }
                if let Some(block) = blocks.get_mut(block_idx) {
                    block.retained_live = true;
                }
                return;
            }
            let is_tenured = flags & GC_FLAG_TENURED != 0;
            if is_tenured {
                snapshot.tenured_still_in_nursery_bytes += total;
            }
            if flags & GC_FLAG_MARKED == 0 {
                if flags & GC_FLAG_PINNED != 0 {
                    if let Some(block) = blocks.get_mut(block_idx) {
                        block.retained_live = true;
                    }
                }
                return;
            }
            if flags & GC_FLAG_PINNED != 0 {
                if let Some(block) = blocks.get_mut(block_idx) {
                    block.retained_live = true;
                }
                return;
            }
            if is_conservatively_pinned(header) {
                snapshot.conservative_pinned_bytes += total;
                if let Some(block) = blocks.get_mut(block_idx) {
                    block.retained_live = true;
                }
                return;
            }
            if !force && !is_tenured {
                if let Some(block) = blocks.get_mut(block_idx) {
                    block.retained_live = true;
                }
                return;
            }
            snapshot.candidate_objects += 1;
            snapshot.candidate_bytes += total;
            if let Some(block) = blocks.get_mut(block_idx) {
                block.candidate_objects += 1;
                block.candidate_bytes += total;
            }
        }
    });

    let general_block_sizes = crate::arena::general_block_sizes();
    for (block_idx, block) in blocks.iter().enumerate().take(general_n) {
        if block.candidate_bytes > 0 && !block.retained_live {
            snapshot.reclaimable_candidate_objects += block.candidate_objects;
            snapshot.reclaimable_candidate_bytes += block.candidate_bytes;
            // This block's reset is blocked ONLY by movable candidates:
            // evacuating them frees the whole block granule. Exclude the
            // caller-saved-register safety window — those blocks are not
            // reset even when empty, so their granule can't be released
            // this cycle regardless of what evacuation moves.
            if !crate::arena::general_block_in_recent_window(block_idx) {
                snapshot.releasable_block_bytes = snapshot
                    .releasable_block_bytes
                    .saturating_add(general_block_sizes.get(block_idx).copied().unwrap_or(0));
            }
        }
    }
    snapshot
}

pub(super) fn evacuation_policy_final_decision(
    mut decision: EvacuationPolicyDecision,
    snapshot: EvacuationPolicySnapshot,
) -> EvacuationPolicyDecision {
    decision.snapshot = snapshot;
    decision.enabled = false;
    if !decision.allowed {
        if decision.reason == "not_evaluated" {
            decision.reason = EVACUATION_POLICY_DISABLED_REASON;
        }
        return decision;
    }
    if !decision.considered {
        decision.reason = "low_pressure";
        return decision;
    }
    if snapshot.effective_candidate_bytes() == 0 {
        decision.reason = "zero_candidates";
        return decision;
    }
    if decision.force {
        decision.enabled = true;
        decision.reason = "force";
        return decision;
    }
    // Hard RSS pressure bypasses every candidate-volume/ratio/pause gate:
    // at 256 MB+ of RSS with ANY movable candidate (checked above), refusing
    // to compact because the candidates look small is exactly backwards —
    // the small scattered candidates are what's pinning the blocks.
    // Previously these gates `return`ed before the RSS checks, so a heap of
    // sparsely-pinned blocks could sit above the hard threshold forever with
    // reason `reclaimable_candidate_bytes_below_threshold`.
    let hard_rss_pressure = snapshot.rss_bytes >= gc_rss_hard_pressure_dyn_bytes();
    if hard_rss_pressure {
        decision.enabled = true;
        decision.reason = "rss_hard_pressure";
        return decision;
    }
    if snapshot.effective_reclaimable_candidate_bytes() == 0 && snapshot.releasable_block_bytes == 0
    {
        decision.reason = "zero_reclaimable_candidates";
        return decision;
    }
    // Volume gate: pass when EITHER the candidate objects' own bytes OR the
    // block/page granule bytes their evacuation releases clear the bar. The
    // ratio gate stays object-bytes-scoped — the granule metric is an
    // absolute-RSS argument, not a proportion of the tenured working set.
    let object_bytes_pass = snapshot.effective_reclaimable_candidate_bytes() >= MIN_CANDIDATE_BYTES;
    let block_bytes_pass = snapshot.releasable_block_bytes >= MIN_CANDIDATE_BYTES;
    if !object_bytes_pass && !block_bytes_pass {
        decision.reason = "reclaimable_candidate_bytes_below_threshold";
        return decision;
    }
    if !block_bytes_pass
        && snapshot.effective_reclaimable_candidate_ratio_pct() < MIN_CANDIDATE_RATIO_PCT
    {
        decision.reason = "reclaimable_candidate_ratio_below_threshold";
        return decision;
    }
    let pause_budget_exceeded = snapshot.previous_pause_us > MAX_PREVIOUS_PAUSE_US
        || snapshot.pre_evac_pause_us > MAX_PREVIOUS_PAUSE_US;
    if pause_budget_exceeded {
        decision.reason = "pause_budget_exceeded";
        return decision;
    }
    decision.enabled = true;
    decision.reason = if !object_bytes_pass && block_bytes_pass {
        // Only the granule metric cleared the bar — the new W3 path.
        "releasable_block_bytes"
    } else if snapshot.rss_bytes >= gc_rss_pressure_dyn_bytes() {
        "rss_pressure"
    } else if snapshot.old_page_selected_pages > 0
        && snapshot.tenured_still_in_nursery_bytes < MIN_TENURED_NURSERY_BYTES
    {
        "old_page_fragmentation"
    } else {
        "nursery_pressure"
    };
    decision
}

pub(super) fn maybe_print_evacuation_policy_diag(
    decision: EvacuationPolicyDecision,
    evacuation: EvacuationTraceStats,
) {
    if std::env::var_os("PERRY_GC_DIAG").is_none() {
        return;
    }
    if !decision.considered && decision.reason != "barriers_inactive" {
        return;
    }
    let snapshot = decision.snapshot;
    eprintln!(
        "[gc-evac-policy] enabled={} reason={} tenured={} candidate_bytes={} candidate_objects={} candidate_ratio_pct={} reclaimable_candidate_bytes={} reclaimable_candidate_objects={} reclaimable_candidate_ratio_pct={} releasable_block_bytes={} old_page_candidate_pages={} old_page_selected_pages={} old_page_selected_live_bytes={} old_page_reclaimable_bytes={} old_page_skipped_pinned_pages={} policy_retained_forwarded_stub_bytes={} policy_retained_forwarded_stub_objects={} cons_pinned={} rss={} prev_pause_us={} pre_evac_pause_us={} moved_bytes={} moved_objects={} old_page_moved_bytes={} old_page_moved_objects={} released_original_bytes={} released_original_objects={} sweep_retained_forwarded_stub_bytes={} sweep_retained_forwarded_stub_objects={}",
        decision.enabled,
        decision.reason,
        snapshot.tenured_still_in_nursery_bytes,
        snapshot.candidate_bytes,
        snapshot.candidate_objects,
        snapshot.candidate_ratio_pct(),
        snapshot.reclaimable_candidate_bytes,
        snapshot.reclaimable_candidate_objects,
        snapshot.reclaimable_candidate_ratio_pct(),
        snapshot.releasable_block_bytes,
        snapshot.old_page_candidate_pages,
        snapshot.old_page_selected_pages,
        snapshot.old_page_selected_live_bytes,
        snapshot.old_page_reclaimable_bytes,
        snapshot.old_page_skipped_pinned_pages,
        snapshot.retained_forwarded_stub_bytes,
        snapshot.retained_forwarded_stub_objects,
        snapshot.conservative_pinned_bytes,
        snapshot.rss_bytes,
        snapshot.previous_pause_us,
        snapshot.pre_evac_pause_us,
        evacuation.moved_bytes,
        evacuation.moved_objects,
        evacuation.old_page_moved_bytes,
        evacuation.old_page_moved_objects,
        evacuation.released_original_bytes,
        evacuation.released_original_objects,
        evacuation.retained_forwarded_stub_bytes,
        evacuation.retained_forwarded_stub_objects,
    );
}

pub(super) fn copied_minor_malloc_sweep_due(trigger_kind: GcTriggerKind) -> bool {
    matches!(trigger_kind, GcTriggerKind::MallocCount)
        || malloc_object_count() >= GC_NEXT_MALLOC_TRIGGER.with(|c| c.get())
}

/// Generational GC (minor collection on every trigger) is now the
/// default model as of Phase D (v0.5.237). Set `PERRY_GEN_GC=0`,
/// `=false`, or `=off` to opt out and fall back to the full
/// mark-sweep — kept as an escape hatch for bisecting GC-related
/// regressions in user programs.
///
/// Why generational is the default now: Phase C (v0.5.222-228) wired
/// the nursery / old-gen split, write barriers, remembered set, and
/// non-moving tenuring; Phase C4b (v0.5.229-236) added forwarding
/// pointer infrastructure, conservative-pinning safety, policy-gated
/// evacuation, reference rewriting,
/// idle-block deallocation, and the trigger ceiling that bounds
/// peak nursery occupancy. The minor-GC path has been the proven-
/// equivalent default in every regression suite (168 unit tests,
/// 9 `test_json_*.ts` × 4 mode combos, 18 memory-stability tests)
/// since C3b landed; flipping the default makes those gains apply

// #854: part of GC full mark-sweep fallback path (PERRY_GEN_GC=0)
#[allow(dead_code)]
pub(super) fn sweep() -> u64 {
    sweep_with_age_bump(false).freed_bytes
}

pub(super) fn sweep_malloc_objects() -> u64 {
    let mut state = MallocSweepCycleState::new(true);
    state.finish_unbounded()
}

pub(super) fn clear_malloc_mark_bits() {
    let mut state = MallocSweepCycleState::new(false);
    state.finish_unbounded();
}

struct MallocSweepCycleState {
    sweep_malloc: bool,
    headers: Vec<*mut GcHeader>,
    positions: crate::fast_hash::PtrHashMap<usize, usize>,
    cursor: usize,
    freed_bytes: u64,
}

impl MallocSweepCycleState {
    fn new(sweep_malloc: bool) -> Self {
        let headers = malloc_sweep_snapshot_headers();
        let mut positions = crate::fast_hash::PtrHashMap::with_capacity_and_hasher(
            headers.len(),
            crate::fast_hash::PtrHasher,
        );
        for (idx, &header) in headers.iter().enumerate() {
            positions.insert(header as usize, idx);
        }
        Self {
            sweep_malloc,
            headers,
            positions,
            cursor: 0,
            freed_bytes: 0,
        }
    }

    fn step(&mut self, budget: usize) -> bool {
        let mut remaining = budget;
        while remaining > 0 && self.cursor < self.headers.len() {
            let header = self.headers[self.cursor];
            self.cursor += 1;
            remaining -= 1;
            let Some(header) = self.revalidate_tracked_header(header) else {
                continue;
            };
            if self.sweep_malloc {
                self.process_sweep_header(header);
            } else {
                unsafe {
                    (*header).gc_flags &= !GC_FLAG_MARKED;
                }
            }
        }
        let done = self.cursor >= self.headers.len();
        if done {
            malloc_sweep_clear_snapshot_tracking();
        }
        done
    }

    fn finish_unbounded(&mut self) -> u64 {
        while !self.step(usize::MAX) {}
        self.freed_bytes
    }

    fn revalidate_tracked_header(
        &mut self,
        snapshot_header: *mut GcHeader,
    ) -> Option<*mut GcHeader> {
        let snapshot_key = snapshot_header as usize;
        let expected_idx = self.positions.get(&snapshot_key).copied()?;
        let Some((current_header, current_idx)) =
            malloc_sweep_revalidate_header(snapshot_header, expected_idx)
        else {
            self.positions.remove(&snapshot_key);
            return None;
        };

        if current_header != snapshot_header {
            self.positions.remove(&snapshot_key);
            self.positions.insert(current_header as usize, current_idx);
        } else if current_idx != expected_idx {
            self.positions.insert(snapshot_key, current_idx);
        }
        Some(current_header)
    }

    fn process_sweep_header(&mut self, header: *mut GcHeader) {
        unsafe {
            if (*header).gc_flags & GC_FLAG_PINNED != 0 {
                (*header).gc_flags &= !GC_FLAG_MARKED;
                return;
            }
            if (*header).gc_flags & GC_FLAG_MARKED != 0 {
                (*header).gc_flags &= !GC_FLAG_MARKED;
                return;
            }

            let total_size = (*header).size as usize;
            let obj_type = (*header).obj_type;
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            self.freed_bytes = self.freed_bytes.saturating_add(total_size as u64);
            layout_clear_for_ptr(user_ptr as usize);
            gc_type_finalize_unmarked_payload(obj_type, user_ptr);
            let layout = Layout::from_size_align(total_size, 8).unwrap();
            dealloc(header as *mut u8, layout);
            self.remove_tracked_header(header, obj_type, total_size as u64);
        }
    }

    fn remove_tracked_header(&mut self, header: *mut GcHeader, obj_type: u8, bytes: u64) {
        let Some(mut idx) = self.positions.remove(&(header as usize)) else {
            return;
        };
        MALLOC_STATE.with(|s| {
            let mut s = s.borrow_mut();
            if idx >= s.objects.len() || s.objects[idx] != header {
                let Some(found) = s.objects.iter().position(|&candidate| candidate == header)
                else {
                    return;
                };
                idx = found;
            }

            let registry_available = s.malloc_registry_available();
            s.objects.swap_remove(idx);
            if idx < s.objects.len() {
                let moved = s.objects[idx];
                if self.positions.contains_key(&(moved as usize)) {
                    self.positions.insert(moved as usize, idx);
                }
            }
            if registry_available {
                s.set.remove(&(header as usize));
            }
            s.record_malloc_free(obj_type, bytes);
        });
    }
}

/// Sweep variant that folds the minor-GC age-bump pass into the same arena walk.
///
/// `gc_collect_minor` previously did:
///   1. arena_walk_objects to update HAS_SURVIVED/TENURED on marked young objects
///   2. arena_walk_objects_with_block_index in `sweep` to free dead objects and
///      compute block_has_live
///
/// Both walks visit every arena object header. With ~1.6M objects per cycle in
/// perf-comprehensive, removing the dedicated age-bump walk saves ~10ms/cycle
/// and avoids touching every header twice. The age-bump update is folded into
/// the sweep walk's "alive" branches, gated on `block_idx < general_n` so only
/// general-arena (nursery) objects age — longlived and old-gen are skipped, as
/// in the original standalone age-bump pass (which used `pointer_in_old_gen`
/// for the same gate).
#[allow(dead_code)]
pub(super) fn sweep_with_age_bump(do_age_bump: bool) -> SweepTraceStats {
    sweep_with_age_bump_and_old_reclaim_targets(do_age_bump, false, None, true)
}

#[allow(dead_code)]
pub(super) fn sweep_with_age_bump_and_malloc(
    do_age_bump: bool,
    sweep_malloc: bool,
) -> SweepTraceStats {
    sweep_with_age_bump_and_old_reclaim_targets(do_age_bump, false, None, sweep_malloc)
}

unsafe fn finalize_dead_arena_payload(
    header: *mut GcHeader,
    user_ptr: *mut u8,
    overflow_active: bool,
) {
    layout_clear_for_ptr(user_ptr as usize);
    if overflow_active {
        gc_type_clear_dead_payload_side_tables((*header).obj_type, user_ptr as usize);
    }
    gc_type_finalize_unmarked_payload((*header).obj_type, user_ptr);
}

pub(super) unsafe fn invalidate_dead_old_arena_header(header: *mut GcHeader, total_size: usize) {
    crate::arena::unregister_old_object_pages(header as usize, total_size);
    (*header).obj_type = 0;
    (*header).gc_flags = 0;
    (*header)._reserved = 0;
}

#[allow(dead_code)]
pub(super) fn sweep_with_age_bump_and_old_reclaim(
    do_age_bump: bool,
    reclaim_dead_old_blocks: bool,
) -> SweepTraceStats {
    sweep_with_age_bump_and_old_reclaim_targets(do_age_bump, reclaim_dead_old_blocks, None, true)
}

#[allow(dead_code)]
pub(super) fn sweep_with_age_bump_and_targeted_old_reclaim_and_malloc(
    do_age_bump: bool,
    selected_old_blocks: &crate::fast_hash::PtrHashSet<usize>,
    sweep_malloc: bool,
) -> SweepTraceStats {
    sweep_with_age_bump_and_old_reclaim_targets(
        do_age_bump,
        false,
        Some(selected_old_blocks),
        sweep_malloc,
    )
}

#[allow(dead_code)]
fn sweep_with_age_bump_and_old_reclaim_targets(
    do_age_bump: bool,
    reclaim_dead_old_blocks: bool,
    targeted_old_blocks: Option<&crate::fast_hash::PtrHashSet<usize>>,
    sweep_malloc: bool,
) -> SweepTraceStats {
    let mut state = IncrementalSweepState::new(
        do_age_bump,
        reclaim_dead_old_blocks,
        targeted_old_blocks.cloned(),
        sweep_malloc,
    );
    state.finish_unbounded()
}

#[allow(dead_code)]
fn legacy_sweep_with_age_bump_and_old_reclaim_targets(
    do_age_bump: bool,
    reclaim_dead_old_blocks: bool,
    targeted_old_blocks: Option<&crate::fast_hash::PtrHashSet<usize>>,
    sweep_malloc: bool,
) -> SweepTraceStats {
    let mut freed_bytes = if sweep_malloc {
        sweep_malloc_objects()
    } else {
        clear_malloc_mark_bits();
        0
    };
    let mut retained_forwarded_stub_objects: usize = 0;
    let mut retained_forwarded_stub_bytes: usize = 0;

    // Sweep arena objects. Two-phase strategy:
    //
    //   1. Fast probe pass: walk objects, clear mark bits, count
    //      dead bytes, track whether ANY block has a live object.
    //      If no live anywhere → entire arena is reclaimable. Skip
    //      every per-block tracking structure and reset all blocks
    //      to offset=0 in O(1). This is the common case for tight
    //      `new ClassName()` loops where nothing escapes.
    //
    //   2. Slow tracking pass (only when some block has live objects):
    //      walk again, this time bucketing dead objects per block so
    //      we can decide which blocks are fully empty (reset) vs
    //      partially empty (push their dead objects to the free list
    //      in a single batched extend).
    //
    // The two-pass split avoids the per-object HashMap insert cost
    // (~50ns) on the common all-dead path, where it would account for
    // 700k × 50ns = 35ms per GC cycle.
    // Sweep arena objects with per-block live tracking.
    //
    // For each object, walk and check mark/pinned state:
    //   - live → set `block_has_live[block_idx]` and clear the mark
    //     bit inline so we don't need a separate pass.
    //   - dead → zero its payload memory (so stale pointers don't
    //     retain other objects on the next GC cycle).
    //
    // We deliberately do NOT push dead objects onto the global
    // ARENA_FREE_LIST. The inline bump allocator never reads the
    // free list — it uses the per-block reset instead. Pushing
    // dead objects to the free list would cost ~50ns per object
    // × ~700k objects per GC × ~12 GC cycles per benchmark = 420ms
    // of pure waste in `object_create`. The function-call allocator
    // path (`js_object_alloc_class_inline_keys` → `arena_alloc_gc`)
    // is the only consumer of the free list, and it's only used
    // for shapes the inline path doesn't cover (anonymous classes,
    // closure body new'd from a slot, etc.) — those are rare enough
    // that running them through the slow path is fine.
    //
    // After the walk, `arena_reset_empty_blocks` resets every block
    // with zero live objects to offset=0. This is the load-bearing
    // optimization that lets the inline bump allocator reuse memory
    // across GC cycles instead of page-faulting through fresh blocks.
    let n_blocks = crate::arena::arena_block_count();
    let mut block_has_live: Vec<bool> = vec![false; n_blocks];
    // Inclusive upper bound on indices that age. `general_block_count()`
    // is the first non-general index; objects with `block_idx < general_n`
    // are nursery-resident and need the age-bump update.
    let resettable_general_n = crate::arena::general_block_count();
    let old_block_start = crate::arena::longlived_end();
    crate::arena::old_pages_reset_sweep_accounting();

    // Hoist the OVERFLOW_FIELDS empty check out of the per-dead-object
    // loop. perf-comprehensive's sweep walks ~1.6 M dead arena headers
    // per cycle and most workloads never write past the 8 inline object
    // slots, so OVERFLOW_FIELDS stays empty for the whole run. The
    // hoisted bool turns 1.6 M `clear_overflow_for_ptr` calls (each one
    // a TLS-load + RefCell borrow + HashMap remove on a missing key)
    // into a single bool test per object. ~1.4 % leaf samples → 0 on
    // the empty-map path, ~80 ms saved on perf-comprehensive.
    // Wave 2: the same gate now also covers the closure dynamic-props
    // dead-payload arm — checked once per sweep, not per object.
    let overflow_active = !crate::object::overflow_fields_is_empty()
        || crate::closure::closure_dynamic_side_tables_nonempty();

    crate::arena::arena_walk_objects_with_block_index(|header_ptr, block_idx| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            // Age-bump for surviving general-arena (nursery) objects, folded
            // into this walk so the standalone `arena_walk_objects` pass in
            // gc_collect_minor can be eliminated. Mirrors the original
            // age-bump's gate (skip old-gen, skip already-tenured, skip
            // unmarked-and-unpinned) and runs BEFORE the mark bit is
            // cleared so the MARKED check stays meaningful.
            let age_bump_this = do_age_bump && block_idx < resettable_general_n;
            let flags = (*header).gc_flags;
            // Fast path: `flags == 0` means the object is dead (MARKED=0)
            // AND has no special bits (PINNED/FORWARDED/HAS_SURVIVED/
            // TENURED). Fresh allocations from the current cycle that
            // never got marked land here — in perf-comprehensive's hot
            // forEach / commandBuffer loops that's the dominant case.
            // Skipping the four flag-bit branches and the age-bump
            // bookkeeping for this common case shaves a measurable amount
            // off the 1.6 M-object-per-cycle sweep walk.
            if flags == 0 {
                let total_size = (*header).size as usize;
                let dead_old = block_idx >= old_block_start;
                if dead_old {
                    crate::arena::old_page_account_swept_object(
                        header as usize,
                        total_size,
                        false,
                        false,
                    );
                }
                let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
                freed_bytes += total_size as u64;
                finalize_dead_arena_payload(header, user_ptr, overflow_active);
                if reclaim_dead_old_blocks && dead_old {
                    invalidate_dead_old_arena_header(header, total_size);
                }
                return;
            }
            if flags & GC_FLAG_PINNED != 0 {
                if block_idx >= old_block_start {
                    crate::arena::old_page_account_swept_object(
                        header as usize,
                        (*header).size as usize,
                        true,
                        true,
                    );
                }
                if block_idx < block_has_live.len() {
                    block_has_live[block_idx] = true;
                }
                if age_bump_this && flags & GC_FLAG_TENURED == 0 {
                    if flags & GC_FLAG_HAS_SURVIVED != 0 {
                        (*header).gc_flags =
                            (flags | GC_FLAG_TENURED) & !GC_FLAG_HAS_SURVIVED & !GC_FLAG_MARKED;
                    } else {
                        (*header).gc_flags = (flags | GC_FLAG_HAS_SURVIVED) & !GC_FLAG_MARKED;
                    }
                } else {
                    (*header).gc_flags = flags & !GC_FLAG_MARKED;
                }
                return;
            }
            // Retained FORWARDED objects keep their containing block alive only
            // when the stub itself was reached this cycle, or when it sits in
            // the same recent-block safety window as arena reset. Older
            // unmarked stubs are stale array-growth remnants; retaining all of
            // them pins one object in nearly every JSON-churn block and prevents
            // RSS from falling after sweep.
            if flags & GC_FLAG_FORWARDED != 0 {
                let retain_stub = flags & GC_FLAG_MARKED != 0
                    || (block_idx < resettable_general_n
                        && crate::arena::general_block_in_recent_window(block_idx));
                if retain_stub {
                    if block_idx >= old_block_start {
                        crate::arena::old_page_account_swept_object(
                            header as usize,
                            (*header).size as usize,
                            true,
                            false,
                        );
                    }
                    if block_idx < resettable_general_n {
                        retained_forwarded_stub_objects += 1;
                        retained_forwarded_stub_bytes += (*header).size as usize;
                    }
                    if block_idx < block_has_live.len() {
                        block_has_live[block_idx] = true;
                    }
                    (*header).gc_flags = flags & !GC_FLAG_MARKED;
                } else {
                    let total_size = (*header).size as usize;
                    let dead_old = block_idx >= old_block_start;
                    if dead_old {
                        crate::arena::old_page_account_swept_object(
                            header as usize,
                            total_size,
                            false,
                            false,
                        );
                    }
                    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
                    freed_bytes += total_size as u64;
                    layout_clear_for_ptr(user_ptr as usize);
                    if overflow_active {
                        gc_type_clear_dead_payload_side_tables(
                            (*header).obj_type,
                            user_ptr as usize,
                        );
                    }
                    if reclaim_dead_old_blocks && dead_old {
                        invalidate_dead_old_arena_header(header, total_size);
                    } else {
                        (*header).gc_flags = flags & !(GC_FLAG_FORWARDED | GC_FLAG_MARKED);
                    }
                }
                return;
            }
            if flags & GC_FLAG_MARKED == 0 {
                let total_size = (*header).size as usize;
                let dead_old = block_idx >= old_block_start;
                if dead_old {
                    crate::arena::old_page_account_swept_object(
                        header as usize,
                        total_size,
                        false,
                        false,
                    );
                }
                let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
                freed_bytes += total_size as u64;
                finalize_dead_arena_payload(header, user_ptr, overflow_active);

                // Note: We deliberately do NOT zero the dead object's
                // payload here. trace_object/trace_array/trace_closure
                // walk objects PRECISELY (only `field_count` /
                // `length` / `capture_count` slots), so unused slots
                // and dead-object payloads are never scanned by the
                // mark phase. The conservative stack scan only walks
                // the C stack, not arbitrary heap memory. So stale
                // pointer-looking bytes inside dead-object payloads
                // can never trigger a false positive — and zeroing
                // them was costing ~2-3ms per `object_create` GC for
                // memory bandwidth (700k × 88 bytes = 62MB written).
                if reclaim_dead_old_blocks && dead_old {
                    invalidate_dead_old_arena_header(header, total_size);
                }
            } else {
                if block_idx >= old_block_start {
                    crate::arena::old_page_account_swept_object(
                        header as usize,
                        (*header).size as usize,
                        true,
                        false,
                    );
                }
                if block_idx < block_has_live.len() {
                    block_has_live[block_idx] = true;
                }
                if age_bump_this && flags & GC_FLAG_TENURED == 0 {
                    if flags & GC_FLAG_HAS_SURVIVED != 0 {
                        (*header).gc_flags =
                            (flags | GC_FLAG_TENURED) & !GC_FLAG_HAS_SURVIVED & !GC_FLAG_MARKED;
                    } else {
                        (*header).gc_flags = (flags | GC_FLAG_HAS_SURVIVED) & !GC_FLAG_MARKED;
                    }
                } else {
                    (*header).gc_flags = flags & !GC_FLAG_MARKED;
                }
            }
        }
    });

    // Reset every block that ended up with zero live objects.
    // Diagnostic: PERRY_GC_DIAG=1 reports block-level liveness.
    if std::env::var_os("PERRY_GC_DIAG").is_some() {
        let live_general = (0..resettable_general_n)
            .filter(|&i| block_has_live[i])
            .count();
        let live_ll = (resettable_general_n..n_blocks)
            .filter(|&i| block_has_live[i])
            .count();
        eprintln!(
            "[gc] blocks: general={} ({} live), longlived={} ({} live), freed_bytes={} retained_forwarded_stub_bytes={} retained_forwarded_stub_objects={}",
            resettable_general_n,
            live_general,
            n_blocks - resettable_general_n,
            live_ll,
            freed_bytes,
            retained_forwarded_stub_bytes,
            retained_forwarded_stub_objects,
        );
    }
    let nursery_reset = crate::arena::arena_reset_empty_blocks(&block_has_live);
    let survivor_reset = if reclaim_dead_old_blocks {
        crate::arena::survivor_arena_reclaim_dead_blocks(&block_has_live)
    } else {
        crate::arena::ArenaResetStats::default()
    };
    let old_reset = if reclaim_dead_old_blocks {
        crate::arena::old_arena_reclaim_dead_blocks(&block_has_live)
    } else if let Some(selected_old_blocks) = targeted_old_blocks {
        crate::arena::old_arena_reclaim_selected_dead_blocks(&block_has_live, selected_old_blocks)
    } else {
        crate::arena::ArenaResetStats::default()
    };
    let reset = crate::arena::ArenaResetStats {
        reset_blocks: nursery_reset
            .reset_blocks
            .saturating_add(survivor_reset.reset_blocks)
            .saturating_add(old_reset.reset_blocks),
        reusable_bytes: nursery_reset
            .reusable_bytes
            .saturating_add(survivor_reset.reusable_bytes)
            .saturating_add(old_reset.reusable_bytes),
        deallocated_blocks: nursery_reset
            .deallocated_blocks
            .saturating_add(survivor_reset.deallocated_blocks)
            .saturating_add(old_reset.deallocated_blocks),
        deallocated_bytes: nursery_reset
            .deallocated_bytes
            .saturating_add(survivor_reset.deallocated_bytes)
            .saturating_add(old_reset.deallocated_bytes),
    };

    SweepTraceStats {
        dead_bytes: freed_bytes,
        freed_bytes,
        reusable_bytes: reset.reusable_bytes,
        returned_bytes: reset.deallocated_bytes,
        reset_blocks: reset.reset_blocks,
        deallocated_blocks: reset.deallocated_blocks,
        deallocated_bytes: reset.deallocated_bytes,
        retained_forwarded_stub_objects,
        retained_forwarded_stub_bytes,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SweepCycleSubphase {
    /// #6010: budget-chunked finalization of dead registered Maps/Sets whose
    /// external side buffers no ordinary sweep path frees (dead in the ACTIVE
    /// nursery allocation block, or reclaimed by bulk block resets that skip
    /// per-object hooks). The dead lists are collected once at sweep entry
    /// (marks fresh — a cheap flag-check walk of the registries), and each
    /// buffer free consumes one work unit here so budgeted cycles keep their
    /// pause bound. Deadness is stable across the incremental steps: an
    /// unreachable header can't be revived, and interior block space is never
    /// re-allocated before this sweep's own BlockCleanup subphase runs.
    CollectionSideBuffers,
    Malloc,
    ArenaObjects,
    BlockCleanup,
    Done,
}

pub(super) struct IncrementalSweepState {
    subphase: SweepCycleSubphase,
    dead_maps: Vec<usize>,
    dead_sets: Vec<usize>,
    dead_buffers: Vec<usize>,
    dead_typed_arrays: Vec<usize>,
    malloc: MallocSweepCycleState,
    arena: ArenaSweepObjectsState,
    cleanup: Option<ArenaSweepCleanupState>,
    reclaim_dead_old_blocks: bool,
    targeted_old_blocks: Option<crate::fast_hash::PtrHashSet<usize>>,
    stats: SweepTraceStats,
}

impl IncrementalSweepState {
    pub(super) fn new(
        do_age_bump: bool,
        reclaim_dead_old_blocks: bool,
        targeted_old_blocks: Option<crate::fast_hash::PtrHashSet<usize>>,
        sweep_malloc: bool,
    ) -> Self {
        Self {
            subphase: SweepCycleSubphase::Malloc,
            dead_maps: Vec::new(),
            dead_sets: Vec::new(),
            dead_buffers: Vec::new(),
            dead_typed_arrays: Vec::new(),
            malloc: MallocSweepCycleState::new(sweep_malloc),
            arena: ArenaSweepObjectsState::new(do_age_bump, reclaim_dead_old_blocks),
            cleanup: None,
            reclaim_dead_old_blocks,
            targeted_old_blocks,
            stats: SweepTraceStats::default(),
        }
    }

    /// #6010: collect the dead registered Maps/Sets NOW (marks are fresh at
    /// sweep entry) and finalize their external buffers budget-chunked as the
    /// first sweep subphase. See `SweepCycleSubphase::CollectionSideBuffers`.
    /// 2026-07-09 audit: buffers and typed arrays joined the same pattern —
    /// their registry/side-table entries are pruned when the owner is
    /// genuinely dead (full traces only; they are all tenured old residents).
    pub(super) fn with_dead_collection_finalize(mut self, full_trace: bool) -> Self {
        // 2026-07-09 GC audit wave 2: death-prune the object-address-keyed
        // side tables in the same marks-fresh window. Cheap (one flag-check
        // walk over tables the root scanners already walk every cycle), so
        // it runs eagerly here rather than budget-chunked.
        super::dead_owner::prune_dead_owner_side_tables_post_trace(full_trace);
        self.dead_maps = crate::map::collect_dead_registered_maps_post_trace(full_trace);
        self.dead_sets = crate::set::collect_dead_registered_sets_post_trace(full_trace);
        self.dead_buffers = crate::buffer::collect_dead_registered_buffers_post_trace(full_trace);
        self.dead_typed_arrays =
            crate::typedarray::collect_dead_registered_typed_arrays_post_trace(full_trace);
        if !self.dead_maps.is_empty()
            || !self.dead_sets.is_empty()
            || !self.dead_buffers.is_empty()
            || !self.dead_typed_arrays.is_empty()
        {
            self.subphase = SweepCycleSubphase::CollectionSideBuffers;
        }
        self
    }

    pub(super) fn step(&mut self, budget: usize) -> bool {
        match self.subphase {
            SweepCycleSubphase::CollectionSideBuffers => {
                let mut spent = 0usize;
                while spent < budget {
                    if let Some(addr) = self.dead_maps.pop() {
                        crate::map::finalize_collected_dead_map(addr);
                    } else if let Some(addr) = self.dead_sets.pop() {
                        crate::set::finalize_collected_dead_set(addr);
                    } else if let Some(addr) = self.dead_buffers.pop() {
                        crate::buffer::finalize_collected_dead_buffer(addr);
                    } else if let Some(addr) = self.dead_typed_arrays.pop() {
                        crate::typedarray::finalize_collected_dead_typed_array(addr);
                    } else {
                        self.subphase = SweepCycleSubphase::Malloc;
                        break;
                    }
                    spent += 1;
                }
                if self.dead_maps.is_empty()
                    && self.dead_sets.is_empty()
                    && self.dead_buffers.is_empty()
                    && self.dead_typed_arrays.is_empty()
                {
                    self.subphase = SweepCycleSubphase::Malloc;
                }
                false
            }
            SweepCycleSubphase::Malloc => {
                if self.malloc.step(budget) {
                    self.subphase = SweepCycleSubphase::ArenaObjects;
                }
                false
            }
            SweepCycleSubphase::ArenaObjects => {
                if self.arena.step(budget) {
                    self.arena.maybe_print_diag();
                    self.cleanup = Some(ArenaSweepCleanupState::new(
                        self.arena.block_has_live(),
                        self.arena.block_snapshots(),
                        self.reclaim_dead_old_blocks,
                        self.targeted_old_blocks.as_ref(),
                    ));
                    self.subphase = SweepCycleSubphase::BlockCleanup;
                }
                false
            }
            SweepCycleSubphase::BlockCleanup => {
                let cleanup = self.cleanup.as_mut().expect("sweep cleanup state exists");
                if cleanup.step(budget) {
                    let reset = cleanup.stats();
                    let freed_bytes = self
                        .malloc
                        .freed_bytes
                        .saturating_add(self.arena.freed_bytes);
                    self.stats = SweepTraceStats {
                        dead_bytes: freed_bytes,
                        freed_bytes,
                        reusable_bytes: reset.reusable_bytes,
                        returned_bytes: reset.deallocated_bytes,
                        reset_blocks: reset.reset_blocks,
                        deallocated_blocks: reset.deallocated_blocks,
                        deallocated_bytes: reset.deallocated_bytes,
                        retained_forwarded_stub_objects: self.arena.retained_forwarded_stub_objects,
                        retained_forwarded_stub_bytes: self.arena.retained_forwarded_stub_bytes,
                    };
                    self.subphase = SweepCycleSubphase::Done;
                    return true;
                }
                false
            }
            SweepCycleSubphase::Done => true,
        }
    }

    #[allow(dead_code)]
    pub(super) fn finish_unbounded(&mut self) -> SweepTraceStats {
        while !self.step(usize::MAX) {}
        self.stats()
    }

    pub(super) fn stats(&self) -> SweepTraceStats {
        self.stats
    }
}

struct ArenaSweepObjectsState {
    cursor: crate::arena::ArenaObjectCursor,
    block_snapshots: Vec<crate::arena::ArenaBlockSnapshot>,
    block_has_live: Vec<bool>,
    resettable_general_n: usize,
    old_block_start: usize,
    overflow_active: bool,
    do_age_bump: bool,
    reclaim_dead_old_blocks: bool,
    freed_bytes: u64,
    retained_forwarded_stub_objects: usize,
    retained_forwarded_stub_bytes: usize,
}

impl ArenaSweepObjectsState {
    fn new(do_age_bump: bool, reclaim_dead_old_blocks: bool) -> Self {
        let n_blocks = crate::arena::arena_block_count();
        let block_snapshots = crate::arena::arena_block_snapshots();
        crate::arena::old_pages_reset_sweep_accounting();
        Self {
            cursor: crate::arena::ArenaObjectCursor::new(crate::arena::ArenaWalkOrder::BlockIndex),
            block_snapshots,
            block_has_live: vec![false; n_blocks],
            resettable_general_n: crate::arena::general_block_count(),
            old_block_start: crate::arena::longlived_end(),
            // Wave 2: also arms the closure dynamic-props dead-payload arm
            // (one gate check per sweep-state build, not per object).
            overflow_active: !crate::object::overflow_fields_is_empty()
                || crate::closure::closure_dynamic_side_tables_nonempty(),
            do_age_bump,
            reclaim_dead_old_blocks,
            freed_bytes: 0,
            retained_forwarded_stub_objects: 0,
            retained_forwarded_stub_bytes: 0,
        }
    }

    fn step(&mut self, budget: usize) -> bool {
        let mut remaining = budget;
        while remaining > 0 {
            let Some((header_ptr, block_idx)) = self.cursor.next() else {
                return true;
            };
            remaining -= 1;
            self.process_object(header_ptr as *mut GcHeader, block_idx);
        }
        false
    }

    fn block_has_live(&self) -> &[bool] {
        &self.block_has_live
    }

    fn block_snapshots(&self) -> &[crate::arena::ArenaBlockSnapshot] {
        &self.block_snapshots
    }

    fn maybe_print_diag(&self) {
        if std::env::var_os("PERRY_GC_DIAG").is_none() {
            return;
        }
        let live_general = (0..self.resettable_general_n)
            .filter(|&i| self.block_has_live[i])
            .count();
        let live_ll = (self.resettable_general_n..self.block_has_live.len())
            .filter(|&i| self.block_has_live[i])
            .count();
        eprintln!(
            "[gc] blocks: general={} ({} live), longlived={} ({} live), freed_bytes={} retained_forwarded_stub_bytes={} retained_forwarded_stub_objects={}",
            self.resettable_general_n,
            live_general,
            self.block_has_live.len() - self.resettable_general_n,
            live_ll,
            self.freed_bytes,
            self.retained_forwarded_stub_bytes,
            self.retained_forwarded_stub_objects,
        );
    }

    fn process_object(&mut self, header: *mut GcHeader, block_idx: usize) {
        unsafe {
            let age_bump_this = self.do_age_bump && block_idx < self.resettable_general_n;
            let flags = (*header).gc_flags;
            if flags == 0 {
                self.reclaim_dead_object(header, block_idx);
                return;
            }
            if flags & GC_FLAG_PINNED != 0 {
                self.keep_live_object(header, block_idx, flags, age_bump_this, true);
                return;
            }
            if flags & GC_FLAG_FORWARDED != 0 {
                self.process_forwarded_object(header, block_idx, flags);
                return;
            }
            if flags & GC_FLAG_MARKED == 0 {
                self.reclaim_dead_object(header, block_idx);
            } else {
                self.keep_live_object(header, block_idx, flags, age_bump_this, false);
            }
        }
    }
}

impl ArenaSweepObjectsState {
    unsafe fn keep_live_object(
        &mut self,
        header: *mut GcHeader,
        block_idx: usize,
        flags: u8,
        age_bump_this: bool,
        pinned: bool,
    ) {
        if block_idx >= self.old_block_start {
            crate::arena::old_page_account_swept_object(
                header as usize,
                (*header).size as usize,
                true,
                pinned,
            );
        }
        if block_idx < self.block_has_live.len() {
            self.block_has_live[block_idx] = true;
        }
        if age_bump_this && flags & GC_FLAG_TENURED == 0 {
            if flags & GC_FLAG_HAS_SURVIVED != 0 {
                (*header).gc_flags =
                    (flags | GC_FLAG_TENURED) & !GC_FLAG_HAS_SURVIVED & !GC_FLAG_MARKED;
            } else {
                (*header).gc_flags = (flags | GC_FLAG_HAS_SURVIVED) & !GC_FLAG_MARKED;
            }
        } else {
            (*header).gc_flags = flags & !GC_FLAG_MARKED;
        }
    }

    unsafe fn process_forwarded_object(
        &mut self,
        header: *mut GcHeader,
        block_idx: usize,
        flags: u8,
    ) {
        let retain_stub = flags & GC_FLAG_MARKED != 0
            || (block_idx < self.resettable_general_n
                && crate::arena::general_block_in_recent_window(block_idx));
        if retain_stub {
            self.keep_live_object(header, block_idx, flags, false, false);
            if block_idx < self.resettable_general_n {
                self.retained_forwarded_stub_objects =
                    self.retained_forwarded_stub_objects.saturating_add(1);
                self.retained_forwarded_stub_bytes = self
                    .retained_forwarded_stub_bytes
                    .saturating_add((*header).size as usize);
            }
            return;
        }

        let total_size = (*header).size as usize;
        let dead_old = block_idx >= self.old_block_start;
        if dead_old {
            crate::arena::old_page_account_swept_object(header as usize, total_size, false, false);
        }
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        self.freed_bytes = self.freed_bytes.saturating_add(total_size as u64);
        layout_clear_for_ptr(user_ptr as usize);
        if self.overflow_active {
            gc_type_clear_dead_payload_side_tables((*header).obj_type, user_ptr as usize);
        }
        if self.reclaim_dead_old_blocks && dead_old {
            invalidate_dead_old_arena_header(header, total_size);
        } else {
            (*header).gc_flags = flags & !(GC_FLAG_FORWARDED | GC_FLAG_MARKED);
        }
    }

    unsafe fn reclaim_dead_object(&mut self, header: *mut GcHeader, block_idx: usize) {
        let total_size = (*header).size as usize;
        let dead_old = block_idx >= self.old_block_start;
        if dead_old {
            crate::arena::old_page_account_swept_object(header as usize, total_size, false, false);
        }
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        self.freed_bytes = self.freed_bytes.saturating_add(total_size as u64);
        finalize_dead_arena_payload(header, user_ptr, self.overflow_active);
        if self.reclaim_dead_old_blocks && dead_old {
            invalidate_dead_old_arena_header(header, total_size);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArenaSweepCleanupSubphase {
    General,
    Survivor,
    Old,
    Done,
}

struct ArenaSweepCleanupState {
    subphase: ArenaSweepCleanupSubphase,
    general: crate::arena::ArenaResetEmptyBlocksState,
    survivor: Option<crate::arena::SurvivorArenaReclaimDeadBlocksState>,
    old: Option<crate::arena::OldArenaReclaimDeadBlocksState>,
    stats: crate::arena::ArenaResetStats,
}

impl ArenaSweepCleanupState {
    fn new(
        block_has_live: &[bool],
        block_snapshots: &[crate::arena::ArenaBlockSnapshot],
        reclaim_dead_old_blocks: bool,
        targeted_old_blocks: Option<&crate::fast_hash::PtrHashSet<usize>>,
    ) -> Self {
        let survivor = reclaim_dead_old_blocks.then(|| {
            crate::arena::SurvivorArenaReclaimDeadBlocksState::new(block_has_live, block_snapshots)
        });
        let old = if reclaim_dead_old_blocks {
            Some(crate::arena::OldArenaReclaimDeadBlocksState::new_full(
                block_has_live,
                block_snapshots,
            ))
        } else {
            targeted_old_blocks.map(|selected| {
                crate::arena::OldArenaReclaimDeadBlocksState::new_selected(
                    block_has_live,
                    block_snapshots,
                    selected,
                )
            })
        };
        Self {
            subphase: ArenaSweepCleanupSubphase::General,
            general: crate::arena::ArenaResetEmptyBlocksState::new(block_has_live, block_snapshots),
            survivor,
            old,
            stats: crate::arena::ArenaResetStats::default(),
        }
    }

    fn step(&mut self, budget: usize) -> bool {
        match self.subphase {
            ArenaSweepCleanupSubphase::General => {
                if self.general.step(budget) {
                    self.stats = add_reset_stats(self.stats, self.general.stats());
                    self.subphase = ArenaSweepCleanupSubphase::Survivor;
                }
                false
            }
            ArenaSweepCleanupSubphase::Survivor => {
                if let Some(survivor) = self.survivor.as_mut() {
                    if !survivor.step(budget) {
                        return false;
                    }
                    self.stats = add_reset_stats(self.stats, survivor.stats());
                }
                self.subphase = ArenaSweepCleanupSubphase::Old;
                false
            }
            ArenaSweepCleanupSubphase::Old => {
                if let Some(old) = self.old.as_mut() {
                    if !old.step(budget) {
                        return false;
                    }
                    self.stats = add_reset_stats(self.stats, old.stats());
                }
                self.subphase = ArenaSweepCleanupSubphase::Done;
                true
            }
            ArenaSweepCleanupSubphase::Done => true,
        }
    }

    fn stats(&self) -> crate::arena::ArenaResetStats {
        self.stats
    }
}

fn add_reset_stats(
    lhs: crate::arena::ArenaResetStats,
    rhs: crate::arena::ArenaResetStats,
) -> crate::arena::ArenaResetStats {
    crate::arena::ArenaResetStats {
        reset_blocks: lhs.reset_blocks.saturating_add(rhs.reset_blocks),
        reusable_bytes: lhs.reusable_bytes.saturating_add(rhs.reusable_bytes),
        deallocated_blocks: lhs
            .deallocated_blocks
            .saturating_add(rhs.deallocated_blocks),
        deallocated_bytes: lhs.deallocated_bytes.saturating_add(rhs.deallocated_bytes),
    }
}

pub(super) fn pin_currently_marked_as_conservative() -> ConservativePinTraceStats {
    let mut stats = ConservativePinTraceStats::default();
    CONS_PINNED.with(|s| {
        let mut pinned = s.borrow_mut();
        crate::arena::arena_walk_objects(|header_ptr| {
            let header = header_ptr as *mut GcHeader;
            unsafe {
                if (*header).gc_flags & GC_FLAG_MARKED != 0 && pinned.insert(header as usize) {
                    stats.pinned_roots += 1;
                    stats.pinned_bytes += (*header).size as usize;
                }
            }
        });
        MALLOC_STATE.with(|m| {
            let m = m.borrow();
            for &header in m.objects.iter() {
                unsafe {
                    if (*header).gc_flags & GC_FLAG_MARKED != 0 && pinned.insert(header as usize) {
                        stats.pinned_roots += 1;
                        stats.pinned_bytes += (*header).size as usize;
                    }
                }
            }
        });
    });
    stats
}

/// Gen-GC Phase C4b-β: walk arena nursery objects and copy
/// non-pinned tenured ones into OLD_ARENA. Install a short-lived GC
/// forwarding pointer at the original nursery slot's user-payload
/// start. Returns evacuated object and byte counts (diagnostic only).
///
/// Candidate filter: the object must be
/// - in the nursery arena (not OLD, not LONGLIVED)
/// - MARKED (alive this cycle)
/// - TENURED (survived ≥2 minor GCs), unless
///   `PERRY_GC_FORCE_EVACUATE=1` is active for stress verification
/// - NOT in CONS_PINNED (no conservative root reaches it)
/// - NOT already FORWARDED (idempotent; duplicate evacuation is
///   safe-skipped)
///
/// Phase C4b-γ-2/3: this function is paired with
/// `rewrite_forwarded_references` and
/// `release_evacuated_original_forwarding_stubs` — every reference
/// site (heap fields, shadow stack, global roots) is rewalked AFTER
/// this function returns and any pointer to a forwarded object is
/// updated to the new address. The original's MARKED bit is cleared at
/// evac time, then its FORWARDED bit is cleared after rewrite/verify so
/// sweep treats the now-stale slot as dead and the nursery block can
/// reset; the new copy is marked MARKED so the rewrite walk picks up
/// its (copied) fields and so sweep keeps it alive.
pub(super) fn evacuate_tenured_nursery_objects_collecting(
    force_evacuation: bool,
    evacuated_new_headers: &mut Vec<*mut GcHeader>,
    evacuated_original_headers: &mut Vec<*mut GcHeader>,
) -> EvacuationTraceStats {
    let mut evacuated = EvacuationTraceStats::default();
    crate::arena::arena_walk_objects(|header_ptr| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            // Skip if not in nursery (LONGLIVED + OLD have their own arenas).
            if !crate::arena::pointer_in_nursery(user_ptr as usize) {
                return;
            }
            let flags = (*header).gc_flags;
            // Already evacuated (shouldn't happen — caller's filter
            // should prevent — but defend against duplicate calls).
            if flags & GC_FLAG_FORWARDED != 0 {
                return;
            }
            // Must be alive and normally tenured. The force mode is
            // evacuation stress only and is active exclusively when the
            // outer evacuation gate is enabled.
            if flags & GC_FLAG_MARKED == 0 {
                return;
            }
            if !force_evacuation && flags & GC_FLAG_TENURED == 0 {
                return;
            }
            if flags & GC_FLAG_PINNED != 0 {
                return;
            }
            if !gc_type_is_movable((*header).obj_type) {
                return;
            }
            // Conservative-pinning blocks evacuation.
            if is_conservatively_pinned(header) {
                return;
            }
            // Allocate the new home in OLD_ARENA. Same size +
            // alignment as the original; same obj_type.
            let total = (*header).size as usize;
            let payload = total - GC_HEADER_SIZE;
            let new_user = crate::arena::arena_alloc_gc_old(payload, 8, (*header).obj_type);
            // Copy the user payload bytes verbatim. The new
            // GcHeader was set up by arena_alloc_gc_old; we don't
            // copy the OLD header (its flags / size match the
            // new alloc by construction).
            std::ptr::copy_nonoverlapping(user_ptr, new_user, payload);
            // Install a GC-evacuation forwarding pointer at the original
            // nursery location. It is load-bearing only until the
            // rewrite/verify phase finishes.
            set_forwarding_address(header, new_user);
            // Clear MARKED on the original so, after the short-lived
            // FORWARDED bit is released, sweep frees its (now-stale)
            // nursery slot. The block can reset once every object in it
            // is either a released evacuation original or unmarked dead.
            (*header).gc_flags &= !GC_FLAG_MARKED;
            // Mark the new copy so (a) the rewrite walk visits
            // its fields and (b) sweep keeps it alive. The mark
            // bit is cleared inline by sweep on surviving objects.
            let new_header = (new_user as *mut u8).sub(GC_HEADER_SIZE) as *mut GcHeader;
            (*new_header)._reserved = (*header)._reserved;
            layout_transfer(user_ptr, new_user);
            (*new_header).gc_flags |= GC_FLAG_MARKED;
            gc_type_after_payload_move((*header).obj_type, user_ptr as usize, new_user as usize);
            // Carry TENURED forward — the new copy is logically
            // the same object, just relocated. Without this the
            // age-bump pass on the next cycle would treat it as
            // a fresh young object.
            (*new_header).gc_flags |= GC_FLAG_TENURED;
            evacuated_original_headers.push(header);
            evacuated_new_headers.push(new_header);
            evacuated.objects += 1;
            evacuated.bytes += total;
            evacuated.moved_objects += 1;
            evacuated.moved_bytes += total;
        }
    });
    evacuated
}

pub(super) fn old_object_pages_all_selected(
    header: *mut GcHeader,
    total_size: usize,
    selected_pages: &crate::fast_hash::PtrHashSet<usize>,
) -> bool {
    let overlaps = crate::arena::old_object_page_overlaps(header as usize, total_size);
    !overlaps.is_empty()
        && overlaps
            .iter()
            .all(|(page, _)| selected_pages.contains(page))
}

pub(super) fn old_object_pages_disjoint_from_selected(
    header: *mut GcHeader,
    total_size: usize,
    selected_pages: &crate::fast_hash::PtrHashSet<usize>,
) -> bool {
    crate::arena::old_object_page_overlaps(header as usize, total_size)
        .iter()
        .all(|(page, _)| !selected_pages.contains(page))
}

pub(super) fn evacuate_selected_old_pages_collecting(
    selected_pages: &crate::fast_hash::PtrHashSet<usize>,
    evacuated_new_headers: &mut Vec<*mut GcHeader>,
    evacuated_original_headers: &mut Vec<*mut GcHeader>,
) -> EvacuationTraceStats {
    let mut evacuated = EvacuationTraceStats::default();
    if selected_pages.is_empty() {
        return evacuated;
    }

    let source_blocks = crate::arena::old_arena_source_blocks_for_pages(selected_pages);
    let excluded_pages = if source_blocks.pages.is_empty() {
        selected_pages
    } else {
        &source_blocks.pages
    };

    crate::arena::old_arena_walk_objects_on_pages(selected_pages, |header_ptr| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            if !crate::arena::pointer_in_old_gen(user_ptr as usize) {
                return;
            }
            let flags = (*header).gc_flags;
            if flags & GC_FLAG_FORWARDED != 0 {
                return;
            }
            if flags & GC_FLAG_MARKED == 0 {
                return;
            }
            if flags & GC_FLAG_PINNED != 0 {
                return;
            }
            if !gc_type_is_movable((*header).obj_type) {
                return;
            }
            if is_conservatively_pinned(header) {
                return;
            }

            let total = (*header).size as usize;
            if !old_object_pages_all_selected(header, total, selected_pages) {
                return;
            }

            let payload = total - GC_HEADER_SIZE;
            let new_user = crate::arena::arena_alloc_gc_old_excluding_pages(
                payload,
                8,
                (*header).obj_type,
                excluded_pages,
            );
            std::ptr::copy_nonoverlapping(user_ptr, new_user, payload);
            set_forwarding_address(header, new_user);
            (*header).gc_flags &= !GC_FLAG_MARKED;

            let new_header = (new_user as *mut u8).sub(GC_HEADER_SIZE) as *mut GcHeader;
            debug_assert!(
                old_object_pages_disjoint_from_selected(new_header, total, excluded_pages),
                "old-page evacuation copy landed in a selected source block"
            );
            (*new_header)._reserved = (*header)._reserved;
            layout_transfer(user_ptr, new_user);
            (*new_header).gc_flags |= GC_FLAG_MARKED
                | GC_FLAG_TENURED
                | (flags & (GC_FLAG_SHAPE_SHARED | GC_FLAG_INTERNED));
            gc_type_after_payload_move((*header).obj_type, user_ptr as usize, new_user as usize);

            evacuated_original_headers.push(header);
            evacuated_new_headers.push(new_header);
            evacuated.objects = evacuated.objects.saturating_add(1);
            evacuated.bytes = evacuated.bytes.saturating_add(total);
            evacuated.moved_objects = evacuated.moved_objects.saturating_add(1);
            evacuated.moved_bytes = evacuated.moved_bytes.saturating_add(total);
            evacuated.old_page_moved_objects = evacuated.old_page_moved_objects.saturating_add(1);
            evacuated.old_page_moved_bytes = evacuated.old_page_moved_bytes.saturating_add(total);
        }
    });

    evacuated
}

pub(super) fn release_evacuated_original_forwarding_stubs(
    evacuated_original_headers: &[*mut GcHeader],
) -> EvacuationTraceStats {
    let mut released = EvacuationTraceStats::default();
    for &header in evacuated_original_headers {
        if header.is_null() {
            continue;
        }
        unsafe {
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            let original_in_old = crate::arena::pointer_in_old_gen(user_ptr as usize);
            let flags = (*header).gc_flags;
            if flags & GC_FLAG_FORWARDED == 0 {
                continue;
            }
            (*header).gc_flags = flags & !GC_FLAG_FORWARDED;
            if original_in_old {
                crate::arena::old_arena_page_index_remove_object(
                    header as usize,
                    (*header).size as usize,
                );
            }
            released.released_original_objects += 1;
            released.released_original_bytes += (*header).size as usize;
        }
    }
    released
}

#[cfg(test)]
pub(super) fn evacuate_tenured_nursery_objects_with_force(
    force_evacuation: bool,
) -> EvacuationTraceStats {
    let mut evacuated_new_headers = Vec::new();
    let mut evacuated_original_headers = Vec::new();
    evacuate_tenured_nursery_objects_collecting(
        force_evacuation,
        &mut evacuated_new_headers,
        &mut evacuated_original_headers,
    )
}

#[cfg(test)]
pub(super) fn evacuate_tenured_nursery_objects() -> EvacuationTraceStats {
    evacuate_tenured_nursery_objects_with_force(gc_force_evacuate_enabled())
}
