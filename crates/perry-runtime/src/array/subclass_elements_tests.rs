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
