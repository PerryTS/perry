//! The copied minor's pointer classifier: what kind of heap location a
//! candidate address names, and the header plausibility test that backs it.
//!
//! A SIBLING of `copying.rs` rather than a child module so that the `super::`
//! paths in these bodies keep resolving to `gc` — this is pure code motion out
//! of a file at the 2000-line cap, not a re-scoping.

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
        let Some((space, range_base, object_starts)) =
            crate::arena::classify_heap_space_in_range(addr)
        else {
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
        // Header fields alone cannot distinguish a genuine allocation from
        // payload bytes that happen to encode the same type, flags and size.
        // The per-block bitmap is allocation-authored evidence that this exact
        // address is an object boundary; test it before dereferencing bytes as
        // a `GcHeader`.
        if !crate::arena::arena_header_is_object_start(header_addr, range_base, object_starts) {
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
    let info = match gc_type_info(obj_type) {
        Some(info) => info,
        None => return false,
    };
    let size = (*header).size as usize;
    if size < GC_HEADER_SIZE || size as u64 > (1u64 << 34) {
        return false;
    }
    // Fixed-layout types have a known, constant total allocation size
    // (header + payload). A fabricated header — produced when
    // `classify_arena` reads an interior arena pointer's preceding bytes
    // as a `GcHeader` — supplies a `size` of ~1024 (the top 32 bits of an
    // arena address near 0x400_0000_0000), which can never equal the real
    // total. This rejects the fabricated-Map corruption path at its
    // source: the only descriptor arm that derives a slot base from a
    // payload word is `GcRewriteDescriptorKind::Map` (`(*map).entries`),
    // and GC_TYPE_MAP (8) is the only valid type ID that is a multiple of
    // 8, so it is the only one an 8-aligned interior pointer can
    // fabricate.
    //
    // The invariant "GcHeader.size == fixed_total" holds for every
    // arena-allocated Map/Set because:
    //   * The nursery bump-allocator sets `size = total` (allocators.rs).
    //   * The old-gen allocator uses exact-match free-list reuse and
    //     also sets `size = total`.
    //   * The nursery free-list reuse path is inert —
    //     `hot_arena_free_list` is never populated — so the branch that
    //     would retain a stale larger `size` is dead code. That path is
    //     also fixed to set `size = total` for safety.
    if let Some(fixed_total) = fixed_layout_total_size(info) {
        if size != fixed_total {
            return false;
        }
    }
    let is_arena = (*header).gc_flags & GC_FLAG_ARENA != 0;
    is_arena == arena
}

/// Total allocation size (GcHeader + payload) for types whose payload
/// layout is fixed regardless of content. `None` for variable-size types
/// (arrays, objects, strings, closures, …) whose `size` field reflects
/// runtime content and cannot be checked against a constant.
///
/// Used by `plausible_gc_header` to reject fabricated headers: an
/// interior arena pointer read as a `GcHeader` yields a `size` of
/// ~1024 (the top 32 bits of the address), which never matches the
/// fixed total.
fn fixed_layout_total_size(info: &GcTypeInfo) -> Option<usize> {
    // MapHeader { size: u32, capacity: u32, entries: *mut f64 } = 16 B.
    // SetHeader { size: u32, capacity: u32, elements: *mut f64 } = 16 B.
    match info.type_id {
        crate::gc::GC_TYPE_MAP | crate::gc::GC_TYPE_SET => Some(GC_HEADER_SIZE + 16),
        _ => None,
    }
}
