use super::barrier::{ConservativePinClearState, RememberedSetClearState};
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
    #[cfg(feature = "diagnostics")]
    #[inline]
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::BuildValidPointerSet => "build_valid_pointer_set",
            Self::RootScan => "root_scan",
            Self::MarkPropagation => "mark_propagation",
            Self::BlockPersistence => "block_persistence",
            Self::AtomicFinalize => "atomic_finalize",
            Self::Sweep => "sweep",
            Self::Reclaim => "reclaim",
            Self::Complete => "complete",
        }
    }

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

    #[inline]
    pub(super) const fn mutator_assist_honors_budget(self) -> bool {
        matches!(
            self,
            Self::BuildValidPointerSet
                | Self::RootScan
                | Self::MarkPropagation
                | Self::BlockPersistence
        )
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
        self.absorb_mark_seeds();
        let done = drain_trace_worklist_step(
            &mut self.worklist,
            &mut self.cursor,
            valid_ptrs,
            self.minor_only,
            budget,
        );
        self.absorb_mark_seeds();
        done && self.cursor >= self.worklist.len()
    }

    fn absorb_mark_seeds(&mut self) {
        let mut seeds = take_mark_seeds();
        if !seeds.is_empty() {
            self.worklist.append(&mut seeds);
        }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RootScanSubphase {
    ConservativeStack,
    MutableSlots,
    MutableRegisteredScanners,
    LegacyRegisteredScanners,
    RememberedSet,
    Done,
}

struct MutableRegisteredRootScanState {
    scanners: Vec<MutableRootScannerEntry>,
    scanner_states: Vec<Option<Box<dyn std::any::Any>>>,
    ffi_scanners: Vec<PerryFfiMutableRootScanner>,
    ffi_named_scanners: Vec<(PerryFfiNamedMutableRootScanner, usize)>,
    scanner_cursor: usize,
    ffi_cursor: usize,
    ffi_named_cursor: usize,
    recorded_counts: bool,
}

impl MutableRegisteredRootScanState {
    fn new() -> Self {
        let scanners = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
        let scanner_states = scanners
            .iter()
            .map(|entry| entry.budgeted_state_factory.map(|factory| factory()))
            .collect();
        Self {
            scanners,
            scanner_states,
            ffi_scanners: FFI_MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone()),
            ffi_named_scanners: FFI_NAMED_MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone()),
            scanner_cursor: 0,
            ffi_cursor: 0,
            ffi_named_cursor: 0,
            recorded_counts: false,
        }
    }

    fn step(
        &mut self,
        valid_ptrs: &ValidPointerSet,
        mut root_sources: Option<&mut RootSourcesTraceStats>,
        budget: usize,
        allow_synchronous_scanners: bool,
    ) -> bool {
        if !self.recorded_counts {
            if let Some(sources) = &mut root_sources {
                sources.runtime_handles.record_registered_scanners(
                    self.scanners
                        .iter()
                        .filter(|entry| entry.source == MutableRootScannerSource::RuntimeHandles)
                        .count(),
                );
                sources.runtime_mutable_scanners.record_registered_scanners(
                    self.scanners
                        .iter()
                        .filter(|entry| {
                            entry.source == MutableRootScannerSource::RuntimeMutableScanner
                        })
                        .count(),
                );
                sources.ffi_mutable_scanners.record_registered_scanners(
                    self.ffi_scanners.len() + self.ffi_named_scanners.len(),
                );
            }
            self.recorded_counts = true;
        }

        let mut remaining = budget;
        let mut visitor = RuntimeRootVisitor::for_mark(valid_ptrs);
        while self.scanner_cursor < self.scanners.len() {
            if remaining == 0 {
                return false;
            }
            let entry = self.scanners[self.scanner_cursor];
            let stats = match &mut root_sources {
                Some(sources) => match entry.source {
                    MutableRootScannerSource::RuntimeHandles => {
                        Some(&mut sources.runtime_handles as *mut RootSourceSlotTraceStats)
                    }
                    MutableRootScannerSource::RuntimeMutableScanner => {
                        Some(&mut sources.runtime_mutable_scanners as *mut RootSourceSlotTraceStats)
                    }
                },
                None => None,
            };
            let previous = visitor.set_root_source_stats(stats);
            let done = if let Some(scanner) = entry.budgeted_scanner {
                let state = self.scanner_states[self.scanner_cursor]
                    .as_deref_mut()
                    .expect("budgeted scanner state exists");
                let before = remaining;
                let done = scanner(&mut visitor, state, &mut remaining);
                if done && remaining == before && remaining != usize::MAX {
                    remaining -= 1;
                }
                done
            } else {
                if !allow_synchronous_scanners {
                    return false;
                }
                remaining -= 1;
                (entry.scanner)(&mut visitor);
                true
            };
            visitor.set_root_source_stats(previous);
            if !done {
                return false;
            }
            self.scanner_cursor += 1;
        }

        if !allow_synchronous_scanners
            && (self.ffi_cursor < self.ffi_scanners.len()
                || self.ffi_named_cursor < self.ffi_named_scanners.len())
        {
            return false;
        }

        while remaining > 0 && self.ffi_cursor < self.ffi_scanners.len() {
            let scanner = self.ffi_scanners[self.ffi_cursor];
            self.ffi_cursor += 1;
            remaining -= 1;
            let stats = match &mut root_sources {
                Some(sources) => {
                    Some(&mut sources.ffi_mutable_scanners as *mut RootSourceSlotTraceStats)
                }
                None => None,
            };
            let previous = visitor.set_root_source_stats(stats);
            let ctx = &mut visitor as *mut RuntimeRootVisitor<'_> as *mut c_void;
            scanner(perry_ffi_visit_mutable_root_slot, ctx);
            visitor.set_root_source_stats(previous);
        }

        while remaining > 0 && self.ffi_named_cursor < self.ffi_named_scanners.len() {
            let (scanner, scanner_id) = self.ffi_named_scanners[self.ffi_named_cursor];
            self.ffi_named_cursor += 1;
            remaining -= 1;
            let stats = match &mut root_sources {
                Some(sources) => {
                    Some(&mut sources.ffi_mutable_scanners as *mut RootSourceSlotTraceStats)
                }
                None => None,
            };
            let previous = visitor.set_root_source_stats(stats);
            let ctx = &mut visitor as *mut RuntimeRootVisitor<'_> as *mut c_void;
            scanner(scanner_id, perry_ffi_visit_mutable_root_slot, ctx);
            visitor.set_root_source_stats(previous);
        }

        self.scanner_cursor >= self.scanners.len()
            && self.ffi_cursor >= self.ffi_scanners.len()
            && self.ffi_named_cursor >= self.ffi_named_scanners.len()
    }
}

