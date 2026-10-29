//! Static GC scan plans (GC slice 4).
//!
//! An object born on a compiler-emitted family layout (a class family or a
//! literal family) is scanned by both collectors from a plan that is DATA in the
//! layout's descriptor, not re-derived from the shape for every object. The
//! descriptor word [`PerryLayoutDesc::gc_plan`](crate::object::layout_desc::PerryLayoutDesc)
//! says which inline slots may hold a heap pointer (codegen derives it from the
//! family's slot count and field representation, `perry_abi::layout_gc_plan`).
//! The walk is then straight-line:
//!
//! * in a full mark, the prototype edge when the shape names a heap prototype
//!   (the runtime prototype table's word, emitted once per identity per trace
//!   exactly as the generic walk does). A copying minor does NOT visit it: the
//!   minor roots every prototype word that names a young object
//!   (`shapes_prototype::scan_shape_prototype_words_mut`, the young log is the
//!   complete set), so the per-carrier visit marks nothing the roots have not;
//! * the meta record and the arguments tail, both per-object header facts;
//! * the payload slots named by the plan's mask, in ascending order.
//!
//! What it does NOT visit, because the family's record is a static record
//! (an external carrier that no prune retires and whose keys the shape table
//! roots on every collection): the shape's keys edge and the old-carrier and
//! carried-in-full-trace notes. The generic walk visits and makes them per
//! object, 3.1M times for a binary-tree run, to learn nothing the table scan
//! does not already know. A family layout has no evaluation holder either (an
//! instance on its family's own layout keeps the holder in its metadata).
//!
//! The plan applies only to an object on its family's OWN layout word
//! (`layout_desc::static_gc_scan_mask`); every other object, and every
//! descriptor without a plan, takes the generic walk unchanged.
//!
//! Drift is the risk of a second enumeration, so it is not trusted: in test and
//! debug-assertion builds every object a plan walks is ALSO enumerated by the
//! generic walk, and the two slot lists must agree, apart from the keys edge the
//! plan deliberately omits (`assert_matches_generic_walk`).
//! `gc::tests::static_scan` proves the check fires on a sabotaged plan and that
//! both collectors take the path.

use super::copying_parent_facts::ParentRemembering;
use super::*;

/// One object's scan: the explicit edges and the payload mask.
pub(super) struct StaticScan {
    /// The prototype edge, for a full mark only (`None` in a minor, see the
    /// module docs).
    pub(super) prototype: Option<*mut u64>,
    pub(super) meta_edge: Option<*mut u64>,
    pub(super) arguments: Option<*mut u64>,
    pub(super) payload: *mut u64,
    /// Payload slot indices to scan, ascending.
    pub(super) mask: u32,
}

/// Is this header an ordinary object the way the plan assumes? Decided from
/// the header and the (constant) type table alone; trace state is the caller's.
#[inline(always)]
pub(super) unsafe fn static_scan_admissible(header: *mut GcHeader) -> bool {
    crate::gc_type_is_object!((*header).obj_type)
        && (*header).gc_flags & GC_FLAG_FORWARDED == 0
        && matches!(
            gc_type_rewrite_descriptor_kind(GC_TYPE_OBJECT),
            GcRewriteDescriptorKind::Object
        )
        && matches!(
            gc_type_layout_slot_kind(GC_TYPE_OBJECT),
            GcLayoutSlotKind::ObjectFields
        )
        && gc_type_rewrite_hook_kind(GC_TYPE_OBJECT) == GcRewriteHookKind::None
}

/// The static plan of an admitted object, or `None` for the generic walk.
/// `full` is a full mark (it adds the prototype edge, whose per-identity
/// de-duplication it decides exactly as the generic walk does).
#[inline(always)]
pub(super) unsafe fn static_scan(header: *mut GcHeader, full: Option<bool>) -> Option<StaticScan> {
    static_scan_body::<{ crate::object::header::meta_form_alloc_bytes(0) }, true>(header, full)
}

