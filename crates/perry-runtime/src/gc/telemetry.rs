use super::*;

pub struct GcStats {
    pub collection_count: u64,
    pub total_freed_bytes: u64,
    pub last_pause_us: u64,
}

thread_local! {
    pub(super) static GC_STATS: RefCell<GcStats> = const { RefCell::new(GcStats {
        collection_count: 0,
        total_freed_bytes: 0,
        last_pause_us: 0,
    }) };
}

#[derive(Clone, Copy, Default)]
pub(super) struct RememberedSetTraceStats {
    pub(super) entries_scanned: usize,
    pub(super) valid_roots: usize,
    pub(super) newly_marked: usize,
    pub(super) dirty_pages_before: usize,
    pub(super) dirty_pages_after: usize,
    pub(super) dirty_pages_scanned: usize,
    pub(super) old_objects_considered: usize,
    pub(super) dirty_objects_scanned: usize,
    pub(super) dirty_slot_pages_considered: usize,
    pub(super) dirty_slot_ranges_scanned: usize,
    pub(super) dirty_slots_scanned: usize,
}

#[derive(Clone, Copy, Default)]
pub(super) struct BlockPersistTraceStats {
    pub(super) iterations: usize,
    pub(super) candidate_blocks: usize,
    pub(super) live_blocks: usize,
    pub(super) marked_objects: usize,
}

#[derive(Clone, Copy, Default)]
pub(super) struct EvacuationTraceStats {
    // Compatibility fields: historically these were the moved counts.
    pub(super) objects: usize,
    pub(super) bytes: usize,
    pub(super) moved_objects: usize,
    pub(super) moved_bytes: usize,
    pub(super) old_page_moved_objects: usize,
    pub(super) old_page_moved_bytes: usize,
    pub(super) released_original_objects: usize,
    pub(super) released_original_bytes: usize,
    pub(super) released_original_reusable_bytes: usize,
    pub(super) released_original_returned_bytes: usize,
    pub(super) retained_forwarded_stub_objects: usize,
    pub(super) retained_forwarded_stub_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CopiedMinorFallbackReason {
    None,
    NotAttempted,
    BarriersInactive,
    ConservativeStack,
    CopyOnlyRoots,
    MallocRegistryUnavailable,
    PinnedYoungRoot,
    PinnedYoungDirtySlot,
    PinnedYoungTransitive,
}

impl CopiedMinorFallbackReason {
    #[inline]
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::NotAttempted => "not_attempted",
            Self::BarriersInactive => "barriers_inactive",
            Self::ConservativeStack => "conservative_stack",
            Self::CopyOnlyRoots => "copy_only_roots",
            Self::MallocRegistryUnavailable => "malloc_registry_unavailable",
            Self::PinnedYoungRoot => "pinned_young_root",
            Self::PinnedYoungDirtySlot => "pinned_young_dirty_slot",
            Self::PinnedYoungTransitive => "pinned_young_transitive",
        }
    }
}

impl Default for CopiedMinorFallbackReason {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct CopyingNurseryTraceStats {
    pub(super) eligible: bool,
    pub(super) copied_objects: usize,
    pub(super) copied_bytes: usize,
    pub(super) promoted_objects: usize,
    pub(super) promoted_bytes: usize,
    pub(super) large_excluded_objects: usize,
    pub(super) large_excluded_bytes: usize,
    pub(super) reset_blocks: usize,
    pub(super) malloc_validation_lookups: usize,
    pub(super) malloc_registry_rebuilds: u64,
    pub(super) malloc_sweep_due: bool,
    pub(super) fallback_reason: CopiedMinorFallbackReason,
}

#[derive(Clone, Copy, Default)]
pub(super) struct LegacyRootTraceStats {
    pub(super) registered_rust_scanners: usize,
    pub(super) registered_ffi_scanners: usize,
    pub(super) emitted_roots: usize,
    pub(super) emitted_young_roots: usize,
    pub(super) emitted_old_roots: usize,
    pub(super) emitted_malloc_roots: usize,
    pub(super) malformed_roots: usize,
    pub(super) pinned_roots: usize,
    pub(super) pinned_bytes: usize,
}

#[derive(Clone, Copy, Default)]
pub(super) struct ConservativeRootTraceStats {
    pub(super) root_count: usize,
}

#[derive(Clone, Copy, Default)]
pub(super) struct ConservativePinTraceStats {
    pub(super) pinned_roots: usize,
    pub(super) pinned_bytes: usize,
}

#[derive(Clone, Copy, Default)]
pub(super) struct ShadowRootTraceStats {
    pub(super) slots_scanned: usize,
    pub(super) nonzero_slots: usize,
    pub(super) pointer_roots: usize,
    pub(super) rewritten_slots: usize,
}

impl ShadowRootTraceStats {
    pub(super) fn record_scan(&mut self, bits: u64) {
        self.slots_scanned = self.slots_scanned.saturating_add(1);
        if bits == 0 {
            return;
        }
        self.nonzero_slots = self.nonzero_slots.saturating_add(1);
        if shadow_slot_pointer_root(bits) {
            self.pointer_roots = self.pointer_roots.saturating_add(1);
        }
    }

