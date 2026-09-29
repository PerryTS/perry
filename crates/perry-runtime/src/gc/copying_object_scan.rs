//! The copying drain's straight-line scan of an ordinary object (#11549).
//!
//! `scan_object_fields` reaches an object's slots through the generic walk:
//! `visit_gc_rewrite_slot_descriptors` → `visit_gc_layout_slot_descriptors` →
//! a `HeapChildSlotIterator` built by `gc_child_slots`, with every slot handed
//! through two visitor closures. For a fully-live binary tree that plumbing —
//! not the marking — was most of the ~1,470 instructions each promoted object
//! cost (callgrind, `gc_collect_minor_with_trigger_inner` only).
//!
//! This is the same walk for the one kind that dominates real heaps,
//! `GC_TYPE_OBJECT`, written out: the same shape record, the same field range,
//! the same payload selection (`heap_payload_slot_selection_from` itself, so the
//! mask logic has ONE copy), the same shape-keys-edge bookkeeping, and the same
//! slots in the same order — keys edge, meta record, payload, overflow fields.
//!
//! It declines (returns `false` having changed nothing) whenever the generic walk
//! would do something it does not reproduce: a residual `setPrototypeOf` entry,
//! a full trace or layout-scan trace in progress, a payload mask wider than one
//! word, or a type-table entry that stopped describing `GC_TYPE_OBJECT` the way
//! this assumes. Every decline happens before the first side effect this path
//! would duplicate.
//!
//! Drift is the risk of a second enumeration, so it is not trusted: in test and
//! debug-assertion builds every object this path handles is ALSO enumerated by
//! the generic walk, and the two slot lists must be identical
//! (`assert_matches_generic_walk`). `gc::tests::copying_object_scan` proves that
//! check fires on a sabotaged plan and that the path is actually taken.

use super::copying_parent_facts::{weak_holder_fact, ParentRemembering};
use super::*;

/// The slots, in visit order, the generic walk would hand the drain for this
/// object — excluding the overflow fields, which the scan visits straight from
/// their side table exactly as the generic walk does. Iterated, not stored: the
/// keys edge and the meta record (null when absent), then the payload slots
/// named by one mask word.
#[derive(Clone, Copy)]
struct PlainObjectPlan {
    prefix: [*mut u64; 2],
    next_prefix: usize,
    payload: HeapSlotRange,
    payload_word: u64,
}

impl PlainObjectPlan {
    #[inline(always)]
    unsafe fn next_slot(&mut self) -> Option<*mut u64> {
        while self.next_prefix < 2 {
            let slot = self.prefix[self.next_prefix];
            self.next_prefix += 1;
            if !slot.is_null() {
                return Some(slot);
            }
        }
        if self.payload_word == 0 {
            return None;
        }
        let index = self.payload_word.trailing_zeros() as usize;
        self.payload_word &= self.payload_word - 1;
        Some(self.payload.slot(index))
    }
}

