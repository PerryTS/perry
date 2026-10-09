//! Copying eligibility checks for registered roots and dirty root slots.
use super::*;

impl CopiedMinorEligibility {
    pub(super) fn copy_only_root_preflight_reason(
        _ptrs: &CopyingPointerSet,
    ) -> (Option<CopiedMinorFallbackReason>, LegacyRootTraceStats) {
        let (registered_rust_scanners, registered_ffi_scanners) = copy_only_root_scanner_counts();
        let stats = LegacyRootTraceStats {
            registered_rust_scanners,
            registered_ffi_scanners,
            ..LegacyRootTraceStats::default()
        };
        let reason = (registered_rust_scanners > 0 || registered_ffi_scanners > 0)
            .then_some(CopiedMinorFallbackReason::CopyOnlyRoots);
        (reason, stats)
    }

    pub(super) fn mutable_root_preflight_reason(
        ptrs: &CopyingPointerSet,
    ) -> Option<CopiedMinorFallbackReason> {
        let mut checker =
            CopyingNurseryPreflight::new(ptrs, CopiedMinorFallbackReason::PinnedYoungRoot);
        visit_mutable_root_slots(|slot| unsafe {
            let bits = slot.read();
            if slot.pointer_word(bits).is_some() {
                checker.check_bits(bits);
            }
        });
        let scanners: Vec<MutableRootScannerEntry> =
            MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
        {
            let mut visitor = RuntimeRootVisitor::for_copying_check(&mut checker);
            for entry in scanners {
                let (_, nanos) = super::scanner_profile::record_scanner(|| {
                    (entry.scanner)(&mut visitor);
                });
                super::scanner_profile::note_scanner(entry.name, nanos, 0, 0, 0);
            }
            visit_ffi_mutable_registered_roots(&mut visitor);
        }
        checker.check_dirty_roots();
        unsafe {
            checker.drain();
        }
        checker.fallback_reason
    }

    pub(super) fn dirty_slot_preflight_reason(
        ptrs: &CopyingPointerSet,
    ) -> Option<CopiedMinorFallbackReason> {
        let snapshot = remembered_dirty_snapshot();
        let mut dirty_checker =
            CopyingNurseryPreflight::new(ptrs, CopiedMinorFallbackReason::PinnedYoungDirtySlot);
        scan_remembered_dirty_slots_copying(
            &snapshot,
            None,
            |slot, _header, _external, _stats| unsafe {
                dirty_checker.check_bits(slot.read());
            },
        );
        unsafe {
            dirty_checker.drain();
        }
        dirty_checker.fallback_reason
    }
}
