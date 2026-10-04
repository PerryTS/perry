//! The [[Prototype]] as a shape fact (`shapes_prototype`).

use super::*;
use crate::object::prototype_chain::{
    object_link_class_default_prototype, object_set_user_prototype, object_static_prototype,
};
use crate::object::{js_object_alloc, ObjectHeader};

fn bits(obj: *mut ObjectHeader) -> u64 {
    crate::value::js_nanbox_pointer(obj as i64).to_bits()
}

#[test]
fn a_class_default_link_records_the_prototype_in_the_shape_only() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let proto = js_object_alloc(0, 0);
    // A function constructor's instance: a synthetic class id.
    let obj = js_object_alloc(crate::object::shapes::SYNTHETIC_CLASS_ID_BASE + 0x51, 0);
    object_link_class_default_prototype(obj as usize, bits(proto));
    let stamp = unsafe { super::super::object_shape_stamp(obj) };
    assert!(
        unsafe { (*obj).meta }.is_null(),
        "a class-default link allocates no per-instance record"
    );
    assert_eq!(shape_prototype_word(stamp), bits(proto));
    assert_eq!(object_static_prototype(obj as usize), Some(bits(proto)));
    assert!(proto_id_carries_word(
        super::super::shape_proto_id(stamp).unwrap()
    ));
}

/// The sabotage target: the prototype identity is part of the shape key, so
/// two receivers with the same keys and different prototypes never share a
/// ShapeId — and so never share a prototype word.
#[test]
fn receivers_with_different_prototypes_never_share_a_shape() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let p1 = js_object_alloc(0, 0);
    let p2 = js_object_alloc(0, 0);
    let a = js_object_alloc(0, 0);
    let b = js_object_alloc(0, 0);
    let c = js_object_alloc(0, 0);
    object_link_class_default_prototype(a as usize, bits(p1));
    object_link_class_default_prototype(b as usize, bits(p2));
    object_link_class_default_prototype(c as usize, bits(p1));
    let (sa, sb, sc) = unsafe {
        (
            super::super::object_shape_stamp(a),
            super::super::object_shape_stamp(b),
            super::super::object_shape_stamp(c),
        )
    };
    assert_ne!(sa, sb, "two prototypes, one ShapeId");
    assert_eq!(sa, sc, "one prototype, one predecessor: one ShapeId");
    assert_eq!(object_static_prototype(a as usize), Some(bits(p1)));
    assert_eq!(object_static_prototype(b as usize), Some(bits(p2)));
    assert_eq!(object_static_prototype(c as usize), Some(bits(p1)));
    // A later prototype change moves the receiver, never the shape's word.
    object_set_user_prototype(c as usize, bits(p2));
    assert_eq!(object_static_prototype(c as usize), Some(bits(p2)));
    assert_eq!(object_static_prototype(a as usize), Some(bits(p1)));
    assert_eq!(shape_prototype_word(sa), bits(p1));
}

#[test]
fn a_null_prototype_is_a_shape_fact() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let obj = js_object_alloc(0, 0);
    object_set_user_prototype(obj as usize, crate::value::TAG_NULL);
    let stamp = unsafe { super::super::object_shape_stamp(obj) };
    assert_eq!(
        super::super::shape_proto_id(stamp),
        Some(super::super::PROTO_ID_NULL)
    );
    assert_eq!(
        unsafe { crate::object::shapes::object_prototype_word(obj) },
        crate::value::TAG_NULL
    );
    assert_eq!(
        object_static_prototype(obj as usize),
        Some(crate::value::TAG_NULL)
    );
    // A link records no flag, so it allocates no meta record.
    assert!(unsafe { (*obj).meta }.is_null());
}

/// How the prototype was linked (`new F()` vs `Object.create(F.prototype)`)
/// is not observable in JS, so the two receivers must share one shape: the
/// identity names the same prototype, and nothing else about the link may
/// split them.
#[test]
fn new_f_and_object_create_of_its_prototype_share_a_shape() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let proto = js_object_alloc(0, 0);
    let created = crate::object::js_object_create(f64::from_bits(bits(proto)));
    let created = crate::value::JSValue::from_bits(created.to_bits()).as_pointer::<ObjectHeader>()
        as *mut ObjectHeader;
    let width = crate::object::shapes::shape_live_inline_slot_count_by_id(unsafe {
        super::super::object_shape_stamp(created)
    })
    .unwrap();
    let constructed = js_object_alloc(crate::object::shapes::SYNTHETIC_CLASS_ID_BASE + 0x53, width);
    object_link_class_default_prototype(constructed as usize, bits(proto));
    let (a, b) = unsafe {
        (
            super::super::object_shape_stamp(created),
            super::super::object_shape_stamp(constructed),
        )
    };
    assert_eq!(object_static_prototype(created as usize), Some(bits(proto)));
    assert_eq!(
        object_static_prototype(constructed as usize),
        Some(bits(proto))
    );
    assert_eq!(a, b, "one prototype, one empty layout: one ShapeId");
}
