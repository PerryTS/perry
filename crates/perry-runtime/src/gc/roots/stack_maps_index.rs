//! Frame lookup: from an `ip` to the records that describe its frame, and the
//! cross-check that holds the lazy answer to the whole-section one.
//!
//! Its own file for the same reason `stack_maps_verify.rs` is: the parent is
//! at the repo's 2000-line cap, and pure code motion is the cheapest way to
//! stay under it.

use super::{
    lazy, EagerIndex, IndexMode, StackMapDerived, StackMapIndex, StackMapLocation, StackMapRecord,
    MAX_SAFEPOINT_RETURN_DELTA,
};

impl StackMapIndex {
    pub(super) fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// Whether the map itself names `function_address` as a function it has
    /// records for. Used by the verify walker's report, which runs on the
    /// failure path and must not dereference an address on the strength of the
    /// very data under suspicion.
    #[cfg(any(target_arch = "aarch64", test))]
    pub(super) fn vouches_for(&self, function_address: usize) -> bool {
        self.functions
            .binary_search_by_key(&function_address, |entry| entry.address)
            .is_ok()
    }

    /// The records describing the frame whose return address is `ip`.
    ///
    /// Under `PERRY_GC_STACK_MAP_CROSSCHECK=1` this also asks the v4 index the
    /// same question and aborts on any difference — see [`Self::cross_check`].
    pub(super) fn match_records(&self, ip: usize) -> Option<lazy::RecordMatch> {
        let matched = lazy::match_records(
            &self.functions,
            &self.sections,
            ip,
            MAX_SAFEPOINT_RETURN_DELTA,
        );
        if self.mode == IndexMode::CrossCheck {
            self.cross_check(ip, matched.as_ref());
        }
        matched
    }

    pub(super) fn matched(&self, matched: &lazy::RecordMatch) -> lazy::MatchedRecords<'_> {
        lazy::MatchedRecords::new(&self.functions, &self.sections, matched)
    }

    /// Materialise a match, for the cross-check and the diagnostic dump only.
    /// The walkers never do this; they visit slot by slot.
    pub(super) fn materialise(
        &self,
        matched: Option<&lazy::RecordMatch>,
    ) -> Vec<(usize, Vec<StackMapLocation>, Vec<StackMapDerived>)> {
        let mut out = Vec::new();
        let Some(matched) = matched else {
            return out;
        };
        let mut records = self.matched(matched);
        while let Some(record) = records.next() {
            let mut roots = Vec::new();
            let mut iter = record.roots();
            while let Some(slot) = iter.next() {
                let Some(slot) = slot else { return out };
                roots.push(slot);
            }
            let mut derived = Vec::new();
            let mut bases = record.derived_base_indices();
            let Some(mut slots) = record.derived_slots() else {
                return out;
            };
            while let Some(base_index) = bases.next() {
                let (Some(base_index), Some(Some(slot))) = (base_index, slots.next()) else {
                    return out;
                };
                derived.push(StackMapDerived { base_index, slot });
            }
            out.push((record.function_address, roots, derived));
        }
        out
    }

    /// Ask the v4 index the same question and abort on any difference.
    ///
    /// The oracle uses the SAME containment rule as the lazy lookup — search
    /// inside the function containing `ip` — rather than v4's global
    /// nearest-then-filter. That is deliberate: the lookup rule change is a
    /// separate, documented decision, and mixing it into this check would make
    /// every run report a difference that is not a defect and hide the ones
    /// that are. What this checks is the part that is genuinely new: that
    /// starting a decode at a recorded per-function stream offset produces the
    /// same records, roots and derived slots as decoding the whole section.
    #[cold]
    pub(super) fn cross_check(&self, ip: usize, matched: Option<&lazy::RecordMatch>) {
        let Some(eager) = self.eager.as_ref() else {
            return;
        };
        let expected = eager.match_records_in_owning_function(ip);
        let expected: Vec<(usize, Vec<StackMapLocation>, Vec<StackMapDerived>)> = expected
            .iter()
            .map(|record| {
                (
                    record.function_address,
                    eager.locations(record).to_vec(),
                    eager.derived_locations(record).to_vec(),
                )
            })
            .collect();
        let mut expected = expected;
        let mut actual = self.materialise(matched);
        // Order-insensitive: the two indexes enumerate a multi-record match in
        // different orders (stream order versus pc order), and an abort from
        // the tool that exists to give confidence must mean a real difference.
        for side in [&mut actual, &mut expected] {
            for entry in side.iter_mut() {
                entry.1.sort_unstable();
                entry.2.sort_unstable();
            }
            side.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        }
        if actual != expected {
            panic!(
                "perry: the lazy stack-map index disagrees with the whole-section index for \
                 ip {ip:#x}. lazy={actual:?} eager={expected:?}. One of them is describing the \
                 wrong words as live, and the collector would rewrite whichever it was given."
            );
        }
    }
}

pub(super) fn closest_record_pc(maps: &[StackMapRecord], ip: usize) -> Option<usize> {
    let insertion = maps.partition_point(|record| record.pc < ip);
    let before = insertion
        .checked_sub(1)
        .and_then(|idx| maps.get(idx))
        .map(|record| record.pc);
    let at_or_after = maps.get(insertion).map(|record| record.pc);
    match (before, at_or_after) {
        (Some(before), Some(after)) => Some(if ip.abs_diff(before) <= ip.abs_diff(after) {
            before
        } else {
            after
        }),
        (Some(before), None) => Some(before),
        (None, Some(after)) => Some(after),
        (None, None) => None,
    }
}

impl EagerIndex {
    /// The v4 records for `ip`, restricted to the function containing it.
    ///
    /// The ±16 window is a distance, not a containment check: nothing in it
    /// says the matched record belongs to the function `ip` is executing.
    /// Functions are adjacent in .text, so an `ip` early in B can sit within
    /// the window of a safepoint at the end of A — and the walker would then
    /// use A's frame offsets against B's frame and rewrite unrelated words.
    ///
    /// Residual gap, stated rather than papered over: a function with no
    /// safepoints is absent from `function_starts`, so an `ip` inside one
    /// resolves to the previous mapped function. Closing that needs a
    /// per-function code extent, which Mach-O does not expose cheaply
    /// (`Lfunc_end` covers only EH-carrying functions; there is no `.size`).
    pub(super) fn match_records_in_owning_function(&self, ip: usize) -> &[StackMapRecord] {
        let Some(owning) = self
            .function_starts
            .partition_point(|start| *start <= ip)
            .checked_sub(1)
            .map(|index| self.function_starts[index])
        else {
            return &[];
        };
        // `records` is sorted by PC. A function's records are nonetheless
        // CONTIGUOUS in it, because every record's pc is its function address
        // plus a non-negative offset and functions occupy disjoint code
        // ranges — the same disjointness the containment check above already
        // rests on. Binary-searching `function_address` would be searching a
        // key this array is not sorted on.
        let first = self.records.partition_point(|record| record.pc < owning);
        let mut last = first;
        while self
            .records
            .get(last)
            .is_some_and(|record| record.function_address == owning)
        {
            last += 1;
        }
        let owned = &self.records[first..last];
        let Some(candidate_pc) = closest_record_pc(owned, ip) else {
            return &[];
        };
        if ip.abs_diff(candidate_pc) > MAX_SAFEPOINT_RETURN_DELTA {
            return &[];
        }
        let from = owned.partition_point(|record| record.pc < candidate_pc);
        let to = owned.partition_point(|record| record.pc <= candidate_pc);
        &owned[from..to]
    }
}
