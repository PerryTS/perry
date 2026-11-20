//! A stamp clears the scalar payload when it leaves the numeric-proof shape.

use super::subclass::js_packed_arraylike_loop_guard;

#[test]
fn a_stamp_retires_the_live_numeric_proof_payload() {
    use crate::object::shapes;
    let _lock = crate::gc::global_side_table_test_lock();
    let _representation =
        super::subclass_elements::ArraySubclassRepresentationGuard::shape_carried();
    let _no_collect = crate::gc::GcSuppressScope::new();
    const CLASS: u32 = 0x0074_86F4;
    crate::object::js_register_class_parent(CLASS, 0xFFFF_0024);
    let obj = crate::object::js_object_alloc(CLASS, 2);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    crate::node_stream::js_array_subclass_init(receiver, 0.0);
    for (i, value) in [11.0, 22.0, 33.0].into_iter().enumerate() {
        crate::object::js_object_set_index_polymorphic(obj as i64, i as f64, value);
    }
    unsafe {
        let unproven = shapes::object_shape_stamp(obj);
        let mut facts = [0u64; 7];
        assert_eq!(
            js_packed_arraylike_loop_guard(receiver, 3.0, 1, facts.as_mut_ptr()),
            2,
            "premise: a live numeric range published its proof"
        );
        assert!(shapes::store_kind::receiver_carries_numeric_proof(obj));
        let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj));
        assert!(!meta.is_null());
        assert_eq!(
            (*meta).flags >> 32,
            u64::from(unproven.0),
            "the payload names its twin"
        );
        // Re-publishing the proof retains its payload; leaving it must clear it.
        let proven = shapes::object_shape_stamp(obj);
        shapes::stamp_object_shape_id_with_carrier_note(obj, proven);
        assert_eq!((*meta).flags >> 32, u64::from(unproven.0));
        shapes::stamp_object_shape_id_with_carrier_note(obj, unproven);
        assert_eq!(
            (*meta).flags >> 32,
            0,
            "the retired twin payload must be cleared"
        );
        assert!(!shapes::store_kind::receiver_carries_numeric_proof(obj));
    }
}
