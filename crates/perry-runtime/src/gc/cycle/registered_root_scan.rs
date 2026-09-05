//! The two registered-root scan cursors of the budgeted root scan.
//!
//! `RootScanCycleState` drives its `MutableRegisteredScanners` and
//! `LegacyRegisteredScanners` subphases through these; each keeps a snapshot
//! of the registry it walks plus a cursor, so a budgeted cycle can stop
//! between scanners and resume where it left off without re-reading a
//! registry the mutator may have grown in between.
//!
//! They live beside `cycle.rs` rather than in it because that file is at the
//! file-size gate; nothing else about the split is meaningful.

use super::*;

pub(super) struct MutableRegisteredRootScanState {
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
    pub(super) fn new() -> Self {
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

    pub(super) fn step(
        &mut self,
        valid_ptrs: &ValidPointerSet,
        mut root_sources: Option<&mut RootSourcesTraceStats>,
        budget: usize,
        allow_synchronous_scanners: bool,
        minor_only: bool,
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
        // #9754: a minor-only trace is a young-scoped visit for the logged
        // side tables (`gc/young_log.rs`); a full trace walks everything.
        let mut visitor = RuntimeRootVisitor::for_mark_scoped(valid_ptrs, minor_only);
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

pub(super) struct LegacyRegisteredRootScanState {
    scanners: Vec<fn(&mut dyn FnMut(f64))>,
    ffi_scanners: Vec<PerryFfiRootScanner>,
    scanner_cursor: usize,
    ffi_cursor: usize,
    stats: LegacyRootTraceStats,
}

impl LegacyRegisteredRootScanState {
    pub(super) fn new() -> Self {
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

    pub(super) fn step(
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

    pub(super) fn stats(&self) -> LegacyRootTraceStats {
        self.stats
    }
}
