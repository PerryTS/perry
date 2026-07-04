use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CopyingPointerKind {
    Eden,
    FromSurvivor,
    ToSurvivor,
    Longlived,
    Old,
    Malloc,
}

#[derive(Clone, Copy)]
pub(super) struct CopyingPointer {
    pub(super) header: *mut GcHeader,
    pub(super) kind: CopyingPointerKind,
}

pub(super) struct CopyingPointerSet {
    pub(super) malloc_registry_available: Cell<bool>,
    pub(super) malloc_registry_empty_at_start: bool,
    pub(super) malloc_validation_lookups: Cell<usize>,
    pub(super) malloc_registry_rebuild_count_start: u64,
}

impl CopyingPointerSet {
    pub(super) fn new() -> Self {
        let (malloc_registry_available, malloc_registry_empty_at_start) = MALLOC_STATE.with(|s| {
            let s = s.borrow();
            (s.malloc_registry_available(), s.objects.is_empty())
        });
        let malloc_registry_rebuild_count_start = MALLOC_REGISTRY_REBUILD_COUNT.with(|c| c.get());
        Self {
            malloc_registry_available: Cell::new(malloc_registry_available),
            malloc_registry_empty_at_start,
            malloc_validation_lookups: Cell::new(0),
            malloc_registry_rebuild_count_start,
        }
    }

    #[inline]
    pub(super) fn classify(&self, addr: usize) -> Option<CopyingPointer> {
        self.classify_arena(addr)
            .or_else(|| self.classify_malloc(addr))
    }

    #[inline]
    pub(super) fn classify_for_preflight(
        &self,
        addr: usize,
        possible_malloc: bool,
    ) -> Result<Option<CopyingPointer>, CopiedMinorFallbackReason> {
        if let Some(ptr) = self.classify_arena(addr) {
            return Ok(Some(ptr));
        }
        if possible_malloc && !self.malloc_registry_available.get() {
            // With no malloc-tracked objects, every non-arena candidate is
            // exactly rejectable without activating the lazy header registry.
            if self.malloc_registry_empty_at_start {
                return Ok(None);
            }
            return Err(CopiedMinorFallbackReason::MallocRegistryUnavailable);
        }
        Ok(self.classify_malloc(addr))
    }

    #[inline]
    pub(super) fn classify_arena(&self, addr: usize) -> Option<CopyingPointer> {
        if addr < GC_HEADER_SIZE {
            return None;
        }
        let space = crate::arena::classify_heap_space(addr);
        if matches!(space, crate::arena::HeapSpace::Unknown) {
            return None;
        }
        let header_addr = addr - GC_HEADER_SIZE;
        if !matches!(
            crate::arena::classify_heap_space(header_addr),
            crate::arena::HeapSpace::NurseryEden
                | crate::arena::HeapSpace::Survivor0
                | crate::arena::HeapSpace::Survivor1
                | crate::arena::HeapSpace::Longlived
                | crate::arena::HeapSpace::Old
        ) {
            return None;
        }
        let header = header_addr as *mut GcHeader;
        if unsafe { !plausible_gc_header(header, true) } {
            return None;
        }
        let active_survivor = crate::arena::active_survivor_space();
        let inactive_survivor = crate::arena::inactive_survivor_space();
        let kind = match space {
            crate::arena::HeapSpace::NurseryEden => CopyingPointerKind::Eden,
            s if s == active_survivor => CopyingPointerKind::FromSurvivor,
            s if s == inactive_survivor => CopyingPointerKind::ToSurvivor,
            crate::arena::HeapSpace::Longlived => CopyingPointerKind::Longlived,
            crate::arena::HeapSpace::Old => CopyingPointerKind::Old,
            _ => return None,
        };
        Some(CopyingPointer { header, kind })
    }

    #[inline]
    pub(super) fn classify_malloc(&self, addr: usize) -> Option<CopyingPointer> {
        if addr < GC_HEADER_SIZE || !self.malloc_registry_available.get() {
            return None;
        }
        let header = unsafe { header_from_user_ptr(addr as *const u8) };
        self.malloc_validation_lookups
            .set(self.malloc_validation_lookups.get().saturating_add(1));
        MALLOC_STATE.with(|s| {
            let mut s = s.borrow_mut();
            if !s.set.contains(&(header as usize)) {
                s.record_copied_minor_validation_lookup(None);
                return None;
            }
            let obj_type =
                unsafe { plausible_gc_header(header, false).then_some((*header).obj_type) };
            s.record_copied_minor_validation_lookup(obj_type);
            obj_type.map(|_| CopyingPointer {
                header,
                kind: CopyingPointerKind::Malloc,
            })
        })
    }

    #[inline]
    pub(super) fn raw_pointer_candidate(bits: u64) -> bool {
        (0x1000..=POINTER_MASK).contains(&bits) && bits & 0x7 == 0
    }