/// The walk's one body: `BASE` is the byte offset of the first slot from the
/// object, `META` whether the form carries a meta word. Only the meta form
/// (`BASE = 16`, `META = true`) is instantiated today.
#[inline(always)]
unsafe fn static_scan_body<const BASE: usize, const META: bool>(
    header: *mut GcHeader,
    full: Option<bool>,
) -> Option<StaticScan> {
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    let obj = user_ptr as *mut crate::object::ObjectHeader;
    let record = crate::object::shapes::object_shape_record(obj)?;
    let mask = crate::object::layout_desc::static_gc_scan_mask(obj, record)?;
    #[cfg(test)]
    let mask = sabotage::perturb(mask);
    #[cfg(test)]
    sabotage::note_hit();
    let scan = StaticScan {
        prototype: match full {
            Some(full) => crate::object::gc_shape_prototype_edge_slot(Some(record), full),
            None => None,
        },
        meta_edge: if META {
            crate::object::gc_object_meta_slot(user_ptr as usize)
        } else {
            None
        },
        arguments: crate::object::arguments_mapping_slot(obj),
        payload: crate::object::header::object_slot_at(obj, BASE as u32),
        mask,
    };
    #[cfg(any(test, debug_assertions))]
    assert_matches_generic_walk(header, record, &scan, full.is_some());
    Some(scan)
}

impl CopyingNurseryCollector {
    /// The copying minor's walk of a [`StaticScan`].
    #[inline(always)]
    pub(super) unsafe fn visit_static_scan<const DIAGNOSTICS: bool>(
        &mut self,
        scan: &StaticScan,
        header: *mut GcHeader,
        weak: bool,
        remembering: ParentRemembering,
    ) {
        if let Some(slot) = scan.meta_edge {
            self.visit_slot_with_parent_facts::<DIAGNOSTICS>(
                GcMutableSlot::pointer(slot.cast()),
                header,
                weak,
                remembering,
            );
        }
        if let Some(slot) = scan.arguments {
            self.visit_slot_with_parent_facts::<DIAGNOSTICS>(
                GcMutableSlot::new(slot, None),
                header,
                weak,
                remembering,
            );
        }
        let mut word = scan.mask;
        while word != 0 {
            let index = word.trailing_zeros() as usize;
            word &= word - 1;
            self.visit_slot_with_parent_facts::<DIAGNOSTICS>(
                GcMutableSlot::new(scan.payload.add(index), None),
                header,
                weak,
                remembering,
            );
        }
    }
}

/// The edges of a [`StaticScan`] for the full mark, with the layout kinds the
/// generic walk labels them with (the trace statistics count by kind).
#[inline(always)]
pub(super) unsafe fn for_each_mark_slot(scan: &StaticScan, mut visit: impl FnMut(GcMutableSlot)) {
    if let Some(slot) = scan.prototype {
        let mut slot = GcMutableSlot::runtime_root(slot);
        slot.layout_kind = Some(HeapChildSlotReadKind::Prefix);
        visit(slot);
    }
    if let Some(slot) = scan.meta_edge {
        let mut slot = GcMutableSlot::pointer(slot.cast());
        slot.layout_kind = Some(HeapChildSlotReadKind::Prefix);
        visit(slot);
    }
    if let Some(slot) = scan.arguments {
        visit(GcMutableSlot::new(
            slot,
            Some(HeapChildSlotReadKind::Prefix),
        ));
    }
    // The generic walk hands an all-`Any` payload over as one range, whose
    // pointer children are prefetched before the first is marked: each is a
    // cold header read the mark would otherwise take one at a time.
    let mut word = scan.mask;
    while word != 0 {
        let index = word.trailing_zeros() as usize;
        word &= word - 1;
        let bits = *scan.payload.add(index);
        let tag = bits & TAG_MASK;
        if tag == POINTER_TAG || tag == STRING_TAG {
            super::prefetch::prefetch_read(
                ((bits & POINTER_MASK) as usize).wrapping_sub(GC_HEADER_SIZE),
            );
        }
    }
    let mut word = scan.mask;
    while word != 0 {
        let index = word.trailing_zeros() as usize;
        word &= word - 1;
        visit(GcMutableSlot::new(
            scan.payload.add(index),
            Some(HeapChildSlotReadKind::Masked),
        ));
    }
}

