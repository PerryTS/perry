//! The `ObjectMeta.elements` edge of an Array-subclass instance is a traced
//! child exactly like `spill`: it must survive owner and meta evacuation, be
//! rewritten to the moved inner array, and keep the inner array alive.
use super::subclass_elements::{elements_of, install_elements, set_elements_head};
use crate::object::{js_object_alloc, ObjectHeader};

const CLASS_ID_ARRAY: u32 = 0xFFFF_0024;

fn live_obj(receiver: f64) -> *mut ObjectHeader {
    (receiver.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader
}

#[test]
fn the_elements_edge_survives_moving_gc_and_keeps_the_inner_array_alive() {
    let _copying_nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force_evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);

    let class_id = 0x0074_8695;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));

    // `super(3)`: three holes, `length` 3, no shape-carried `length`.
    unsafe { install_elements(live_obj(receiver_h.get_nanbox_f64()), 3) };
    let before_elements = unsafe { elements_of(live_obj(receiver_h.get_nanbox_f64())) };
    assert!(!before_elements.is_null());
    assert_eq!(unsafe { (*before_elements).length }, 3);
    // Idempotent: a second install keeps the store.
    unsafe { install_elements(live_obj(receiver_h.get_nanbox_f64()), 9) };
    assert_eq!(
        unsafe { elements_of(live_obj(receiver_h.get_nanbox_f64())) },
        before_elements
    );

    // An append past the exact capacity re-allocates the inner array; the
    // head is written back through the barriered meta slot.
    let grown = crate::array::js_array_push_f64(before_elements, 44.0);
    unsafe { set_elements_head(live_obj(receiver_h.get_nanbox_f64()), grown) };
    crate::array::js_array_set_f64(grown, 0, 11.0);
    assert_eq!(unsafe { (*grown).length }, 4);

    let before_cycles = crate::gc::copying_minor_cycles();
    let _ = crate::gc::gc_collect_minor();
    assert!(crate::gc::copying_minor_cycles() > before_cycles);

    let live = live_obj(receiver_h.get_nanbox_f64());
    let elements = unsafe { elements_of(live) };
    assert!(
        !elements.is_null(),
        "the edge must be rewritten, not dropped"
    );
    assert_ne!(
        elements, grown,
        "forced evacuation must have moved the inner array"
    );
    let header = unsafe { crate::value::addr_class::try_read_gc_header(elements as usize) }
        .expect("the moved inner array is a live heap object");
    assert_eq!(header.obj_type, crate::gc::GC_TYPE_ARRAY);
    assert_eq!(header.gc_flags & crate::gc::GC_FLAG_FORWARDED, 0);
    assert_eq!(unsafe { (*elements).length }, 4);
    assert_eq!(crate::array::js_array_get_f64(elements, 0), 11.0);
    assert_eq!(crate::array::js_array_get_f64(elements, 3), 44.0);
    // The untouched indices are still absent (a hole, or `undefined` once the
    // read resolves it through the prototype chain), never a stale value.
    let hole = crate::array::js_array_get_f64(elements, 1).to_bits();
    assert!(
        hole == crate::value::TAG_HOLE || hole == crate::value::TAG_UNDEFINED,
        "index 1 must still be absent: {hole:#x}"
    );
}

/// The hot runtime entries route an elements-backed instance to its inner
/// array: `length`, `[i]` get/set, append (including the re-allocating one,
/// with the owner rooted across it) and pop — through both the value-taking
/// `array_subclass_fast_*` entries and the raw `js_array_*` entries the
/// codegen fallbacks call, and across a forced-evacuation minor GC.
#[test]
fn hot_entries_route_to_the_elements_store() {
    use super::subclass::{
        array_subclass_fast_index_get, array_subclass_fast_index_set, array_subclass_fast_length,
        array_subclass_fast_pop, array_subclass_fast_push_one,
    };
    let _copying_nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force_evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);

    let class_id = 0x0074_8696;
    crate::object::js_register_class_parent(class_id, CLASS_ID_ARRAY);
    let obj = js_object_alloc(class_id, 2);
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
    unsafe { install_elements(live_obj(receiver_h.get_nanbox_f64()), 0) };
    let recv = || receiver_h.get_nanbox_f64();
    let as_arr = || (recv().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut crate::array::ArrayHeader;

    assert_eq!(array_subclass_fast_length(recv()), Some(0.0));
    assert_eq!(array_subclass_fast_index_get(recv(), 0), None);
    // 40 appends from an exact-capacity-0 store: several re-allocations.
    for i in 0..40u32 {
        assert_eq!(
            array_subclass_fast_push_one(recv(), f64::from(i)),
            Some(f64::from(i + 1))
        );
        if i == 17 {
            let _ = crate::gc::gc_collect_minor();
        }
    }
    assert_eq!(array_subclass_fast_length(recv()), Some(40.0));
    for i in 0..40u32 {
        assert_eq!(array_subclass_fast_index_get(recv(), i), Some(f64::from(i)));
    }
    assert_eq!(array_subclass_fast_index_get(recv(), 40), None);
    // In-bounds write, appending write, hole-creating write (declined).
    assert!(array_subclass_fast_index_set(recv(), 3, 300.0));
    assert_eq!(array_subclass_fast_index_get(recv(), 3), Some(300.0));
    assert!(array_subclass_fast_index_set(recv(), 40, 400.0));
    assert_eq!(array_subclass_fast_length(recv()), Some(41.0));
    assert!(!array_subclass_fast_index_set(recv(), 50, 500.0));
    assert_eq!(array_subclass_fast_length(recv()), Some(41.0));
    // Pop through the value entry and through the raw `js_array_*` entries
    // the codegen fallbacks call with the object address as an ArrayHeader.
    assert_eq!(array_subclass_fast_pop(recv()), Some(400.0));
    assert_eq!(crate::array::js_array_pop_f64(as_arr()), 39.0);
    assert_eq!(crate::array::js_array_length(as_arr()), 39);
    let _ = crate::array::js_array_push_f64(as_arr(), 77.0);
    assert_eq!(crate::array::js_array_length(as_arr()), 40);
    assert_eq!(crate::array::js_array_get_f64(as_arr(), 39), 77.0);
    assert_eq!(array_subclass_fast_index_get(recv(), 39), Some(77.0));
    let _ = crate::gc::gc_collect_minor();
    assert_eq!(array_subclass_fast_length(recv()), Some(40.0));
    assert_eq!(array_subclass_fast_index_get(recv(), 3), Some(300.0));
    assert_eq!(crate::array::js_array_get_f64(as_arr(), 39), 77.0);
    // The shape-carried machinery never learned anything for this instance.
    assert_eq!(
        unsafe { (*(*live_obj(recv())).meta).array_subclass_dense_key },
        0
    );
}
