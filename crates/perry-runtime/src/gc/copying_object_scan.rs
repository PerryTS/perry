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
//! `GC_TYPE_OBJECT`, written out: the same residual-prototype edge first, the
//! same shape record, field range and payload selection
//! (`heap_payload_slot_selection_from` itself, so the mask logic has ONE copy),
//! the same shape-keys-edge bookkeeping, and the same slots in the same order —
//! residual prototype, keys edge, meta record, payload, overflow fields.
//!
//! It declines (returns `false` having done nothing) only up front, from the
//! header and thread state: a full trace or layout-scan trace in progress (the
//! generic walk records carrier notes and slot counters this path does not), or
//! a type-table entry that stopped describing `GC_TYPE_OBJECT` the way this
//! assumes. Once admitted it never hands the object back, so no edge is visited
//! twice.
//!
//! Drift is the risk of a second enumeration, so it is not trusted: in test and
//! debug-assertion builds every object this path handles is ALSO enumerated by
//! the generic walk, and the two slot lists must be identical
//! (`assert_matches_generic_walk`). `gc::tests::copying_object_scan` proves that
//! check fires on a sabotaged plan and that the path is actually taken.

use super::copying_parent_facts::{weak_holder_fact, ParentRemembering};
use super::*;

/// The slots, in visit order, the generic walk's layout arm hands the drain for
/// this object: the keys edge, the shape's prototype edge and the meta record
/// (null when absent), then the payload slots its selection names. Iterated,
/// not stored.
#[derive(Clone)]
struct PlainObjectPlan {
    prefix: [*mut u64; 3],
    next_prefix: usize,
    payload: HeapSlotRange,
    walk: PayloadWalk,
}

/// The payload selection, as the generic walk would iterate it.
#[derive(Clone)]
enum PayloadWalk {
    /// A one-word mask (`take_inline_mask_word`'s walk), already limited.
    Word(u64),
    /// Every slot `next..count` (`AllPointers` / `All`: a `Range`).
    Range { next: usize, count: usize },
    /// A mask wider than one word (the iterator's `Masked` arm).
    Mask {
        mask: LayoutSlotMask,
        cursor: usize,
        count: usize,
    },
}

impl PlainObjectPlan {
    #[inline(always)]
    unsafe fn next_slot(&mut self) -> Option<*mut u64> {
        while self.next_prefix < 3 {
            let slot = self.prefix[self.next_prefix];
            self.next_prefix += 1;
            if !slot.is_null() {
                return Some(slot);
            }
        }
        let index = match &mut self.walk {
            PayloadWalk::Word(word) => {
                if *word == 0 {
                    return None;
                }
                let index = word.trailing_zeros() as usize;
                *word &= *word - 1;
                index
            }
            PayloadWalk::Range { next, count } => {
                if *next >= *count {
                    return None;
                }
                *next += 1;
                *next - 1
            }
            PayloadWalk::Mask {
                mask,
                cursor,
                count,
            } => {
                let index = mask.next_slot_at_or_after(*cursor, *count)?;
                *cursor = index + 1;
                index
            }
        };
        Some(self.payload.slot(index))
    }
}

/// May this path scan the object at all? Decided before ANY side effect, from
/// the header and per-thread/process state alone. Past this point the path never
/// hands the object back to the generic walk.
#[inline(always)]
unsafe fn plain_object_admissible(header: *mut GcHeader) -> bool {
    (*header).obj_type == GC_TYPE_OBJECT
        && (*header).gc_flags & GC_FLAG_FORWARDED == 0
        // A full trace notes every carrier; a layout-scan trace counts every
        // slot. Neither is reproduced here.
        && !full_trace_active()
        && !layout_scan_trace_active()
        && matches!(
            gc_type_rewrite_descriptor_kind(GC_TYPE_OBJECT),
            GcRewriteDescriptorKind::Object
        )
        && matches!(
            gc_type_layout_slot_kind(GC_TYPE_OBJECT),
            GcLayoutSlotKind::ObjectFields
        )
}