struct LegacyRegisteredRootScanState {
    scanners: Vec<fn(&mut dyn FnMut(f64))>,
    ffi_scanners: Vec<PerryFfiRootScanner>,
    scanner_cursor: usize,
    ffi_cursor: usize,
    stats: LegacyRootTraceStats,
}

impl LegacyRegisteredRootScanState {
    fn new() -> Self {
        let scanners: Vec<fn(&mut dyn FnMut(f64))> = ROOT_SCANNERS.with(|s| s.borrow().clone());
        let ffi_scanners: Vec<PerryFfiRootScanner> = FFI_ROOT_SCANNERS.with(|s| s.borrow().clone());
        let stats = LegacyRootTraceStats {
            registered_rust_scanners: scanners.len(),
            registered_ffi_scanners: ffi_scanners.len(),
            ..LegacyRootTraceStats::default()
        };
        Self {
            scanners,
            ffi_scanners,
            scanner_cursor: 0,
            ffi_cursor: 0,
            stats,
        }
    }

    fn step(
        &mut self,
        valid_ptrs: &ValidPointerSet,
        pin_discoveries: bool,
        budget: usize,
        allow_synchronous_scanners: bool,
    ) -> bool {
        if !allow_synchronous_scanners
            && (self.scanner_cursor < self.scanners.len()
                || self.ffi_cursor < self.ffi_scanners.len())
        {
            return false;
        }
        let mut remaining = budget;
        while remaining > 0 && self.scanner_cursor < self.scanners.len() {
            let scanner = self.scanners[self.scanner_cursor];
            self.scanner_cursor += 1;
            remaining -= 1;
            scanner(&mut |value: f64| {
                record_copy_only_scanner_mark_emission(
                    value.to_bits(),
                    valid_ptrs,
                    &mut self.stats,
                );
                if let Some(bytes) =
                    mark_copy_only_scanner_bits(value.to_bits(), valid_ptrs, pin_discoveries)
                {
                    self.stats.pinned_roots += 1;
                    self.stats.pinned_bytes += bytes;
                }
            });
        }

        while remaining > 0 && self.ffi_cursor < self.ffi_scanners.len() {
            let scanner = self.ffi_scanners[self.ffi_cursor];
            self.ffi_cursor += 1;
            remaining -= 1;
            let mut ctx = RegisteredRootMarkContext {
                valid_ptrs: valid_ptrs as *const ValidPointerSet,
                pin_discoveries,
                legacy_stats: &mut self.stats as *mut LegacyRootTraceStats,
            };
            let ctx = &mut ctx as *mut RegisteredRootMarkContext as *mut c_void;
            scanner(perry_ffi_mark_root, ctx);
        }

        self.scanner_cursor >= self.scanners.len() && self.ffi_cursor >= self.ffi_scanners.len()
    }