    #[inline]
    pub(super) fn decode_bits(&self, bits: u64) -> Option<(usize, bool, u64)> {
        let tag = bits & TAG_MASK;
        if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
            let addr = (bits & POINTER_MASK) as usize;
            return (addr != 0).then_some((addr, true, tag));
        }
        if tag >= 0x7FF8_0000_0000_0000 {
            return None;
        }
        if !Self::raw_pointer_candidate(bits) {
            return None;
        }
        let addr = bits as usize;
        self.classify(addr).map(|_| (addr, false, 0))
    }

    #[inline]
    pub(super) fn decode_bits_for_preflight(
        &self,
        bits: u64,
    ) -> Result<Option<(usize, CopyingPointer)>, CopiedMinorFallbackReason> {
        let tag = bits & TAG_MASK;
        if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
            let addr = (bits & POINTER_MASK) as usize;
            if addr == 0 {
                return Ok(None);
            }
            return self
                .classify_for_preflight(addr, true)
                .map(|ptr| ptr.map(|ptr| (addr, ptr)));
        }
        if tag >= 0x7FF8_0000_0000_0000 || !Self::raw_pointer_candidate(bits) {
            return Ok(None);
        }
        let addr = bits as usize;
        self.classify_for_preflight(addr, true)
            .map(|ptr| ptr.map(|ptr| (addr, ptr)))
    }

    #[inline]
    pub(super) fn malloc_validation_lookups(&self) -> usize {
        self.malloc_validation_lookups.get()
    }

    #[inline]
    pub(super) fn malloc_registry_rebuilds(&self) -> u64 {
        MALLOC_REGISTRY_REBUILD_COUNT.with(|c| {
            c.get()
                .saturating_sub(self.malloc_registry_rebuild_count_start)
        })
    }
}

pub(super) unsafe fn plausible_gc_header(header: *mut GcHeader, arena: bool) -> bool {
    if header.is_null() {
        return false;
    }
    let obj_type = (*header).obj_type;
    if gc_type_info(obj_type).is_none() {
        return false;
    }
    let size = (*header).size as usize;
    if size < GC_HEADER_SIZE || size as u64 > (1u64 << 34) {
        return false;
    }
    let is_arena = (*header).gc_flags & GC_FLAG_ARENA != 0;
    is_arena == arena
}

pub(super) struct CopyingNurseryPreflight {
    pub(super) ptrs: *const CopyingPointerSet,
    pub(super) fallback_reason: Option<CopiedMinorFallbackReason>,
    pub(super) pinned_reason: CopiedMinorFallbackReason,
    pub(super) worklist: Vec<*mut GcHeader>,
    pub(super) seen: crate::fast_hash::PtrHashSet<usize>,
}

impl CopyingNurseryPreflight {
    pub(super) fn new(ptrs: &CopyingPointerSet, pinned_reason: CopiedMinorFallbackReason) -> Self {
        Self {
            ptrs,
            fallback_reason: None,
            pinned_reason,
            worklist: Vec::new(),
            seen: crate::fast_hash::new_ptr_hash_set(),
        }
    }

    pub(super) fn ptrs(&self) -> &CopyingPointerSet {
        unsafe { &*self.ptrs }
    }

    pub(super) fn check_bits(&mut self, bits: u64) {
        self.check_bits_with_reason(bits, self.pinned_reason);
    }