    pub(super) fn record_rewrite(&mut self) {
        self.rewritten_slots = self.rewritten_slots.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct LayoutScanTraceStats {
    pub(super) pointer_slots_read: usize,
    pub(super) masked_pointer_slots_read: usize,
    pub(super) unknown_layout_slots_read: usize,
    pub(super) pointer_free_ranges_skipped: usize,
    pub(super) pointer_free_slots_skipped: usize,
}

impl LayoutScanTraceStats {
    pub(super) const fn zero() -> Self {
        Self {
            pointer_slots_read: 0,
            masked_pointer_slots_read: 0,
            unknown_layout_slots_read: 0,
            pointer_free_ranges_skipped: 0,
            pointer_free_slots_skipped: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HeapChildSlotReadKind {
    Prefix,
    Masked,
    Unknown,
}

thread_local! {
    pub(super) static LAYOUT_SCAN_TRACE_ACTIVE: Cell<bool> = const { Cell::new(false) };
    pub(super) static LAYOUT_SCAN_TRACE_STATS: Cell<LayoutScanTraceStats> =
        const { Cell::new(LayoutScanTraceStats::zero()) };
}

#[inline]
pub(super) fn begin_layout_scan_trace() {
    LAYOUT_SCAN_TRACE_STATS.with(|stats| stats.set(LayoutScanTraceStats::zero()));
    LAYOUT_SCAN_TRACE_ACTIVE.with(|active| active.set(true));
}

#[inline]
pub(super) fn finish_layout_scan_trace() -> LayoutScanTraceStats {
    LAYOUT_SCAN_TRACE_ACTIVE.with(|active| {
        if active.replace(false) {
            LAYOUT_SCAN_TRACE_STATS.with(|stats| {
                let snapshot = stats.get();
                stats.set(LayoutScanTraceStats::zero());
                snapshot
            })
        } else {
            LayoutScanTraceStats::zero()
        }
    })
}

#[inline]
pub(super) fn layout_scan_trace_active() -> bool {
    LAYOUT_SCAN_TRACE_ACTIVE.with(Cell::get)
}

#[inline]
pub(super) fn record_layout_child_slot_read(kind: HeapChildSlotReadKind) {
    if !layout_scan_trace_active() {
        return;
    }
    LAYOUT_SCAN_TRACE_STATS.with(|stats| {
        let mut current = stats.get();
        current.pointer_slots_read = current.pointer_slots_read.saturating_add(1);
        match kind {
            HeapChildSlotReadKind::Prefix => {}
            HeapChildSlotReadKind::Masked => {
                current.masked_pointer_slots_read =
                    current.masked_pointer_slots_read.saturating_add(1);
            }
            HeapChildSlotReadKind::Unknown => {
                current.unknown_layout_slots_read =
                    current.unknown_layout_slots_read.saturating_add(1);
            }
        }
        stats.set(current);
    });
}

#[inline]
pub(super) fn record_layout_pointer_free_range_skipped(slot_count: usize) {
    if slot_count == 0 || !layout_scan_trace_active() {
        return;
    }
    LAYOUT_SCAN_TRACE_STATS.with(|stats| {
        let mut current = stats.get();
        current.pointer_free_ranges_skipped = current.pointer_free_ranges_skipped.saturating_add(1);
        current.pointer_free_slots_skipped = current
            .pointer_free_slots_skipped
            .saturating_add(slot_count);
        stats.set(current);
    });
}

#[derive(Clone, Copy, Default)]
pub(super) struct BarrierTraceCounters {
    pub(super) calls: u64,
    pub(super) non_pointer_parent_skips: u64,
    pub(super) non_pointer_child_skips: u64,
    pub(super) parent_not_old_skips: u64,
    pub(super) child_not_young_skips: u64,
    pub(super) remembered_set_insert_attempts: u64,
    pub(super) new_inserts: u64,
    pub(super) dirty_page_mark_attempts: u64,
    pub(super) new_dirty_pages: u64,
    pub(super) conservative_parent_span_marks: u64,
}

impl BarrierTraceCounters {
    pub(super) const fn zero() -> Self {
        Self {
            calls: 0,
            non_pointer_parent_skips: 0,
            non_pointer_child_skips: 0,
            parent_not_old_skips: 0,
            child_not_young_skips: 0,
            remembered_set_insert_attempts: 0,
            new_inserts: 0,
            dirty_page_mark_attempts: 0,
            new_dirty_pages: 0,
            conservative_parent_span_marks: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum BarrierTraceCounter {
    Calls,
    NonPointerParentSkips,
    NonPointerChildSkips,
    ParentNotOldSkips,
    ChildNotYoungSkips,
    RememberedSetInsertAttempts,
    NewInserts,
    DirtyPageMarkAttempts,
    NewDirtyPages,
    ConservativeParentSpanMarks,
}

pub(super) struct GcCycleTrace {
    pub(super) collection_kind: GcCollectionKind,
    pub(super) trigger_kind: GcTriggerKind,
    pub(super) steps_before: GcStepSnapshot,
    pub(super) pause_us: u64,
    pub(super) phase_us: BTreeMap<&'static str, u64>,
    pub(super) arena_before: crate::arena::ArenaTelemetrySnapshot,
    pub(super) malloc_before: usize,
    pub(super) remembered_set_before: usize,
    pub(super) remembered_set: RememberedSetTraceStats,
    pub(super) old_pages: crate::arena::OldPageSummary,
    pub(super) conservative_root_count: usize,
    pub(super) conservative_pinned: usize,
    pub(super) conservative_pinned_bytes: usize,
    pub(super) legacy_copy_only_scanner_pinned: LegacyRootTraceStats,
    pub(super) shadow_roots: ShadowRootTraceStats,
    pub(super) layout_scans: LayoutScanTraceStats,
    pub(super) evacuation_policy: EvacuationPolicyDecision,
    pub(super) evacuation: EvacuationTraceStats,
    pub(super) copying_nursery: CopyingNurseryTraceStats,
    pub(super) block_persist: BlockPersistTraceStats,
    pub(super) sweep: SweepTraceStats,
    pub(super) write_barrier: BarrierTraceCounters,
}

impl GcCycleTrace {
    pub(super) fn new(
        collection_kind: GcCollectionKind,
        trigger: GcTriggerSnapshot,
    ) -> Option<Self> {
        let steps_before = trigger.steps_before?;
        begin_layout_scan_trace();
        let mut phase_us = BTreeMap::new();
        for name in [
            "build_valid_pointer_set",
            "root_marking",
            "remembered_set_marking",
            "trace_worklist",
            "block_persistence",
            "evacuation",
            "copying_nursery",
            "reference_rewrite",
            "sweep",
            "remembered_set_clear",
            "conservative_pin_clear",
            "malloc_trim",
        ] {
            phase_us.insert(name, 0);
        }
        Some(Self {
            collection_kind,
            trigger_kind: trigger.kind,
            steps_before,
            pause_us: 0,
            phase_us,
            arena_before: crate::arena::arena_telemetry_snapshot(),
            malloc_before: malloc_object_count(),
            remembered_set_before: remembered_set_size(),
            remembered_set: RememberedSetTraceStats::default(),
            old_pages: crate::arena::OldPageSummary::default(),
            conservative_root_count: 0,
            conservative_pinned: 0,
            conservative_pinned_bytes: 0,
            legacy_copy_only_scanner_pinned: LegacyRootTraceStats::default(),
            shadow_roots: ShadowRootTraceStats::default(),
            layout_scans: LayoutScanTraceStats::default(),
            evacuation_policy: EvacuationPolicyDecision::default(),
            evacuation: EvacuationTraceStats::default(),
            copying_nursery: CopyingNurseryTraceStats {
                fallback_reason: CopiedMinorFallbackReason::NotAttempted,
                ..CopyingNurseryTraceStats::default()
            },
            block_persist: BlockPersistTraceStats::default(),
            sweep: SweepTraceStats::default(),
            write_barrier: take_write_barrier_trace_counters(),
        })
    }

    #[inline]
    pub(super) fn record_phase(&mut self, name: &'static str, elapsed: Duration) {
        *self.phase_us.entry(name).or_insert(0) += elapsed.as_micros() as u64;
    }

    pub(super) fn capture_layout_scans(&mut self) {
        if layout_scan_trace_active() {
            self.layout_scans = finish_layout_scan_trace();
        }
    }

    pub(super) fn into_json(mut self, steps_after: GcStepSnapshot) -> serde_json::Value {
        self.capture_layout_scans();
        let arena_after = crate::arena::arena_telemetry_snapshot();
        let malloc_after = malloc_object_count();
        let remembered_set_after = remembered_set_size();
        let malloc_kinds = take_malloc_kind_telemetry_json();
        serde_json::json!({
            "event": "gc_cycle",
            "collection_kind": self.collection_kind.as_str(),
            "pause_us": self.pause_us,
            "phase_us": self.phase_us,
            "arena_bytes": {
                "before": arena_snapshot_json(self.arena_before),
                "after": arena_snapshot_json(arena_after),
            },
            "malloc_objects": {
                "before": self.malloc_before,
                "after": malloc_after,
            },
            "malloc_kinds": malloc_kinds,
            "remembered_set": {
                "before": self.remembered_set_before,
                "after": remembered_set_after,
                "entries_scanned": self.remembered_set.entries_scanned,
                "valid_roots": self.remembered_set.valid_roots,
                "newly_marked": self.remembered_set.newly_marked,
                "dirty_pages_before": self.remembered_set.dirty_pages_before,
                "dirty_pages_after": remembered_dirty_page_count(),
                "dirty_pages_scanned": self.remembered_set.dirty_pages_scanned,
                "old_objects_considered": self.remembered_set.old_objects_considered,
                "dirty_objects_scanned": self.remembered_set.dirty_objects_scanned,
                "dirty_slot_pages_considered": self.remembered_set.dirty_slot_pages_considered,
                "dirty_slot_ranges_scanned": self.remembered_set.dirty_slot_ranges_scanned,
                "dirty_slots_scanned": self.remembered_set.dirty_slots_scanned,
            },
            "old_pages": {
                "pages": self.old_pages.pages,
                "allocated_bytes": self.old_pages.allocated_bytes,
                "live_bytes": self.old_pages.live_bytes,
                "dead_bytes": self.old_pages.dead_bytes,
                "reusable_bytes": self.old_pages.reusable_bytes,
                "returned_bytes": self.old_pages.returned_bytes,
                "pinned_bytes": self.old_pages.pinned_bytes,
                "object_count": self.old_pages.object_count,
                "live_object_count": self.old_pages.live_object_count,
                "dead_object_count": self.old_pages.dead_object_count,
                "pinned_object_count": self.old_pages.pinned_object_count,
                "dirty_pages": self.old_pages.dirty_pages,
                "dirty_slots": self.old_pages.dirty_slots,
                "fragmented_pages": self.old_pages.fragmented_pages,
                "evacuation_eligible_pages": self.old_pages.evacuation_eligible_pages,
            },
            "conservative_root_count": self.conservative_root_count,
            "conservative_pinned": self.conservative_pinned,
            "conservative_pinned_bytes": self.conservative_pinned_bytes,
            "legacy_copy_only_scanner_pinned": {
                "registered_rust_scanners": self.legacy_copy_only_scanner_pinned.registered_rust_scanners,
                "registered_ffi_scanners": self.legacy_copy_only_scanner_pinned.registered_ffi_scanners,
                "emitted_roots": self.legacy_copy_only_scanner_pinned.emitted_roots,
                "emitted_young_roots": self.legacy_copy_only_scanner_pinned.emitted_young_roots,
                "emitted_old_roots": self.legacy_copy_only_scanner_pinned.emitted_old_roots,
                "emitted_malloc_roots": self.legacy_copy_only_scanner_pinned.emitted_malloc_roots,
                "malformed_roots": self.legacy_copy_only_scanner_pinned.malformed_roots,
                "roots": self.legacy_copy_only_scanner_pinned.pinned_roots,
                "bytes": self.legacy_copy_only_scanner_pinned.pinned_bytes,
            },
            "shadow_roots": {
                "slots_scanned": self.shadow_roots.slots_scanned,
                "nonzero_slots": self.shadow_roots.nonzero_slots,
                "pointer_roots": self.shadow_roots.pointer_roots,
                "rewritten_slots": self.shadow_roots.rewritten_slots,
            },
            "layout_scans": {
                "pointer_slots_read": self.layout_scans.pointer_slots_read,
                "masked_pointer_slots_read": self.layout_scans.masked_pointer_slots_read,
                "unknown_layout_slots_read": self.layout_scans.unknown_layout_slots_read,
                "pointer_free_ranges_skipped": self.layout_scans.pointer_free_ranges_skipped,
                "pointer_free_slots_skipped": self.layout_scans.pointer_free_slots_skipped,
            },
            "evacuation": {
                "objects": self.evacuation.objects,
                "bytes": self.evacuation.bytes,
                "moved_objects": self.evacuation.moved_objects,
                "moved_bytes": self.evacuation.moved_bytes,
                "old_page_moved_objects": self.evacuation.old_page_moved_objects,
                "old_page_moved_bytes": self.evacuation.old_page_moved_bytes,
                "released_original_objects": self.evacuation.released_original_objects,
                "released_original_bytes": self.evacuation.released_original_bytes,
                "released_original_reusable_bytes": self.evacuation.released_original_reusable_bytes,
                "released_original_returned_bytes": self.evacuation.released_original_returned_bytes,
                "retained_forwarded_stub_objects": self.evacuation.retained_forwarded_stub_objects,
                "retained_forwarded_stub_bytes": self.evacuation.retained_forwarded_stub_bytes,
            },
            "copying_nursery": {
                "eligible": self.copying_nursery.eligible,
                "copied_objects": self.copying_nursery.copied_objects,
                "copied_bytes": self.copying_nursery.copied_bytes,
                "promoted_objects": self.copying_nursery.promoted_objects,
                "promoted_bytes": self.copying_nursery.promoted_bytes,
                "large_excluded_objects": self.copying_nursery.large_excluded_objects,
                "large_excluded_bytes": self.copying_nursery.large_excluded_bytes,
                "reset_blocks": self.copying_nursery.reset_blocks,
                "malloc_validation_lookups": self.copying_nursery.malloc_validation_lookups,
                "malloc_registry_rebuilds": self.copying_nursery.malloc_registry_rebuilds,
                "malloc_sweep_due": self.copying_nursery.malloc_sweep_due,
                "fallback_reason": self.copying_nursery.fallback_reason.as_str(),
            },
            "evacuation_policy": {
                "allowed": self.evacuation_policy.allowed,
                "considered": self.evacuation_policy.considered,
                "force": self.evacuation_policy.force,
                "enabled": self.evacuation_policy.enabled,
                "reason": self.evacuation_policy.reason,
                "tenured_still_in_nursery_bytes": self.evacuation_policy.snapshot.tenured_still_in_nursery_bytes,
                "candidate_bytes": self.evacuation_policy.snapshot.candidate_bytes,
                "candidate_objects": self.evacuation_policy.snapshot.candidate_objects,
                "candidate_ratio_pct": self.evacuation_policy.snapshot.candidate_ratio_pct(),
                "reclaimable_candidate_bytes": self.evacuation_policy.snapshot.reclaimable_candidate_bytes,
                "reclaimable_candidate_objects": self.evacuation_policy.snapshot.reclaimable_candidate_objects,
                "reclaimable_candidate_ratio_pct": self.evacuation_policy.snapshot.reclaimable_candidate_ratio_pct(),
                "old_page_candidate_pages": self.evacuation_policy.snapshot.old_page_candidate_pages,
                "old_page_selected_pages": self.evacuation_policy.snapshot.old_page_selected_pages,
                "old_page_selected_live_bytes": self.evacuation_policy.snapshot.old_page_selected_live_bytes,
                "old_page_reclaimable_bytes": self.evacuation_policy.snapshot.old_page_reclaimable_bytes,
                "old_page_skipped_pinned_pages": self.evacuation_policy.snapshot.old_page_skipped_pinned_pages,
                "retained_forwarded_stub_bytes": self.evacuation_policy.snapshot.retained_forwarded_stub_bytes,
                "retained_forwarded_stub_objects": self.evacuation_policy.snapshot.retained_forwarded_stub_objects,
                "conservative_pinned_bytes": self.evacuation_policy.snapshot.conservative_pinned_bytes,
                "rss_bytes": self.evacuation_policy.snapshot.rss_bytes,
                "previous_pause_us": self.evacuation_policy.snapshot.previous_pause_us,
                "pre_evac_pause_us": self.evacuation_policy.snapshot.pre_evac_pause_us,
            },
            "block_persist": {
                "iterations": self.block_persist.iterations,
                "candidate_blocks": self.block_persist.candidate_blocks,
                "live_blocks": self.block_persist.live_blocks,
                "marked_objects": self.block_persist.marked_objects,
            },
            "sweep": {
                "dead_bytes": self.sweep.dead_bytes,
                "freed_bytes": self.sweep.freed_bytes,
                "reusable_bytes": self.sweep.reusable_bytes,
                "returned_bytes": self.sweep.returned_bytes,
                "reset_blocks": self.sweep.reset_blocks,
                "deallocated_blocks": self.sweep.deallocated_blocks,
                "deallocated_bytes": self.sweep.deallocated_bytes,
                "retained_forwarded_stub_objects": self.sweep.retained_forwarded_stub_objects,
                "retained_forwarded_stub_bytes": self.sweep.retained_forwarded_stub_bytes,
            },
            "write_barrier": {
                "calls": self.write_barrier.calls,
                "non_pointer_parent_skips": self.write_barrier.non_pointer_parent_skips,
                "non_pointer_child_skips": self.write_barrier.non_pointer_child_skips,
                "parent_not_old_skips": self.write_barrier.parent_not_old_skips,
                "child_not_young_skips": self.write_barrier.child_not_young_skips,
                "remembered_set_insert_attempts": self.write_barrier.remembered_set_insert_attempts,
                "new_inserts": self.write_barrier.new_inserts,
                "dirty_page_mark_attempts": self.write_barrier.dirty_page_mark_attempts,
                "new_dirty_pages": self.write_barrier.new_dirty_pages,
                "conservative_parent_span_marks": self.write_barrier.conservative_parent_span_marks,
            },
            "trigger": {
                "kind": self.trigger_kind.as_str(),
            },
            "steps": steps_json(self.steps_before, steps_after),
        })
    }

    pub(super) fn emit(self, steps_after: GcStepSnapshot) {
        let event = self.into_json(steps_after);
        if let Ok(line) = serde_json::to_string(&event) {
            eprintln!("{line}");
        }
    }
}

pub(super) struct GcCollectOutcome {
    pub(super) freed_bytes: u64,
    pub(super) malloc_swept: bool,
    pub(super) trace: Option<GcCycleTrace>,
}

pub(super) struct CopiedMinorFastPathOutcome {
    pub(super) freed_bytes: u64,
    pub(super) malloc_swept: bool,
}

pub(super) fn gc_last_pause_us() -> u64 {
    GC_STATS.with(|stats| stats.borrow().last_pause_us)
}

impl GcCollectOutcome {
    #[inline]
    pub(super) fn emit_after_current(self) -> u64 {
        let Self {
            freed_bytes, trace, ..
        } = self;
        if let Some(trace) = trace {
            trace.emit(GcStepSnapshot::current());
        }
        freed_bytes
    }
}

#[inline]
pub(super) fn trace_phase_start(trace: &Option<GcCycleTrace>) -> Option<Instant> {
    trace.as_ref().map(|_| Instant::now())
}

#[inline]
pub(super) fn trace_phase_record(
    trace: &mut Option<GcCycleTrace>,
    name: &'static str,
    start: Option<Instant>,
) {
    if let (Some(trace), Some(start)) = (trace.as_mut(), start) {
        trace.record_phase(name, start.elapsed());
    }
}

#[inline]
pub(super) fn malloc_object_count() -> usize {
    MALLOC_STATE.with(|s| s.borrow().objects.len())
}

pub(super) fn malloc_kind_telemetry_row(
    obj_type: u8,
    counters: MallocKindTelemetry,
) -> serde_json::Value {
    serde_json::json!({
        "obj_type": obj_type,
        "kind": gc_type_name(obj_type),
        "allocated_count": counters.allocated_count,
        "allocated_bytes": counters.allocated_bytes,
        "realloc_count": counters.realloc_count,
        "realloc_old_bytes": counters.realloc_old_bytes,
        "realloc_new_bytes": counters.realloc_new_bytes,
        "freed_count": counters.freed_count,
        "freed_bytes": counters.freed_bytes,
        "survivor_count": counters.survivor_count,
        "survivor_bytes": counters.survivor_bytes,
        "copied_minor_validation_lookups": counters.copied_minor_validation_lookups,
    })
}

pub(super) fn malloc_kind_telemetry_json_from_snapshot(
    snapshot: [MallocKindTelemetry; MALLOC_KIND_BUCKET_COUNT],
) -> serde_json::Value {
    let mut rows = Vec::with_capacity(MALLOC_KIND_BUCKET_COUNT);
    for info in gc_type_infos() {
        let obj_type = info.type_id;
        rows.push(malloc_kind_telemetry_row(
            obj_type,
            snapshot[obj_type as usize],
        ));
    }
    rows.push(malloc_kind_telemetry_row(
        0,
        snapshot[MALLOC_KIND_UNKNOWN_INDEX],
    ));
    serde_json::Value::Array(rows)
}

pub(super) fn take_malloc_kind_telemetry_json() -> serde_json::Value {
    let snapshot = MALLOC_STATE.with(|s| s.borrow_mut().take_kind_telemetry());
    malloc_kind_telemetry_json_from_snapshot(snapshot)
}

pub(super) fn arena_region_json(region: crate::arena::ArenaRegionTelemetry) -> serde_json::Value {
    serde_json::json!({
        "in_use_bytes": region.in_use_bytes,
        "reserved_bytes": region.reserved_bytes,
        "block_count": region.block_count,
    })
}

pub(super) fn arena_snapshot_json(
    snapshot: crate::arena::ArenaTelemetrySnapshot,
) -> serde_json::Value {
    serde_json::json!({
        "arena": arena_region_json(snapshot.arena),
        "survivor0": arena_region_json(snapshot.survivor0),
        "survivor1": arena_region_json(snapshot.survivor1),
        "longlived": arena_region_json(snapshot.longlived),
        "old": arena_region_json(snapshot.old),
        "total_in_use_bytes": snapshot.total_in_use_bytes,
        "total_reserved_bytes": snapshot.total_reserved_bytes,
        "total_block_count": snapshot.total_block_count,
    })
}

pub(super) fn steps_json(before: GcStepSnapshot, after: GcStepSnapshot) -> serde_json::Value {
    serde_json::json!({
        "arena_step_bytes": {
            "before": before.arena_step_bytes,
            "after": after.arena_step_bytes,
        },
        "next_arena_trigger_bytes": {
            "before": before.next_arena_trigger_bytes,
            "after": after.next_arena_trigger_bytes,
        },
        "malloc_step": {
            "before": before.malloc_step,
            "after": after.malloc_step,
        },
        "next_malloc_trigger": {
            "before": before.next_malloc_trigger,
            "after": after.next_malloc_trigger,
        },
        "trigger_bumped": {
            "before": before.trigger_bumped,
            "after": after.trigger_bumped,
        },
    })
}

// ---------------------------------------------------------------------------
// Phase A — precise root tracking via shadow stack
// (docs/generational-gc-plan.md Phase A)
// ---------------------------------------------------------------------------
//
// Each compiled function gets a *shadow-stack frame* that holds the
// currently-live heap-pointer-typed locals. Codegen emits:
//   - push at function entry with a precomputed slot count
//   - slot stores at each safepoint (allocation + runtime-call sites)
//   - pop at every return path
//
// The shadow stack is built but not yet consumed by GC in this phase.
// Phase B+ will teach the GC tracer to walk it as a precise-root source
// in parallel with the existing conservative scanner.
//
// Layout: the shadow stack is a contiguous `Vec<u64>` (per-thread).
// Each frame is:
//   [u64 prev_frame_top, u64 slot_count, u64 slot_0, u64 slot_1, ...]
// `SHADOW_STACK_FRAME_TOP` points at the current frame's slot_0 so
// slot stores are a single indexed write. `prev_frame_top` is the
// saved top from before this frame was pushed — so pop is a single
// load + store.
//
// Slots hold NaN-boxed `JSValue` bits (u64) — same format codegen
// already uses for pointer-typed locals. The GC tracer in Phase B+
// will call `try_mark_value` on each non-zero slot, matching the
// closure-capture tracer's pattern.
