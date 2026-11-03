//! Atomic finalization of the collector state machine.
use super::*;

impl GcCycleState {
    pub(super) fn step_atomic_finalize<const DIAGNOSTICS: bool>(&mut self, budget: GcWorkBudget) {
        let remark = self.progress_kind.is_budgeted();
        self.atomic_finalize
            .get_or_insert_with(|| AtomicFinalizeCycleState::new(self.collection_kind, remark));
        if budget.work_units == 0 {
            // Status probe: never start the atomic tail on a zero budget.
            return;
        }
        loop {
            let subphase = self
                .atomic_finalize
                .as_ref()
                .expect("atomic finalize state exists")
                .subphase;
            // SLICED subphases honor the caller's budget and may return to the
            // mutator. Post-remark windows are sound only because EVERY way the
            // mutator can acquire a heap reference is shaded: stores by the
            // incremental mark barrier, births by allocate-black, and weak
            // READS by `weakref::read_barrier` (#7900 — that arm was missing).
            let sliced = matches!(
                subphase,
                AtomicFinalizeSubphase::BarrierSeedDrain
                    | AtomicFinalizeSubphase::RememberedSetReady
                    | AtomicFinalizeSubphase::WeakProcessing
                    | AtomicFinalizeSubphase::EphemeronClear
            );
            let sub_budget = if sliced {
                budget.work_units
            } else {
                usize::MAX
            };
            let phase_start = if DIAGNOSTICS {
                trace_phase_start(&self.trace)
            } else {
                None
            };
            self.step_atomic_finalize_current_subphase::<DIAGNOSTICS>(sub_budget);
            if DIAGNOSTICS {
                trace_phase_record(&mut self.trace, "atomic_finalize", phase_start)
            };
            if self.phase != GcCyclePhase::AtomicFinalize {
                break;
            }
            let advanced = self
                .atomic_finalize
                .as_ref()
                .expect("atomic finalize state exists")
                .subphase
                != subphase;
            if sliced && !advanced && budget.work_units != usize::MAX {
                break;
            }
        }
    }

