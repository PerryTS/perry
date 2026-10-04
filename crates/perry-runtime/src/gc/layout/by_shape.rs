//! Charter step 5: the collector traces an object BY ITS SHAPE (DESIGN §3.1).
//! The shape record's `rep` word says which inline slots 0..32 are `F64` (or
//! deprecated `F64`, the same invariant): those hold a JS Number as raw bits
//! and are skipped. Every other slot, every slot at or past 32 and every spill
//! slot is `Any` and gets the exact tag test of the unknown walk. No
//! per-object mask, no intact bit, no shape-layout map is read.
//!
//! A debug build verifies every selection it hands the collector: a skipped
//! slot that holds a heap pointer is an `F64` lane holding a pointer, i.e. a
//! writer that stored past the field-representation store check, and aborts.

#[cfg(any(debug_assertions, test))]
use super::layout_pointer_bearing_bits;
use super::{HeapPayloadSlotSelection, HeapSlotRange, LayoutSlotMask};
use crate::object::field_rep;
use crate::object::shapes::ShapeRecordRef;

/// Slots at or past this many payload slots are all `Any` and traced by the
/// unknown walk, which is exact; the mask is only an accelerator.
const INLINE_MASK_SLOTS: usize = 64;

/// The payload selection for an object traced by its shape.
#[inline]
pub(super) fn selection_by_shape(
    shape: Option<ShapeRecordRef>,
    payload: HeapSlotRange,
) -> HeapPayloadSlotSelection {
    selection_for_rep_with_special(
        shape.map_or(0, |record| record.rep()),
        shape.map_or(0, |record| record.special_constfn_mask()),
        payload,
    )
}

/// The payload selection for an object whose shape carries `rep`: every
/// non-`Any` lane is skipped, everything else gets the tag test.
#[inline]
#[cfg(test)]
fn selection_for_rep(rep: u64, payload: HeapSlotRange) -> HeapPayloadSlotSelection {
    selection_for_rep_with_special(rep, 0, payload)
}

#[inline]
fn selection_for_rep_with_special(
    rep: u64,
    special_constfn_mask: u32,
    payload: HeapSlotRange,
) -> HeapPayloadSlotSelection {
    let slot_count = payload.slot_count();
    if slot_count == 0 {
        return HeapPayloadSlotSelection::Empty;
    }
    let skip = field_rep::non_pointer_slot_bits(rep, special_constfn_mask);
    if skip == 0 || slot_count > INLINE_MASK_SLOTS {
        return HeapPayloadSlotSelection::All { cursor: 0 };
    }
    let live = if slot_count == 64 {
        u64::MAX
    } else {
        (1u64 << slot_count) - 1
    };
    let select = live & !(skip as u64);
    // The skipped F64 lanes are the raw-numeric object fields the layout-scan
    // telemetry counts.
    let raw_numeric_object_slots = (live & skip as u64).count_ones() as usize;
    if select == 0 {
        return HeapPayloadSlotSelection::PointerFree {
            emitted: false,
            raw_numeric_array: false,
            raw_numeric_object_slots,
        };
    }
    HeapPayloadSlotSelection::Masked {
        mask: LayoutSlotMask::Inline(select),
        cursor: 0,
        raw_numeric_object_slots,
        raw_numeric_recorded: false,
    }
}

/// Does `selection` visit payload slot `index`?
#[cfg(any(debug_assertions, test))]
fn selection_visits(selection: &HeapPayloadSlotSelection, index: usize) -> bool {
    match selection {
        HeapPayloadSlotSelection::Empty | HeapPayloadSlotSelection::PointerFree { .. } => false,
        HeapPayloadSlotSelection::All { .. } => true,
        HeapPayloadSlotSelection::Masked { mask, .. } => match mask {
            LayoutSlotMask::AllPointers => true,
            LayoutSlotMask::Inline(bits) => index < 64 && bits & (1u64 << index) != 0,
            LayoutSlotMask::Heap(words) => words
                .get(index / 64)
                .is_some_and(|word| word & (1u64 << (index % 64)) != 0),
        },
    }
}

/// The first payload slot `selection` skips although it holds a heap pointer
/// (index, bits). A canonical double whose raw bits fall in the untagged
/// pointer range (a subnormal) is a Number, so skipping it is right.
#[cfg(any(debug_assertions, test))]
pub(super) unsafe fn skipped_pointer_slot(
    payload: HeapSlotRange,
    selection: &HeapPayloadSlotSelection,
) -> Option<(usize, u64)> {
    (0..payload.slot_count()).find_map(|index| {
        if selection_visits(selection, index) {
            return None;
        }
        let bits = *payload.slot(index);
        (layout_pointer_bearing_bits(bits) && field_rep::f64_slot_bits(bits) != Some(bits))
            .then_some((index, bits))
    })
}

