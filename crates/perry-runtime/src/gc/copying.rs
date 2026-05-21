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

    pub(super) fn ensure_malloc_registry_for_copy_only_preflight(&self) {
        if self.malloc_registry_available.get() {
            return;
        }
        MALLOC_STATE.with(|s| {
            let mut s = s.borrow_mut();
            ensure_set_built(&mut s);
            if s.malloc_registry_available() {
                self.malloc_registry_available.set(true);
            }
        });
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
    if size < GC_HEADER_SIZE || size > (1usize << 34) {
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
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        match (*header).obj_type {
            GC_TYPE_ARRAY | GC_TYPE_OBJECT => self.scan_gc_child_fields(header),
            GC_TYPE_CLOSURE => self.scan_closure_fields(header, user_ptr),
            GC_TYPE_PROMISE => self.scan_promise_fields(user_ptr),
            GC_TYPE_ERROR => self.scan_error_fields(user_ptr),
            GC_TYPE_MAP => self.scan_map_fields(user_ptr),
            GC_TYPE_LAZY_ARRAY => self.scan_lazy_array_fields(user_ptr),
            GC_TYPE_STRING | GC_TYPE_BIGINT | GC_TYPE_BUFFER | GC_TYPE_TYPED_ARRAY => {}
            _ => {}
        }
    }

    pub(super) unsafe fn scan_gc_child_fields(&mut self, header: *mut GcHeader) {
        for child_slot in gc_child_slots(header) {
            if let HeapChildSlot::Child(slot, layout_kind) = child_slot {
                record_layout_child_slot_read(layout_kind);
                self.scan_slot(slot as *const u64);
            }
        }
    }

    pub(super) unsafe fn scan_closure_fields(&mut self, header: *mut GcHeader, user_ptr: *mut u8) {
        self.scan_gc_child_fields(header);
        crate::closure::visit_closure_dynamic_prop_values_mut(user_ptr as usize, |value| {
            self.check_bits_with_reason(
                value.to_bits(),
                CopiedMinorFallbackReason::PinnedYoungTransitive,
            );
        });
    }

    pub(super) unsafe fn scan_slot(&mut self, slot: *const u64) {
        if slot.is_null() {
            return;
        }
        self.check_bits_with_reason(*slot, CopiedMinorFallbackReason::PinnedYoungTransitive);
    }

    pub(super) unsafe fn scan_promise_fields(&mut self, user_ptr: *mut u8) {
        let promise = user_ptr as *const crate::promise::Promise;
        self.scan_slot(&(*promise).value as *const f64 as *const u64);
        self.scan_slot(&(*promise).reason as *const f64 as *const u64);
        self.scan_slot(&(*promise).on_fulfilled as *const _ as *const u64);
        self.scan_slot(&(*promise).on_rejected as *const _ as *const u64);
        self.scan_slot(&(*promise).next as *const _ as *const u64);
    }

    pub(super) unsafe fn scan_error_fields(&mut self, user_ptr: *mut u8) {
        let error = user_ptr as *const crate::error::ErrorHeader;
        self.scan_slot(&(*error).message as *const _ as *const u64);
        self.scan_slot(&(*error).name as *const _ as *const u64);
        self.scan_slot(&(*error).stack as *const _ as *const u64);
        self.scan_slot(&(*error).cause as *const f64 as *const u64);
        self.scan_slot(&(*error).errors as *const _ as *const u64);
    }

    pub(super) unsafe fn scan_map_fields(&mut self, user_ptr: *mut u8) {
        let map = user_ptr as *const crate::map::MapHeader;
        let size = (*map).size;
        let capacity = (*map).capacity;
        if size > capacity || size > 100_000 || (*map).entries.is_null() {
            return;
        }
        let entries = (*map).entries as *const u64;
        for i in 0..(size as usize) {
            self.scan_slot(entries.add(i * 2));
            self.scan_slot(entries.add(i * 2 + 1));
        }
    }

    pub(super) unsafe fn scan_lazy_array_fields(&mut self, user_ptr: *mut u8) {
        let lazy = user_ptr as *const crate::json_tape::LazyArrayHeader;
        if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
            return;
        }
        self.scan_slot(&(*lazy).blob_str as *const _ as *const u64);
        self.scan_slot(&(*lazy).materialized as *const _ as *const u64);
        self.scan_slot(&(*lazy).materialized_elements as *const _ as *const u64);
        self.scan_slot(&(*lazy).materialized_bitmap as *const _ as *const u64);

        let cached_length = (*lazy).cached_length as usize;
        let cache = (*lazy).materialized_elements;
        let bitmap = (*lazy).materialized_bitmap;
        if cache.is_null() || bitmap.is_null() || cached_length == 0 {
            return;
        }
        let bitmap_words = cached_length.div_ceil(64);
        for w in 0..bitmap_words {
            let word = *bitmap.add(w);
            if word == 0 {
                continue;
            }
            let base_idx = w * 64;
            for b in 0..64usize {
                if word & (1u64 << b) == 0 {
                    continue;
                }
                let i = base_idx + b;
                if i >= cached_length {
                    break;
                }
                self.scan_slot(cache.add(i) as *const u64);
            }
        }
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
        if (*header).obj_type == GC_TYPE_CLOSURE {
            crate::closure::closure_dynamic_props_owner_moved(old_user as usize, new_user as usize);
        }

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
        let bits = *slot;
        if let Some(new_bits) = self.visit_value_bits(bits) {
            *slot = new_bits;
        }
        if !parent_header.is_null() {
            let parent_user = (parent_header as *mut u8).add(GC_HEADER_SIZE) as usize;
            if matches!(
                crate::arena::classify_heap_generation(parent_user),
                crate::arena::HeapGeneration::Old
            ) {
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

    pub(super) unsafe fn scan_object_fields(&mut self, header: *mut GcHeader) {
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        match (*header).obj_type {
            GC_TYPE_ARRAY | GC_TYPE_OBJECT => self.scan_gc_child_fields(header),
            GC_TYPE_CLOSURE => self.scan_closure_fields(header, user_ptr),
            GC_TYPE_PROMISE => self.scan_promise_fields(header, user_ptr),
            GC_TYPE_ERROR => self.scan_error_fields(header, user_ptr),
            GC_TYPE_MAP => self.scan_map_fields(header, user_ptr),
            GC_TYPE_LAZY_ARRAY => self.scan_lazy_array_fields(header, user_ptr),
            GC_TYPE_STRING | GC_TYPE_BIGINT | GC_TYPE_BUFFER | GC_TYPE_TYPED_ARRAY => {}
            _ => {}
        }
    }

    pub(super) unsafe fn scan_gc_child_fields(&mut self, header: *mut GcHeader) {
        for child_slot in gc_child_slots(header) {
            if let HeapChildSlot::Child(slot, layout_kind) = child_slot {
                record_layout_child_slot_read(layout_kind);
                self.visit_slot_with_parent(slot, header, false);
            }
        }
    }

    pub(super) unsafe fn scan_closure_fields(&mut self, header: *mut GcHeader, user_ptr: *mut u8) {
        self.scan_gc_child_fields(header);
        crate::closure::visit_closure_dynamic_prop_value_slots_mut(user_ptr as usize, |slot| {
            self.visit_slot_with_parent(slot, header, true);
        });
    }

    pub(super) unsafe fn scan_promise_fields(&mut self, header: *mut GcHeader, user_ptr: *mut u8) {
        let promise = user_ptr as *mut crate::promise::Promise;
        self.visit_slot_with_parent(&(*promise).value as *const f64 as *mut u64, header, false);
        self.visit_slot_with_parent(&(*promise).reason as *const f64 as *mut u64, header, false);
        self.visit_slot_with_parent(
            &(*promise).on_fulfilled as *const _ as *mut u64,
            header,
            false,
        );
        self.visit_slot_with_parent(
            &(*promise).on_rejected as *const _ as *mut u64,
            header,
            false,
        );
        self.visit_slot_with_parent(&(*promise).next as *const _ as *mut u64, header, false);
    }

    pub(super) unsafe fn scan_error_fields(&mut self, header: *mut GcHeader, user_ptr: *mut u8) {
        let error = user_ptr as *mut crate::error::ErrorHeader;
        self.visit_slot_with_parent(&(*error).message as *const _ as *mut u64, header, false);
        self.visit_slot_with_parent(&(*error).name as *const _ as *mut u64, header, false);
        self.visit_slot_with_parent(&(*error).stack as *const _ as *mut u64, header, false);
        self.visit_slot_with_parent(&(*error).cause as *const f64 as *mut u64, header, false);
        self.visit_slot_with_parent(&(*error).errors as *const _ as *mut u64, header, false);
    }

    pub(super) unsafe fn scan_map_fields(&mut self, header: *mut GcHeader, user_ptr: *mut u8) {
        let map = user_ptr as *const crate::map::MapHeader;
        let size = (*map).size;
        let capacity = (*map).capacity;
        if size > capacity || size > 100_000 || (*map).entries.is_null() {
            return;
        }
        let entries = (*map).entries as *mut u64;
        for i in 0..(size as usize) {
            self.visit_slot_with_parent(entries.add(i * 2), header, true);
            self.visit_slot_with_parent(entries.add(i * 2 + 1), header, true);
        }
    }

    pub(super) unsafe fn scan_lazy_array_fields(
        &mut self,
        header: *mut GcHeader,
        user_ptr: *mut u8,
    ) {
        let lazy = user_ptr as *mut crate::json_tape::LazyArrayHeader;
        if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
            return;
        }
        self.visit_slot_with_parent(&(*lazy).blob_str as *const _ as *mut u64, header, false);
        self.visit_slot_with_parent(&(*lazy).materialized as *const _ as *mut u64, header, false);
        self.visit_slot_with_parent(
            &(*lazy).materialized_elements as *const _ as *mut u64,
            header,
            false,
        );
        self.visit_slot_with_parent(
            &(*lazy).materialized_bitmap as *const _ as *mut u64,
            header,
            false,
        );

        let cached_length = (*lazy).cached_length as usize;
        let cache = (*lazy).materialized_elements;
        let bitmap = (*lazy).materialized_bitmap;
        if cache.is_null() || bitmap.is_null() || cached_length == 0 {
            return;
        }
        let bitmap_words = cached_length.div_ceil(64);
        for w in 0..bitmap_words {
            let word = *bitmap.add(w);
            if word == 0 {
                continue;
            }
            let base_idx = w * 64;
            for b in 0..64usize {
                if word & (1u64 << b) == 0 {
                    continue;
                }
                let i = base_idx + b;
                if i >= cached_length {
                    break;
                }
                self.visit_slot_with_parent(cache.add(i) as *mut u64, header, false);
            }
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
        if !plausible_gc_header(header, true) {
            return;
        }
        let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        if !matches!(
            crate::arena::classify_heap_generation(user),
            crate::arena::HeapGeneration::Old
        ) {
            return;
        }
        stats.old_objects_considered += 1;
        stats.valid_roots += 1;
        stats.dirty_objects_scanned += 1;
        let mut visit_slot = |slot: *mut u64, stats: &mut RememberedSetTraceStats| {
            let external = !matches!(
                crate::arena::classify_heap_generation(slot as usize),
                crate::arena::HeapGeneration::Old
            );
            visit(slot, header, external, stats);
        };
        scan_dirty_object_slots(header, &snapshot.dirty_pages, stats, &mut visit_slot);
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

pub(super) struct CopyOnlyRootPreflight<'a> {
    pub(super) ptrs: &'a CopyingPointerSet,
    pub(super) fallback_reason: Option<CopiedMinorFallbackReason>,
    pub(super) stats: LegacyRootTraceStats,
}

impl<'a> CopyOnlyRootPreflight<'a> {
    pub(super) fn new(
        ptrs: &'a CopyingPointerSet,
        registered_rust_scanners: usize,
        registered_ffi_scanners: usize,
    ) -> Self {
        Self {
            ptrs,
            fallback_reason: None,
            stats: LegacyRootTraceStats {
                registered_rust_scanners,
                registered_ffi_scanners,
                ..LegacyRootTraceStats::default()
            },
        }
    }

    pub(super) fn check_bits(&mut self, bits: u64) {
        self.stats.emitted_roots += 1;
        let Some(addr) = self.decode_copy_only_addr(bits) else {
            return;
        };
        let Some(ptr) = self.ptrs.classify_arena(addr) else {
            self.ptrs.ensure_malloc_registry_for_copy_only_preflight();
            if self.ptrs.classify_malloc(addr).is_some() {
                self.stats.emitted_malloc_roots += 1;
                self.fallback_reason = Some(CopiedMinorFallbackReason::CopyOnlyRoots);
            } else {
                self.stats.malformed_roots += 1;
            }
            return;
        };

        match ptr.kind {
            CopyingPointerKind::Eden
            | CopyingPointerKind::FromSurvivor
            | CopyingPointerKind::ToSurvivor => {
                self.stats.emitted_young_roots += 1;
                self.fallback_reason = Some(CopiedMinorFallbackReason::CopyOnlyRoots);
            }
            CopyingPointerKind::Longlived | CopyingPointerKind::Old => {
                self.stats.emitted_old_roots += 1;
            }
            CopyingPointerKind::Malloc => unreachable!("malloc roots are classified separately"),
        }
    }

    pub(super) fn decode_copy_only_addr(&mut self, bits: u64) -> Option<usize> {
        let tag = bits & TAG_MASK;
        if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
            let addr = (bits & POINTER_MASK) as usize;
            return (addr != 0).then_some(addr);
        }
        if tag >= 0x7FF8_0000_0000_0000 {
            return None;
        }
        if !CopyingPointerSet::raw_pointer_candidate(bits) {
            return None;
        }
        Some(bits as usize)
    }
}

pub(super) extern "C" fn perry_ffi_copy_only_preflight_root(value: f64, ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    unsafe {
        (*(ctx as *mut CopyOnlyRootPreflight<'_>)).check_bits(value.to_bits());
    }
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
        let malloc_sweep_due = copied_minor_malloc_sweep_due(trigger_kind);
        if !old_to_young_tracking_complete() {
            return Self::fallback(
                CopiedMinorFallbackReason::BarriersInactive,
                malloc_sweep_due,
            );
        }
        if matches!(
            conservative_stack_scan_decision(),
            ConservativeStackScanDecision::Scan
        ) {
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
        ptrs: &CopyingPointerSet,
    ) -> (Option<CopiedMinorFallbackReason>, LegacyRootTraceStats) {
        let scanners: Vec<fn(&mut dyn FnMut(f64))> = ROOT_SCANNERS.with(|s| s.borrow().clone());
        let ffi_scanners: Vec<PerryFfiRootScanner> = FFI_ROOT_SCANNERS.with(|s| s.borrow().clone());
        let mut preflight = CopyOnlyRootPreflight::new(ptrs, scanners.len(), ffi_scanners.len());

        for scanner in scanners {
            scanner(&mut |value: f64| {
                preflight.check_bits(value.to_bits());
            });
        }

        let ctx = &mut preflight as *mut CopyOnlyRootPreflight<'_> as *mut c_void;
        for scanner in ffi_scanners {
            scanner(perry_ffi_copy_only_preflight_root, ctx);
        }

        (preflight.fallback_reason, preflight.stats)
    }

    pub(super) fn mutable_root_preflight_reason(
        ptrs: &CopyingPointerSet,
    ) -> Option<CopiedMinorFallbackReason> {
        let mut checker =
            CopyingNurseryPreflight::new(ptrs, CopiedMinorFallbackReason::PinnedYoungRoot);
        visit_mutable_root_slots(|slot| unsafe {
            checker.check_bits(slot.read());
        });
        let scanners: Vec<MutableRootScanner> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
        {
            let mut visitor = RuntimeRootVisitor::for_copying_check(&mut checker);
            for scanner in scanners {
                scanner(&mut visitor);
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
    if let Some(trace) = trace.as_mut() {
        trace.copying_nursery = eligibility.trace_stats();
        trace.legacy_copy_only_scanner_pinned = eligibility.legacy_root_stats;
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
        if matches!(slot.kind, MutableRootSlotKind::ShadowStack) {
            if let Some(trace) = trace.as_mut() {
                trace.shadow_roots.record_scan(bits);
            }
        }
        if bits == 0 {
            return;
        }
        if let Some(new_bits) = collector.visit_value_bits(bits) {
            slot.write(new_bits);
            if matches!(slot.kind, MutableRootSlotKind::ShadowStack) {
                if let Some(trace) = trace.as_mut() {
                    trace.shadow_roots.record_rewrite();
                }
            }
        }
    });

    let scanners: Vec<MutableRootScanner> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    {
        let mut visitor = RuntimeRootVisitor::for_copying_mark(&mut collector);
        for scanner in scanners {
            scanner(&mut visitor);
        }
        visit_ffi_mutable_registered_roots(&mut visitor);
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

    unsafe {
        collector.drain();
    }
    {
        let scanners: Vec<MutableRootScanner> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
        let mut visitor = RuntimeRootVisitor::for_copying_rewrite(&collector);
        for scanner in scanners {
            scanner(&mut visitor);
        }
        visit_ffi_mutable_registered_roots(&mut visitor);
    }
    trace_phase_record(trace, "copying_nursery", phase_start);

    if gc_verify_evacuation_enabled() {
        let phase_start = trace_phase_start(trace);
        let valid_ptrs = build_valid_pointer_set();
        verify_evacuated_no_stale_forwarded_refs(&valid_ptrs);
        trace_phase_record(trace, "evacuation_verify", phase_start);
    }

    let reset = crate::arena::copying_reset_from_spaces_and_flip();
    collector.stats.reset_blocks += reset.reset_blocks;
    if let Some(trace) = trace.as_mut() {
        trace.old_pages = crate::arena::old_page_summary();
    }
    remembered_set_clear();
    collector.sticky.restore();
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