    pub(super) fn step_atomic_finalize_current_subphase<const DIAGNOSTICS: bool>(
        &mut self,
        budget: usize,
    ) {
        let subphase = self
            .atomic_finalize
            .as_ref()
            .expect("atomic finalize state exists")
            .subphase;
        match subphase {
            AtomicFinalizeSubphase::FinalRootRemark => {
                if budget == 0 {
                    return;
                }
                // Re-scan every root with the marks nearly final (see the enum
                // doc). DELIBERATELY ATOMIC and NOT bounded by the step budget:
                // the scan is bounded by root-set size, but the drain below is
                // bounded by the newly-reachable graph, which a root installed
                // after the initial scan can make arbitrarily large. Measured
                // rather than claimed — `final_remark_max_us` in
                // `[gc-step-bounds]`; docs/src/internals/gc-step-bounds.md has
                // the bound and rejected alternatives (#7903).
                // consider_evacuation is false: pinning was decided already.
                {
                    let _remark_timer = instruments::FinalRemarkTimer::start();
                    let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
                    let minor_only = self.minor.is_some();
                    let remark_minor_only = matches!(self.collection_kind, GcCollectionKind::Minor);
                    let remark_scan = self
                        .root_scan
                        .get_or_insert_with(|| RootScanCycleState::new(remark_minor_only));
                    while !remark_scan.step_current_subphase::<DIAGNOSTICS>(
                        valid_ptrs,
                        &mut self.trace,
                        /* consider_evacuation = */ false,
                        usize::MAX,
                        /* allow_synchronous_scanners = */ true,
                        minor_only,
                    ) {}
                    self.root_scan = None;
                    // Trace everything the remark newly discovered so weak
                    // processing sees a complete mark set and newly traced old
                    // parents contribute their remembered entries.
                    let mut remark_drain = TraceWorklistCycleState::new(minor_only);
                    while !remark_drain.step::<DIAGNOSTICS>(
                        valid_ptrs,
                        usize::MAX,
                        self.live_old_to_young_sticky.as_mut(),
                        &mut self.ephemerons,
                    ) {}
                }
                let next = if self.minor.is_some() {
                    AtomicFinalizeSubphase::WeakProcessing
                } else {
                    AtomicFinalizeSubphase::RememberedSetReady
                };
                self.atomic_finalize
                    .as_mut()
                    .expect("atomic finalize state exists")
                    .subphase = next;
            }
            AtomicFinalizeSubphase::WeakProcessing => {
                self.step_weak_ephemerons::<DIAGNOSTICS>(budget, false)
            }
            AtomicFinalizeSubphase::EphemeronClear => {
                self.step_weak_ephemerons::<DIAGNOSTICS>(budget, true)
            }
            AtomicFinalizeSubphase::MinorPrelude => {
                if budget == 0 {
                    return;
                }
                self.atomic_finalize_minor_prelude::<DIAGNOSTICS>();
                self.atomic_finalize
                    .as_mut()
                    .expect("atomic finalize state exists")
                    .subphase = AtomicFinalizeSubphase::RememberedSetReady;
            }
            AtomicFinalizeSubphase::BarrierSeedDrain => {
                let minor_only = self.minor.is_some();
                let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
                let done = {
                    let state = self
                        .atomic_finalize
                        .as_mut()
                        .expect("atomic finalize state exists");
                    let drain = state
                        .barrier_drain
                        .get_or_insert_with(|| TraceWorklistCycleState::new(minor_only));
                    drain.step::<DIAGNOSTICS>(
                        valid_ptrs,
                        budget,
                        self.live_old_to_young_sticky.as_mut(),
                        &mut self.ephemerons,
                    )
                };
                if done {
                    let state = self
                        .atomic_finalize
                        .as_mut()
                        .expect("atomic finalize state exists");
                    state.barrier_drain = None;
                    // Kind-specific continuation (see AtomicFinalizeCycleState::new).
                    state.subphase = if state.remark {
                        AtomicFinalizeSubphase::FinalRootRemark
                    } else if minor_only {
                        AtomicFinalizeSubphase::WeakProcessing
                    } else {
                        AtomicFinalizeSubphase::RememberedSetReady
                    };
                }
            }
            AtomicFinalizeSubphase::RememberedSetReady => {
                // A minor keeps its incremental remembered set (#6181), plus
                // evacuation entries and the pre-clear dirty snapshot repair.
                // Only full mark drains populate live_old_to_young_sticky.
                if self.minor.is_some() {
                    // Drain any seeds the barrier pushed since
                    // BarrierSeedDrain completed (late stores mark the child
                    // but its children still need the trace). Bounded by
                    // late-store volume.
                    //
                    // The barrier deliberately STAYS ENABLED here: the sweep
                    // state (and its per-block fill snapshot) is only built
                    // on the next step_sweep slice, and the mutator runs in
                    // between. With the barrier (and its allocate-black birth
                    // flags) off in that window, an object allocated and
                    // linked there is WHITE yet INSIDE the sweep snapshot —
                    // it gets freed live (observed: React fibers created
                    // during a render burst between finalize and sweep lost
                    // their side-table fields; the compiled TUI's
                    // pendingProps/lanes/slice crashes). step_sweep drains
                    // the gap's seeds and disables the barrier in the same
                    // slice that takes the snapshot.
                    let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
                    let mut final_drain = TraceWorklistCycleState::new(true);
                    while !final_drain.step::<DIAGNOSTICS>(
                        valid_ptrs,
                        usize::MAX,
                        self.live_old_to_young_sticky.as_mut(),
                        &mut self.ephemerons,
                    ) {}
                    self.atomic_finalize = None;
                    self.phase = GcCyclePhase::Sweep;
                    return;
                }
                // Full drains already remembered each live old parent's strong
                // slots while tracing them. Keep that buffer in the cycle until
                // reclaim, including entries from remark and sweep-gap seeds.
                // Weak processing follows without another heap/descriptor walk.
                self.atomic_finalize
                    .as_mut()
                    .expect("atomic finalize state exists")
                    .subphase = AtomicFinalizeSubphase::WeakProcessing;
            }
            AtomicFinalizeSubphase::DisableBarrier => {
                if budget == 0 {
                    return;
                }
                // Same late-seed closure as the minor path: trace anything
                // the barrier shaded after BarrierSeedDrain completed, so no
                // marked-but-untraced object reaches Sweep with unmarked
                // children. The barrier stays enabled until step_sweep takes
                // the block snapshot (see the minor arm's gap comment).
                {
                    let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
                    let mut final_drain = TraceWorklistCycleState::new(false);
                    while !final_drain.step::<DIAGNOSTICS>(
                        valid_ptrs,
                        usize::MAX,
                        self.live_old_to_young_sticky.as_mut(),
                        &mut self.ephemerons,
                    ) {}
                }
                #[cfg(any(test, perry_check_contracts))]
                if DIAGNOSTICS && crate::gc::gc_verify_mark_enabled() {
                    if DIAGNOSTICS {
                        super::verify_honest::verify_honest_pointers("full-sweep-start");
                    };
                }
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

    pub(super) fn atomic_finalize_minor_prelude<const DIAGNOSTICS: bool>(&mut self) {
        let valid_ptrs = self.valid_ptrs.as_ref().expect("valid pointer set built");
        if DIAGNOSTICS && gc_verify_evacuation_enabled() {
            let phase_start = if DIAGNOSTICS {
                trace_phase_start(&self.trace)
            } else {
                None
            };
            let old_young_edge_verifier = verify_old_to_young_edges_covered();
            if DIAGNOSTICS {
                trace_phase_record(&mut self.trace, "old_young_edge_verify", phase_start)
            };
            if let Some(trace) = self.trace.as_mut().filter(|_| DIAGNOSTICS) {
                trace.old_young_edge_verifier = old_young_edge_verifier;
            }
        }
        // Diagnostic (PERRY_GC_VERIFY_MARK): marks are final for this minor and
        // sweep has not yet run — report any OLD parent whose young/malloc child
        // is UNMARKED (about to be swept live = dropped remembered-set edge).
        if DIAGNOSTICS && crate::gc::gc_verify_mark_enabled() {
            if DIAGNOSTICS {
                super::verify::verify_minor_unmarked_young_children_report("minor-prelude")
            };
            #[cfg(any(test, perry_check_contracts))]
            if DIAGNOSTICS {
                super::verify_honest::verify_honest_pointers("minor-prelude");
            };
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
        if let Some(trace) = self.trace.as_mut().filter(|_| DIAGNOSTICS) {
            trace.evacuation_policy = minor.evacuation_policy;
        }
        assert!(
            !progress_kind.is_budgeted() || !minor.evacuation_policy.enabled,
            "budgeted low-pause minor GC must remain non-moving"
        );

        let mut evacuation = EvacuationTraceStats::default();
        let mut evacuation_sticky = StickyRememberedSet::default();
        if minor.evacuation_policy.enabled {
            let _heap_change = crate::gc::heap_generation::HeapChange::begin(
                crate::gc::heap_generation::HeapChangeKind::Evacuation,
            );
            let phase_start = if DIAGNOSTICS {
                trace_phase_start(&self.trace)
            } else {
                None
            };
            let mut evacuated_new_headers = Vec::new();
            let mut evacuated_original_headers = Vec::new();
            evacuation = evacuate_tenured_nursery_objects_collecting(
                minor.evacuation_policy.force,
                &mut evacuated_new_headers,
                &mut evacuated_original_headers,
            );
            let old_page_evacuation = {
                let _compaction = crate::gc::heap_generation::HeapChange::begin(
                    crate::gc::heap_generation::HeapChangeKind::Compaction,
                );
                evacuate_selected_old_pages_collecting(
                    &minor.old_page_selection.pages,
                    &mut evacuated_new_headers,
                    &mut evacuated_original_headers,
                )
            };
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
            if DIAGNOSTICS {
                trace_phase_record(&mut self.trace, "evacuation", phase_start)
            };
            if evacuation.objects > 0 {
                let phase_start = if DIAGNOSTICS {
                    trace_phase_start(&self.trace)
                } else {
                    None
                };
                match self.trace.as_mut().filter(|_| DIAGNOSTICS) {
                    Some(trace) => rewrite_forwarded_references::<DIAGNOSTICS>(
                        valid_ptrs,
                        Some(&mut trace.shadow_roots),
                        Some(&mut trace.root_sources),
                    ),
                    None => rewrite_forwarded_references::<DIAGNOSTICS>(valid_ptrs, None, None),
                }
                evacuation_sticky = rebuild_evacuated_old_to_young_remembered_set::<DIAGNOSTICS>(
                    &evacuated_new_headers,
                );
                if DIAGNOSTICS {
                    trace_phase_record(&mut self.trace, "reference_rewrite", phase_start)
                };
                if DIAGNOSTICS && gc_verify_evacuation_enabled() {
                    let phase_start = if DIAGNOSTICS {
                        trace_phase_start(&self.trace)
                    } else {
                        None
                    };
                    let context = begin_evacuation_verify_cycle(self.trigger_kind, None);
                    verify_evacuated_no_stale_forwarded_refs(
                        EvacuationVerifier::all_forwarded(valid_ptrs).with_context(context),
                    );
                    if DIAGNOSTICS {
                        trace_phase_record(&mut self.trace, "evacuation_verify", phase_start)
                    };
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
}