/// Debug verify of the shape walk: an `F64` lane holding a pointer would be
/// lost by the collector, so a debug build stops at the first one.
#[cfg(debug_assertions)]
pub(super) unsafe fn debug_verify(
    header: *mut crate::gc::GcHeader,
    payload: HeapSlotRange,
    shape: Option<ShapeRecordRef>,
    selection: &HeapPayloadSlotSelection,
) {
    if let Some((index, bits)) = skipped_pointer_slot(payload, selection) {
        let obj = (header as *mut u8).add(crate::gc::GC_HEADER_SIZE)
            as *const crate::object::ObjectHeader;
        eprintln!(
            "GC by shape: F64 lane holds a pointer (writer gap): obj {obj:p} shape {:#x} rep {:#x} slot {index} bits {bits:#018x} class {}",
            crate::object::shapes::object_shape_stamp(obj),
            shape.map_or(0, |record| record.rep()),
            (*obj).class_id,
        );
        std::process::abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{GcHeader, GC_HEADER_SIZE};

    #[test]
    fn f64_lanes_are_skipped_and_everything_else_is_traced() {
        let mut slots = [0u64; 40];
        let payload = HeapSlotRange::new(slots.as_mut_ptr(), 3);
        // No record: every slot is Any, the exact unknown walk.
        assert!(matches!(
            selection_by_shape(None, payload),
            HeapPayloadSlotSelection::All { .. }
        ));
        let rep = field_rep::with_slot_rep(
            field_rep::with_slot_rep(0, 0, field_rep::REP_F64),
            2,
            field_rep::REP_F64_DEPRECATED,
        );
        assert_eq!(field_rep::non_any_slot_bits(rep), 0b101);
        let wide = HeapSlotRange::new(slots.as_mut_ptr(), 40);
        let live = (1u64 << 40) - 1;
        let expect = live & !0b101;
        let sel = selection_for_rep(rep, wide);
        assert!(
            matches!(sel, HeapPayloadSlotSelection::Masked { mask: LayoutSlotMask::Inline(m), .. } if m == expect)
        );
        // Every lane F64 on a 3-slot object: nothing to trace.
        let all = field_rep::with_slot_rep(rep, 1, field_rep::REP_F64);
        assert!(matches!(
            selection_for_rep(all, payload),
            HeapPayloadSlotSelection::PointerFree { .. }
        ));
    }

    #[test]
    fn constfn_special_lane_is_traced_but_optional_nopointer_is_skipped() {
        let mut slots = [0u64; 4];
        let payload = HeapSlotRange::new(slots.as_mut_ptr(), slots.len());
        let rep = field_rep::with_slot_rep(
            field_rep::with_slot_rep(0, 0, field_rep::REP_SPECIAL),
            1,
            field_rep::REP_SPECIAL,
        );
        // Slot 0 is a current closure; slot 1 demonstrates the reserved
        // NoPointer interpretation if P5 is accepted. No producer exists yet.
        let selection = selection_for_rep_with_special(rep, 0b01, payload);
        assert!(selection_visits(&selection, 0));
        assert!(!selection_visits(&selection, 1));
    }

    /// `{n: 1.5, s: "txt"}` and its shape; the key-adds earn `n` an `F64`
    /// lane and leave `s` `Any` (P2b).
    unsafe fn number_and_string() -> (*mut crate::object::ObjectHeader, u32) {
        let key =
            |name: &str| crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let obj = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(obj, key("byshape_n"), 1.5);
        let text = crate::string::js_string_from_bytes(b"txt".as_ptr(), 3);
        let text = f64::from_bits(crate::JSValue::string_ptr(text).bits());
        crate::object::js_object_set_field_by_name(obj, key("byshape_s"), text);
        (obj, crate::object::shapes::object_shape_stamp(obj))
    }

    unsafe fn header_of(obj: *mut crate::object::ObjectHeader) -> *mut GcHeader {
        (obj as *mut u8).sub(GC_HEADER_SIZE) as *mut GcHeader
    }

    /// The collector's own entry (`gc_child_slots`) skips the `F64` lane and
    /// traces the string. The sabotage stamps an `F64` lane over the string
    /// pointer (a writer that bypassed the store check): the debug verify must
    /// find it, or it could not fail.
    #[test]
    fn the_collector_walk_skips_f64_lanes_and_the_verify_can_fail() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let (obj, id) = number_and_string();
            let rep = crate::object::shapes::shape_descriptor_by_id(id)
                .expect("shape")
                .rep;
            assert_eq!(
                field_rep::non_any_slot_bits(rep) & 0b11,
                0b01,
                "premise: n F64, s Any"
            );

            let slots = super::super::gc_child_slots(header_of(obj));
            let masked = matches!(
                slots.selection,
                HeapPayloadSlotSelection::Masked { mask: LayoutSlotMask::Inline(m), .. } if m & 0b11 == 0b10
            );
            let fields = (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>());
            let payload = HeapSlotRange::new(fields as *mut u64, 2);
            let correct =
                selection_by_shape(crate::object::shapes::object_shape_record(obj), payload);
            let correct_ok = skipped_pointer_slot(payload, &correct).is_none();

            // Sabotage: the same keys with BOTH lanes F64, stamped on the object.
            let d = crate::object::shapes::shape_descriptor_by_id(id).expect("shape");
            let wrong = crate::object::shapes::publish_shape_result(
                crate::object::shapes::shape_descriptor_ensure_with_rep(
                    d.keys as usize as *const crate::array::ArrayHeader,
                    d.logical_key_count,
                    d.live_inline_slot_count,
                    d.semantic_generation,
                    d.object_kind,
                    d.hole_count,
                    d.proto_id,
                    crate::object::shapes::ReceiverFacts::of_descriptor(&d, d.summary),
                    field_rep::with_slot_rep(d.rep, 1, field_rep::REP_F64),
                    None,
                ),
            );
            // Straight to the selection and its verify: the collector entry
            // would stop earlier, at the debug invariant in `gc_field_slot_range`.
            crate::object::shapes::stamp_object_shape_id_with_carrier_note(obj, wrong);
            let record = crate::object::shapes::object_shape_record(obj);
            let sabotaged = selection_by_shape(record, payload);
            let found = skipped_pointer_slot(payload, &sabotaged);
            crate::object::shapes::stamp_object_shape_id_with_carrier_note(obj, id);

            assert!(masked, "By shape, only the Any lane is traced");
            assert!(correct_ok, "a correct object: no pointer is skipped");
            assert_eq!(
                found.map(|(index, _)| index),
                Some(1),
                "an F64 lane over a pointer is found"
            );
        }
    }
}