/// The generic walk must enumerate exactly the plan's slots, plus the two shape
/// edges the plan omits by design: the keys edge (rooted by the shape table
/// for a static record) and, in a minor, the prototype word (rooted by the
/// young log). The second is checked, not assumed: a prototype word naming a
/// minor-collectible object must be in the young log the minor roots from.
#[cfg(any(test, debug_assertions))]
unsafe fn assert_matches_generic_walk(
    header: *mut GcHeader,
    record: crate::object::shapes::ShapeRecordRef,
    scan: &StaticScan,
    full: bool,
) {
    let keys_edge = record.keys_slot() as usize;
    let prototype_edge = crate::object::gc_shape_prototype_edge_slot(Some(record), false);
    if let Some(word) = prototype_edge {
        if !full {
            assert!(
                crate::object::shapes::prototype_word_rooted_by_minor(word),
                "static_scan: a minor skips the prototype edge of header {header:p}, but its \
                 word {word:p} names a young object that is not in the minor's young log"
            );
        }
    }
    let mut generic = Vec::new();
    visit_gc_rewrite_slots::<false>(header, |slot| {
        let address = slot.slot as usize;
        // A full trace emits a prototype identity once, to its first carrier;
        // the generic walk just asked for it again.
        if address != keys_edge && Some(address) != prototype_edge.map(|w| w as usize) {
            generic.push((address, slot.is_parent_owned()));
        }
    });
    let mut expected = Vec::new();
    if let Some(slot) = scan.meta_edge {
        expected.push((
            slot as usize,
            GcMutableSlot::pointer(slot.cast()).is_parent_owned(),
        ));
    }
    if let Some(slot) = scan.arguments {
        expected.push((
            slot as usize,
            GcMutableSlot::new(slot, None).is_parent_owned(),
        ));
    }
    let mut word = scan.mask;
    while word != 0 {
        let index = word.trailing_zeros() as usize;
        word &= word - 1;
        let slot = GcMutableSlot::new(scan.payload.add(index), None);
        expected.push((slot.slot as usize, slot.is_parent_owned()));
    }
    assert!(
        generic == expected,
        "static_scan: the static plan enumerated different slots than the generic walk for \
         header {header:p} — a traced edge would be lost or invented \
         (generic {generic:x?}, plan {expected:x?})"
    );
}

/// Test-only sabotage and liveness counters for the static plan. Witness:
/// `gc::tests::static_scan`.
#[cfg(test)]
pub(crate) mod sabotage {
    use std::cell::Cell;

    thread_local! {
        static DROP_TOP_SLOT: Cell<bool> = const { Cell::new(false) };
        static HITS: Cell<u32> = const { Cell::new(0) };
    }

    pub(crate) struct DropTopSlot(bool);

    impl DropTopSlot {
        pub(crate) fn arm() -> Self {
            Self(DROP_TOP_SLOT.with(|c| c.replace(true)))
        }
    }

    impl Drop for DropTopSlot {
        fn drop(&mut self) {
            DROP_TOP_SLOT.with(|c| c.set(self.0));
        }
    }

    /// Forget the highest payload slot: the one a mask mistake loses.
    pub(super) fn perturb(mask: u32) -> u32 {
        if mask == 0 || !DROP_TOP_SLOT.with(Cell::get) {
            return mask;
        }
        mask & !(1 << (31 - mask.leading_zeros()))
    }

    pub(super) fn note_hit() {
        HITS.with(|c| c.set(c.get().wrapping_add(1)));
    }

    /// Objects that were scanned through a static plan on this thread.
    pub(crate) fn hits() -> u32 {
        HITS.with(Cell::get)
    }
}