/// `gc_child_slots`' ObjectFields arm and `visit_gc_layout_slot_descriptors`'
/// shape-keys bookkeeping, step for step and with the same side effects, as a
/// plan instead of an iterator.
#[inline(always)]
unsafe fn plain_object_plan(header: *mut GcHeader) -> PlainObjectPlan {
    #[cfg(test)]
    sabotage::note_plan_attempt();
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    let obj = user_ptr as *mut crate::object::ObjectHeader;
    let shape = crate::object::shapes::object_shape_record(obj);
    let Some(range) = crate::object::gc_field_slot_range(obj, shape) else {
        // `gc_child_slots` returns the EMPTY iterator: no shape, so no keys
        // edge and no carrier note, no meta edge, no payload.
        return PlainObjectPlan {
            prefix: [std::ptr::null_mut(); 3],
            next_prefix: 3,
            payload: HeapSlotRange::new(std::ptr::null_mut(), 0),
            walk: PayloadWalk::Word(0),
        };
    };
    let meta = crate::object::gc_object_meta_slot(user_ptr as usize);
    let count = range.slot_count();
    let walk = match heap_payload_slot_selection_from(header, range, shape) {
        HeapPayloadSlotSelection::Empty | HeapPayloadSlotSelection::PointerFree { .. } => {
            PayloadWalk::Word(0)
        }
        HeapPayloadSlotSelection::Masked {
            mask: LayoutSlotMask::Inline(bits),
            ..
        } => {
            // `take_inline_mask_word`'s limit, exactly.
            let limit = count.min(64);
            PayloadWalk::Word(
                bits & if limit == 64 {
                    u64::MAX
                } else {
                    (1u64 << limit) - 1
                },
            )
        }
        HeapPayloadSlotSelection::Masked {
            mask: LayoutSlotMask::AllPointers,
            ..
        }
        | HeapPayloadSlotSelection::All { .. } => PayloadWalk::Range { next: 0, count },
        HeapPayloadSlotSelection::Masked { mask, .. } => PayloadWalk::Mask {
            mask,
            cursor: 0,
            count,
        },
    };
    #[cfg(test)]
    let walk = sabotage::perturb(walk);
    // The shape-keys edge. `full_trace_active` is false here, so only the
    // old-carrier note applies — and when the note would change nothing, the
    // generation probe that gates it is skipped.
    #[cfg(test)]
    let already_noted = sabotage::claiming_noted_carriers()
        || crate::object::shapes::old_generation_carrier_already_noted(shape);
    #[cfg(not(test))]
    let already_noted = crate::object::shapes::old_generation_carrier_already_noted(shape);
    if !already_noted && !crate::arena::pointer_in_nursery(user_ptr as usize) {
        crate::object::shapes::note_old_generation_carrier(shape);
    }
    let keys_edge = crate::object::gc_shape_keys_edge_slot(shape);
    let prototype_edge = crate::object::gc_shape_prototype_edge_slot(shape, false);
    // Visit order of the generic walk: prefix (none for objects), keys edge,
    // prototype edge, meta, meta2 (none), payload.
    PlainObjectPlan {
        prefix: [
            keys_edge.unwrap_or(std::ptr::null_mut()),
            prototype_edge.unwrap_or(std::ptr::null_mut()),
            meta.unwrap_or(std::ptr::null_mut()),
        ],
        next_prefix: 0,
        payload: range,
        walk,
    }
}