    pub(super) fn check_bits_with_reason(
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

    pub(super) fn check_addr(&mut self, addr: usize) {
        self.check_addr_with_reason(addr, self.pinned_reason);
    }

    pub(super) fn check_addr_with_reason(
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

    pub(super) fn check_ptr_with_reason(
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

    pub(super) unsafe fn drain(&mut self) {
        let mut i = 0usize;
        while i < self.worklist.len() && self.fallback_reason.is_none() {
            let header = self.worklist[i];
            i += 1;
            if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
                continue;
            }
            self.scan_object_fields(header);
        }
    }

    pub(super) unsafe fn scan_object_fields(&mut self, header: *mut GcHeader) {
        visit_gc_rewrite_slots(header, |slot| unsafe {
            // Weak-only reachability imposes no copy constraint: the
            // collector never evacuates through a weak edge (a weak-only
            // young target dies in place and tombstones), so a pinned
            // target behind one must not force the fallback path.
            if crate::weakref::is_weak_target_trace_slot(header, slot.slot) {
                return;
            }
            slot.record_layout_read();
            self.scan_slot(slot.slot as *const u64);
        });
    }

    pub(super) unsafe fn scan_slot(&mut self, slot: *const u64) {
        if slot.is_null() {
            return;
        }
        self.check_bits_with_reason(*slot, CopiedMinorFallbackReason::PinnedYoungTransitive);
    }
}

#[derive(Default)]
pub(super) struct StickyRememberedSet {
    pub(super) old_pages: crate::fast_hash::PtrHashSet<usize>,
    pub(super) external_pages: Vec<(usize, usize)>,
}

impl StickyRememberedSet {
    pub(super) fn remember_slot(
        &mut self,
        parent_header: *mut GcHeader,
        slot: *mut u64,
        external: bool,
    ) {
        if parent_header.is_null() || slot.is_null() {
            return;
        }
        let page = crate::arena::generation_page_for_addr(slot as usize);
        if external {
            self.external_pages.push((parent_header as usize, page));
        } else {
            self.old_pages.insert(page);
        }
    }

    pub(super) fn restore(&self) {
        for &page in &self.old_pages {
            mark_dirty_old_page(page);
        }
        for &(header, page) in &self.external_pages {
            mark_dirty_external_slot_page(header, page);
        }
    }

    pub(super) fn extend(&mut self, other: StickyRememberedSet) {
        self.old_pages.extend(other.old_pages);
        self.external_pages.extend(other.external_pages);
    }
}

pub(super) struct CopyingNurseryCollector {
    pub(super) ptrs: CopyingPointerSet,
    pub(super) worklist: Vec<*mut GcHeader>,
    pub(super) marked_headers: Vec<*mut GcHeader>,
    pub(super) moved_headers: Vec<*mut GcHeader>,
    pub(super) large_excluded_headers: crate::fast_hash::PtrHashSet<usize>,
    pub(super) sticky: StickyRememberedSet,
    pub(super) stats: CopyingNurseryTraceStats,
    pub(super) live_from_bytes: usize,
    /// Weak target slots (WeakRef referent / WeakMap-WeakSet entry key /
    /// FinalizationRegistry record target) seen during the copy scan. The
    /// scan must NOT evacuate through them (that would strengthen the weak
    /// edge), but a target moved via some strong edge AFTER the slot was
    /// scanned still needs its address repaired — `repair_weak_slots` runs
    /// them once more after the final drain. Slots are stable: they live in
    /// to-space copies or non-moving objects, which don't move again within
    /// the cycle.
    pub(super) weak_slots: Vec<*mut u64>,
}

impl CopyingNurseryCollector {
    pub(super) fn new(ptrs: CopyingPointerSet) -> Self {
        Self {
            ptrs,
            worklist: Vec::new(),
            marked_headers: Vec::new(),
            moved_headers: Vec::new(),
            large_excluded_headers: crate::fast_hash::new_ptr_hash_set(),
            sticky: StickyRememberedSet::default(),
            stats: CopyingNurseryTraceStats {
                eligible: true,
                fallback_reason: CopiedMinorFallbackReason::None,
                ..CopyingNurseryTraceStats::default()
            },
            live_from_bytes: 0,
            weak_slots: Vec::new(),
        }
    }

    pub(super) unsafe fn record_large_excluded(&mut self, header: *mut GcHeader) {
        if header.is_null() {
            return;
        }
        let total = (*header).size as usize;
        if !is_large_object_total_size(total) {
            return;
        }
        if self.large_excluded_headers.insert(header as usize) {
            self.stats.large_excluded_objects = self.stats.large_excluded_objects.saturating_add(1);
            self.stats.large_excluded_bytes = self.stats.large_excluded_bytes.saturating_add(total);
        }
    }

    pub(super) fn visit_value_bits(&mut self, bits: u64) -> Option<u64> {
        let (addr, is_nanbox, tag) = self.ptrs.decode_bits(bits)?;
        let new_addr = self.mark_addr(addr)?;
        if new_addr == addr {
            return None;
        }
        Some(if is_nanbox {
            tag | (new_addr as u64 & POINTER_MASK)
        } else {
            new_addr as u64
        })
    }

    pub(super) fn visit_raw_addr(&mut self, addr: usize) -> Option<usize> {
        let new_addr = self.mark_addr(addr)?;
        (new_addr != addr).then_some(new_addr)
    }

    pub(super) fn rewrite_value_bits(&self, bits: u64) -> Option<u64> {
        let (addr, is_nanbox, tag) = self.ptrs.decode_bits(bits)?;
        let new_addr = self.rewrite_raw_addr(addr)?;
        Some(if is_nanbox {
            tag | (new_addr as u64 & POINTER_MASK)
        } else {
            new_addr as u64
        })
    }

    pub(super) fn rewrite_raw_addr(&self, addr: usize) -> Option<usize> {
        let ptr = self.ptrs.classify(addr)?;
        unsafe {
            if (*ptr.header).gc_flags & GC_FLAG_FORWARDED == 0 {
                return None;
            }
            Some(forwarding_address(ptr.header) as usize)
        }
    }

    pub(super) fn mark_addr(&mut self, addr: usize) -> Option<usize> {
        let ptr = self.ptrs.classify(addr)?;
        match ptr.kind {
            CopyingPointerKind::Eden | CopyingPointerKind::FromSurvivor => {
                Some(unsafe { self.move_young(ptr) })
            }
            CopyingPointerKind::ToSurvivor => Some(addr),
            CopyingPointerKind::Longlived | CopyingPointerKind::Malloc => {
                unsafe {
                    let flags = (*ptr.header).gc_flags;
                    if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
                        (*ptr.header).gc_flags = flags | GC_FLAG_MARKED;
                        self.worklist.push(ptr.header);
                        self.marked_headers.push(ptr.header);
                    }
                }
                Some(addr)
            }
            CopyingPointerKind::Old => {
                unsafe {
                    self.record_large_excluded(ptr.header);
                }
                Some(addr)
            }
        }
    }

    pub(super) unsafe fn move_young(&mut self, ptr: CopyingPointer) -> usize {
        let header = ptr.header;
        let old_user = (header as *mut u8).add(GC_HEADER_SIZE);
        let flags = (*header).gc_flags;
        if flags & GC_FLAG_FORWARDED != 0 {
            let forwarded = forwarding_address(header) as usize;
            // Array growth also uses GC_FLAG_FORWARDED to leave a stable
            // forwarding stub at the pre-grow address. A root may still point
            // at that stub when copied-minor starts; following it is not
            // enough because the current array can still be in from-space and
            // must itself be marked, moved, and scanned.
            return self.mark_addr(forwarded).unwrap_or(forwarded);
        }

        let total = (*header).size as usize;
        let payload = total - GC_HEADER_SIZE;
        let prior_age = copied_survival_age((*header)._reserved, flags);
        let next_age = prior_age.saturating_add(1);
        let promote = flags & GC_FLAG_TENURED != 0 || next_age >= GC_COPY_PROMOTION_SURVIVALS;
        let new_user = if promote {
            crate::arena::arena_alloc_gc_old(payload, 8, (*header).obj_type)
        } else {
            crate::arena::arena_alloc_gc_survivor(payload, 8, (*header).obj_type)
        };
        std::ptr::copy_nonoverlapping(old_user, new_user, payload);

        let new_header = header_from_user_ptr(new_user);
        (*new_header)._reserved = reserved_with_copied_survival_age(
            (*header)._reserved,
            if promote {
                GC_COPY_PROMOTION_SURVIVALS
            } else {
                next_age
            },
        );
        layout_transfer(old_user, new_user);
        let preserved = flags & (GC_FLAG_SHAPE_SHARED | GC_FLAG_INTERNED | GC_FLAG_PINNED);
        (*new_header).gc_flags = GC_FLAG_ARENA
            | GC_FLAG_MARKED
            | preserved
            | if promote {
                GC_FLAG_TENURED
            } else {
                GC_FLAG_HAS_SURVIVED
            };
        if promote {
            crate::arena::old_page_account_promoted_object(
                new_header as usize,
                total,
                preserved & GC_FLAG_PINNED != 0,
            );
        }

        set_forwarding_address(header, new_user);
        (*header).gc_flags &= !GC_FLAG_MARKED;
        gc_type_after_payload_move((*header).obj_type, old_user as usize, new_user as usize);

        self.worklist.push(new_header);
        self.moved_headers.push(new_header);
        self.live_from_bytes += total;
        if promote {
            self.stats.promoted_objects += 1;
            self.stats.promoted_bytes += total;
        } else {
            self.stats.copied_objects += 1;
            self.stats.copied_bytes += total;
        }
        new_user as usize
    }

    pub(super) unsafe fn visit_slot_with_parent(
        &mut self,
        slot: *mut u64,
        parent_header: *mut GcHeader,
        external: bool,
    ) {
        if slot.is_null() {
            return;
        }
        // Weak target edge (WeakRef referent / weak entry key / finreg
        // record target): never evacuate through it — the mark/barrier
        // paths skip these (`is_weak_target_trace_slot`), and copying
        // through them strengthened the reference, so WeakMap entries
        // never tombstoned and FinalizationRegistry never fired while
        // copied-minor was the operative cycle. Repair an already-moved
        // target's address now and queue the slot so `repair_weak_slots`
        // fixes targets evacuated after this visit; the after-mark pass
        // (`process_weak_targets_after_mark`) then tombstones dead ones.
        // No remembered-set entry either — the write barrier skips weak
        // slots the same way.
        if !parent_header.is_null()
            && crate::weakref::is_weak_target_trace_slot(parent_header, slot)
        {
            if let Some(new_bits) = self.rewrite_value_bits(*slot) {
                *slot = new_bits;
            }
            self.weak_slots.push(slot);
            return;
        }
        let bits = *slot;
        if let Some(new_bits) = self.visit_value_bits(bits) {
            *slot = new_bits;
        }
        if !parent_header.is_null() {
            let parent_user = (parent_header as *mut u8).add(GC_HEADER_SIZE) as usize;
            if barrier_parent_needs_remembering(parent_user, external) {
                if let Some((child_addr, _, _)) = self.ptrs.decode_bits(*slot) {
                    if crate::arena::pointer_in_nursery(child_addr) {
                        self.sticky.remember_slot(parent_header, slot, external);
                    }
                }
            }
        }
    }

    pub(super) unsafe fn drain(&mut self) {
        let mut i = 0usize;
        while i < self.worklist.len() {
            let header = self.worklist[i];
            i += 1;
            if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
                continue;
            }
            self.scan_object_fields(header);
        }
    }

