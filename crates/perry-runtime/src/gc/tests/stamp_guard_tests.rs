//! The stamp funnel keeps admission, proof retirement and carrier publication live.

use super::super::*;
use super::support::{collect_minor_trace, ptr_bits, CopyingNurseryTestGuard};
use crate::object::shapes;

#[test]
fn locked_prototype_stamp_refuses_before_the_header_changes() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_collect = crate::gc::GcSuppressScope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 2);
        let donor = crate::object::js_object_alloc(0, 2);
        let key = crate::string::js_string_from_bytes(b"stamp_guard".as_ptr(), 11);
        let added = crate::string::js_string_from_bytes(b"stamp_added".as_ptr(), 11);
        for obj in [proto, donor] {
            crate::object::js_object_set_field_by_name(obj, key, 1.0);
        }
        crate::object::js_object_set_field_by_name(donor, added, 2.0);
        let target = shapes::object_shape_stamp(donor);
        crate::object::builtin_proto::seal_object(proto, perry_abi::BuiltinProto::Object);
        assert!(crate::object::proto_validity::object_is_marked_prototype(
            proto as usize
        ));
        let before = (*proto).parent_class_id;
        assert_ne!(before, target.0, "premise: the stamp changes the key set");
        let result = crate::exception::catch_js_throw(|| {
            shapes::stamp_object_shape_id_with_carrier_note(proto, target);
        });
        assert!(
            result.is_err(),
            "the locked prototype must refuse the stamp"
        );
        assert_eq!(
            (*proto).parent_class_id,
            before,
            "refusal precedes publication"
        );
        assert_eq!(crate::object::object_keys(proto).count(), 1);
    }
}

#[test]
fn an_old_stamp_keeps_its_young_keys_alive_through_a_copying_minor() {
    let _guard = CopyingNurseryTestGuard::new(2);
    // Unit fixtures register the metadata root scanner that production GC init installs.
    gc_register_mutable_root_scanner(shapes::scan_shape_table_rekey_mut);
    let before_keys;
    unsafe {
        let _no_collect = crate::gc::GcSuppressScope::new();
        let old = crate::object::object_alloc_prototype_old(0, 2);
        let young = crate::object::js_object_alloc(0, 2);
        let keys = crate::array::js_array_alloc(1);
        let key = crate::string::js_string_from_bytes(b"carrier_stamp".as_ptr(), 13);
        crate::array::js_array_push_f64(keys, crate::value::js_nanbox_string(key as i64));
        assert!(crate::arena::pointer_in_nursery(keys as usize));
        assert!(crate::arena::pointer_in_nursery(young as usize));
        assert!(!crate::arena::pointer_in_nursery(old as usize));
        shapes::stamp_object_shape(young, keys, 1, 1);
        let id = shapes::object_shape_stamp(young);
        assert!(!shapes::object_shape_descriptor(young).unwrap().old_carrier);
        shapes::stamp_object_shape_id_with_carrier_note(old, id);
        let descriptor = shapes::object_shape_descriptor(old).unwrap();
        assert!(
            descriptor.old_carrier,
            "the old receiver must publish its carrier duty"
        );
        before_keys = descriptor.keys;
        js_shadow_slot_set(0, ptr_bits(old as usize));
        js_shadow_slot_set(1, ptr_bits(crate::object::js_object_alloc(0, 0) as usize));
    }
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.copied_objects > 0,
        "the minor must actually copy"
    );
    let old = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    let after = unsafe { shapes::object_shape_descriptor(old) }
        .expect("the live carrier retains its shape");
    assert_ne!(
        after.keys, before_keys,
        "the descriptor must rewrite its young keys"
    );
    assert_eq!(after.logical_key_count, 1);
    assert_eq!(
        unsafe { (*(after.keys as *const crate::array::ArrayHeader)).length },
        1
    );
}
