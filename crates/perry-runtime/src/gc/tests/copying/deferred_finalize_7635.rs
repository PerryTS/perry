//! Object fields are traced by ShapeId rep, including JSON materialization
//! before and after a copying minor. Construction leaves object header
//! layout bits clear; two objects with one ShapeId agree on rep.

use super::*;

use crate::object::ObjectHeader;

const FIELD_VALUES: [&[u8]; 2] = [b"value_alpha", b"value_bravo"];

fn fresh_string(bytes: &[u8]) -> usize {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as usize
}

unsafe fn field_bits(obj: *mut ObjectHeader, index: usize) -> u64 {
    let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    *fields.add(index)
}

unsafe fn layout_state_of(user_ptr: usize) -> u16 {
    (*header_from_user_ptr(user_ptr as *const u8))._reserved & GC_LAYOUT_STATE_MASK
}

/// Addresses of the slots the collector says it will visit inside `user_ptr`.
/// ShapeId Any lanes contribute visited child slots; F64 lanes do not.
unsafe fn enumerated_slot_addrs(user_ptr: usize) -> Vec<usize> {
    test_heap_child_slots_for_user(user_ptr as *mut u8)
        .into_iter()
        .filter_map(|slot| match slot {
            HeapChildSlot::Child(p, _) => Some(p as usize),
            HeapChildSlot::PointerFreeRange(_) => None,
        })
        .collect()
}

unsafe fn field_slot_addr(obj: *mut ObjectHeader, index: usize) -> usize {
    let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    fields.add(index) as usize
}

/// Construct a record with deferred slot stores. ShapeId rep determines tracing.
unsafe fn materialise_record() -> *mut ObjectHeader {
    let obj = crate::object::js_object_alloc(0, FIELD_VALUES.len() as u32);
    assert_eq!(
        layout_state_of(obj as usize),
        GC_LAYOUT_UNKNOWN,
        "an object birth must leave header layout state clear"
    );
    let mut saw_pointer = false;
    for (index, bytes) in FIELD_VALUES.iter().enumerate() {
        let child = fresh_string(bytes);
        saw_pointer |=
            crate::object::store_object_field_slot_layout_deferred(obj, index, string_bits(child));
    }
    assert!(
        saw_pointer,
        "test premise: storing heap strings must be reported as pointer-bearing"
    );
    assert_eq!(layout_state_of(obj as usize), GC_LAYOUT_UNKNOWN);
    obj
}

/// What the collector enumerates is the shape's answer, not the finalize's.
/// A numeric record with `Any` lanes enumerates each of its fields (the tag
/// test rejects the numbers); the same record re-stamped with `F64` lanes
/// enumerates none, and its payload is one pointer-free range.
#[test]
fn the_shape_decides_what_a_record_enumerates() {
    let _guard = CopyingNurseryTestGuard::new(1);
    unsafe {
        let numeric = crate::object::js_object_alloc(0, 2);
        for index in 0..2usize {
            assert!(
                !crate::object::store_object_field_slot_layout_deferred(
                    numeric,
                    index,
                    crate::value::JSValue::number(index as f64 + 1.0).bits(),
                ),
                "a number store must not be reported as pointer-bearing"
            );
        }
        assert_eq!(
            test_heap_child_slot_count(numeric as *mut u8),
            2,
            "an `Any` lane is visited (and tag-tested) whatever the finalize said"
        );

        restamp_with_rep(numeric, f64_lanes(0..2));
        assert_eq!(
            test_heap_child_slot_count(numeric as *mut u8),
            0,
            "`F64` lanes are skipped: the collector enumerates zero payload slots"
        );
        assert!(
            test_heap_child_slots_for_user(numeric as *mut u8)
                .into_iter()
                .any(|slot| matches!(slot, HeapChildSlot::PointerFreeRange(_))),
            "an all-`F64` payload is one pointer-free range"
        );

        let pointered = materialise_record();
        assert_eq!(
            test_heap_child_slot_count(pointered as *mut u8),
            FIELD_VALUES.len(),
            "a record holding strings enumerates every field"
        );
    }
}

