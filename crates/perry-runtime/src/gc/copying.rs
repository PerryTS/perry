use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CopyingPointerKind {
    Eden,
    FromSurvivor,
    ToSurvivor,
    Longlived,
    Old,
    Malloc,
    /// On a block this cycle is promoting whole, in place (#7742). Generation
    /// is already `Old` — so every barrier predicate reads old-gen semantics —
    /// but the object was young when the cycle began and therefore still owes
    /// the collector exactly one field scan.
    PromotedYoung,
}

#[derive(Clone, Copy)]
pub(crate) struct CopyingPointer {
    pub(crate) header: *mut GcHeader,
    pub(super) kind: CopyingPointerKind,
}

pub(crate) struct CopyingPointerSet {
    pub(super) malloc_registry_available: Cell<bool>,
    pub(super) malloc_registry_empty_at_start: bool,
    pub(super) malloc_validation_lookups: Cell<usize>,
    pub(super) malloc_registry_rebuild_count_start: u64,
}

impl CopyingPointerSet {
    pub(super) fn new() -> Self {
        let (malloc_registry_available, malloc_registry_empty_at_start) = MALLOC_STATE.with(|s| {
            let mut s = s.borrow_mut();
            // Moving-nursery mode (`PERRY_GC_MOVING_LOOP_POLLS`): eagerly build the
            // malloc registry so this copying minor can CLASSIFY malloc-tracked
            // objects and evacuate, instead of hitting
            // `MallocRegistryUnavailable` and falling back to a non-moving minor
            // (which reclaims ~nothing on reallocation-heavy async/Map/generator
            // code — measured: broad3 192 MiB / 100 fallbacks). The
            // O(malloc-objects) rebuild is paid back by the RSS win. Default
            // (non-moving) copied minors keep the lazy behavior — see
            // `ensure_set_built`'s "keep copied-minor from rebuilding" note.
            if super::gc_moving_loop_polls_enabled() && !s.objects.is_empty() {
                super::malloc::ensure_set_built(&mut s);
            }
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
    pub(crate) fn classify(&self, addr: usize) -> Option<CopyingPointer> {
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
        // ONE range lookup answers both classifications this needs. The header
        // sits `GC_HEADER_SIZE` below the user pointer and a block always
        // begins with a header, so a real object's header is on the same
        // registered range as its user pointer; `range_base` is the guard that
        // keeps a garbage candidate sitting at the very start of a range from
        // becoming a read of the unmapped page below it. Before #7742 this was
        // two `classify_heap_space` calls for addresses 8 bytes apart, on
        // EVERY visited slot.
        let Some((space, range_base)) = crate::arena::classify_heap_space_in_range(addr) else {
            return None;
        };
        let header_addr = addr - GC_HEADER_SIZE;
        if header_addr < range_base {
            return None;
        }
        debug_assert_eq!(
            crate::arena::classify_heap_space(header_addr),
            space,
            "an object's header and user pointer must classify identically"
        );
        if !matches!(
            space,
            crate::arena::HeapSpace::NurseryEden
                | crate::arena::HeapSpace::Survivor0
                | crate::arena::HeapSpace::Survivor1
                | crate::arena::HeapSpace::Longlived
                | crate::arena::HeapSpace::Old
                | crate::arena::HeapSpace::PromotedYoung
        ) {
            return None;
        }
        let header = header_addr as *mut GcHeader;
        if unsafe { !plausible_gc_header(header, true) } {
            return None;
        }
        // The two survivor-space readings are TLS loads, and Darwin has no
        // local-exec TLS — each is a real `_tlv_get_addr` call. Reading them
        // eagerly cost two per classified pointer on workloads that never touch
        // a survivor at all (`retain.ts` classifies Eden / PromotedYoung / Old
        // and nothing else). They can only ever answer `Survivor0`, `Survivor1`
        // or `Unknown`, and `space` is already narrowed to the six accepted
        // spaces, so hoisting the non-survivor arms above them changes no
        // verdict — it just stops paying for an answer the arm does not use.
        let kind = match space {
            crate::arena::HeapSpace::NurseryEden => CopyingPointerKind::Eden,
            crate::arena::HeapSpace::PromotedYoung => CopyingPointerKind::PromotedYoung,
            crate::arena::HeapSpace::Longlived => CopyingPointerKind::Longlived,
            crate::arena::HeapSpace::Old => CopyingPointerKind::Old,
            s if s == crate::arena::active_survivor_space() => CopyingPointerKind::FromSurvivor,
            s if s == crate::arena::inactive_survivor_space() => CopyingPointerKind::ToSurvivor,
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
            // #7538: an owner's external buffer can contribute thousands of
            // slots (a lazy JSON array's sparse element cache is one 8-byte
            // slot per element), and they are visited in address order — so
            // one adjacent-duplicate check collapses a whole page's worth of
            // pushes into a single entry. `restore` dedupes again inside
            // `mark_dirty_external_slot_page`; this keeps the intermediate
            // Vec from growing with the element count.
            let entry = (parent_header as usize, page);
            if self.external_pages.last() != Some(&entry) {
                self.external_pages.push(entry);
            }
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
    /// Per-cycle snapshot of the adaptive tenuring threshold (gc/tenuring.rs)
    /// so every object in one cycle sees the same policy. Deliberately the
    /// ONLY per-cycle promotion input: an earlier mid-cycle overflow valve
    /// ("stop copying once N bytes are in to-space") made the
    /// copied/promoted split depend on root traversal order, which is
    /// address-dependent — the gc-ratchet's bit-identical-counters contract
    /// caught it as a ±2-object jitter on the first heavy cycle.
    pub(super) tenuring_survivals: u8,
    /// #7742: every remembered-set insertion this cycle could make is provably
    /// impossible, so the passes that make them are skipped.
    ///
    /// A remembered-set entry is only ever created when
    /// `remembered_child_needs_tracking(child)` says yes, and that is yes for
    /// exactly two child populations: nursery-generation objects, and
    /// malloc-registry objects. On a whole-block promoting cycle the first
    /// population is EMPTY by construction — `retag_young_for_in_place_promotion`
    /// takes every in-use Eden and survivor block, so after the retag nothing in
    /// the heap classifies as `Nursery`. When the malloc registry was also empty
    /// at cycle start the second population is empty too, and no mutator runs
    /// mid-cycle to create one.
    ///
    /// So this is a proof, not a heuristic: three whole passes over the
    /// surviving cohort's slots (`visit_slot_with_parent`'s re-decode +
    /// remember, `rebuild_evacuated_old_to_young_remembered_set`, and
    /// `restore_surviving_dirty_coverage`) can only insert nothing, and are
    /// skipped. `debug_assert_no_remembering_possible` re-derives the premise at
    /// runtime in debug builds.
    pub(super) skip_remembering: bool,
    /// Weak target slots (WeakRef referent / WeakMap-WeakSet entry key /
    /// FinalizationRegistry record target) seen during the copy scan. The
    /// scan must NOT evacuate through them (that would strengthen the weak
    /// edge), but a target moved via some strong edge AFTER the slot was
    /// scanned still needs its address repaired — `repair_weak_slots` runs
    /// them once more after the final drain. Slots are stable: they live in
    /// to-space copies or non-moving objects, which don't move again within
    /// the cycle.
    pub(super) weak_slots: Vec<*mut u64>,
    /// One-entry memo for [`CopyingNurseryCollector::mark_addr`]: the last
    /// address it classified successfully, and the address it returned.
    ///
    /// `mark_addr` is idempotent with a stable result for the whole cycle —
    /// a second call finds `GC_FLAG_MARKED` (or `GC_FLAG_FORWARDED`) already
    /// set and returns the same address — so replaying the answer is exact,
    /// not approximate. What it buys: an object's SHAPE-SHARED children are
    /// the same addresses for every instance, so the mark drain classifies
    /// one `keys_array` pointer once per surviving object. On
    /// `gc-handoff/bench/retain.ts` that is ~750 k classifications of a
    /// single address per cycle, each a page-map lookup plus a
    /// `plausible_gc_header` read.
    ///
    /// `0` is the empty state: `classify_arena` rejects every address below
    /// `GC_HEADER_SIZE`, so it can never be a memoized key.
    memo_addr: usize,
    memo_result: usize,
}

/// Survivor count of the previous copying minor, used only to pre-size this
/// one's `worklist` / `moved_headers`.
///
/// Both grow to one entry per survivor — 750 k on a fully-live nursery — from
/// `Vec::new()`, so each cycle paid ~20 reallocations whose `memmove` and
/// `mi_malloc` were visible in a symbolicated profile of the MARK loop. A
/// nursery's survivor count is strongly autocorrelated between adjacent cycles
/// (it is the same program in the same phase), so the previous count is a good
/// estimate; over-estimating costs only untouched reserved bytes, and
/// under-estimating just falls back to the ordinary growth.
static PREVIOUS_SURVIVOR_ESTIMATE: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Cap the pre-size so a one-off huge cycle cannot make every later cycle
/// reserve 100 MB of pointers.
const SURVIVOR_ESTIMATE_CAP: usize = 1 << 21;

pub(super) fn note_survivor_count_for_presizing(count: usize) {
    PREVIOUS_SURVIVOR_ESTIMATE.store(
        count.min(SURVIVOR_ESTIMATE_CAP),
        std::sync::atomic::Ordering::Relaxed,
    );
}

impl CopyingNurseryCollector {
    pub(super) fn new(ptrs: CopyingPointerSet) -> Self {
        let tenuring_survivals = tenuring_survivals();
        let estimate = PREVIOUS_SURVIVOR_ESTIMATE.load(std::sync::atomic::Ordering::Relaxed);
        Self {
            ptrs,
            worklist: Vec::with_capacity(estimate),
            marked_headers: Vec::new(),
            moved_headers: Vec::with_capacity(estimate),
            large_excluded_headers: crate::fast_hash::new_ptr_hash_set(),
            sticky: StickyRememberedSet::default(),
            stats: CopyingNurseryTraceStats {
                eligible: true,
                fallback_reason: CopiedMinorFallbackReason::None,
                tenuring_survivals,
                ..CopyingNurseryTraceStats::default()
            },
            live_from_bytes: 0,
            tenuring_survivals,
            skip_remembering: false,
            weak_slots: Vec::new(),
            memo_addr: 0,
            memo_result: 0,
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

    /// Follow the forwarding chain for a raw metadata key/address the SAME
    /// way the evacuation verifier does (`verify::try_rewrite_raw_addr`), so
    /// the post-copy rewrite pass and the verifier never DISAGREE about a
    /// moved address (#scavenge-cause).
    ///
    /// The old body classified `addr` via `self.ptrs.classify()` and bailed to
    /// `None` whenever that returned `None`. But the verifier follows the
    /// forwarding pointer gated only by its live census, so any from-space key
    /// the classifier rejected stayed *un-rekeyed* in a runtime mutable
    /// metadata table (e.g. `shapes.entries`, keyed by keys-array heap address)
    /// — and the verifier then aborted on that still-stale forwarded key
    /// (`slot=0x0 ... in runtime mutable root scanner`). Because
    /// `rewrite_raw_addr` is the single shared path for every metadata scanner
    /// (shapes, map/set, symbol, proxy, weakref, descriptor/class registries,
    /// …), the disagreement is fixed for all of them at once.
    ///
    /// Gate on a heap-region check instead of `classify`: `GC_FLAG_FORWARDED`
    /// is set ONLY by `set_forwarding_address`, and during this rewrite pass
    /// the from-space is still intact and page-registered
    /// (`copying_reset_from_spaces_and_flip` runs strictly later — after both
    /// this rewrite pass and the verify pass), so any address in a known heap
    /// region carrying that flag IS genuinely forwarded. Mirrors
    /// `try_rewrite_raw_addr`'s 64-hop cap and `next == 0 || next == current`
    /// stops, returning `rewrote.then_some(current)` (Some only when the
    /// address actually moved).
    pub(super) fn rewrite_raw_addr(&self, addr: usize) -> Option<usize> {
        if addr < GC_HEADER_SIZE {
            return None;
        }
        let mut current = addr;
        let mut rewrote = false;
        for _ in 0..64 {
            if current < GC_HEADER_SIZE {
                return rewrote.then_some(current);
            }
            let header_addr = current - GC_HEADER_SIZE;
            if matches!(
                crate::arena::classify_heap_space(header_addr),
                crate::arena::HeapSpace::Unknown
            ) {
                return rewrote.then_some(current);
            }
            let header = header_addr as *mut GcHeader;
            unsafe {
                if (*header).gc_flags & GC_FLAG_FORWARDED == 0 {
                    return rewrote.then_some(current);
                }
                let next = forwarding_address(header) as usize;
                if next == 0 || next == current {
                    return rewrote.then_some(current);
                }
                current = next;
                rewrote = true;
            }
        }
        rewrote.then_some(current)
    }

    pub(super) fn mark_addr(&mut self, addr: usize) -> Option<usize> {
        // See `memo_addr`: replaying the previous answer is exact. Only
        // successful classifications are memoized — a `None` must stay a
        // `None`, and re-deriving it costs one page-map probe.
        if addr == self.memo_addr {
            return Some(self.memo_result);
        }
        let ptr = self.ptrs.classify(addr)?;
        let result = match ptr.kind {
            CopyingPointerKind::Eden | CopyingPointerKind::FromSurvivor => unsafe {
                self.move_young(ptr)
            },
            CopyingPointerKind::ToSurvivor => addr,
            CopyingPointerKind::Longlived | CopyingPointerKind::Malloc => {
                unsafe {
                    let flags = (*ptr.header).gc_flags;
                    if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
                        (*ptr.header).gc_flags = flags | GC_FLAG_MARKED;
                        self.worklist.push(ptr.header);
                        self.marked_headers.push(ptr.header);
                    }
                }
                addr
            }
            CopyingPointerKind::Old => {
                unsafe {
                    self.record_large_excluded(ptr.header);
                }
                addr
            }
            CopyingPointerKind::PromotedYoung => unsafe { self.mark_promoted_young(ptr) },
        };
        self.memo_addr = addr;
        self.memo_result = result;
        Some(result)
    }

    /// #7742: the object's block is being promoted whole, in place. It does not
    /// move, so this is a pure mark — the address it is already at is its final
    /// address, and every slot in the heap that points at it is already
    /// correct.
    ///
    /// It still goes on the worklist, and on `moved_headers`: it was young when
    /// the cycle began, so it owes exactly one field scan (to evacuate any
    /// child that is NOT on a promoted block, and to record the old→young and
    /// old→malloc remembered-set edges its new generation implies), and the
    /// mark has to be cleared at the end like any other.
    pub(super) unsafe fn mark_promoted_young(&mut self, ptr: CopyingPointer) -> usize {
        let header = ptr.header;
        let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        let flags = (*header).gc_flags;
        if flags & GC_FLAG_FORWARDED != 0 {
            // Array growth leaves a forwarding stub at the pre-grow address;
            // follow it exactly as `move_young` does.
            let forwarded = forwarding_address(header) as usize;
            return self.mark_addr(forwarded).unwrap_or(forwarded);
        }
        if flags & GC_FLAG_MARKED == 0 {
            (*header).gc_flags = flags | GC_FLAG_MARKED;
            let total = (*header).size as usize;
            self.worklist.push(header);
            self.moved_headers.push(header);
            self.stats.promoted_objects += 1;
            self.stats.promoted_bytes += total;
            self.stats.in_place_promoted_objects += 1;
            self.live_from_bytes += total;
            // Survivor-influx accounting: an in-place promotion consumes the
            // whole young generation at once, so the split the adaptive
            // tenuring loop reads has no meaning here. Everything is credited
            // as Eden influx, which is what keeps `tenuring_survivals` pinned
            // low for the workloads this path fires on.
            self.stats.eden_live_bytes += total;
        }
        user
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

        // #7645: on a cycle that skipped the eligibility preflight, this is the
        // exact instant an incomplete young-pin latch turns into a
        // use-after-move: the collector is about to relocate a pinned object
        // whose holder (the cross-thread promise queue, an AppKit string
        // return) keeps a raw address no scanner will rewrite. `flags` is
        // already loaded, so the check is one `and` and a never-taken branch.
        // It is deliberately NOT applied when the preflight ran: that path is
        // unchanged from before this issue, and a divergence between the
        // preflight's traversal and the copier's is a separate bug that should
        // not newly abort a program.
        if self.stats.preflight_skipped && flags & GC_FLAG_PINNED != 0 {
            pinned_young_move_under_skipped_preflight(header);
        }

        let total = (*header).size as usize;
        // Safety net (partial mitigation, NOT a full fix): a genuine
        // young/survivor object is always small — large objects are allocated
        // old-gen/malloc, never in the copying nursery — so a "young" object
        // whose size is out of range is a corrupt/mis-classified header (e.g. an
        // off-heap pointer whose preceding bytes coincidentally pass
        // `plausible_gc_header`). Refuse to memmove through such a garbage size:
        // that turns the worst outcome (a wild out-of-bounds copy → SIGSEGV)
        // into a no-op, and surfaces it under PERRY_GC_DIAG. It does NOT catch a
        // plausible-but-wrong *small* size; the root fix is stronger arena
        // classification / page unregistration so off-heap addresses never
        // reach here. See the copying-minor relocation issue.
        const MAX_YOUNG_MOVE_BYTES: usize = 1 << 20; // 1 MiB, >> any real young object
        if total < GC_HEADER_SIZE || total > MAX_YOUNG_MOVE_BYTES {
            if std::env::var_os("PERRY_GC_DIAG").is_some() {
                eprintln!(
                    "[gc-move-guard] refusing wild young move user={:#x} obj_type={} size={}",
                    old_user as usize,
                    (*header).obj_type,
                    total
                );
            }
            return old_user as usize;
        }
        let payload = total - GC_HEADER_SIZE;
        let prior_age = copied_survival_age((*header)._reserved, flags);
        let next_age = prior_age.saturating_add(1);
        // Adaptive tenuring (gc/tenuring.rs): the survivals threshold is
        // re-derived from survivor influx after every cycle. The decision is
        // purely per-object (flags + age) so the copied/promoted split stays
        // deterministic regardless of root traversal order.
        let promote = flags & GC_FLAG_TENURED != 0 || next_age >= self.tenuring_survivals;
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
        // Survivor-influx accounting for the adaptive tenuring feedback loop:
        // live bytes moved out of Eden this cycle, split from re-copies of
        // survivor-space residents. Threshold-invariant (live Eden bytes get
        // moved somewhere at any threshold), which is what makes the loop's
        // fixed point stable.
        match ptr.kind {
            CopyingPointerKind::Eden => self.stats.eden_live_bytes += total,
            _ => self.stats.survivor_live_bytes += total,
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
        if !parent_header.is_null() && !self.skip_remembering {
            let parent_user = (parent_header as *mut u8).add(GC_HEADER_SIZE) as usize;
            if barrier_parent_needs_remembering(parent_user, external) {
                if let Some((child_addr, _, _)) = self.ptrs.decode_bits(*slot) {
                    // Keep old→malloc pages dirty alongside old→nursery:
                    // the malloc child is spared by this cycle's mark
                    // (mark_addr handles CopyingPointerKind::Malloc) but
                    // the NEXT minor's malloc sweep needs the edge again.
                    if crate::gc::barrier::remembered_child_needs_tracking(child_addr) {
                        self.sticky.remember_slot(parent_header, slot, external);
                    }
                }
            }
        }
    }

    pub(super) unsafe fn drain(&mut self) {
        let mut i = 0usize;
        while i < self.worklist.len() {
            // The worklist is a list of COLD headers: on a promotion-heavy
            // cycle the marking pass that filled it has since walked tens of
            // MB, so every `(*header).gc_flags` read below is a DRAM round
            // trip. The addresses are known `PREFETCH_DISTANCE` iterations
            // ahead, so overlap the round trips instead of serialising them.
            if let Some(&ahead) = self.worklist.get(i + super::prefetch::PREFETCH_DISTANCE) {
                super::prefetch::prefetch_read(ahead as usize);
            }
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
        if changed {
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            run_gc_rewrite_hook((*header).obj_type, user_ptr as usize);
        }
    }

    pub(super) unsafe fn clear_marks(&mut self) {
        // Same cold-header problem as `drain`, and the same fix: this is a
        // read-modify-write of one byte per survivor, in mark order, over a
        // cohort far larger than any cache.
        clear_marks_in(&self.marked_headers);
        clear_marks_in(&self.moved_headers);
    }
}

/// Clear `GC_FLAG_MARKED` across a header list, prefetching ahead.
unsafe fn clear_marks_in(headers: &[*mut GcHeader]) {
    for (i, &header) in headers.iter().enumerate() {
        if let Some(&ahead) = headers.get(i + super::prefetch::PREFETCH_DISTANCE) {
            super::prefetch::prefetch_read(ahead as usize);
        }
        (*header).gc_flags &= !GC_FLAG_MARKED;
    }
}

/// Is a stress or verification instrument armed that an untraced promotion
/// would silently stop exercising?
///
/// Each of these instruments takes the trace itself as its subject:
/// `PERRY_GC_VERIFY_EVACUATION` checks the old→young edge coverage the scan
/// records and the rewrite the drain performs; `PERRY_GC_FROMSPACE_SCAN` walks
/// for stale from-space pointers the trace should have rewritten;
/// `PERRY_GC_VERIFY_MARK` reads the marks. A cycle that produces no marks and
/// rewrites nothing would let all three report success having examined
/// nothing — the exact failure mode CLAUDE.md's "a gate must assert its
/// subject was live" rule names. `PERRY_GC_FORCE_EVACUATE` (and every mode
/// that implies it) is not listed because it already vetoes in-place promotion
/// outright, which is a precondition here.
fn untraced_promotion_instrument_veto() -> bool {
    gc_verify_evacuation_enabled()
        || super::fromspace_scan::fromspace_scan_enabled()
        || std::env::var_os("PERRY_GC_VERIFY_MARK").is_some()
        || super::barrier::incremental_mark_in_progress()
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
        if changed {
            run_gc_rewrite_hook((*header).obj_type, user);
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

/// The young-pin latch was clear, the preflight was skipped on that proof, and
/// the copier then met a pinned young object anyway — so the latch is
/// incomplete and a pin site exists that does not go through `gc::pin_object`.
///
/// There is no recovery: leaving the object in from-space strands the
/// referring slot on memory `copying_reset_from_spaces_and_flip` is about to
/// retire, and moving it invalidates a raw address nothing will rewrite. Abort
/// loudly at the faulting site instead of corrupting the heap silently.
#[cold]
#[inline(never)]
unsafe fn pinned_young_move_under_skipped_preflight(header: *mut GcHeader) -> ! {
    eprintln!(
        "[gc-pin-latch] FATAL: copying minor is about to relocate a PINNED young \
         object on a preflight-skipped cycle. header={:#x} obj_type={} size={} \
         flags={:#04x}\n\
         The young-pin latch (gc/pin.rs) is incomplete: some site sets \
         GC_FLAG_PINNED without going through gc::pin_object. Find it with \
         `python3 scripts/gc_pin_sites.py` and route it through pin_object (#7645).",
        header as usize,
        (*header).obj_type,
        (*header).size,
        (*header).gc_flags,
    );
    std::process::abort()
}

pub(super) struct CopiedMinorEligibility {
    pub(super) eligible: bool,
    pub(super) fallback_reason: CopiedMinorFallbackReason,
    pub(super) malloc_sweep_due: bool,
    pub(super) malloc_validation_lookups: usize,
    pub(super) malloc_registry_rebuilds: u64,
    pub(super) legacy_root_stats: LegacyRootTraceStats,
    /// #7645: both eligibility preflight walks were provably no-ops and were
    /// skipped. Carried into the collector so `move_young` can abort rather
    /// than relocate a pinned object on a cycle that took the unproven path.
    pub(super) preflight_skipped: bool,
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
        // #7645: both walks below are a transitive traversal of the whole live
        // young graph that answers two booleans and produces no collection
        // result. When both booleans are already decided the traversal is
        // provably a no-op, so skip it — see `preflight_walks_decided`.
        let preflight_skipped = Self::preflight_walks_decided(&ptrs);
        if preflight_skipped {
            // The ONE side effect the skipped walks carried, kept at its
            // original point in the cycle. `dirty_slot_preflight_reason` took
            // a `remembered_dirty_snapshot()`, whose first call on a thread
            // arms the barrier and rebuilds the remembered set from the heap
            // — a walk that assumes "nothing is marked yet". Letting it fall
            // through to the copy phase's snapshot would run it AFTER
            // `visit_mutable_root_slots` had already evacuated root-reachable
            // young objects, i.e. against a half-moved heap. It is a one-shot
            // per thread (`REMEMBERED_SET_RECONSTRUCTED`), so on every later
            // cycle this is a thread-local flag read.
            arm_and_reconstruct_remembered_set_if_unarmed();
            note_preflight_skipped();
        } else {
            note_preflight_walked();
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
        }

        Self {
            eligible: true,
            fallback_reason: CopiedMinorFallbackReason::None,
            malloc_sweep_due,
            malloc_validation_lookups: ptrs.malloc_validation_lookups(),
            malloc_registry_rebuilds: ptrs.malloc_registry_rebuilds(),
            legacy_root_stats,
            preflight_skipped,
            ptrs: Some(ptrs),
        }
    }

    /// Are both of the preflight walks' outputs already known?
    ///
    /// The walks can only produce three verdicts, and each has an O(1) proof
    /// of absence:
    ///
    /// * `PinnedYoungRoot` / `PinnedYoungDirtySlot` / `PinnedYoungTransitive`
    ///   come from `CopyingNurseryPreflight::check_ptr_with_reason`, which
    ///   trips only on an `Eden`/`FromSurvivor` object carrying
    ///   `GC_FLAG_PINNED`. `gc::pin` records every creation of such a pin in a
    ///   monotone latch, so a clear latch means no such object exists — which
    ///   is strictly stronger than "none is reachable".
    /// * `MallocRegistryUnavailable` comes from
    ///   `CopyingPointerSet::classify_for_preflight`, which returns it only
    ///   when a non-arena candidate is met while the malloc registry is both
    ///   unavailable *and* was non-empty at cycle start. If the registry is
    ///   available, or was empty at start, no candidate can produce it.
    ///
    /// When either proof is unavailable the walk runs exactly as before, so
    /// the decision this function guards is never *changed* — only skipped
    /// when its outcome is already determined.
    fn preflight_walks_decided(ptrs: &CopyingPointerSet) -> bool {
        if young_pin_latch_armed() {
            return false;
        }
        ptrs.malloc_registry_available.get() || ptrs.malloc_registry_empty_at_start
    }

    pub(super) fn fallback(reason: CopiedMinorFallbackReason, malloc_sweep_due: bool) -> Self {
        Self {
            eligible: false,
            fallback_reason: reason,
            malloc_sweep_due,
            malloc_validation_lookups: 0,
            malloc_registry_rebuilds: 0,
            legacy_root_stats: LegacyRootTraceStats::default(),
            preflight_skipped: false,
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
            preflight_skipped: false,
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
            preflight_skipped: self.preflight_skipped,
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

/// Re-derive `skip_remembering`'s premise from the heap itself, in debug
/// builds: no in-use young block survived the retag, and the malloc registry is
/// empty. Either being false would make three skipped passes non-empty and turn
/// a dropped remembered-set entry into a swept-live-object crash a cycle later,
/// so it is worth re-deriving rather than trusting the argument.
fn debug_assert_no_remembering_possible() {
    #[cfg(debug_assertions)]
    {
        let young_in_use = crate::arena::young_in_use_bytes_after_retag();
        debug_assert_eq!(
            young_in_use, 0,
            "in-place promotion left {young_in_use} bytes of young generation in use; \
             `skip_remembering` would drop real old->young remembered-set entries"
        );
        let malloc_objects = MALLOC_STATE.with(|s| s.borrow().objects.len());
        debug_assert_eq!(
            malloc_objects, 0,
            "malloc registry is non-empty; `skip_remembering` would drop old->malloc edges"
        );
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
    _trigger_kind: GcTriggerKind,
) -> Option<CopiedMinorFastPathOutcome> {
    if let Some(trace) = trace.as_mut() {
        trace.copying_nursery = eligibility.trace_stats();
        trace.legacy_copy_only_scanner_pinned = eligibility.legacy_root_stats;
        let decision = conservative_stack_scan_decision();
        trace.root_sources.native_stack_fallback.decision = decision;
        trace.root_sources.native_stack_fallback.scanned =
            matches!(decision, ConservativeStackScanDecision::Scan);
    }
    if std::env::var_os("PERRY_GC_DIAG").is_some() {
        let reason = match eligibility.fallback_reason {
            CopiedMinorFallbackReason::None => "none",
            CopiedMinorFallbackReason::NotAttempted => "not_attempted",
            CopiedMinorFallbackReason::BarriersInactive => "barriers_inactive",
            CopiedMinorFallbackReason::ConservativeStack => "conservative_stack",
            CopiedMinorFallbackReason::CopyOnlyRoots => "copy_only_roots",
            CopiedMinorFallbackReason::MallocRegistryUnavailable => "malloc_registry_unavailable",
            CopiedMinorFallbackReason::PinnedYoungRoot => "pinned_young_root",
            CopiedMinorFallbackReason::PinnedYoungDirtySlot => "pinned_young_dirty_slot",
            CopiedMinorFallbackReason::PinnedYoungTransitive => "pinned_young_transitive",
        };
        eprintln!(
            "[gc-copy-minor] eligible={} fallback={} preflight_skipped={} (skips={} walks={})",
            eligibility.eligible,
            reason,
            eligibility.preflight_skipped,
            super::copied_minor_preflight_skips(),
            super::copied_minor_preflight_walks(),
        );
    }
    if !eligibility.eligible {
        return None;
    }
    let preflight_skipped = eligibility.preflight_skipped;
    let malloc_sweep_due = eligibility.malloc_sweep_due;
    let ptrs = eligibility
        .ptrs
        .expect("eligible copied-minor decision must carry pointer classifier");

    let phase_start = trace_phase_start(trace);
    let from_space_bytes = crate::arena::copying_from_space_in_use_bytes();
    // #7742: decide BEFORE anything classifies, then retag the young blocks so
    // every classification for the rest of this cycle already reads the
    // generation those objects will have when it ends. The eligibility
    // preflight above ran against the pre-retag labels, which is correct — it
    // answers "may this cycle move objects at all", a question the retag does
    // not change.
    let promotion = if super::should_promote_young_in_place() {
        crate::arena::retag_young_for_in_place_promotion()
    } else {
        crate::arena::InPlacePromotion::default()
    };
    // An empty plan (nothing in use to promote) falls back to the ordinary
    // path, so the from-space reset still runs.
    let promoting_in_place = !promotion.is_empty();
    let mut collector = CopyingNurseryCollector::new(ptrs);
    collector.stats.eligible = true;
    collector.stats.fallback_reason = CopiedMinorFallbackReason::None;
    collector.stats.malloc_sweep_due = malloc_sweep_due;
    collector.stats.preflight_skipped = preflight_skipped;
    collector.stats.in_place_promotion = promoting_in_place;
    collector.stats.in_place_promoted_blocks = promotion.block_count();
    // See `CopyingNurseryCollector::skip_remembering` for the proof.
    collector.skip_remembering =
        promoting_in_place && collector.ptrs.malloc_registry_empty_at_start;
    if collector.skip_remembering {
        debug_assert_no_remembering_possible();
    }
    collector.stats.remembering_skipped = collector.skip_remembering;
    // #7888: the cycle promotes the WHOLE young generation in place, so the
    // trace has no products left worth its cost — skip it too.
    //
    // What the trace does on a promoting cycle, exhaustively, and why each is
    // covered:
    //
    // * **Nothing moves.** `retag_young_for_in_place_promotion` takes every
    //   in-use Eden and survivor block, so after it no address in the heap
    //   classifies as `Nursery` and `move_young` is unreachable. Every root
    //   walk, slot rewrite and forwarding repair is therefore a provable no-op,
    //   not an approximation.
    // * **No remembered-set entry can be created** — that is `skip_remembering`'s
    //   existing proof, which this reuses verbatim (it is a precondition here).
    //   With no young generation left, `remembered_set_clear()` is exact.
    // * **The address-keyed death-pruning passes prune nothing anyway.**
    //   `dead_owner::owner_is_dead` and the map/set/error finalizers all require
    //   the owner to classify as `Nursery` on a minor; after the retag none do.
    //   They still run below, and still find nothing, at their usual O(registered
    //   holders) cost.
    // * **Weak semantics need marks**, so a cycle with any weak-target holder
    //   registered is excluded outright.
    // * **The malloc sweep and `Longlived` marking need marks**, so this reuses
    //   `malloc_registry_empty_at_start`.
    // * **The stress/verify instruments need a trace to instrument**, so any of
    //   them being armed excludes this path — an instrument that silently stops
    //   exercising its subject is the failure mode CLAUDE.md's "a gate must
    //   assert its subject was live" rule is about.
    //
    // That leaves liveness for the old-gen page index (answered by
    // `PromotionLiveness::AssumeAllLive`) and the survival ratio itself, which
    // is what `should_promote_young_untraced`'s budget bounds.
    let untraced = promoting_in_place
        && collector.skip_remembering
        && !crate::weakref::weak_target_holders_allocated()
        && !untraced_promotion_instrument_veto()
        && super::should_promote_young_untraced();
    collector.stats.reset_blocks += crate::arena::copying_prepare_to_space();

    let native_stack_walk = if untraced {
        Default::default()
    } else {
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
                    root_source_for_mutable_slot(&mut trace.root_sources, slot.kind)
                        .record_rewrite();
                    if matches!(slot.kind, MutableRootSlotKind::ShadowStack) {
                        trace.shadow_roots.record_rewrite();
                    }
                }
            }
        })
    };
    let mut root_sources = trace.as_mut().map(|trace| &mut trace.root_sources);
    record_native_stack_walk_source(native_stack_walk, &mut root_sources);

    let scanners: Vec<MutableRootScannerEntry> = if untraced {
        Vec::new()
    } else {
        MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone())
    };
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

    // On an untraced promotion the dirty SCAN is where the whole per-object
    // mark pass lived: `retain`'s array store has a young child in every page
    // of its backing store, so the scan walks all three million slots and marks
    // the record behind each one. With nothing to mark and nothing to rewrite,
    // it has no product left — `remembered_set_clear()` below is exact once the
    // young generation is empty.
    //
    // The SNAPSHOT is still taken: it is O(dirty pages), and it is the sole
    // read path for the remembered set, which is where #7187's lazy barrier
    // arming happens. Skipping it would leave the barrier unarmed for the next
    // cycle — a missing-edge bug one collection later.
    let snapshot = remembered_dirty_snapshot();
    if !untraced {
        let remembered_stats = scan_remembered_dirty_slots_copying(
            &snapshot,
            |slot, header, external, stats| unsafe {
                let before = *slot;
                collector.visit_slot_with_parent(slot, header, external);
                if *slot != before {
                    stats.newly_marked += 1;
                }
            },
        );
        if let Some(trace) = trace.as_mut() {
            trace.remembered_set = remembered_stats;
        }
    }
    if !collector.skip_remembering {
        let promoted_sticky =
            rebuild_evacuated_old_to_young_remembered_set(&collector.moved_headers);
        promoted_sticky.restore();
        collector.sticky.extend(promoted_sticky);
    }
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
        let scanners: Vec<MutableRootScannerEntry> = if untraced {
            Vec::new()
        } else {
            MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone())
        };
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
    // visited, then run the registry-scoped tombstone pass. Must run
    // BEFORE `copying_reset_from_spaces_and_flip` below: liveness is
    // MARKED|PINNED on pre-flip headers (to-space copies carry MARKED
    // until `clear_marks`), and dead holders' from-space headers are still
    // intact/classifiable before the flip. Gated on the weak-holder latch
    // (now "registry non-empty") so programs that never allocate — or that
    // once did but whose holders have all died — skip the pass entirely.
    //
    // 2026-07-09 GC audit (#6182): this used to build a full-heap
    // `build_valid_pointer_set()` BTreeSet AND `arena_walk_objects` over
    // EVERY live object to find the 3 weak-holder class_ids — two O(all
    // objects) passes forfeited forever once any WeakMap/WeakRef/FinReg was
    // allocated. `process_weak_targets_from_registry` instead walks only the
    // registered holders and classifies targets with the O(1) page-metadata
    // classifier the copy already built (`collector.ptrs`) — no BTreeSet, no
    // arena walk. The full-cycle path (cycle.rs `WeakProcessing`) is
    // untouched and still uses the valid-pointer set it built for its trace.
    unsafe {
        collector.repair_weak_slots();
    }
    if crate::weakref::weak_target_holders_allocated() {
        let phase_start = trace_phase_start(trace);
        // Enqueue FinalizationRegistry cleanup jobs on every trigger kind —
        // see the matching WeakProcessing comment in cycle.rs (2026-07-09 GC
        // audit: delivery was gated on the Manual trigger).
        crate::weakref::process_weak_targets_from_registry(
            &collector.ptrs,
            /* enqueue_callbacks = */ true,
        );
        trace_phase_record(trace, "weak_processing", phase_start);
    }

    if gc_verify_evacuation_enabled() {
        let phase_start = trace_phase_start(trace);
        let valid_ptrs = build_valid_pointer_set();
        verify_evacuated_no_stale_forwarded_refs(&valid_ptrs);
        trace_phase_record(trace, "evacuation_verify", phase_start);
    }

    // Diagnostic (PERRY_GC_VERIFY_MARK): before from-space reset frees the dead
    // young objects, check that no MARKED (survived) object references an
    // UNMARKED (about-to-be-freed) child — i.e. a live parent whose child is
    // being swept. Non-fatal; logs parent/child obj_types.
    if std::env::var_os("PERRY_GC_VERIFY_MARK").is_some() {
        super::verify::verify_marked_heap_report_nonfatal("copying-minor");
    }

    // #7035: whole-heap from-space scan. MUST run here — after the rewrite
    // pass, before from-space is reset — and it is deliberately independent of
    // the root enumeration the rewrite pass and the evacuation verifier share.
    super::fromspace_scan::run_fromspace_scan(&snapshot);

    crate::promise::cleanup_copied_minor_promise_contexts_for_gc();
    finalize_dead_copied_minor_from_space_side_allocations();
    // #7742: on a promoting cycle the young blocks are handed to old-gen
    // instead of being reset. This MUST stay before `clear_marks` — the finish
    // walk reads `GC_FLAG_MARKED` to decide which objects to index — and it
    // takes the place of, never runs alongside, the from-space reset: the
    // blocks the reset would recycle are the blocks this keeps.
    let (reset, promotion_stats) = if promoting_in_place {
        let phase_start = trace_phase_start(trace);
        super::note_promoted_young_capacity(promotion.reserved_bytes());
        let promotion_stats = crate::arena::finish_in_place_promotion(
            promotion,
            if untraced {
                crate::arena::PromotionLiveness::AssumeAllLive
            } else {
                crate::arena::PromotionLiveness::Marked
            },
        );
        trace_phase_record(trace, "in_place_promotion", phase_start);
        (
            crate::arena::ArenaResetStats {
                reset_blocks: 0,
                reusable_bytes: 0,
                deallocated_blocks: 0,
                deallocated_bytes: 0,
            },
            promotion_stats,
        )
    } else {
        (
            crate::arena::copying_reset_from_spaces_and_flip(),
            crate::arena::InPlacePromotionStats::default(),
        )
    };
    collector.stats.reset_blocks += reset.reset_blocks;
    if untraced {
        // The finish walk is the only census an untraced cycle has, and it is
        // an exact one for everything except liveness: it parsed every object
        // on every promoted block. Promotion counters come from it so the trace
        // and the `[gc-copy-minor]` line stay comparable across both paths —
        // ns-per-promoted-object is the acceptance measurement, and a path that
        // reported zero promotions would read as infinitely fast.
        collector.stats.promoted_objects = promotion_stats.objects;
        collector.stats.in_place_promoted_objects = promotion_stats.objects;
        collector.stats.promoted_bytes = promotion_stats.bytes;
        collector.stats.eden_live_bytes = promotion_stats.bytes;
        collector.live_from_bytes = promotion_stats.bytes;
    }
    collector.stats.in_place_dead_bytes = promotion_stats
        .bytes
        .saturating_sub(promotion_stats.live_bytes);
    collector.stats.in_place_sparse_blocks = promotion_stats.sparse_blocks;
    if let Some(trace) = trace.as_mut() {
        trace.old_pages = crate::arena::old_page_summary();
    }
    remembered_set_clear();
    collector.sticky.restore();
    if !collector.skip_remembering {
        restore_surviving_dirty_coverage(&snapshot);
    }
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
    // #7742: feed the policy its measurement. This runs on every copying minor
    // that TRACED — promoting ones included, which is why a promoting cycle
    // still traces once its untraced budget is spent — so the ratio the next
    // decision reads is never stale.
    //
    // #7888: an untraced cycle measured nothing. Recording its own assumption
    // as a measurement would make the predictor a mirror — permanently 1000‰,
    // permanently armed, and unable to notice the workload changing. It charges
    // the untraced budget instead, and the cycle that spends that budget is the
    // one that measures.
    if untraced {
        super::note_untraced_promotion(promotion_stats.bytes, promotion_stats.objects);
    } else {
        super::note_young_survival(from_space_bytes, collector.live_from_bytes);
    }
    if !untraced {
        // An untraced cycle marked nothing, so its `moved_headers` is empty and
        // says nothing about the next cycle's survivor count. Leave the last
        // real observation in place rather than resetting the estimate to zero.
        note_survivor_count_for_presizing(collector.moved_headers.len());
    }
    collector.stats.young_survival_permille =
        super::last_young_survival_permille().unwrap_or_default();
    // A promoting cycle frees NOTHING: the dead young bytes were promoted
    // along with the live ones and are reclaimable only by a full collection.
    // Reporting them as freed would tell the pacer it had made progress it had
    // not made.
    let nursery_freed_bytes = if promoting_in_place {
        super::note_in_place_promotion(
            from_space_bytes,
            collector.live_from_bytes,
            collector.stats.in_place_promoted_objects,
        );
        0
    } else {
        from_space_bytes.saturating_sub(collector.live_from_bytes) as u64
    };
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
            // The copying minor's Eden census is `stats.eden_live_bytes`, fed
            // to `retune_after_scavenge` directly; the #7598 sweep seed covers
            // the collections that run NO copying minor.
            eden_live_bytes: 0,
            eden_dead_bytes: 0,
        };
        trace.pause_us = start.elapsed().as_micros() as u64;
        trace.capture_layout_scans();
    }
    // #7592: this is the promotion the survivor-promotion handoff exists to
    // enable, so it releases the latch that suppressed a repeat handoff.
    note_copying_minor_completed();
    // #7604: the process-wide liveness counters. A copying minor ran, and this
    // is how much it actually relocated -- the only evidence that distinguishes
    // "the instrument was armed" from "the instrument fired".
    super::instruments::note_copying_minor_moved(
        collector.stats.copied_objects,
        collector.stats.promoted_objects,
    );
    // #7592: promoted bytes are live by construction — credit them to the
    // old-reclaim baseline BEFORE the pressure check below, or the check reads
    // the stale baseline and schedules a full that is guaranteed to free
    // nothing (see `credit_promoted_bytes_to_old_baseline`).
    credit_promoted_bytes_to_old_baseline(collector.stats.promoted_bytes);
    // The same argument one trigger over: a young generation that did not die
    // is a heap growing by LIVE data, so arena-growth pacing must not read that
    // growth as garbage accumulating. Fed here, after the reset, so the
    // re-baseline sees post-collection occupancy.
    note_copying_minor_young_survival(collector.stats.young_survival_permille);
    maybe_schedule_old_reclaim_after_copied_minor();
    retune_after_scavenge(
        collector.stats.eden_live_bytes,
        collector.stats.copied_bytes,
        collector.stats.survivor_live_bytes,
    );
    if std::env::var_os("PERRY_GC_DIAG").is_some() {
        eprintln!(
            "[gc-copy-minor] ran in_place={} untraced={} untraced_cycles={} untraced_objects={} in_place_blocks={} in_place_dead_bytes={} sparse_blocks={} survival_permille={} copied_objects={} copied_bytes={} promoted_objects={} promoted_bytes={} freed_bytes={} tenuring_survivals={} eden_live_bytes={} trigger={:?} declared_safepoint={}",
            collector.stats.in_place_promotion,
            untraced,
            super::untraced_promotion_cycles(),
            super::untraced_promoted_objects(),
            collector.stats.in_place_promoted_blocks,
            collector.stats.in_place_dead_bytes,
            collector.stats.in_place_sparse_blocks,
            collector.stats.young_survival_permille,
            collector.stats.copied_objects,
            collector.stats.copied_bytes,
            collector.stats.promoted_objects,
            collector.stats.promoted_bytes,
            freed_bytes,
            collector.stats.tenuring_survivals,
            collector.stats.eden_live_bytes,
            _trigger_kind,
            super::policy::GC_AT_DECLARED_SAFEPOINT.with(std::cell::Cell::get)
        );
    }
    Some(CopiedMinorFastPathOutcome {
        freed_bytes,
        malloc_swept: malloc_sweep_due,
    })
}

fn finalize_dead_copied_minor_from_space_side_allocations() {
    crate::map::finalize_dead_copied_minor_from_space_maps();
    crate::set::finalize_dead_copied_minor_from_space_sets();
    crate::node_submodules::diagnostics_gc::finalize_dead_copied_minor_from_space_errors();
    // 2026-07-09 GC audit wave 2: the from-space flip runs no per-object
    // finalize hooks, so entries keyed by dead from-space owners in the
    // object-address-keyed side tables are pruned here (headers still intact).
    super::dead_owner::prune_dead_owner_side_tables_copied_minor();
}