/// Build the plan, or `None` — having performed no side effect the generic walk
/// would then repeat — when the object is not one this path reproduces.
#[inline(always)]
unsafe fn plain_object_plan(header: *mut GcHeader) -> Option<PlainObjectPlan> {
    if (*header).obj_type != GC_TYPE_OBJECT || (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        return None;
    }
    // The generic walk's first edge, ahead of its kind arms.
    if crate::object::prototype_chain::object_static_prototypes_maybe_nonempty()
        && crate::object::prototype_chain::residual_prototype_owner_type(GC_TYPE_OBJECT)
        && crate::object::prototype_chain::residual_entry_possible_for(header)
    {
        return None;
    }
    // A full trace notes every carrier; a layout-scan trace counts every slot.
    // Neither is reproduced here.
    if full_trace_active() || layout_scan_trace_active() {
        return None;
    }
    if !matches!(
        gc_type_rewrite_descriptor_kind(GC_TYPE_OBJECT),
        GcRewriteDescriptorKind::Object
    ) || !matches!(
        gc_type_layout_slot_kind(GC_TYPE_OBJECT),
        GcLayoutSlotKind::ObjectFields
    ) {
        return None;
    }
    #[cfg(test)]
    sabotage::note_plan_attempt();
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    let obj = user_ptr as *mut crate::object::ObjectHeader;
    // `gc_child_slots`' ObjectFields arm, step for step.
    let shape = crate::object::shapes::object_shape_record(obj);
    let range = crate::object::gc_field_slot_range(obj, shape)?;
    let meta = crate::object::gc_object_meta_slot(user_ptr as usize);
    // The selection's only side effect (demoting a SIDE_MASK header whose mask
    // is gone to UNKNOWN) is one the generic walk would make identically and
    // then read back as `All`, so declining after it is still exact.
    let selection = heap_payload_slot_selection_from(header, range, shape);
    let payload_word = match selection {
        HeapPayloadSlotSelection::Empty | HeapPayloadSlotSelection::PointerFree { .. } => 0,
        HeapPayloadSlotSelection::Masked {
            mask: LayoutSlotMask::Inline(bits),
            ..
        } => {
            // `take_inline_mask_word`'s limit, exactly.
            let limit = range.slot_count().min(64);
            bits & if limit == 64 {
                u64::MAX
            } else {
                (1u64 << limit) - 1
            }
        }
        HeapPayloadSlotSelection::Masked {
            mask: LayoutSlotMask::AllPointers,
            ..
        }
        | HeapPayloadSlotSelection::All { .. } => {
            if range.slot_count() > 64 {
                return None;
            }
            if range.slot_count() == 64 {
                u64::MAX
            } else {
                (1u64 << range.slot_count()) - 1
            }
        }
        HeapPayloadSlotSelection::Masked {
            mask: LayoutSlotMask::Heap(_),
            ..
        } => return None,
    };
    // `visit_gc_layout_slot_descriptors`' shape-keys edge. `full_trace_active`
    // is false here, so only the old-carrier note applies — and when the note
    // would change nothing, the generation probe that gates it is skipped.
    #[cfg(test)]
    let already_noted = sabotage::claiming_noted_carriers()
        || crate::object::shapes::old_generation_carrier_already_noted(shape);
    #[cfg(not(test))]
    let already_noted = crate::object::shapes::old_generation_carrier_already_noted(shape);
    if !already_noted && !crate::arena::pointer_in_nursery(user_ptr as usize) {
        crate::object::shapes::note_old_generation_carrier(shape);
    }
    let keys_edge = crate::object::gc_shape_keys_edge_slot(shape);
    #[cfg(test)]
    let payload_word = sabotage::perturb(payload_word);
    // Visit order of the generic walk: prefix (none for objects), keys edge,
    // meta, meta2 (none), payload ascending.
    let plan = PlainObjectPlan {
        prefix: [
            keys_edge.unwrap_or(std::ptr::null_mut()),
            meta.unwrap_or(std::ptr::null_mut()),
        ],
        next_prefix: 0,
        payload: range,
        payload_word,
    };
    Some(plan)
}

impl CopyingNurseryCollector {
    /// Scan an ordinary object through its [`PlainObjectPlan`]. `false` means
    /// nothing was done and the caller must take the generic walk.
    #[inline(always)]
    pub(super) unsafe fn scan_plain_object(&mut self, header: *mut GcHeader) -> bool {
        let Some(mut plan) = plain_object_plan(header) else {
            return false;
        };
        #[cfg(test)]
        let cross_check = !sabotage::cross_check_disabled();
        #[cfg(all(not(test), debug_assertions))]
        let cross_check = true;
        #[cfg(any(test, debug_assertions))]
        if cross_check {
            assert_matches_generic_walk(header, &plan);
        }
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        let mut changed = false;
        // Asked lazily by the generic walk, at its first slot; both are
        // properties of the header alone, so asking them up front is the same
        // answer.
        let weak = weak_holder_fact(header);
        let remembering = ParentRemembering::of(header, self.skip_remembering);
        while let Some(slot) = plan.next_slot() {
            let before = *slot;
            self.visit_slot_with_parent_facts(
                GcMutableSlot::new(slot, None),
                header,
                weak,
                remembering,
            );
            changed |= *slot != before;
        }
        // The generic walk's last Object-arm edge, from the same side table.
        crate::object::visit_overflow_field_slots_mut(user_ptr, |slot| {
            let before = *slot;
            self.visit_overflow_slot(slot, header, weak, remembering);
            changed |= *slot != before;
        });
        if changed {
            run_gc_rewrite_hook(GC_TYPE_OBJECT, user_ptr);
        }
        true
    }