    fn stats(&self) -> LegacyRootTraceStats {
        self.stats
    }
}

struct RootScanCycleState {
    subphase: RootScanSubphase,
    mutable_slot_cursor: MutableRootSlotScanCursor,
    mutable_registered: Option<MutableRegisteredRootScanState>,
    legacy_registered: Option<LegacyRegisteredRootScanState>,
    remembered_set: Option<RememberedSetRootMarkState>,
}

impl RootScanCycleState {
    fn new() -> Self {
        Self {
            subphase: RootScanSubphase::ConservativeStack,
            mutable_slot_cursor: MutableRootSlotScanCursor::default(),
            mutable_registered: None,
            legacy_registered: None,
            remembered_set: None,
        }
    }

    fn trace_phase_name(&self) -> &'static str {
        match self.subphase {
            RootScanSubphase::RememberedSet => "remembered_set_marking",
            _ => "root_marking",
        }
    }

    fn step_current_subphase(
        &mut self,
        valid_ptrs: &ValidPointerSet,
        trace: &mut Option<GcCycleTrace>,
        consider_evacuation: bool,
        budget: usize,
        allow_synchronous_scanners: bool,
        pin_only_old_conservative: bool,
    ) -> bool {
        match self.subphase {
            RootScanSubphase::ConservativeStack => {
                if budget == 0 {
                    return false;
                }
                let conservative_scan_decision = conservative_stack_scan_decision();
                // #5029: minors retain old-gen conservative discoveries
                // pin-only (no trace) — see try_mark_conservative_word.
                let conservative_root_stats = mark_stack_roots_for_decision(
                    valid_ptrs,
                    conservative_scan_decision,
                    pin_only_old_conservative,
                );
                let conservative_pin_stats = if consider_evacuation
                    && matches!(
                        conservative_scan_decision,
                        ConservativeStackScanDecision::Scan
                    ) {
                    pin_currently_marked_as_conservative()
                } else {
                    ConservativePinTraceStats::default()
                };
                if let Some(trace) = trace.as_mut() {
                    trace.conservative_root_count = conservative_root_stats.root_count;
                    trace.conservative_pinned = conservative_pin_stats.pinned_roots;
                    trace.conservative_pinned_bytes = conservative_pin_stats.pinned_bytes;
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
                self.subphase = RootScanSubphase::MutableSlots;
                false
            }
            RootScanSubphase::MutableSlots => {
                let done = match trace.as_mut() {
                    Some(trace) => mark_mutable_root_slots_step(
                        valid_ptrs,
                        Some(&mut trace.shadow_roots),
                        Some(&mut trace.root_sources),
                        &mut self.mutable_slot_cursor,
                        budget,
                    ),
                    None => mark_mutable_root_slots_step(
                        valid_ptrs,
                        None,
                        None,
                        &mut self.mutable_slot_cursor,
                        budget,
                    ),
                };
                if done {
                    self.subphase = RootScanSubphase::MutableRegisteredScanners;
                }
                false
            }
            RootScanSubphase::MutableRegisteredScanners => {
                let state = self
                    .mutable_registered
                    .get_or_insert_with(MutableRegisteredRootScanState::new);
                let done = match trace.as_mut() {
                    Some(trace) => state.step(
                        valid_ptrs,
                        Some(&mut trace.root_sources),
                        budget,
                        allow_synchronous_scanners,
                    ),
                    None => state.step(valid_ptrs, None, budget, allow_synchronous_scanners),
                };
                if done {
                    self.subphase = RootScanSubphase::LegacyRegisteredScanners;
                }
                false
            }
            RootScanSubphase::LegacyRegisteredScanners => {
                let state = self
                    .legacy_registered
                    .get_or_insert_with(LegacyRegisteredRootScanState::new);
                if state.step(
                    valid_ptrs,
                    consider_evacuation,
                    budget,
                    allow_synchronous_scanners,
                ) {
                    if let Some(trace) = trace.as_mut() {
                        trace.legacy_copy_only_scanner_pinned = state.stats();
                    }
                    self.subphase = RootScanSubphase::RememberedSet;
                }
                false
            }
            RootScanSubphase::RememberedSet => {
                let state = self
                    .remembered_set
                    .get_or_insert_with(RememberedSetRootMarkState::new);
                if state.step(valid_ptrs, budget) {
                    if let Some(trace) = trace.as_mut() {
                        trace.remembered_set = state.stats();
                    }
                    self.subphase = RootScanSubphase::Done;
                }
                false
            }
            RootScanSubphase::Done => true,
        }
    }
}

