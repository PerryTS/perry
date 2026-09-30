//! The heap's child-slot views: which payload words of an object the
//! collector visits (`gc_child_slots`, `heap_payload_slot_selection`) and the
//! mutable slot handles the copying and compacting passes rewrite.
//!
//! Split out of `gc/layout.rs` (pure relocation): the per-object layout
//! state and descriptors stay there; this file only reads them.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HeapSlotRange {
    pub(in crate::gc) slots: *mut u64,
    pub(in crate::gc) slot_count: usize,
}

impl HeapSlotRange {
    #[inline]
    pub(crate) fn new(slots: *mut u64, slot_count: usize) -> Self {
        Self { slots, slot_count }
    }

    #[inline]
    pub(in crate::gc) fn is_empty(self) -> bool {
        self.slots.is_null() || self.slot_count == 0
    }

    #[inline]
    pub(in crate::gc) fn slots(self) -> *mut u64 {
        self.slots
    }

    #[inline]
    pub(in crate::gc) fn slot_count(self) -> usize {
        self.slot_count
    }

    #[inline]
    pub(in crate::gc) unsafe fn slot(self, index: usize) -> *mut u64 {
        debug_assert!(index < self.slot_count);
        self.slots.add(index)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HeapChildSlot {
    Child(*mut u64, HeapChildSlotReadKind),
    PointerFreeRange(HeapSlotRange),
}

pub(in crate::gc) enum HeapPayloadSlotScan {
    Empty,
    PointerFree {
        raw_numeric_array: bool,
        raw_numeric_object_slots: usize,
    },
    /// [`LayoutSlotMask::AllPointers`]: the mask selects EVERY live payload
    /// slot, so the slot set is a contiguous range and the descriptor visitor
    /// emits one `Range` rather than `slot_count` individual `Slot`s.
    ///
    /// This is not a micro-optimisation. `scan_dirty_object_slots`'s `Slot` arm
    /// answers "is this slot on a dirty page?" with a hash-set probe **per
    /// slot**, so a 3M-element array of pointers cost 3M probes on every minor
    /// — O(live array) rather than O(dirty pages) — even though the remembered
    /// set knew only a few hundred of its pages were dirty. Its `Range` arm
    /// intersects the range with the dirty-page set directly
    /// (`dirty_slot_ranges_for`), which is what `dirty_slot_ranges_scanned == 0`
    /// in every `retain.ts` GC trace was recording: the cheap arm was never
    /// reached, because an all-pointer array is `Masked`, not `All` (#7787).
    AllPointers {
        raw_numeric_object_slots: usize,
    },
    Masked,
    All(HeapSlotRange),
}

#[derive(Clone)]
pub(in crate::gc) enum HeapPayloadSlotSelection {
    Empty,
    PointerFree {
        emitted: bool,
        raw_numeric_array: bool,
        raw_numeric_object_slots: usize,
    },
    Masked {
        mask: LayoutSlotMask,
        cursor: usize,
        raw_numeric_object_slots: usize,
        raw_numeric_recorded: bool,
    },
    All {
        cursor: usize,
    },
}

pub(crate) struct HeapChildSlotIterator {
    pub(in crate::gc) prefix_slot: Option<*mut u64>,
    /// #6812: second prefix — the object's `meta` header edge. Kept
    /// separate from `prefix_slot` so payload indices stay mask-aligned.
    pub(in crate::gc) meta_slot: Option<*mut u64>,
    /// A second explicit meta edge — the Array-subclass `elements` store at
    /// the end of the `ObjectMeta` record — read exactly like `meta_slot`.
    pub(in crate::gc) meta_slot2: Option<*mut u64>,
    pub(in crate::gc) payload: HeapSlotRange,
    pub(in crate::gc) selection: HeapPayloadSlotSelection,
    /// #8122: the receiver's shape record, resolved ONCE by [`gc_child_slots`]
    /// for an ObjectFields object and borrowed here in place (#10362), so
    /// `visit_gc_layout_slot_descriptors` reads the same record instead of
    /// probing the shape table again. `None` for every other kind, and for
    /// an unstamped object.
    pub(in crate::gc) object_shape: Option<crate::object::shapes::ShapeRecordRef>,
}

impl HeapChildSlotIterator {
    pub(in crate::gc) fn empty() -> Self {
        Self {
            prefix_slot: None,
            meta_slot: None,
            meta_slot2: None,
            payload: HeapSlotRange::new(std::ptr::null_mut(), 0),
            selection: HeapPayloadSlotSelection::Empty,
            object_shape: None,
        }
    }

    pub(in crate::gc) fn new(
        header: *mut GcHeader,
        prefix_slot: Option<*mut u64>,
        payload: HeapSlotRange,
    ) -> Self {
        let selection = unsafe { heap_payload_slot_selection(header, payload) };
        Self {
            prefix_slot,
            meta_slot: None,
            meta_slot2: None,
            payload,
            selection,
            object_shape: None,
        }
    }

    /// [`Self::new`] for an ObjectFields receiver whose shape record the
    /// caller already resolved (#8122). The payload-mask selection reuses it
    /// instead of probing the shape table, and it is retained on the iterator
    /// for the slot visitor.
    pub(in crate::gc) fn new_object(
        header: *mut GcHeader,
        prefix_slot: Option<*mut u64>,
        payload: HeapSlotRange,
        object_shape: Option<crate::object::shapes::ShapeRecordRef>,
    ) -> Self {
        let selection = unsafe { heap_payload_slot_selection_from(header, payload, object_shape) };
        Self {
            prefix_slot,
            meta_slot: None,
            meta_slot2: None,
            payload,
            selection,
            object_shape,
        }
    }

    pub(in crate::gc) fn with_meta_slot(mut self, slot: Option<*mut u64>) -> Self {
        self.meta_slot = slot;
        self
    }

    pub(in crate::gc) fn with_meta_slot2(mut self, slot: Option<*mut u64>) -> Self {
        self.meta_slot2 = slot;
        self
    }

    pub(in crate::gc) fn take_meta_child_slot(&mut self) -> Option<*mut u64> {
        self.meta_slot.take()
    }

    pub(in crate::gc) fn take_meta_child_slot2(&mut self) -> Option<*mut u64> {
        self.meta_slot2.take()
    }

    pub(in crate::gc) fn take_prefix_child_slot(&mut self) -> Option<*mut u64> {
        self.prefix_slot.take()
    }

    pub(in crate::gc) fn payload_scan(&self) -> HeapPayloadSlotScan {
        match self.selection {
            HeapPayloadSlotSelection::Empty => HeapPayloadSlotScan::Empty,
            HeapPayloadSlotSelection::PointerFree {
                raw_numeric_array,
                raw_numeric_object_slots,
                ..
            } => HeapPayloadSlotScan::PointerFree {
                raw_numeric_array,
                raw_numeric_object_slots,
            },
            HeapPayloadSlotSelection::Masked {
                mask: LayoutSlotMask::AllPointers,
                raw_numeric_object_slots,
                raw_numeric_recorded,
                ..
            } => HeapPayloadSlotScan::AllPointers {
                // Mirror the iterator's one-shot accounting: `next` records the
                // raw-numeric skip on its first call and never again, so a
                // descriptor visit that replaces the whole iteration records it
                // exactly once too.
                raw_numeric_object_slots: if raw_numeric_recorded {
                    0
                } else {
                    raw_numeric_object_slots
                },
            },
            HeapPayloadSlotSelection::Masked { .. } => HeapPayloadSlotScan::Masked,
            HeapPayloadSlotSelection::All { .. } => HeapPayloadSlotScan::All(self.payload),
        }
    }
}

impl Iterator for HeapChildSlotIterator {
    type Item = HeapChildSlot;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(slot) = self.prefix_slot.take() {
            return Some(HeapChildSlot::Child(slot, HeapChildSlotReadKind::Prefix));
        }
        if let Some(slot) = self.meta_slot.take() {
            return Some(HeapChildSlot::Child(slot, HeapChildSlotReadKind::Prefix));
        }
        if let Some(slot) = self.meta_slot2.take() {
            return Some(HeapChildSlot::Child(slot, HeapChildSlotReadKind::Prefix));
        }
        match &mut self.selection {
            HeapPayloadSlotSelection::Empty => None,
            HeapPayloadSlotSelection::PointerFree {
                emitted,
                raw_numeric_array,
                raw_numeric_object_slots,
            } => {
                if *emitted || self.payload.is_empty() {
                    None
                } else {
                    *emitted = true;
                    record_layout_pointer_free_range_skipped(self.payload.slot_count());
                    if *raw_numeric_array {
                        record_layout_raw_numeric_array_range_skipped(self.payload.slot_count());
                    }
                    if *raw_numeric_object_slots != 0 {
                        record_layout_raw_numeric_object_field_range_skipped(
                            *raw_numeric_object_slots,
                        );
                    }
                    Some(HeapChildSlot::PointerFreeRange(self.payload))
                }
            }
            HeapPayloadSlotSelection::Masked {
                mask,
                cursor,
                raw_numeric_object_slots,
                raw_numeric_recorded,
            } => {
                if !*raw_numeric_recorded {
                    *raw_numeric_recorded = true;
                    if *raw_numeric_object_slots != 0 {
                        record_layout_raw_numeric_object_field_range_skipped(
                            *raw_numeric_object_slots,
                        );
                    }
                }
                let index = mask.next_slot_at_or_after(*cursor, self.payload.slot_count())?;
                *cursor = index + 1;
                Some(HeapChildSlot::Child(
                    unsafe { self.payload.slot(index) },
                    HeapChildSlotReadKind::Masked,
                ))
            }
            HeapPayloadSlotSelection::All { cursor } => {
                if *cursor >= self.payload.slot_count() {
                    return None;
                }
                let index = *cursor;
                *cursor += 1;
                Some(HeapChildSlot::Child(
                    unsafe { self.payload.slot(index) },
                    HeapChildSlotReadKind::Unknown,
                ))
            }
        }
    }
}

pub(in crate::gc) unsafe fn heap_payload_slot_selection(
    header: *mut GcHeader,
    payload: HeapSlotRange,
) -> HeapPayloadSlotSelection {
    heap_payload_slot_selection_impl(header, payload, |_, _| None)
}

/// [`heap_payload_slot_selection`] for an ObjectFields receiver whose shape
/// record the caller already resolved (#8122): the shared-shape
/// pointer-mask lookup reuses it instead of probing the shape table twice.
pub(in crate::gc) unsafe fn heap_payload_slot_selection_from(
    header: *mut GcHeader,
    payload: HeapSlotRange,
    shape: Option<crate::object::shapes::ShapeRecordRef>,
) -> HeapPayloadSlotSelection {
    // Charter step 5: an object is traced by its shape record's `rep` word;
    // no per-object layout state or mask is read.
    if header.is_null() {
        return HeapPayloadSlotSelection::Empty;
    }
    let selection = by_shape::selection_by_shape(shape, payload);
    #[cfg(debug_assertions)]
    by_shape::debug_verify(header, payload, shape, &selection);
    selection
}

#[inline]
unsafe fn heap_payload_slot_selection_impl(
    header: *mut GcHeader,
    payload: HeapSlotRange,
    shared_mask: impl FnOnce(usize, *const GcHeader) -> Option<LayoutSlotMask>,
) -> HeapPayloadSlotSelection {
    if header.is_null() || payload.is_empty() {
        return HeapPayloadSlotSelection::Empty;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
    // Objects are selected by shape (`heap_payload_slot_selection_from`), which
    // reports their skipped F64 lanes itself; nothing here is an object.
    let raw_numeric_object_slots = 0;
    match (*header)._reserved & GC_LAYOUT_STATE_MASK {
        GC_LAYOUT_POINTER_FREE => HeapPayloadSlotSelection::PointerFree {
            emitted: false,
            raw_numeric_array: (*header).obj_type == GC_TYPE_ARRAY
                && (*header)._reserved & GC_ARRAY_RAW_F64_LAYOUT != 0,
            raw_numeric_object_slots,
        },
        GC_LAYOUT_SIDE_MASK => {
            if (*header)._reserved & GC_LAYOUT_ALL_POINTERS != 0 {
                return HeapPayloadSlotSelection::Masked {
                    mask: LayoutSlotMask::AllPointers,
                    cursor: 0,
                    raw_numeric_object_slots,
                    raw_numeric_recorded: false,
                };
            }
            let mask = per_object_slot_mask(user_ptr).or_else(|| shared_mask(user_ptr, header));
            match mask {
                Some(mask) => HeapPayloadSlotSelection::Masked {
                    mask,
                    cursor: 0,
                    raw_numeric_object_slots,
                    raw_numeric_recorded: false,
                },
                None => {
                    set_layout_state(header, GC_LAYOUT_UNKNOWN);
                    HeapPayloadSlotSelection::All { cursor: 0 }
                }
            }
        }
        _ => HeapPayloadSlotSelection::All { cursor: 0 },
    }
}

/// #10362: every arm returns the iterator it builds, never through an `Option`
/// combinator whose temporary is copied out — a per-object memmove per GC walk.
pub(in crate::gc) unsafe fn gc_child_slots(header: *mut GcHeader) -> HeapChildSlotIterator {
    if header.is_null() || (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        return HeapChildSlotIterator::empty();
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    match gc_type_layout_slot_kind((*header).obj_type) {
        GcLayoutSlotKind::ArrayElements => {
            let arr = user_ptr as *mut crate::array::ArrayHeader;
            let Some(range) = crate::array::gc_element_slot_range(arr) else {
                return HeapChildSlotIterator::empty();
            };
            HeapChildSlotIterator::new(header, None, range)
        }
        GcLayoutSlotKind::ObjectFields => {
            let obj = user_ptr as *mut crate::object::ObjectHeader;
            // #8122: resolve the receiver's shape record ONCE and thread it
            // through every step that needs a shape fact — the field range,
            // the keys edge, the shared pointer mask (`new_object`) and the
            // slot visitor (`object_shape` on the iterator). These used to be
            // five independent `shape_descriptor_by_id` probes per traced
            // object, the top leaf of a traced in-place-promotion cycle.
            let shape = crate::object::shapes::object_shape_record(obj);
            let Some(range) = crate::object::gc_field_slot_range(obj, shape) else {
                return HeapChildSlotIterator::empty();
            };
            // #6812: the meta record is a raw-pointer child edge; before the
            // spill buffer it was enumerated only on the rewrite path, which
            // left it invisible to MARKING (latent for custom prototypes,
            // which are usually rooted elsewhere; fatal for the spill
            // buffer, reachable through meta alone). A second prefix slot
            // keeps payload slot indices aligned with the layout masks.
            HeapChildSlotIterator::new_object(header, None, range, shape)
                .with_meta_slot(crate::object::gc_object_meta_slot(user_ptr as usize))
        }
        GcLayoutSlotKind::RegExpFields => {
            let (pattern_slot, slot_count, last_index_slot) =
                crate::regex::regex_gc_slot_ptrs(user_ptr as *mut crate::regex::RegExpHeader);
            HeapChildSlotIterator::new(
                header,
                Some(last_index_slot),
                HeapSlotRange::new(pattern_slot, slot_count),
            )
            // #6759 phase 1: the metadata edge. RegExp reaches marking through
            // THIS iterator (its rewrite arm delegates here), so the edge has
            // to be enumerated at this point, not in the rewrite match.
            .with_meta_slot(crate::object::cell_meta_slot(user_ptr as usize).map(|s| s as *mut u64))
            .with_meta_slot2(crate::regex::regex_program_slot(user_ptr))
        }
        GcLayoutSlotKind::ObjectMeta => {
            // DIVERGENT AND UNREACHABLE (#10868 step 2.5 stage 1): the
            // authoritative meta enumerator is layout_slot_visit ObjectMeta
            // rewrite arm, which does not delegate here. This iterator has
            // four edge sources and the record has seven pointer words, so
            // expando, dictionary_keys and arguments are absent below.
            // Prototype and the private-evaluation brand are explicit prefix
            // edges. Keep the brand out of the payload selection: its class
            // object can be reachable only through this metadata record, so
            // treating it as ordinary payload lets a stale/partial layout
            // mask silently collect the class evaluation identity.
            let meta = user_ptr as *mut crate::object::ObjectMeta;
            let proto_slot = Some(&mut (*meta).prototype as *mut u64);
            let brand_slot = Some(&mut (*meta).private_evaluation_brand as *mut u64);
            let range = HeapSlotRange::new(&mut (*meta).spill as *mut u64, 1);
            // The Array-subclass elements store is a raw-pointer child edge
            // (0 = none) exactly like `spill`; it sits at the end of the
            // record, so it is enumerated as a second explicit meta edge.
            let elements_slot = Some(&mut (*meta).elements as *mut u64);
            HeapChildSlotIterator::new(header, proto_slot, range)
                .with_meta_slot(brand_slot)
                .with_meta_slot2(elements_slot)
        }
        GcLayoutSlotKind::ClosureCaptures => {
            let closure = user_ptr as *mut crate::closure::ClosureHeader;
            let Some(range) = crate::closure::gc_capture_slot_range(closure) else {
                return HeapChildSlotIterator::empty();
            };
            // D1: the function's own-property bag is a raw-pointer child
            // edge (0 = none), like an `ObjectHeader`'s `meta`: marking keeps
            // it alive and evacuation rewrites it.
            let props = &mut (*closure).props as *mut _ as *mut u64;
            HeapChildSlotIterator::new(header, None, range)
                .with_meta_slot((*props != 0).then_some(props))
        }
        GcLayoutSlotKind::None => HeapChildSlotIterator::empty(),
    }
}

#[derive(Clone, Copy)]
pub(in crate::gc) struct GcMutableSlot {
    pub(in crate::gc) slot: *mut u64,
    pub(in crate::gc) layout_kind: Option<HeapChildSlotReadKind>,
}

impl GcMutableSlot {
    #[inline]
    pub(in crate::gc) fn new(slot: *mut u64, layout_kind: Option<HeapChildSlotReadKind>) -> Self {
        Self { slot, layout_kind }
    }

    /// Is the slot's address outside old-gen? #10182: classified on demand (its
    /// one reader asks at once), so the full mark no longer classifies per slot.
    #[inline]
    pub(in crate::gc) fn external(self) -> bool {
        let generation = crate::arena::classify_heap_generation(self.slot as usize);
        !matches!(generation, crate::arena::HeapGeneration::Old)
    }

    #[inline]
    pub(in crate::gc) fn record_layout_read(self) {
        if let Some(kind) = self.layout_kind {
            record_layout_child_slot_read(kind);
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::gc) enum GcMutableSlotDescriptor {
    Slot(GcMutableSlot),
    Range {
        range: HeapSlotRange,
        layout_kind: Option<HeapChildSlotReadKind>,
    },
    PointerFreeRange(HeapSlotRange),
}

impl GcMutableSlotDescriptor {
    pub(in crate::gc) unsafe fn visit_slots(self, visit: &mut dyn FnMut(GcMutableSlot)) {
        match self {
            GcMutableSlotDescriptor::Slot(slot) => visit(slot),
            GcMutableSlotDescriptor::Range { range, layout_kind } => {
                for i in 0..range.slot_count() {
                    visit(GcMutableSlot::new(range.slot(i), layout_kind));
                }
            }
            GcMutableSlotDescriptor::PointerFreeRange(_) => {}
        }
    }
}

#[inline(always)]
pub(in crate::gc) fn record_trace_slot_read() {
    #[cfg(test)]
    TRACE_SLOT_READS.with(|c| c.set(c.get() + 1));
}
