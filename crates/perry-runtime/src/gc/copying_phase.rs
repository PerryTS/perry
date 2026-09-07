//! Diagnostic-only phase accounting for the copying minor.
//!
//! The collector's phases are not all contiguous: registered roots are walked
//! once to evacuate and once to repair forwarding addresses, and in-place
//! promotion has an early retag plus a late finish.  The accumulator therefore
//! records non-overlapping spans into semantic buckets.  Anything outside a
//! priced span is reported as `other`; top-level buckets plus `other` are an
//! exact partition of the same `Instant` interval used for `pause_us`.

use std::fmt::Write;
use std::time::Instant;

#[derive(Clone, Copy)]
pub(super) enum CopyingMinorPhase {
    RootScan,
    CopyEvacuation,
    RememberedSetYoungLogs,
    Promotion,
    DeadOwnerSideTablePruning,
    FromSpaceFinalization,
    ForwardingFixups,
    BlockResetFlip,
}

#[derive(Default)]
pub(super) struct CopyingMinorPhaseDiag {
    root_scan_ns: u64,
    copy_evacuation_ns: u64,
    remembered_set_young_logs_ns: u64,
    promotion_ns: u64,
    dead_owner_side_table_pruning_ns: u64,
    from_space_finalization_ns: u64,
    forwarding_fixups_ns: u64,
    block_reset_flip_ns: u64,
}

impl CopyingMinorPhaseDiag {
    #[inline]
    pub(super) fn enabled() -> Option<Self> {
        super::gc_diag_enabled().then(Self::default)
    }

    #[inline]
    pub(super) fn start(diag: &Option<Self>) -> Option<Instant> {
        diag.as_ref().map(|_| Instant::now())
    }

    #[inline]
    pub(super) fn record(&mut self, phase: CopyingMinorPhase, start: Option<Instant>) {
        let Some(start) = start else {
            return;
        };
        self.add_nanos(phase, start.elapsed().as_nanos() as u64);
    }

    #[inline]
    pub(super) fn add_nanos(&mut self, phase: CopyingMinorPhase, nanos: u64) {
        let slot = match phase {
            CopyingMinorPhase::RootScan => &mut self.root_scan_ns,
            CopyingMinorPhase::CopyEvacuation => &mut self.copy_evacuation_ns,
            CopyingMinorPhase::RememberedSetYoungLogs => &mut self.remembered_set_young_logs_ns,
            CopyingMinorPhase::Promotion => &mut self.promotion_ns,
            CopyingMinorPhase::DeadOwnerSideTablePruning => {
                &mut self.dead_owner_side_table_pruning_ns
            }
            CopyingMinorPhase::FromSpaceFinalization => &mut self.from_space_finalization_ns,
            CopyingMinorPhase::ForwardingFixups => &mut self.forwarding_fixups_ns,
            CopyingMinorPhase::BlockResetFlip => &mut self.block_reset_flip_ns,
        };
        *slot = slot.saturating_add(nanos);
    }

    fn named_nanos(&self) -> u64 {
        self.root_scan_ns
            .saturating_add(self.copy_evacuation_ns)
            .saturating_add(self.remembered_set_young_logs_ns)
            .saturating_add(self.promotion_ns)
            .saturating_add(self.dead_owner_side_table_pruning_ns)
            .saturating_add(self.from_space_finalization_ns)
            .saturating_add(self.forwarding_fixups_ns)
            .saturating_add(self.block_reset_flip_ns)
    }