    /// Second pass over the weak target slots collected during the scan:
    /// a weak target evacuated via a strong edge AFTER its slot was
    /// visited still points at the from-space original — rewrite it to
    /// the forwarding address so `process_weak_targets_after_mark` (and
    /// the mutator) read the live copy. Targets never forwarded are
    /// either old-gen/pinned live (no rewrite needed) or dead (left for
    /// the after-mark tombstone pass).
    pub(super) unsafe fn repair_weak_slots(&mut self) {
        let slots = std::mem::take(&mut self.weak_slots);
        for slot in slots {
            if let Some(new_bits) = self.rewrite_value_bits(*slot) {
                *slot = new_bits;
            }
        }
    }

    pub(super) unsafe fn scan_object_fields(&mut self, header: *mut GcHeader) {
        let mut changed = false;
        visit_gc_rewrite_slots(header, |slot| unsafe {
            slot.record_layout_read();
            let before = *slot.slot;
            self.visit_slot_with_parent(slot.slot, header, slot.external);
            changed |= *slot.slot != before;
        });
        if changed && gc_type_rewrite_hook_kind((*header).obj_type) == GcRewriteHookKind::SetIndex {
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            crate::set::rebuild_set_index_for_gc(user_ptr as *mut crate::set::SetHeader);
        }
    }