struct MinorCycleContext {
    prev_in_alloc: u8,
    previous_pause_us: u64,
    current_rss_bytes: u64,
    malloc_sweep_due: bool,
    evacuation_policy_allowed: bool,
    force_evacuation: bool,
    evacuation_policy_disabled_reason: &'static str,
    old_page_selection: OldPageDefragSelection,
    old_page_source_blocks: crate::arena::OldArenaSourceBlockSelection,
    evacuation_policy: EvacuationPolicyDecision,
    evacuation: EvacuationTraceStats,
    evacuation_sticky: StickyRememberedSet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReclaimSubphase {
    RememberedSet,
    ConservativePins,
    MallocTrim,
    Publish,
    Done,
}

struct ReclaimCycleState {
    subphase: ReclaimSubphase,
    remembered_set_clear: Option<RememberedSetClearState>,
    conservative_pin_clear: Option<ConservativePinClearState>,
}

impl ReclaimCycleState {
    fn new() -> Self {
        Self {
            subphase: ReclaimSubphase::RememberedSet,
            remembered_set_clear: None,
            conservative_pin_clear: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MallocTrimOutcome {
    status: AllocatorMaintenanceStatus,
    reason: AllocatorMaintenanceReason,
    elapsed_us: u64,
}

#[cfg(test)]
thread_local! {
    static TEST_MALLOC_TRIM_CALLS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(super) fn reset_test_malloc_trim_call_count() {
    TEST_MALLOC_TRIM_CALLS.with(|calls| calls.set(0));
}

#[cfg(test)]
pub(super) fn test_malloc_trim_call_count() -> usize {
    TEST_MALLOC_TRIM_CALLS.with(Cell::get)
}

#[cfg(all(test, target_env = "gnu"))]
fn record_test_malloc_trim_call() {
    TEST_MALLOC_TRIM_CALLS.with(|calls| calls.set(calls.get().saturating_add(1)));
}

fn run_malloc_trim(progress_kind: GcProgressKind) -> MallocTrimOutcome {
    if progress_kind.is_budgeted() {
        return MallocTrimOutcome {
            status: AllocatorMaintenanceStatus::Skipped,
            reason: AllocatorMaintenanceReason::OrdinaryBudgeted,
            elapsed_us: 0,
        };
    }

    #[cfg(target_env = "gnu")]
    {
        #[cfg(test)]
        record_test_malloc_trim_call();

        let start = Instant::now();
        unsafe {
            libc::malloc_trim(0);
        }
        return MallocTrimOutcome {
            status: AllocatorMaintenanceStatus::Executed,
            reason: AllocatorMaintenanceReason::ExplicitOrEmergency,
            elapsed_us: start.elapsed().as_micros() as u64,
        };
    }

    #[cfg(not(target_env = "gnu"))]
    {
        MallocTrimOutcome {
            status: AllocatorMaintenanceStatus::Unsupported,
            reason: AllocatorMaintenanceReason::NotSupported,
            elapsed_us: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AtomicFinalizeSubphase {
    WeakProcessing,
    MinorPrelude,
    BarrierSeedDrain,
    RememberedSetRebuild,
    DisableBarrier,
    Done,
}

struct AtomicFinalizeCycleState {
    subphase: AtomicFinalizeSubphase,
    barrier_drain: Option<TraceWorklistCycleState>,
    remembered_rebuild: Option<OldToYoungRememberedRebuildState>,
}

impl AtomicFinalizeCycleState {
    fn new(collection_kind: GcCollectionKind) -> Self {
        let subphase = match collection_kind {
            GcCollectionKind::Minor => AtomicFinalizeSubphase::WeakProcessing,
            GcCollectionKind::Full => AtomicFinalizeSubphase::BarrierSeedDrain,
        };
        Self {
            subphase,
            barrier_drain: None,
            remembered_rebuild: None,
        }
    }
}

pub(super) struct GcCycleState {
    collection_kind: GcCollectionKind,
    trigger_kind: GcTriggerKind,
    progress_kind: GcProgressKind,
    phase: GcCyclePhase,
    trace: Option<GcCycleTrace>,
    active_elapsed: Duration,
    active_step_start: Option<Instant>,
    valid_builder: Option<ValidPointerSetBuilder>,
    valid_ptrs: Option<ValidPointerSet>,
    root_scan: Option<RootScanCycleState>,
    trace_worklist: Option<TraceWorklistCycleState>,
    block_persist: Option<BlockPersistCycleState>,
    atomic_finalize: Option<AtomicFinalizeCycleState>,
    minor: Option<MinorCycleContext>,
    live_old_to_young_sticky: Option<StickyRememberedSet>,
    /// Dirty snapshot captured just before this cycle's remembered_set_clear
    /// begins, for the post-restore coverage repair (#5029).
    pre_clear_dirty_snapshot: Option<super::barrier::RememberedDirtySnapshot>,
    sweep_state: Option<IncrementalSweepState>,
    reclaim_state: Option<ReclaimCycleState>,
    sweep: Option<SweepTraceStats>,
    freed_bytes: u64,
    outcome: Option<GcCollectOutcome>,
}

impl GcCycleState {
    pub(super) fn new_full(trigger: GcTriggerSnapshot) -> Self {
        let trigger_kind = trigger.kind;
        let trace = GcCycleTrace::new(GcCollectionKind::Full, trigger);
        let start = Instant::now();
        crate::arena::old_pages_begin_gc_cycle();
        clear_mark_seeds();
        Self {
            collection_kind: GcCollectionKind::Full,
            trigger_kind,
            progress_kind: trigger_kind.progress_kind(GcCollectionKind::Full),
            phase: GcCyclePhase::BuildValidPointerSet,
            trace,
            active_elapsed: start.elapsed(),
            active_step_start: None,
            valid_builder: None,
            valid_ptrs: None,
            root_scan: None,
            trace_worklist: None,
            block_persist: None,
            atomic_finalize: None,
            minor: None,
            live_old_to_young_sticky: None,
            pre_clear_dirty_snapshot: None,
            sweep_state: None,
            reclaim_state: None,
            sweep: None,
            freed_bytes: 0,
            outcome: None,
        }
    }

    pub(super) fn new_minor_fallback(
        trigger: GcTriggerSnapshot,
        trace: Option<GcCycleTrace>,
        start: Instant,
        progress_kind: GcProgressKind,
        prev_in_alloc: u8,
        previous_pause_us: u64,
        current_rss_bytes: u64,
        evacuation_policy_allowed: bool,
        force_evacuation: bool,
        evacuation_policy_disabled_reason: &'static str,
        old_page_selection: OldPageDefragSelection,
        old_page_source_blocks: crate::arena::OldArenaSourceBlockSelection,
    ) -> Self {
        let malloc_sweep_due = copied_minor_malloc_sweep_due(trigger.kind);
        let trigger_kind = trigger.kind;
        Self {
            collection_kind: GcCollectionKind::Minor,
            trigger_kind,
            progress_kind,
            phase: GcCyclePhase::BuildValidPointerSet,
            trace,
            active_elapsed: start.elapsed(),
            active_step_start: None,
            valid_builder: None,
            valid_ptrs: None,
            root_scan: None,
            trace_worklist: None,
            block_persist: None,
            atomic_finalize: None,
            minor: Some(MinorCycleContext {
                prev_in_alloc,
                previous_pause_us,
                current_rss_bytes,
                malloc_sweep_due,
                evacuation_policy_allowed,
                force_evacuation,
                evacuation_policy_disabled_reason,
                old_page_selection,
                old_page_source_blocks,
                evacuation_policy: EvacuationPolicyDecision::default(),
                evacuation: EvacuationTraceStats::default(),
                evacuation_sticky: StickyRememberedSet::default(),
            }),
            live_old_to_young_sticky: None,
            pre_clear_dirty_snapshot: None,
            sweep_state: None,
            reclaim_state: None,
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
        self.progress_kind = progress_kind;
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

        let debt_before = self.trace.as_ref().map(|_| GcDebtSnapshot::current());
        let step_start = Instant::now();
        self.active_step_start = Some(step_start);
        match self.phase {
            GcCyclePhase::BuildValidPointerSet => self.step_build_valid_pointer_set(budget),
            GcCyclePhase::RootScan => self.step_root_scan(budget),
            GcCyclePhase::MarkPropagation => self.step_mark_propagation(budget),
            GcCyclePhase::BlockPersistence => self.step_block_persistence(budget),
            GcCyclePhase::AtomicFinalize => self.step_atomic_finalize(budget),
            GcCyclePhase::Sweep => self.step_sweep(budget),
            GcCyclePhase::Reclaim => self.step_reclaim(budget),
            GcCyclePhase::Complete => {}
        }
        self.active_step_start = None;
        let step_elapsed = step_start.elapsed();
        self.active_elapsed = self.active_elapsed.saturating_add(step_elapsed);
        if let Some(debt_before) = debt_before {
            let debt_after = GcDebtSnapshot::current();
            if let Some(trace) = self.trace.as_mut() {
                trace.record_pause_step(
                    phase_before,
                    self.phase,
                    budget.work_units,
                    step_elapsed,
                    debt_before,
                    debt_after,
                );
            } else if let Some(trace) = self
                .outcome
                .as_mut()
                .and_then(|outcome| outcome.trace.as_mut())
            {
                trace.record_pause_step(
                    phase_before,
                    self.phase,
                    budget.work_units,
                    step_elapsed,
                    debt_before,
                    debt_after,
                );
            }
        }
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
        if matches!(self.collection_kind, GcCollectionKind::Full) {
            let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
            incremental_mark_barrier_enable(valid_ptrs);
        }

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
                minor.evacuation_policy_disabled_reason,
                old_to_young_tracking_complete(),
                minor.old_page_selection.selected_pages,
            );
            if let Some(trace) = self.trace.as_mut() {
                trace.evacuation_policy = minor.evacuation_policy;
            }
        }

        self.phase = GcCyclePhase::RootScan;
    }

    fn step_root_scan(&mut self, budget: GcWorkBudget) {
        let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
        let consider_evacuation = self
            .minor
            .as_ref()
            .is_some_and(|minor| minor.evacuation_policy.considered);

        self.root_scan.get_or_insert_with(RootScanCycleState::new);
        // Phase 4 (incremental old-gen, gated): allow the initial root-scan step
        // of a budgeted cycle to run unbudgeted scanners synchronously (a
        // bounded initial-mark pause), so the stepper can start on programs that
        // register unbudgeted mutable scanners. Marking still proceeds
        // incrementally in later steps (they don't consult this flag).
        let allow_synchronous_scanners =
            !self.progress_kind.is_budgeted() || super::gc_incremental_enabled();
        loop {
            let phase_name = self
                .root_scan
                .as_ref()
                .expect("root scan state exists")
                .trace_phase_name();
            let phase_start = trace_phase_start(&self.trace);
            let done = self
                .root_scan
                .as_mut()
                .expect("root scan state exists")
                .step_current_subphase(
                    valid_ptrs,
                    &mut self.trace,
                    consider_evacuation,
                    budget.work_units,
                    allow_synchronous_scanners,
                    self.minor.is_some(),
                );
            trace_phase_record(&mut self.trace, phase_name, phase_start);
            if done {
                self.root_scan = None;
                self.phase = GcCyclePhase::MarkPropagation;
                break;
            }
            if budget.work_units != usize::MAX {
                break;
            }
        }
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
        // #6010: block persistence exists to protect REGISTER-HELD recent
        // objects that precise (shadow-stack) roots can't see (#43/#44). A
        // cycle whose root scan ran the FULL conservative stack+register
        // scan (`ManualGcScanGuard::force_full_scan` — every automatic
        // direct-arm collection in a compiled program, and explicit `gc()`)
        // has already pinned exactly those objects, so resurrecting every
        // dead neighbor in the recent-block window is pure over-retention.
        // Low-allocation workloads never rotate the active block out of the
        // window, which made garbage there immortal — dead Maps/Sets kept
        // multi-MB external buffers for the life of the process (#6010:
        // 1.4 GB RSS on a Map-churn benchmark whose live heap was ~1 MB).
        if matches!(
            super::roots::conservative_stack_scan_decision(),
            super::roots::ConservativeStackScanDecision::Scan
        ) {
            self.phase = GcCyclePhase::AtomicFinalize;
            return;
        }
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

    fn step_atomic_finalize(&mut self, budget: GcWorkBudget) {
        self.atomic_finalize
            .get_or_insert_with(|| AtomicFinalizeCycleState::new(self.collection_kind));
        loop {
            let phase_start = trace_phase_start(&self.trace);
            self.step_atomic_finalize_current_subphase(budget.work_units);
            trace_phase_record(&mut self.trace, "atomic_finalize", phase_start);
            if self.phase != GcCyclePhase::AtomicFinalize || budget.work_units != usize::MAX {
                break;
            }
        }
    }

    fn step_atomic_finalize_current_subphase(&mut self, budget: usize) {
        let subphase = self
            .atomic_finalize
            .as_ref()
            .expect("atomic finalize state exists")
            .subphase;
        match subphase {
            AtomicFinalizeSubphase::WeakProcessing => {
                if budget == 0 {
                    return;
                }
                let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
                let minor_only = self.minor.is_some();
                // Enqueue FinalizationRegistry cleanup jobs on EVERY cycle
                // kind, not just Manual (2026-07-09 GC audit: callbacks only
                // ever fired after an explicit `gc()`). Enqueue-once per
                // record is guaranteed by the record's pending-flag reset;
                // delivery happens at the explicit-`gc()` tail or the next
                // microtask-pump drain (`drain_pending_finalization_jobs`).
                crate::weakref::process_weak_targets_after_mark(
                    valid_ptrs, minor_only, /* enqueue_callbacks = */ true,
                );
                let next = if minor_only {
                    AtomicFinalizeSubphase::MinorPrelude
                } else {
                    AtomicFinalizeSubphase::DisableBarrier
                };
                self.atomic_finalize
                    .as_mut()
                    .expect("atomic finalize state exists")
                    .subphase = next;
            }
            AtomicFinalizeSubphase::MinorPrelude => {
                if budget == 0 {
                    return;
                }
                self.atomic_finalize_minor_prelude();
                self.atomic_finalize
                    .as_mut()
                    .expect("atomic finalize state exists")
                    .subphase = AtomicFinalizeSubphase::RememberedSetRebuild;
            }
            AtomicFinalizeSubphase::BarrierSeedDrain => {
                let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
                let done = {
                    let state = self
                        .atomic_finalize
                        .as_mut()
                        .expect("atomic finalize state exists");
                    let drain = state
                        .barrier_drain
                        .get_or_insert_with(|| TraceWorklistCycleState::new(false));
                    drain.step(valid_ptrs, budget)
                };
                if done {
                    let state = self
                        .atomic_finalize
                        .as_mut()
                        .expect("atomic finalize state exists");
                    state.barrier_drain = None;
                    state.subphase = AtomicFinalizeSubphase::RememberedSetRebuild;
                }
            }
            AtomicFinalizeSubphase::RememberedSetRebuild => {
                let require_marked = self.minor.is_none();
                let done = {
                    let state = self
                        .atomic_finalize
                        .as_mut()
                        .expect("atomic finalize state exists");
                    let rebuild = state.remembered_rebuild.get_or_insert_with(|| {
                        OldToYoungRememberedRebuildState::new(require_marked)
                    });
                    rebuild.step(budget)
                };
                if done {
                    let rebuild = self
                        .atomic_finalize
                        .as_mut()
                        .expect("atomic finalize state exists")
                        .remembered_rebuild
                        .take()
                        .expect("remembered rebuild state exists");
                    self.live_old_to_young_sticky = Some(rebuild.finish());
                    if self.minor.is_some() {
                        self.atomic_finalize = None;
                        self.phase = GcCyclePhase::Sweep;
                    } else {
                        self.atomic_finalize
                            .as_mut()
                            .expect("atomic finalize state exists")
                            .subphase = AtomicFinalizeSubphase::WeakProcessing;
                    }
                }
            }
            AtomicFinalizeSubphase::DisableBarrier => {
                if budget == 0 {
                    return;
                }
                incremental_mark_barrier_disable();
                if let Some(state) = self.atomic_finalize.as_mut() {
                    state.subphase = AtomicFinalizeSubphase::Done;
                }
                self.atomic_finalize = None;
                self.phase = GcCyclePhase::Sweep;
            }
            AtomicFinalizeSubphase::Done => {
                self.atomic_finalize = None;
                self.phase = GcCyclePhase::Sweep;
            }
        }
    }

    fn atomic_finalize_minor_prelude(&mut self) {
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
        let progress_kind = self.progress_kind;
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
        assert!(
            !progress_kind.is_budgeted() || !minor.evacuation_policy.enabled,
            "budgeted low-pause minor GC must remain non-moving"
        );

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
    }

    fn step_sweep(&mut self, budget: GcWorkBudget) {
        let phase_start = trace_phase_start(&self.trace);
        if self.sweep_state.is_none() {
            let full_trace = self.minor.is_none();
            let (do_age_bump, reclaim_dead_old_blocks, targeted_old_blocks, sweep_malloc) =
                if let Some(minor) = self.minor.as_ref() {
                    let targeted_old_blocks = (minor.evacuation.old_page_moved_bytes > 0)
                        .then(|| minor.old_page_source_blocks.block_indices.clone());
                    (true, false, targeted_old_blocks, minor.malloc_sweep_due)
                } else {
                    (false, true, None, true)
                };
            self.sweep_state = Some(
                IncrementalSweepState::new(
                    do_age_bump,
                    reclaim_dead_old_blocks,
                    targeted_old_blocks,
                    sweep_malloc,
                )
                // #6010: dead Maps'/Sets' external side buffers are freed as
                // the first sweep subphase, budget-chunked — no ordinary
                // sweep path reaches collections that die inside the ACTIVE
                // nursery allocation block, and bulk block resets skip
                // per-object finalizers. Minor traces never mark the old
                // generation, so deadness there is only trusted for
                // untenured nursery headers.
                .with_dead_collection_finalize(full_trace),
            );
        }
        let done = self
            .sweep_state
            .as_mut()
            .expect("sweep state exists")
            .step(budget.work_units);
        trace_phase_record(&mut self.trace, "sweep", phase_start);
        if !done {
            return;
        }

        let sweep = self.sweep_state.take().expect("sweep state exists").stats();
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

    fn step_reclaim(&mut self, budget: GcWorkBudget) {
        self.reclaim_state
            .get_or_insert_with(ReclaimCycleState::new);
        let mut remaining = budget.work_units;
        while remaining > 0 {
            let subphase = self
                .reclaim_state
                .as_ref()
                .expect("reclaim state exists")
                .subphase;
            match subphase {
                ReclaimSubphase::RememberedSet => {
                    let reclaim_start = trace_phase_start(&self.trace);
                    let phase_start = trace_phase_start(&self.trace);
                    let clear = {
                        let reclaim_state =
                            self.reclaim_state.as_mut().expect("reclaim state exists");
                        if reclaim_state.remembered_set_clear.is_none()
                            && self.pre_clear_dirty_snapshot.is_none()
                        {
                            // Snapshot the pre-clear dirty set so the
                            // post-restore repair can rescan it (#5029).
                            self.pre_clear_dirty_snapshot =
                                Some(super::barrier::remembered_dirty_snapshot());
                        }
                        reclaim_state
                            .remembered_set_clear
                            .get_or_insert_with(RememberedSetClearState::new)
                            .step_counted(remaining)
                    };
                    remaining = remaining.saturating_sub(clear.work_units);
                    trace_phase_record(&mut self.trace, "remembered_set_clear", phase_start);
                    trace_phase_record(&mut self.trace, "reclaim", reclaim_start);
                    if clear.done {
                        if let Some(minor) = self.minor.as_ref() {
                            minor.evacuation_sticky.restore();
                        }
                        if let Some(sticky) = self.live_old_to_young_sticky.as_ref() {
                            sticky.restore();
                        }
                        if let Some(snapshot) = self.pre_clear_dirty_snapshot.take() {
                            restore_surviving_dirty_coverage(&snapshot);
                        }
                        let reclaim_state =
                            self.reclaim_state.as_mut().expect("reclaim state exists");
                        reclaim_state.remembered_set_clear = None;
                        reclaim_state.subphase = ReclaimSubphase::ConservativePins;
                    } else {
                        break;
                    }
                }
                ReclaimSubphase::ConservativePins => {
                    let reclaim_start = trace_phase_start(&self.trace);
                    let phase_start = trace_phase_start(&self.trace);
                    let done = if self.minor.is_some() {
                        let clear = {
                            let reclaim_state =
                                self.reclaim_state.as_mut().expect("reclaim state exists");
                            reclaim_state
                                .conservative_pin_clear
                                .get_or_insert_with(ConservativePinClearState::new)
                                .step_counted(remaining)
                        };
                        remaining = remaining.saturating_sub(clear.work_units);
                        clear.done
                    } else {
                        true
                    };
                    trace_phase_record(&mut self.trace, "conservative_pin_clear", phase_start);
                    trace_phase_record(&mut self.trace, "reclaim", reclaim_start);
                    if done {
                        let reclaim_state =
                            self.reclaim_state.as_mut().expect("reclaim state exists");
                        reclaim_state.conservative_pin_clear = None;
                        reclaim_state.subphase = ReclaimSubphase::MallocTrim;
                    } else {
                        break;
                    }
                }
                ReclaimSubphase::MallocTrim => {
                    let reclaim_start = trace_phase_start(&self.trace);
                    let trim = run_malloc_trim(self.progress_kind);
                    if let Some(trace) = self.trace.as_mut() {
                        if trim.status == AllocatorMaintenanceStatus::Executed {
                            trace.record_phase(
                                "malloc_trim",
                                Duration::from_micros(trim.elapsed_us),
                            );
                        }
                        trace.record_malloc_trim_maintenance(
                            trim.status,
                            trim.reason,
                            trim.elapsed_us,
                        );
                    }
                    trace_phase_record(&mut self.trace, "reclaim", reclaim_start);
                    self.reclaim_state
                        .as_mut()
                        .expect("reclaim state exists")
                        .subphase = ReclaimSubphase::Publish;
                    remaining -= 1;
                }
                ReclaimSubphase::Publish => {
                    let reclaim_start = trace_phase_start(&self.trace);
                    self.publish_reclaim_outcome();
                    trace_phase_record(&mut self.trace, "reclaim", reclaim_start);
                    self.reclaim_state
                        .as_mut()
                        .expect("reclaim state exists")
                        .subphase = ReclaimSubphase::Done;
                    self.phase = GcCyclePhase::Complete;
                    break;
                }
                ReclaimSubphase::Done => {
                    self.phase = GcCyclePhase::Complete;
                    break;
                }
            }
        }
    }

    fn publish_reclaim_outcome(&mut self) {
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

        let malloc_swept = self
            .minor
            .as_ref()
            .map(|minor| minor.malloc_sweep_due)
            .unwrap_or(true);

        self.outcome = Some(GcCollectOutcome {
            freed_bytes: self.freed_bytes,
            malloc_swept,
            trace: self.trace.take(),
        });
    }
}

impl Drop for GcCycleState {
    fn drop(&mut self) {
        if matches!(self.collection_kind, GcCollectionKind::Full)
            && self.phase != GcCyclePhase::Complete
        {
            incremental_mark_barrier_disable();
            clear_mark_seeds();
        }
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