    pub(super) fn render(
        &self,
        pause_ns: u64,
        scan_us: u64,
        copied_objects: usize,
        copied_bytes: usize,
        promoted_objects: usize,
        promoted_bytes: usize,
        remembered_entries: usize,
        remembered_slots: usize,
        finalization: &CopiedMinorFinalizationDiag,
    ) -> String {
        let named_ns = self.named_nanos();
        let other_ns = pause_ns.saturating_sub(named_ns);
        let phase_sum_ns = named_ns.saturating_add(other_ns);
        let mut out = String::new();
        write!(
            out,
            "root_scan={}/{} copy_evacuation={}/{}/{} remembered_set_young_logs={}/{}/{} promotion={}/{}/{} dead_owner_side_table_pruning={}{} from_space_finalization={}/map:{}/{}+set:{}/{}+errors:{}/{}+regex:{}/{} forwarding_fixups={} block_reset_flip={} other={} phase_sum_us={}",
            self.root_scan_ns / 1000,
            scan_us,
            self.copy_evacuation_ns / 1000,
            copied_objects,
            copied_bytes,
            self.remembered_set_young_logs_ns / 1000,
            remembered_entries,
            remembered_slots,
            self.promotion_ns / 1000,
            promoted_objects,
            promoted_bytes,
            self.dead_owner_side_table_pruning_ns / 1000,
            finalization.dead_owner_detail,
            self.from_space_finalization_ns / 1000,
            finalization.map_ns / 1000,
            finalization.maps,
            finalization.set_ns / 1000,
            finalization.sets,
            finalization.errors_ns / 1000,
            finalization.errors,
            finalization.regex_ns / 1000,
            finalization.regexps,
            self.forwarding_fixups_ns / 1000,
            self.block_reset_flip_ns / 1000,
            other_ns / 1000,
            phase_sum_ns / 1000,
        )
        .expect("writing phase diagnostics to a String cannot fail");
        out
    }
}

#[derive(Default)]
pub(super) struct CopiedMinorFinalizationDiag {
    pub(super) total_ns: u64,
    pub(super) map_ns: u64,
    pub(super) maps: usize,
    pub(super) set_ns: u64,
    pub(super) sets: usize,
    pub(super) errors_ns: u64,
    pub(super) errors: usize,
    pub(super) regex_ns: u64,
    pub(super) regexps: usize,
    pub(super) dead_owner_ns: u64,
    pub(super) dead_owner_detail: String,
}

/// Finalize the side allocations whose from-space owners just died.  The
/// clocks live here, beside the calls they price; without `PERRY_GC_DIAG` this
/// performs the original calls without reading the clock or building strings.
pub(super) fn finalize_dead_copied_minor_from_space_side_allocations() -> CopiedMinorFinalizationDiag
{
    let diag = super::gc_diag_enabled();
    let total_start = diag.then(Instant::now);
    let mut out = CopiedMinorFinalizationDiag::default();

    crate::promise::cleanup_copied_minor_promise_contexts_for_gc();

    let start = diag.then(Instant::now);
    out.maps = crate::map::finalize_dead_copied_minor_from_space_maps();
    out.map_ns = start.map_or(0, |start| start.elapsed().as_nanos() as u64);

    let start = diag.then(Instant::now);
    out.sets = crate::set::finalize_dead_copied_minor_from_space_sets();
    out.set_ns = start.map_or(0, |start| start.elapsed().as_nanos() as u64);

    let start = diag.then(Instant::now);
    out.errors =
        crate::node_submodules::diagnostics_gc::finalize_dead_copied_minor_from_space_errors();
    out.errors_ns = start.map_or(0, |start| start.elapsed().as_nanos() as u64);

    let start = diag.then(Instant::now);
    out.regexps = crate::regex::finalize_dead_copied_minor_from_space_regexps();
    out.regex_ns = start.map_or(0, |start| start.elapsed().as_nanos() as u64);

    let start = diag.then(Instant::now);
    out.dead_owner_detail = super::dead_owner::prune_dead_owner_side_tables_copied_minor();
    out.dead_owner_ns = start.map_or(0, |start| start.elapsed().as_nanos() as u64);
    out.total_ns = total_start.map_or(0, |start| start.elapsed().as_nanos() as u64);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_minor_phase_residual_makes_the_partition_exact() {
        let mut diag = CopyingMinorPhaseDiag::default();
        diag.add_nanos(CopyingMinorPhase::RootScan, 11_000);
        diag.add_nanos(CopyingMinorPhase::CopyEvacuation, 7_000);
        diag.add_nanos(CopyingMinorPhase::BlockResetFlip, 3_000);

        let pause_ns: u64 = 29_000;
        let other_ns = pause_ns.saturating_sub(diag.named_nanos());
        assert_eq!(diag.named_nanos() + other_ns, pause_ns);
        assert_eq!(other_ns, 8_000);
        // Sabotage: remove one named bucket from `named_nanos`; this exact
        // residual assertion changes and the test fails rather than merely
        // checking that phase reporting did not panic.
    }
}
