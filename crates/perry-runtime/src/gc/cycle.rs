use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GcCyclePhase {
    BuildValidPointerSet,
    RootScan,
    MarkPropagation,
    BlockPersistence,
    AtomicFinalize,
    Sweep,
    Reclaim,
    Complete,
}

impl GcCyclePhase {
    #[inline]
    pub(super) const fn ffi_code(self) -> u32 {
        match self {
            Self::BuildValidPointerSet => 1,
            Self::RootScan => 2,
            Self::MarkPropagation => 3,
            Self::BlockPersistence => 4,
            Self::AtomicFinalize => 5,
            Self::Sweep => 6,
            Self::Reclaim => 7,
            Self::Complete => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct GcWorkBudget {
    work_units: usize,
}

impl GcWorkBudget {
    #[inline]
    pub(super) const fn bounded(work_units: usize) -> Self {
        Self { work_units }
    }

    #[inline]
    pub(super) const fn unbounded() -> Self {
        Self {
            work_units: usize::MAX,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct GcCycleStepResult {
    pub(super) phase: GcCyclePhase,
    pub(super) completed: bool,
}

struct TraceWorklistCycleState {
    worklist: Vec<*mut GcHeader>,
    cursor: usize,
    minor_only: bool,
}

impl TraceWorklistCycleState {
    fn new(minor_only: bool) -> Self {
        Self {
            worklist: take_mark_seeds(),
            cursor: 0,
            minor_only,
        }
    }

    fn step(&mut self, valid_ptrs: &ValidPointerSet, budget: usize) -> bool {
        drain_trace_worklist_step(
            &mut self.worklist,
            &mut self.cursor,
            valid_ptrs,
            self.minor_only,
            budget,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BlockPersistSubphase {
    StartIteration,
    ScanLiveBlocks,
    MarkLiveBlockObjects,
    DrainMarkedObjects,
    Done,
}

struct BlockPersistCycleState {
    subphase: BlockPersistSubphase,
    stats: BlockPersistTraceStats,
    worklist: Vec<*mut GcHeader>,
    worklist_cursor: usize,
    arena_cursor: Option<crate::arena::ArenaObjectCursor>,
    block_has_live: Vec<bool>,
    general_n: usize,
    persist_low: usize,
    newly_marked: usize,
}

impl BlockPersistCycleState {
    fn new() -> Self {
        Self {
            subphase: BlockPersistSubphase::StartIteration,
            stats: BlockPersistTraceStats::default(),
            worklist: Vec::new(),
            worklist_cursor: 0,
            arena_cursor: None,
            block_has_live: Vec::new(),
            general_n: 0,
            persist_low: 0,
            newly_marked: 0,
        }
    }

    fn step(&mut self, valid_ptrs: &ValidPointerSet, budget: usize) -> bool {
        let mut remaining = budget;
        loop {
            match self.subphase {
                BlockPersistSubphase::StartIteration => {
                    self.begin_iteration();
                }
                BlockPersistSubphase::ScanLiveBlocks => {
                    if !self.scan_live_blocks(&mut remaining) {
                        return false;
                    }
                    self.finish_live_block_scan();
                    self.arena_cursor = Some(crate::arena::ArenaObjectCursor::new(
                        crate::arena::ArenaWalkOrder::BlockIndex,
                    ));
                    self.newly_marked = 0;
                    self.subphase = BlockPersistSubphase::MarkLiveBlockObjects;
                }
                BlockPersistSubphase::MarkLiveBlockObjects => {
                    if !self.mark_live_block_objects(&mut remaining) {
                        return false;
                    }
                    self.stats.marked_objects =
                        self.stats.marked_objects.saturating_add(self.newly_marked);
                    if self.newly_marked == 0 {
                        self.subphase = BlockPersistSubphase::Done;
                        return true;
                    }
                    self.worklist_cursor = 0;
                    self.subphase = BlockPersistSubphase::DrainMarkedObjects;
                }
                BlockPersistSubphase::DrainMarkedObjects => {
                    if remaining == 0 {
                        return false;
                    }
                    let before = self.worklist_cursor;
                    let done = drain_trace_worklist_step(
                        &mut self.worklist,
                        &mut self.worklist_cursor,
                        valid_ptrs,
                        false,
                        remaining,
                    );
                    let consumed = self.worklist_cursor.saturating_sub(before);
                    remaining = remaining.saturating_sub(consumed);
                    if !done {
                        return false;
                    }
                    self.subphase = BlockPersistSubphase::StartIteration;
                }
                BlockPersistSubphase::Done => return true,
            }
        }
    }

    fn begin_iteration(&mut self) {
        self.stats.iterations = self.stats.iterations.saturating_add(1);
        let n_blocks = crate::arena::arena_block_count();
        self.general_n = crate::arena::general_block_count();
        self.persist_low = self.general_n.saturating_sub(BLOCK_PERSIST_WINDOW);
        self.block_has_live.clear();
        self.block_has_live.resize(n_blocks, false);
        self.arena_cursor = Some(crate::arena::ArenaObjectCursor::new(
            crate::arena::ArenaWalkOrder::BlockIndex,
        ));
        self.newly_marked = 0;
        self.subphase = BlockPersistSubphase::ScanLiveBlocks;
    }

    fn scan_live_blocks(&mut self, remaining: &mut usize) -> bool {
        while *remaining > 0 {
            let next = self
                .arena_cursor
                .as_mut()
                .and_then(crate::arena::ArenaObjectCursor::next);
            let Some((header_ptr, block_idx)) = next else {
                self.arena_cursor = None;
                return true;
            };
            *remaining -= 1;
            if block_idx < self.persist_low
                || block_idx >= self.general_n
                || block_idx >= self.block_has_live.len()
            {
                continue;
            }
            let header = header_ptr as *mut GcHeader;
            unsafe {
                if (*header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0 {
                    self.block_has_live[block_idx] = true;
                }
            }
        }
        false
    }

    fn finish_live_block_scan(&mut self) {
        let live_blocks_this = self.block_has_live.iter().filter(|&&live| live).count();
        let candidate_blocks_this = (self.persist_low..self.general_n)
            .filter(|&block_idx| self.block_has_live.get(block_idx).copied().unwrap_or(false))
            .count();
        self.stats.live_blocks = self.stats.live_blocks.saturating_add(live_blocks_this);
        self.stats.candidate_blocks = self
            .stats
            .candidate_blocks
            .saturating_add(candidate_blocks_this);
    }

    fn mark_live_block_objects(&mut self, remaining: &mut usize) -> bool {
        while *remaining > 0 {
            let next = self
                .arena_cursor
                .as_mut()
                .and_then(crate::arena::ArenaObjectCursor::next);
            let Some((header_ptr, block_idx)) = next else {
                self.arena_cursor = None;
                return true;
            };
            *remaining -= 1;
            if block_idx < self.persist_low
                || block_idx >= self.general_n
                || !self.block_has_live.get(block_idx).copied().unwrap_or(false)
            {
                continue;
            }
            let header = header_ptr as *mut GcHeader;
            unsafe {
                if (*header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
                    (*header).gc_flags |= GC_FLAG_MARKED;
                    self.worklist.push(header);
                    self.newly_marked = self.newly_marked.saturating_add(1);
                }
            }
        }
        false
    }

    fn stats(&self) -> BlockPersistTraceStats {
        self.stats
    }
}

struct MinorCycleContext {
    prev_in_alloc: u8,
    previous_pause_us: u64,
    current_rss_bytes: u64,
    evacuation_policy_allowed: bool,
    force_evacuation: bool,
    old_page_selection: OldPageDefragSelection,
    old_page_source_blocks: crate::arena::OldArenaSourceBlockSelection,
    evacuation_policy: EvacuationPolicyDecision,
    evacuation: EvacuationTraceStats,
    evacuation_sticky: StickyRememberedSet,
}

pub(super) struct GcCycleState {
    collection_kind: GcCollectionKind,
    phase: GcCyclePhase,
    trace: Option<GcCycleTrace>,
    active_elapsed: Duration,
    active_step_start: Option<Instant>,
    valid_builder: Option<ValidPointerSetBuilder>,
    valid_ptrs: Option<ValidPointerSet>,
    trace_worklist: Option<TraceWorklistCycleState>,
    block_persist: Option<BlockPersistCycleState>,
    minor: Option<MinorCycleContext>,
    live_old_to_young_sticky: Option<StickyRememberedSet>,
    sweep: Option<SweepTraceStats>,
    freed_bytes: u64,
    outcome: Option<GcCollectOutcome>,
}

impl GcCycleState {
    pub(super) fn new_full(trigger: GcTriggerSnapshot) -> Self {
        let trace = GcCycleTrace::new(GcCollectionKind::Full, trigger);
        let start = Instant::now();
        crate::arena::old_pages_begin_gc_cycle();
        clear_mark_seeds();
        Self {
            collection_kind: GcCollectionKind::Full,
            phase: GcCyclePhase::BuildValidPointerSet,
            trace,
            active_elapsed: start.elapsed(),
            active_step_start: None,
            valid_builder: None,
            valid_ptrs: None,
            trace_worklist: None,
            block_persist: None,
            minor: None,
            live_old_to_young_sticky: None,
            sweep: None,
            freed_bytes: 0,
            outcome: None,
        }
    }

    pub(super) fn new_minor_fallback(
        trigger: GcTriggerSnapshot,
        trace: Option<GcCycleTrace>,
        start: Instant,
        prev_in_alloc: u8,
        previous_pause_us: u64,
        current_rss_bytes: u64,
        evacuation_policy_allowed: bool,
        force_evacuation: bool,
        old_page_selection: OldPageDefragSelection,
        old_page_source_blocks: crate::arena::OldArenaSourceBlockSelection,
    ) -> Self {
        let _ = trigger;
        Self {
            collection_kind: GcCollectionKind::Minor,
            phase: GcCyclePhase::BuildValidPointerSet,
            trace,
            active_elapsed: start.elapsed(),
            active_step_start: None,
            valid_builder: None,
            valid_ptrs: None,
            trace_worklist: None,
            block_persist: None,
            minor: Some(MinorCycleContext {
                prev_in_alloc,
                previous_pause_us,
                current_rss_bytes,
                evacuation_policy_allowed,
                force_evacuation,
                old_page_selection,
                old_page_source_blocks,
                evacuation_policy: EvacuationPolicyDecision::default(),
                evacuation: EvacuationTraceStats::default(),
                evacuation_sticky: StickyRememberedSet::default(),
            }),
            live_old_to_young_sticky: None,
            sweep: None,
            freed_bytes: 0,
            outcome: None,
        }
    }

    pub(super) fn phase(&self) -> GcCyclePhase {
        self.phase
    }

    pub(super) fn collection_kind(&self) -> GcCollectionKind {
        self.collection_kind
    }

    pub(super) fn set_progress_kind(&mut self, progress_kind: GcProgressKind) {
        if let Some(trace) = self.trace.as_mut() {
            trace.progress_kind = progress_kind;
        }
    }

    pub(super) fn step(&mut self, budget: GcWorkBudget) -> GcCycleStepResult {
        let phase_before = self.phase;
        if self.phase == GcCyclePhase::Complete {
            return GcCycleStepResult {
                phase: phase_before,
                completed: true,
            };
        }

        let step_start = Instant::now();
        self.active_step_start = Some(step_start);
        match self.phase {
            GcCyclePhase::BuildValidPointerSet => self.step_build_valid_pointer_set(budget),
            GcCyclePhase::RootScan => self.step_root_scan(),
            GcCyclePhase::MarkPropagation => self.step_mark_propagation(budget),
            GcCyclePhase::BlockPersistence => self.step_block_persistence(budget),
            GcCyclePhase::AtomicFinalize => self.step_atomic_finalize(),
            GcCyclePhase::Sweep => self.step_sweep(),
            GcCyclePhase::Reclaim => self.step_reclaim(),
            GcCyclePhase::Complete => {}
        }
        self.active_step_start = None;
        self.active_elapsed = self.active_elapsed.saturating_add(step_start.elapsed());
        GcCycleStepResult {
            phase: phase_before,
            completed: self.phase == GcCyclePhase::Complete,
        }
    }

    pub(super) fn run_to_completion(mut self) -> GcCollectOutcome {
        while self.phase != GcCyclePhase::Complete {
            self.step(GcWorkBudget::unbounded());
        }
        self.outcome
            .take()
            .expect("completed GC cycle must produce an outcome")
    }

    pub(super) fn take_outcome(&mut self) -> Option<GcCollectOutcome> {
        self.outcome.take()
    }

    fn active_elapsed(&self) -> Duration {
        match self.active_step_start {
            Some(start) => self.active_elapsed.saturating_add(start.elapsed()),
            None => self.active_elapsed,
        }
    }

    fn active_elapsed_us(&self) -> u64 {
        self.active_elapsed().as_micros() as u64
    }

    fn step_build_valid_pointer_set(&mut self, budget: GcWorkBudget) {
        let phase_start = trace_phase_start(&self.trace);
        let builder = self
            .valid_builder
            .get_or_insert_with(ValidPointerSetBuilder::new);
        if !builder.step(budget.work_units) {
            trace_phase_record(&mut self.trace, "build_valid_pointer_set", phase_start);
            return;
        }
        let builder = self
            .valid_builder
            .take()
            .expect("valid-pointer builder exists");
        self.valid_ptrs = Some(builder.finish());
        trace_phase_record(&mut self.trace, "build_valid_pointer_set", phase_start);

        let active_elapsed_us = self.active_elapsed_us();
        if let Some(minor) = self.minor.as_mut() {
            let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
            minor.evacuation_policy = evacuation_policy_initial_decision(
                valid_ptrs.tenured_nursery_bytes(),
                minor.current_rss_bytes,
                minor.previous_pause_us,
                active_elapsed_us,
                minor.evacuation_policy_allowed,
                minor.force_evacuation,
                old_to_young_tracking_complete(),
                minor.old_page_selection.selected_pages,
            );
            if let Some(trace) = self.trace.as_mut() {
                trace.evacuation_policy = minor.evacuation_policy;
            }
        }

        self.phase = GcCyclePhase::RootScan;
    }

    fn step_root_scan(&mut self) {
        let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
        let phase_start = trace_phase_start(&self.trace);
        let conservative_scan_decision = conservative_stack_scan_decision();
        let conservative_root_stats =
            mark_stack_roots_for_decision(valid_ptrs, conservative_scan_decision);

        let consider_evacuation = self
            .minor
            .as_ref()
            .is_some_and(|minor| minor.evacuation_policy.considered);
        let conservative_pin_stats = if consider_evacuation
            && matches!(
                conservative_scan_decision,
                ConservativeStackScanDecision::Scan
            ) {
            pin_currently_marked_as_conservative()
        } else {
            ConservativePinTraceStats::default()
        };

        match self.trace.as_mut() {
            Some(trace) => mark_mutable_root_slots(
                valid_ptrs,
                Some(&mut trace.shadow_roots),
                Some(&mut trace.root_sources),
            ),
            None => mark_mutable_root_slots(valid_ptrs, None, None),
        }
        match self.trace.as_mut() {
            Some(trace) => mark_mutable_registered_roots_with_sources(
                valid_ptrs,
                Some(&mut trace.root_sources),
            ),
            None => mark_mutable_registered_roots(valid_ptrs),
        }
        let legacy_root_stats = mark_registered_roots(valid_ptrs, consider_evacuation);
        if let Some(trace) = self.trace.as_mut() {
            trace.conservative_root_count = conservative_root_stats.root_count;
            trace.conservative_pinned = conservative_pin_stats.pinned_roots;
            trace.conservative_pinned_bytes = conservative_pin_stats.pinned_bytes;
            trace.legacy_copy_only_scanner_pinned = legacy_root_stats;
            trace.root_sources.native_stack_fallback.decision = conservative_scan_decision;
            trace.root_sources.native_stack_fallback.scanned = matches!(
                conservative_scan_decision,
                ConservativeStackScanDecision::Scan
            );
            trace.root_sources.native_stack_fallback.roots_found =
                conservative_root_stats.root_count;
            trace.root_sources.native_stack_fallback.pinned_roots =
                conservative_pin_stats.pinned_roots;
            trace.root_sources.native_stack_fallback.pinned_bytes =
                conservative_pin_stats.pinned_bytes;
        }
        trace_phase_record(&mut self.trace, "root_marking", phase_start);

        let phase_start = trace_phase_start(&self.trace);
        let remembered_set = mark_remembered_set_roots(valid_ptrs);
        trace_phase_record(&mut self.trace, "remembered_set_marking", phase_start);
        if let Some(trace) = self.trace.as_mut() {
            trace.remembered_set = remembered_set;
        }

        self.phase = GcCyclePhase::MarkPropagation;
    }

    fn step_mark_propagation(&mut self, budget: GcWorkBudget) {
        let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
        let phase_start = trace_phase_start(&self.trace);
        let minor_only = matches!(self.collection_kind, GcCollectionKind::Minor);
        let trace_worklist = self
            .trace_worklist
            .get_or_insert_with(|| TraceWorklistCycleState::new(minor_only));
        if trace_worklist.step(valid_ptrs, budget.work_units) {
            self.trace_worklist = None;
            self.phase = GcCyclePhase::BlockPersistence;
        }
        trace_phase_record(&mut self.trace, "trace_worklist", phase_start);
    }

    fn step_block_persistence(&mut self, budget: GcWorkBudget) {
        let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
        let phase_start = trace_phase_start(&self.trace);
        let block_persist = if budget.work_units == usize::MAX && self.block_persist.is_none() {
            mark_block_persisting_arena_objects(valid_ptrs)
        } else {
            let block_persist = self
                .block_persist
                .get_or_insert_with(BlockPersistCycleState::new);
            if !block_persist.step(valid_ptrs, budget.work_units) {
                trace_phase_record(&mut self.trace, "block_persistence", phase_start);
                return;
            }
            self.block_persist
                .take()
                .expect("block-persist state exists")
                .stats()
        };
        trace_phase_record(&mut self.trace, "block_persistence", phase_start);
        if let Some(trace) = self.trace.as_mut() {
            trace.block_persist = block_persist;
        }
        self.phase = GcCyclePhase::AtomicFinalize;
    }

    fn step_atomic_finalize(&mut self) {
        if self.minor.is_some() {
            self.atomic_finalize_minor();
        } else {
            self.live_old_to_young_sticky = Some(rebuild_live_old_to_young_remembered_set());
        }
        self.phase = GcCyclePhase::Sweep;
    }

    fn atomic_finalize_minor(&mut self) {
        let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
        if gc_verify_evacuation_enabled() {
            let phase_start = trace_phase_start(&self.trace);
            let old_young_edge_verifier = verify_old_to_young_edges_covered();
            trace_phase_record(&mut self.trace, "old_young_edge_verify", phase_start);
            if let Some(trace) = self.trace.as_mut() {
                trace.old_young_edge_verifier = old_young_edge_verifier;
            }
        }

        let active_elapsed_us = self.active_elapsed_us();
        let minor = self.minor.as_mut().expect("minor context exists");
        if minor.evacuation_policy.considered {
            let snapshot = evacuation_policy_snapshot_after_mark(
                minor.evacuation_policy.snapshot,
                minor.evacuation_policy.force,
                active_elapsed_us,
                &minor.old_page_selection,
            );
            minor.evacuation_policy =
                evacuation_policy_final_decision(minor.evacuation_policy, snapshot);
        } else {
            minor.evacuation_policy.snapshot.pre_evac_pause_us = active_elapsed_us;
        }
        if let Some(trace) = self.trace.as_mut() {
            trace.evacuation_policy = minor.evacuation_policy;
        }

        let mut evacuation = EvacuationTraceStats::default();
        let mut evacuation_sticky = StickyRememberedSet::default();
        if minor.evacuation_policy.enabled {
            let phase_start = trace_phase_start(&self.trace);
            let mut evacuated_new_headers = Vec::new();
            let mut evacuated_original_headers = Vec::new();
            evacuation = evacuate_tenured_nursery_objects_collecting(
                minor.evacuation_policy.force,
                &mut evacuated_new_headers,
                &mut evacuated_original_headers,
            );
            let old_page_evacuation = evacuate_selected_old_pages_collecting(
                &minor.old_page_selection.pages,
                &mut evacuated_new_headers,
                &mut evacuated_original_headers,
            );
            evacuation.objects = evacuation
                .objects
                .saturating_add(old_page_evacuation.objects);
            evacuation.bytes = evacuation.bytes.saturating_add(old_page_evacuation.bytes);
            evacuation.moved_objects = evacuation
                .moved_objects
                .saturating_add(old_page_evacuation.moved_objects);
            evacuation.moved_bytes = evacuation
                .moved_bytes
                .saturating_add(old_page_evacuation.moved_bytes);
            evacuation.old_page_moved_objects = old_page_evacuation.old_page_moved_objects;
            evacuation.old_page_moved_bytes = old_page_evacuation.old_page_moved_bytes;
            trace_phase_record(&mut self.trace, "evacuation", phase_start);
            if evacuation.objects > 0 {
                let phase_start = trace_phase_start(&self.trace);
                match self.trace.as_mut() {
                    Some(trace) => rewrite_forwarded_references(
                        valid_ptrs,
                        Some(&mut trace.shadow_roots),
                        Some(&mut trace.root_sources),
                    ),
                    None => rewrite_forwarded_references(valid_ptrs, None, None),
                }
                evacuation_sticky =
                    rebuild_evacuated_old_to_young_remembered_set(&evacuated_new_headers);
                trace_phase_record(&mut self.trace, "reference_rewrite", phase_start);
                if gc_verify_evacuation_enabled() {
                    let phase_start = trace_phase_start(&self.trace);
                    verify_evacuated_no_stale_forwarded_refs(valid_ptrs);
                    trace_phase_record(&mut self.trace, "evacuation_verify", phase_start);
                }
                let released =
                    release_evacuated_original_forwarding_stubs(&evacuated_original_headers);
                evacuation.released_original_objects = released.released_original_objects;
                evacuation.released_original_bytes = released.released_original_bytes;
                evacuation.released_original_reusable_bytes =
                    released.released_original_reusable_bytes;
                evacuation.released_original_returned_bytes =
                    released.released_original_returned_bytes;
            }
        }

        minor.evacuation = evacuation;
        minor.evacuation_sticky = evacuation_sticky;
        self.live_old_to_young_sticky = Some(rebuild_live_old_to_young_remembered_set());
    }

    fn step_sweep(&mut self) {
        let phase_start = trace_phase_start(&self.trace);
        let sweep = if let Some(minor) = self.minor.as_ref() {
            if minor.evacuation.old_page_moved_bytes > 0 {
                sweep_with_age_bump_and_targeted_old_reclaim(
                    true,
                    &minor.old_page_source_blocks.block_indices,
                )
            } else {
                sweep_with_age_bump(true)
            }
        } else {
            sweep_with_age_bump_and_old_reclaim(false, true)
        };
        trace_phase_record(&mut self.trace, "sweep", phase_start);
        self.freed_bytes = sweep.freed_bytes;

        if let Some(minor) = self.minor.as_mut() {
            minor.evacuation.retained_forwarded_stub_objects =
                sweep.retained_forwarded_stub_objects;
            minor.evacuation.retained_forwarded_stub_bytes = sweep.retained_forwarded_stub_bytes;
            maybe_print_evacuation_policy_diag(minor.evacuation_policy, minor.evacuation);
            if let Some(trace) = self.trace.as_mut() {
                trace.evacuation = minor.evacuation;
            }
        }
        if let Some(trace) = self.trace.as_mut() {
            trace.sweep = sweep;
            trace.old_pages = crate::arena::old_page_summary();
        }
        self.sweep = Some(sweep);
        self.phase = GcCyclePhase::Reclaim;
    }

    fn step_reclaim(&mut self) {
        let reclaim_start = trace_phase_start(&self.trace);

        let phase_start = trace_phase_start(&self.trace);
        remembered_set_clear();
        if let Some(minor) = self.minor.as_ref() {
            minor.evacuation_sticky.restore();
        }
        if let Some(sticky) = self.live_old_to_young_sticky.as_ref() {
            sticky.restore();
        }
        trace_phase_record(&mut self.trace, "remembered_set_clear", phase_start);

        if self.minor.is_some() {
            let phase_start = trace_phase_start(&self.trace);
            CONS_PINNED.with(|s| s.borrow_mut().clear());
            trace_phase_record(&mut self.trace, "conservative_pin_clear", phase_start);
        }

        #[cfg(target_env = "gnu")]
        {
            let phase_start = trace_phase_start(&self.trace);
            unsafe {
                libc::malloc_trim(0);
            }
            trace_phase_record(&mut self.trace, "malloc_trim", phase_start);
        }

        trace_phase_record(&mut self.trace, "reclaim", reclaim_start);

        let elapsed_us = self.active_elapsed_us();
        GC_STATS.with(|stats| {
            let mut stats = stats.borrow_mut();
            stats.collection_count += 1;
            stats.total_freed_bytes = stats.total_freed_bytes.saturating_add(self.freed_bytes);
            stats.last_pause_us = elapsed_us;
        });

        if let Some(minor) = self.minor.as_ref() {
            restore_minor_in_alloc(minor.prev_in_alloc);
        }
        if let Some(trace) = self.trace.as_mut() {
            trace.pause_us = elapsed_us;
            trace.capture_layout_scans();
        }
        if self.minor.is_none() {
            finish_full_old_reclaim_baseline();
        }

        self.outcome = Some(GcCollectOutcome {
            freed_bytes: self.freed_bytes,
            malloc_swept: true,
            trace: self.trace.take(),
        });
        self.phase = GcCyclePhase::Complete;
    }
}

pub(super) fn restore_minor_in_alloc(prev_in_alloc: u8) {
    GC_FLAGS.with(|f| {
        let cur = f.get();
        if prev_in_alloc != 0 {
            f.set(cur | GC_FLAG_IN_ALLOC);
        } else {
            f.set(cur & !GC_FLAG_IN_ALLOC);
        }
    });
}
