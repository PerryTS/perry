//! Transitive copying-nursery eligibility walk.
use super::*;

pub(in crate::gc) struct CopyingNurseryPreflight {
    pub(in crate::gc) ptrs: *const CopyingPointerSet,
    pub(in crate::gc) fallback_reason: Option<CopiedMinorFallbackReason>,
    pub(in crate::gc) pinned_reason: CopiedMinorFallbackReason,
    pub(in crate::gc) worklist: Vec<*mut GcHeader>,
    pub(in crate::gc) seen: crate::fast_hash::PtrHashSet<usize>,
    pub(in crate::gc) ephemerons: super::ephemeron::Ephemerons,
}

impl CopyingNurseryPreflight {
    pub(in crate::gc) fn new(
        ptrs: &CopyingPointerSet,
        pinned_reason: CopiedMinorFallbackReason,
    ) -> Self {
        Self {
            ptrs,
            fallback_reason: None,
            pinned_reason,
            worklist: Vec::new(),
            seen: crate::fast_hash::new_ptr_hash_set(),
            ephemerons: super::ephemeron::Ephemerons::default(),
        }
    }

    pub(in crate::gc) fn ptrs(&self) -> &CopyingPointerSet {
        unsafe { &*self.ptrs }
    }

    pub(in crate::gc) fn check_bits(&mut self, bits: u64) {
        self.check_bits_with_reason(bits, self.pinned_reason);
    }

    pub(in crate::gc) fn check_bits_with_reason(
        &mut self,
        bits: u64,
        pinned_reason: CopiedMinorFallbackReason,
    ) {
        if self.fallback_reason.is_some() {
            return;
        }
        match self.ptrs().decode_bits_for_preflight(bits) {
            Ok(Some((_addr, ptr))) => self.check_ptr_with_reason(ptr, pinned_reason),
            Ok(None) => {}
            Err(reason) => self.fallback_reason = Some(reason),
        }
    }

    pub(in crate::gc) fn check_addr(&mut self, addr: usize) {
        self.check_addr_with_reason(addr, self.pinned_reason);
    }

    pub(in crate::gc) fn check_addr_with_reason(
        &mut self,
        addr: usize,
        pinned_reason: CopiedMinorFallbackReason,
    ) {
        if self.fallback_reason.is_some() {
            return;
        }
        let ptr = match self.ptrs().classify_for_preflight(addr, true) {
            Ok(Some(ptr)) => ptr,
            Ok(None) => return,
            Err(reason) => {
                self.fallback_reason = Some(reason);
                return;
            }
        };
        self.check_ptr_with_reason(ptr, pinned_reason);
    }

    pub(in crate::gc) fn check_ptr_with_reason(
        &mut self,
        ptr: CopyingPointer,
        pinned_reason: CopiedMinorFallbackReason,
    ) {
        unsafe {
            if matches!(
                ptr.kind,
                CopyingPointerKind::Eden | CopyingPointerKind::FromSurvivor
            ) && (*ptr.header).gc_flags & GC_FLAG_PINNED != 0
            {
                self.fallback_reason = Some(pinned_reason);
                return;
            }
        }
        if matches!(
            ptr.kind,
            CopyingPointerKind::Eden
                | CopyingPointerKind::FromSurvivor
                | CopyingPointerKind::Longlived
                | CopyingPointerKind::Malloc
        ) && self.seen.insert(ptr.header as usize)
        {
            self.worklist.push(ptr.header);
        }
    }

    pub(in crate::gc) unsafe fn drain<const DIAGNOSTICS: bool>(&mut self) {
        let mut i = 0usize;
        loop {
            while i < self.worklist.len() && self.fallback_reason.is_none() {
                let header = self.worklist[i];
                i += 1;
                if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
                    continue;
                }
                self.scan_object_fields::<DIAGNOSTICS>(header);
            }
            self.check_ephemerons();
            if i == self.worklist.len() || self.fallback_reason.is_some() {
                break;
            }
        }
    }

    pub(in crate::gc) unsafe fn scan_object_fields<const DIAGNOSTICS: bool>(
        &mut self,
        header: *mut GcHeader,
    ) {
        if (*header).obj_type == GC_TYPE_WEAK_STORAGE {
            self.ephemerons
                .add((header as *mut u8).add(GC_HEADER_SIZE).cast());
            return;
        }
        let mut weak_holder: Option<bool> = None;
        visit_gc_rewrite_slots::<DIAGNOSTICS>(header, |slot| unsafe {
            // Weak-only reachability imposes no copy constraint: the
            // collector never evacuates through a weak edge (a weak-only
            // young target dies in place and tombstones), so a pinned
            // target behind one must not force the fallback path.
            if *weak_holder.get_or_insert_with(|| weak_holder_fact(header))
                && crate::weakref::is_weak_target_trace_slot(header, slot.slot)
            {
                return;
            }
            if DIAGNOSTICS {
                slot.record_layout_read();
            }
            self.check_bits_with_reason(
                slot.read(),
                CopiedMinorFallbackReason::PinnedYoungTransitive,
            );
        });
    }
}