impl CopyingNurseryCollector {
    /// Scan an ordinary object through its [`PlainObjectPlan`]. `false` means
    /// nothing was done and the caller must take the generic walk.
    #[inline(always)]
    pub(super) unsafe fn scan_plain_object(&mut self, header: *mut GcHeader) -> bool {
        if !plain_object_admissible(header) {
            return false;
        }
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        let mut changed = false;
        // Asked lazily by the generic walk, at its first slot; both are
        // properties of the header alone, so asking them up front is the same
        // answer.
        let weak = weak_holder_fact(header);
        let remembering = ParentRemembering::of(header, self.skip_remembering);
        // The generic walk's first edge, ahead of its kind arms: an explicit
        // prototype in the residual registry. For `GC_TYPE_OBJECT` the
        // per-owner half of the gate is conservatively `true`, so once the
        // process latch is armed every object asks.
        let mut residual_slots = 0usize;
        if crate::object::prototype_chain::object_static_prototypes_maybe_nonempty()
            && crate::object::prototype_chain::residual_prototype_owner_type(GC_TYPE_OBJECT)
            && crate::object::prototype_chain::residual_entry_possible_for(header)
        {
            crate::object::prototype_chain::visit_object_static_prototype_slot_mut(
                user_ptr,
                |slot| {
                    residual_slots += 1;
                    let before = *slot;
                    self.visit_side_slot(slot, header, weak, remembering);
                    changed |= *slot != before;
                },
            );
        }
        let mut plan = plain_object_plan(header);
        #[cfg(test)]
        let cross_check = !sabotage::cross_check_disabled();
        #[cfg(all(not(test), debug_assertions))]
        let cross_check = true;
        #[cfg(any(test, debug_assertions))]
        if cross_check {
            assert_matches_generic_walk(header, &plan, residual_slots);
        }
        let _ = residual_slots;
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
            self.visit_side_slot(slot, header, weak, remembering);
            changed |= *slot != before;
        });
        if changed {
            run_gc_rewrite_hook(GC_TYPE_OBJECT, user_ptr);
        }
        true
    }

    /// Side-table edges (residual prototype, overflow fields) are rare; keep
    /// their visit out of the hot loop's code.
    #[inline(never)]
    unsafe fn visit_side_slot(
        &mut self,
        slot: *mut u64,
        header: *mut GcHeader,
        weak: bool,
        remembering: ParentRemembering,
    ) {
        self.visit_slot_with_parent_facts(
            GcMutableSlot::new(slot, None),
            header,
            weak,
            remembering,
        );
    }
}

/// The drift check: the generic walk, run for its slot list only, must name
/// exactly the plan's slots followed by the overflow fields, after the
/// residual-prototype slots the scan already visited. Those are a stack
/// temporary of the registry's visitor, different on every call, so they are
/// matched by count, not address.
#[cfg(any(test, debug_assertions))]
unsafe fn assert_matches_generic_walk(
    header: *mut GcHeader,
    plan: &PlainObjectPlan,
    residual_slots: usize,
) {
    let mut generic = Vec::new();
    visit_gc_rewrite_slots(header, |slot| generic.push(slot.slot as usize));
    let mut replay = plan.clone();
    let mut expected = Vec::new();
    while let Some(slot) = replay.next_slot() {
        expected.push(slot as usize);
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
    crate::object::visit_overflow_field_slots_mut(user_ptr, |slot| expected.push(slot as usize));
    assert!(
        generic.len() == residual_slots + expected.len()
            && generic[residual_slots..] == expected[..],
        "copying_object_scan: the plain-object plan enumerated different slots than the \
         generic walk for header {header:p} — a traced edge would be lost or invented \
         (generic {generic:x?}, residual {residual_slots}, plan {expected:x?})"
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
        static PLAN_ATTEMPTS: Cell<u32> = const { Cell::new(0) };
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
        PLAN_ATTEMPTS.with(|c| c.set(c.get().wrapping_add(1)));
    }

    /// Objects that reached plan construction on this thread.
    pub(crate) fn plan_attempts() -> u32 {
        PLAN_ATTEMPTS.with(Cell::get)
    }

    /// Forget the highest payload slot — the one a limit mistake loses.
    pub(super) fn perturb(walk: super::PayloadWalk) -> super::PayloadWalk {
        if !DROP_TOP_PAYLOAD_SLOT.with(Cell::get) {
            return walk;
        }
        match walk {
            super::PayloadWalk::Word(word) if word != 0 => {
                super::PayloadWalk::Word(word & !(1u64 << (63 - word.leading_zeros())))
            }
            super::PayloadWalk::Range { next, count } if count > next => {
                super::PayloadWalk::Range {
                    next,
                    count: count - 1,
                }
            }
            other => other,
        }
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