    /// Overflow fields are rare; keep their visit out of the hot loop's code.
    #[inline(never)]
    unsafe fn visit_overflow_slot(
        &mut self,
        slot: *mut u64,
        header: *mut GcHeader,
        weak: bool,
        remembering: ParentRemembering,
    ) {
        self.visit_slot_with_parent_facts(GcMutableSlot::new(slot, None), header, weak, remembering);
    }
}

/// The drift check: the generic walk, run for its slot list only, must name
/// exactly the plan's slots followed by the overflow fields, in that order.
#[cfg(any(test, debug_assertions))]
unsafe fn assert_matches_generic_walk(header: *mut GcHeader, plan: &PlainObjectPlan) {
    let mut generic = Vec::new();
    visit_gc_rewrite_slots(header, |slot| generic.push(slot.slot as usize));
    let mut replay = *plan;
    let mut expected = Vec::new();
    while let Some(slot) = replay.next_slot() {
        expected.push(slot as usize);
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
    crate::object::visit_overflow_field_slots_mut(user_ptr, |slot| expected.push(slot as usize));
    assert_eq!(
        generic, expected,
        "copying_object_scan: the plain-object plan enumerated different slots than the \
         generic walk for header {header:p} — a traced edge would be lost or invented"
    );
}

/// Test-only sabotage and liveness counters for the plain-object path. Witness:
/// `gc::tests::copying_object_scan`.
#[cfg(test)]
pub(crate) mod sabotage {
    use std::cell::Cell;

    thread_local! {
        static DROP_TOP_PAYLOAD_SLOT: Cell<bool> = const { Cell::new(false) };
        static CLAIM_NOTED_CARRIERS: Cell<bool> = const { Cell::new(false) };
        static NO_CROSS_CHECK: Cell<bool> = const { Cell::new(false) };
        static PLAN_ATTEMPTS: Cell<u64> = const { Cell::new(0) };
    }

    /// The generic-walk cross-check off. Its walk makes the old-carrier note
    /// itself, so a test of whether THIS path makes the note must not run it.
    pub(super) fn cross_check_disabled() -> bool {
        NO_CROSS_CHECK.with(Cell::get)
    }

    pub(crate) struct NoCrossCheck(bool);

    impl NoCrossCheck {
        pub(crate) fn arm() -> Self {
            Self(NO_CROSS_CHECK.with(|c| c.replace(true)))
        }
    }

    impl Drop for NoCrossCheck {
        fn drop(&mut self) {
            NO_CROSS_CHECK.with(|c| c.set(self.0));
        }
    }

    /// Every shape reads as already noted, so the old-carrier note never runs.
    pub(super) fn claiming_noted_carriers() -> bool {
        CLAIM_NOTED_CARRIERS.with(Cell::get)
    }

    pub(crate) struct ClaimNotedCarriers(bool);

    impl ClaimNotedCarriers {
        pub(crate) fn arm() -> Self {
            Self(CLAIM_NOTED_CARRIERS.with(|c| c.replace(true)))
        }
    }

    impl Drop for ClaimNotedCarriers {
        fn drop(&mut self) {
            CLAIM_NOTED_CARRIERS.with(|c| c.set(self.0));
        }
    }

    pub(super) fn note_plan_attempt() {
        PLAN_ATTEMPTS.with(|c| c.set(c.get() + 1));
    }

    /// Objects that reached plan construction on this thread.
    pub(crate) fn plan_attempts() -> u64 {
        PLAN_ATTEMPTS.with(Cell::get)
    }

    /// Forget the highest payload slot — the one a limit mistake loses.
    pub(super) fn perturb(word: u64) -> u64 {
        if DROP_TOP_PAYLOAD_SLOT.with(Cell::get) && word != 0 {
            return word & !(1u64 << (63 - word.leading_zeros()));
        }
        word
    }

    pub(crate) struct DropTopPayloadSlot(bool);

    impl DropTopPayloadSlot {
        pub(crate) fn arm() -> Self {
            Self(DROP_TOP_PAYLOAD_SLOT.with(|c| c.replace(true)))
        }
    }

    impl Drop for DropTopPayloadSlot {
        fn drop(&mut self) {
            DROP_TOP_PAYLOAD_SLOT.with(|c| c.set(self.0));
        }
    }
}