/// The positive arm. A record built exactly as the JSON materialiser builds it,
/// holding the ONLY reference to each of its children, must have every field
/// enumerated as a child edge and must survive a copying minor with both
/// children relocated and both slots rewritten.
#[test]
fn a_materialised_record_keeps_its_children_traced_and_rewritten_7635() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let obj = unsafe { materialise_record() };
    let before: Vec<usize> = (0..FIELD_VALUES.len())
        .map(|index| unsafe { (field_bits(obj, index) & POINTER_MASK) as usize })
        .collect();
    assert_eq!(
        test_heap_child_slot_count(obj as *mut u8),
        FIELD_VALUES.len(),
        "the collector must enumerate every pointer-bearing field of a \
         finalized record; a POINTER_FREE record enumerates ZERO"
    );

    // The record's slots are now the sole path to each child.
    js_shadow_slot_set(0, ptr_bits(obj as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.copied_objects >= FIELD_VALUES.len() + 1,
        "this test proves nothing unless the cycle actually MOVED the record \
         and both children (copied_objects = {})",
        trace.copying_nursery.copied_objects
    );

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize as *mut ObjectHeader;
    assert_ne!(moved as usize, obj as usize, "the record itself must move");
    unsafe {
        for (index, bytes) in FIELD_VALUES.iter().enumerate() {
            let child = (field_bits(moved, index) & POINTER_MASK) as usize;
            assert_ne!(
                child, before[index],
                "field {index} must have been relocated and its slot rewritten"
            );
            assert!(
                crate::arena::pointer_in_nursery(child) || crate::arena::pointer_in_old_gen(child),
                "field {index} must name a live heap object, not a stale address"
            );
            assert_string_bytes(child as *const crate::StringHeader, bytes);
        }
    }
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
}

/// The same invariant driven through the REAL entry point, `js_json_parse`, so
/// the finalize CALL SITE in `json/parser.rs` is covered and not merely the
/// helper it calls. #7635's sabotage was applied at that call site, and the two
/// tests above would stay green through it.
///
/// The parsed record holds the only reference to each of its string values —
/// only the KEYS are interned into the (rooted) parse-key cache — so a
/// misdeclared record strands them.
#[test]
fn json_parse_record_keeps_its_string_values_traced_and_rewritten_7635() {
    let _guard = CopyingNurseryTestGuard::new(1);

    // Values are 11 bytes, well above `SHORT_STRING_MAX_LEN`, so they are real
    // heap `StringHeader`s in the nursery — collectable and movable — rather
    // than inline short strings that no layout state could strand.
    let text = br#"{"alpha":"value_alpha","bravo":"value_bravo"}"#;
    let parsed = unsafe {
        crate::json::js_json_parse(crate::string::js_string_from_bytes(
            text.as_ptr(),
            text.len() as u32,
        ))
    };
    js_shadow_slot_set(0, parsed.bits());

    let obj = (js_shadow_slot_get(0) & POINTER_MASK) as usize as *mut ObjectHeader;
    let before: Vec<usize> = (0..FIELD_VALUES.len())
        .map(|index| unsafe { (field_bits(obj, index) & POINTER_MASK) as usize })
        .collect();
    unsafe {
        assert_eq!(
            layout_state_of(obj as usize),
            GC_LAYOUT_UNKNOWN,
            "JSON objects must carry no header layout state"
        );
        for (index, bytes) in FIELD_VALUES.iter().enumerate() {
            assert_eq!(
                field_bits(obj, index) & TAG_MASK,
                STRING_TAG,
                "test premise: field {index} must hold a HEAP string"
            );
            assert_string_bytes(before[index] as *const crate::StringHeader, bytes);
        }
        let enumerated = enumerated_slot_addrs(obj as usize);
        for index in 0..FIELD_VALUES.len() {
            assert!(
                enumerated.contains(&field_slot_addr(obj, index)),
                "the collector must enumerate parsed field {index} as a child \
                 edge; it reported {enumerated:?}"
            );
        }
    }

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.copied_objects >= FIELD_VALUES.len() + 1,
        "this test proves nothing unless the cycle actually MOVED the record \
         and both values (copied_objects = {})",
        trace.copying_nursery.copied_objects
    );

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize as *mut ObjectHeader;
    unsafe {
        for (index, bytes) in FIELD_VALUES.iter().enumerate() {
            let child = (field_bits(moved, index) & POINTER_MASK) as usize;
            assert_ne!(
                child, before[index],
                "parsed field {index} must have been relocated and its slot \
                 rewritten"
            );
            assert_string_bytes(child as *const crate::StringHeader, bytes);
        }
    }
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
}

/// Two independently allocated records with the same ShapeId must agree on
/// their Any-lane rep and enumerate every field.
#[test]
fn two_records_with_one_shape_id_trace_all_children() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let first = unsafe { materialise_record() };
    let second = unsafe { materialise_record() };

    unsafe {
        assert_eq!(
            layout_state_of(second as usize),
            GC_LAYOUT_UNKNOWN,
            "both object headers must leave layout state clear"
        );
        assert_eq!(
            crate::object::shapes::object_shape_stamp(first),
            crate::object::shapes::object_shape_stamp(second),
            "the two records must share a ShapeId"
        );
        for index in 0..FIELD_VALUES.len() {
            assert_eq!(
                field_bits(second, index) & TAG_MASK,
                STRING_TAG,
                "field {index} really does hold a heap string in both arms"
            );
        }
        assert_eq!(
            test_heap_child_slot_count(first as *mut u8),
            FIELD_VALUES.len()
        );
        assert_eq!(
            test_heap_child_slot_count(second as *mut u8),
            FIELD_VALUES.len(),
            "the shared shape must trace every Any field"
        );
    }
}