    pub(super) unsafe fn clear_marks(&mut self) {
        for &header in &self.marked_headers {
            (*header).gc_flags &= !GC_FLAG_MARKED;
        }
        for &header in &self.moved_headers {
            (*header).gc_flags &= !GC_FLAG_MARKED;
        }
    }
}

pub(super) fn scan_remembered_dirty_slots_copying(
    snapshot: &RememberedDirtySnapshot,
    mut visit: impl FnMut(*mut u64, *mut GcHeader, bool, &mut RememberedSetTraceStats),
) -> RememberedSetTraceStats {
    let mut stats = RememberedSetTraceStats {
        entries_scanned: snapshot.dirty_old_pages.len()
            + snapshot.external_dirty_entries.len()
            + snapshot.fallback_headers.len(),
        dirty_pages_before: snapshot.dirty_pages.len(),
        dirty_pages_scanned: snapshot.dirty_pages.len(),
        ..RememberedSetTraceStats::default()
    };
    let mut seen_headers = crate::fast_hash::new_ptr_hash_set();

    let mut scan_header = |header: *mut GcHeader, stats: &mut RememberedSetTraceStats| unsafe {
        if header.is_null() || !seen_headers.insert(header as usize) {
            return;
        }
        let arena_parent = plausible_gc_header(header, true);
        let malloc_parent = !arena_parent && plausible_gc_header(header, false);
        if !arena_parent && !malloc_parent {
            return;
        }
        let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        if arena_parent
            && !matches!(
                crate::arena::classify_heap_generation(user),
                crate::arena::HeapGeneration::Old
            )
        {
            return;
        }
        stats.old_objects_considered += 1;
        stats.valid_roots += 1;
        stats.dirty_objects_scanned += 1;
        let mut changed = false;
        let mut visit_slot = |slot: *mut u64, stats: &mut RememberedSetTraceStats| {
            let external = !matches!(
                crate::arena::classify_heap_generation(slot as usize),
                crate::arena::HeapGeneration::Old
            );
            let before = *slot;
            visit(slot, header, external, stats);
            changed |= *slot != before;
        };
        scan_dirty_object_slots(header, &snapshot.dirty_pages, stats, &mut visit_slot);
        if changed && gc_type_rewrite_hook_kind((*header).obj_type) == GcRewriteHookKind::SetIndex {
            crate::set::rebuild_set_index_for_gc(user as *mut crate::set::SetHeader);
        }
    };

    if !snapshot.dirty_old_pages.is_empty() {
        crate::arena::old_arena_walk_objects_on_pages(&snapshot.dirty_old_pages, |header| {
            scan_header(header as *mut GcHeader, &mut stats);
        });
    }
    for &(_, header_addr) in &snapshot.external_dirty_entries {
        scan_header(header_addr as *mut GcHeader, &mut stats);
    }
    for header_addr in snapshot.fallback_headers.iter().copied() {
        scan_header(header_addr as *mut GcHeader, &mut stats);
    }

    stats.dirty_pages_after = remembered_dirty_page_count();
    stats
}

