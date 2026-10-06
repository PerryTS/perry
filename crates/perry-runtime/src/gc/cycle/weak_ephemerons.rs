//! Bounded conditional-edge closure and weak decisions.
use super::*;

impl GcCycleState {
    pub(super) fn step_weak_ephemerons(&mut self, budget: usize, clearing: bool) {
        if budget == 0 {
            return;
        }
        let valid = self.valid_ptrs.as_ref().expect("valid pointer set built");
        let minor = self.minor.is_some();
        let state = self
            .atomic_finalize
            .as_mut()
            .expect("atomic finalize state");
        if state.weak_closure.is_none() {
            let closure = TraceWorklistCycleState::new(minor);
            // Mark bits are monotone during a non-moving cycle. With no new
            // strong seed, proxy observation or discovered table, the previous
            // fixed point remains valid; do not rescan every table per holder.
            if !closure.worklist.is_empty() || self.ephemerons.has_pending_work() {
                self.ephemerons.restart_ready_scan();
                state.weak_closure = Some(closure);
            }
        }
        if let Some(closure) = state.weak_closure.as_mut() {
            let atomic_registry = state
                .weak_processing
                .as_mut()
                .is_some_and(|weak| weak.registry_closure_requires_atomic_finish(valid, minor));
            if atomic_registry {
                // The existing adversarial-registry fallback must also close
                // conditional edges. Mutator writes cannot prevent its capped
                // restart counter from advancing while closure is suspended.
                let start = trace_phase_start(&self.trace);
                while !closure.step(
                    valid,
                    usize::MAX,
                    self.live_old_to_young_sticky.as_mut(),
                    &mut self.ephemerons,
                ) {}
                trace_phase_record(&mut self.trace, "weak_registry_closure_atomic", start);
            } else {
                if !closure.step(
                    valid,
                    budget,
                    self.live_old_to_young_sticky.as_mut(),
                    &mut self.ephemerons,
                ) {
                    return;
                }
            }
        }
        // The closure and decision phases each consume at most this budget.
        // Enter decisions in the same call that closes the fixed point: a
        // one-unit budget must not perpetually restart at its last table edge.
        state.weak_closure = None;
        if !clearing {
            let weak = state
                .weak_processing
                .get_or_insert_with(crate::weakref::FullWeakProcessingState::new);
            if weak.step(valid, minor, true, budget) {
                state.weak_processing = None;
                state.subphase = AtomicFinalizeSubphase::EphemeronClear;
            }
        } else if self.ephemerons.finish_step(valid, minor, budget) {
            // Clear only after all recoverable WeakRefs have been decided.
            // Drop pending addresses before the relocating minor prelude.
            self.ephemerons = super::super::ephemeron::Ephemerons::default();
            state.subphase = if minor {
                AtomicFinalizeSubphase::MinorPrelude
            } else {
                AtomicFinalizeSubphase::DisableBarrier
            };
        }
    }
}