pub(super) struct CopiedMinorEligibility {
    pub(super) eligible: bool,
    pub(super) fallback_reason: CopiedMinorFallbackReason,
    pub(super) malloc_sweep_due: bool,
    pub(super) malloc_validation_lookups: usize,
    pub(super) malloc_registry_rebuilds: u64,
    pub(super) legacy_root_stats: LegacyRootTraceStats,
    pub(super) ptrs: Option<CopyingPointerSet>,
}

impl CopiedMinorEligibility {
    pub(super) fn evaluate(trigger_kind: GcTriggerKind) -> Self {
        Self::evaluate_with_stack_decision(trigger_kind, conservative_stack_scan_decision())
    }

    pub(super) fn evaluate_with_stack_decision(
        trigger_kind: GcTriggerKind,
        stack_decision: ConservativeStackScanDecision,
    ) -> Self {
        let malloc_sweep_due = copied_minor_malloc_sweep_due(trigger_kind);
        if !old_to_young_tracking_complete() {
            return Self::fallback(
                CopiedMinorFallbackReason::BarriersInactive,
                malloc_sweep_due,
            );
        }
        if matches!(stack_decision, ConservativeStackScanDecision::Scan) {
            return Self::fallback(
                CopiedMinorFallbackReason::ConservativeStack,
                malloc_sweep_due,
            );
        }
        let ptrs = CopyingPointerSet::new();
        let (copy_only_reason, legacy_root_stats) = Self::copy_only_root_preflight_reason(&ptrs);
        if let Some(reason) = copy_only_reason {
            return Self::fallback_with_ptrs_and_legacy(
                reason,
                malloc_sweep_due,
                ptrs,
                legacy_root_stats,
            );
        }
        if let Some(reason) = Self::mutable_root_preflight_reason(&ptrs) {
            return Self::fallback_with_ptrs_and_legacy(
                reason,
                malloc_sweep_due,
                ptrs,
                legacy_root_stats,
            );
        }
        if let Some(reason) = Self::dirty_slot_preflight_reason(&ptrs) {
            return Self::fallback_with_ptrs_and_legacy(
                reason,
                malloc_sweep_due,
                ptrs,
                legacy_root_stats,
            );
        }

        Self {
            eligible: true,
            fallback_reason: CopiedMinorFallbackReason::None,
            malloc_sweep_due,
            malloc_validation_lookups: ptrs.malloc_validation_lookups(),
            malloc_registry_rebuilds: ptrs.malloc_registry_rebuilds(),
            legacy_root_stats,
            ptrs: Some(ptrs),
        }
    }

    pub(super) fn fallback(reason: CopiedMinorFallbackReason, malloc_sweep_due: bool) -> Self {
        Self {
            eligible: false,
            fallback_reason: reason,
            malloc_sweep_due,
            malloc_validation_lookups: 0,
            malloc_registry_rebuilds: 0,
            legacy_root_stats: LegacyRootTraceStats::default(),
            ptrs: None,
        }
    }

    pub(super) fn fallback_with_ptrs_and_legacy(
        reason: CopiedMinorFallbackReason,
        malloc_sweep_due: bool,
        ptrs: CopyingPointerSet,
        legacy_root_stats: LegacyRootTraceStats,
    ) -> Self {
        Self {
            eligible: false,
            fallback_reason: reason,
            malloc_sweep_due,
            malloc_validation_lookups: ptrs.malloc_validation_lookups(),
            malloc_registry_rebuilds: ptrs.malloc_registry_rebuilds(),
            legacy_root_stats,
            ptrs: Some(ptrs),
        }
    }

    pub(super) fn trace_stats(&self) -> CopyingNurseryTraceStats {
        CopyingNurseryTraceStats {
            eligible: self.eligible,
            fallback_reason: self.fallback_reason,
            malloc_sweep_due: self.malloc_sweep_due,
            malloc_validation_lookups: self.malloc_validation_lookups,
            malloc_registry_rebuilds: self.malloc_registry_rebuilds,
            ..CopyingNurseryTraceStats::default()
        }
    }

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
            checker.check_bits(slot.read());
        });
        let scanners: Vec<MutableRootScannerEntry> =
            MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
        {
            let mut visitor = RuntimeRootVisitor::for_copying_check(&mut checker);
            for entry in scanners {
                (entry.scanner)(&mut visitor);
            }
            visit_ffi_mutable_registered_roots(&mut visitor);
        }
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
        scan_remembered_dirty_slots_copying(&snapshot, |slot, _header, _external, _stats| unsafe {
            dirty_checker.check_bits(*slot);
        });
        unsafe {
            dirty_checker.drain();
        }
        dirty_checker.fallback_reason
    }
}

pub(super) fn gc_collect_minor_copying_fast_path(
    trace: &mut Option<GcCycleTrace>,
    start: Instant,
    trigger_kind: GcTriggerKind,
) -> Option<CopiedMinorFastPathOutcome> {
    let eligibility = CopiedMinorEligibility::evaluate(trigger_kind);
    gc_collect_minor_copying_fast_path_with_eligibility(trace, start, eligibility, trigger_kind)
}

pub(super) fn gc_collect_minor_copying_fast_path_with_eligibility(
    trace: &mut Option<GcCycleTrace>,
    start: Instant,
    eligibility: CopiedMinorEligibility,
    trigger_kind: GcTriggerKind,
) -> Option<CopiedMinorFastPathOutcome> {
    if let Some(trace) = trace.as_mut() {
        trace.copying_nursery = eligibility.trace_stats();
        trace.legacy_copy_only_scanner_pinned = eligibility.legacy_root_stats;
        let decision = conservative_stack_scan_decision();
        trace.root_sources.native_stack_fallback.decision = decision;
        trace.root_sources.native_stack_fallback.scanned =
            matches!(decision, ConservativeStackScanDecision::Scan);
    }
    if !eligibility.eligible {
        return None;
    }
    let malloc_sweep_due = eligibility.malloc_sweep_due;
    let ptrs = eligibility
        .ptrs
        .expect("eligible copied-minor decision must carry pointer classifier");

    let phase_start = trace_phase_start(trace);
    let from_space_bytes = crate::arena::copying_from_space_in_use_bytes();
    let mut collector = CopyingNurseryCollector::new(ptrs);
    collector.stats.eligible = true;
    collector.stats.fallback_reason = CopiedMinorFallbackReason::None;
    collector.stats.malloc_sweep_due = malloc_sweep_due;
    collector.stats.reset_blocks += crate::arena::copying_prepare_to_space();

    visit_mutable_root_slots(|slot| unsafe {
        let bits = slot.read();
        if let Some(trace) = trace.as_mut() {
            let pointer_root = collector.ptrs.decode_bits(bits).is_some();
            root_source_for_mutable_slot(&mut trace.root_sources, slot.kind)
                .record_scan(bits != 0, pointer_root);
            if matches!(slot.kind, MutableRootSlotKind::ShadowStack) {
                trace.shadow_roots.record_scan(bits);
            }
        }
        if bits == 0 {
            return;
        }
        if let Some(new_bits) = collector.visit_value_bits(bits) {
            slot.write(new_bits);
            if let Some(trace) = trace.as_mut() {
                root_source_for_mutable_slot(&mut trace.root_sources, slot.kind).record_rewrite();
                if matches!(slot.kind, MutableRootSlotKind::ShadowStack) {
                    trace.shadow_roots.record_rewrite();
                }
            }
        }
    });

    let scanners: Vec<MutableRootScannerEntry> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    {
        let mut root_sources = trace.as_mut().map(|trace| &mut trace.root_sources);
        if let Some(sources) = &mut root_sources {
            sources.runtime_handles.record_registered_scanners(
                scanners
                    .iter()
                    .filter(|entry| entry.source == MutableRootScannerSource::RuntimeHandles)
                    .count(),
            );
            sources.runtime_mutable_scanners.record_registered_scanners(
                scanners
                    .iter()
                    .filter(|entry| entry.source == MutableRootScannerSource::RuntimeMutableScanner)
                    .count(),
            );
        }
        let mut visitor = RuntimeRootVisitor::for_copying_mark(&mut collector);
        for entry in scanners {
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
            (entry.scanner)(&mut visitor);
            visitor.set_root_source_stats(previous);
        }
        visit_ffi_mutable_registered_roots_with_sources(&mut visitor, root_sources);
    }

    let snapshot = remembered_dirty_snapshot();
    let remembered_stats =
        scan_remembered_dirty_slots_copying(&snapshot, |slot, header, external, stats| unsafe {
            let before = *slot;
            collector.visit_slot_with_parent(slot, header, external);
            if *slot != before {
                stats.newly_marked += 1;
            }
        });
    if let Some(trace) = trace.as_mut() {
        trace.remembered_set = remembered_stats;
    }
    let promoted_sticky = rebuild_evacuated_old_to_young_remembered_set(&collector.moved_headers);
    promoted_sticky.restore();
    collector.sticky.extend(promoted_sticky);
    if gc_verify_evacuation_enabled() {
        let phase_start = trace_phase_start(trace);
        let old_young_edge_verifier = verify_old_to_young_edges_covered();
        trace_phase_record(trace, "old_young_edge_verify", phase_start);
        if let Some(trace) = trace.as_mut() {
            trace.old_young_edge_verifier = old_young_edge_verifier;
        }
    }

    unsafe {
        collector.drain();
    }
    {
        let scanners: Vec<MutableRootScannerEntry> =
            MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
        let mut root_sources = trace.as_mut().map(|trace| &mut trace.root_sources);
        let mut visitor = RuntimeRootVisitor::for_copying_rewrite(&collector);
        for entry in scanners {
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
            (entry.scanner)(&mut visitor);
            visitor.set_root_source_stats(previous);
        }
        visit_ffi_mutable_registered_roots_with_sources(&mut visitor, root_sources);
    }
    trace_phase_record(trace, "copying_nursery", phase_start);

    // Weak semantics for the copied-minor fast path. This path bypasses
    // cycle.rs's `WeakProcessing` subphase entirely, so before this block
    // existed NOTHING here tombstoned dead weak targets — and the scan
    // used to evacuate THROUGH weak slots, so the targets never died in
    // the first place: WeakMap entries never tombstoned and
    // FinalizationRegistry never fired while copied-minor was the
    // operative cycle (unbounded retention in long-running servers).
    // Now the scan records weak slots without evacuating; here we repair
    // any whose target was moved via a strong edge after the slot was
    // visited, then run the shared after-mark tombstone pass. Must run
    // BEFORE `copying_reset_from_spaces_and_flip` below: liveness is
    // MARKED|PINNED on pre-flip headers (to-space copies carry MARKED
    // until `clear_marks`). Gated on the weak-holder latch so programs
    // that never allocate a weak container skip the full arena walk.
    unsafe {
        collector.repair_weak_slots();
    }
    if crate::weakref::weak_target_holders_allocated() {
        let phase_start = trace_phase_start(trace);
        let valid_ptrs = build_valid_pointer_set();
        crate::weakref::process_weak_targets_after_mark(
            &valid_ptrs,
            /* minor_only = */ true,
            matches!(trigger_kind, GcTriggerKind::Manual),
        );
        trace_phase_record(trace, "weak_processing", phase_start);
    }

    if gc_verify_evacuation_enabled() {
        let phase_start = trace_phase_start(trace);
        let valid_ptrs = build_valid_pointer_set();
        verify_evacuated_no_stale_forwarded_refs(&valid_ptrs);
        trace_phase_record(trace, "evacuation_verify", phase_start);
    }

    crate::promise::cleanup_copied_minor_promise_contexts_for_gc();
    finalize_dead_copied_minor_from_space_side_allocations();
    let reset = crate::arena::copying_reset_from_spaces_and_flip();
    collector.stats.reset_blocks += reset.reset_blocks;
    if let Some(trace) = trace.as_mut() {
        trace.old_pages = crate::arena::old_page_summary();
    }
    remembered_set_clear();
    collector.sticky.restore();
    restore_surviving_dirty_coverage(&snapshot);
    let malloc_freed_bytes = if malloc_sweep_due {
        let phase_start = trace_phase_start(trace);
        let freed = sweep_malloc_objects();
        trace_phase_record(trace, "malloc_sweep", phase_start);
        freed
    } else {
        0
    };
    unsafe {
        collector.clear_marks();
    }

    CONS_PINNED.with(|s| s.borrow_mut().clear());
    let nursery_freed_bytes = from_space_bytes.saturating_sub(collector.live_from_bytes) as u64;
    let freed_bytes = nursery_freed_bytes.saturating_add(malloc_freed_bytes);
    collector.stats.malloc_validation_lookups = collector.ptrs.malloc_validation_lookups();
    collector.stats.malloc_registry_rebuilds = collector.ptrs.malloc_registry_rebuilds();
    if let Some(trace) = trace.as_mut() {
        trace.copying_nursery = collector.stats;
        trace.sweep = SweepTraceStats {
            dead_bytes: freed_bytes,
            freed_bytes,
            reusable_bytes: reset.reusable_bytes,
            returned_bytes: reset.deallocated_bytes,
            reset_blocks: reset.reset_blocks,
            deallocated_blocks: reset.deallocated_blocks,
            deallocated_bytes: reset.deallocated_bytes,
            retained_forwarded_stub_objects: 0,
            retained_forwarded_stub_bytes: 0,
        };
        trace.pause_us = start.elapsed().as_micros() as u64;
        trace.capture_layout_scans();
    }
    maybe_schedule_old_reclaim_after_copied_minor();
    Some(CopiedMinorFastPathOutcome {
        freed_bytes,
        malloc_swept: malloc_sweep_due,
    })
}

fn finalize_dead_copied_minor_from_space_side_allocations() {
    crate::map::finalize_dead_copied_minor_from_space_maps();
    crate::set::finalize_dead_copied_minor_from_space_sets();
}
