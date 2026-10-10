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

#[test]
fn s7b_class_identity_publishes_and_replaces_its_holder_word() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let declared = 190_708;
    unsafe { crate::object::js_register_class_name(declared, b"".as_ptr(), 0) };
    let declared_pid = super::super::PROTO_ID_CLASS | u64::from(declared);
    assert_eq!(
        identity_prototype_word(declared_pid),
        crate::value::TAG_UNDEFINED
    );
    let declared_holder = crate::object::class_decl_prototype_value(declared).to_bits();
    assert_eq!(
        declared_holder & crate::value::TAG_MASK,
        crate::value::POINTER_TAG
    );
    assert_eq!(identity_prototype_word(declared_pid), declared_holder);
    let cid = 190_702;
    let pid = super::super::PROTO_ID_CLASS | u64::from(cid);
    let recv = js_object_alloc(cid, 0);
    let stamp = unsafe { super::super::object_shape_stamp(recv) };
    assert_eq!(shape_prototype_word(stamp), 0);
    let first = js_object_alloc(0, 0);
    crate::object::class_registry::class_prototype_object_root_store(cid, first);
    assert_eq!(shape_prototype_word(stamp), bits(first));
    let second = js_object_alloc(0, 0);
    crate::object::class_registry::class_prototype_object_root_store(cid, second);
    assert_eq!(shape_prototype_word(stamp), bits(second));
    assert_eq!(identity_prototype_word(pid), bits(second));
    assert_eq!(unsafe { super::super::object_shape_stamp(recv) }, stamp);
}

#[test]
fn s7b_constructor_heritage_is_not_an_instance_holder_word() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 190_705;
    let parent = js_object_alloc(190_706, 0);
    crate::object::js_object_mark_class(parent as i64);
    crate::object::class_registry::class_prototype_object_root_store(cid, parent);
    assert_eq!(
        identity_prototype_word(super::super::class_proto_id(cid)),
        0
    );
    assert!(crate::object::class_holder_prototype(cid).is_null());
}

#[test]
fn c3_identity_cell_is_stable_across_directory_growth_and_gc_rewrite() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let proto = js_object_alloc(0, 0);
    let receiver = js_object_alloc(0, 0);
    object_link_class_default_prototype(receiver as usize, bits(proto));
    let stamp = unsafe { super::super::object_shape_stamp(receiver) };
    let record = super::super::ShapeSlab::agent_record(stamp);
    let cell = unsafe { (*record).proto_cell as usize as *mut u64 };
    assert!(!cell.is_null());
    assert_eq!(unsafe { *cell }, bits(proto));
    let pid = unsafe { (*record).proto_id };
    // Force the directory to grow after this page's address was borrowed.
    super::write_identity_word(1 << 18, crate::value::TAG_UNDEFINED);
    assert_eq!(identity_word_slot(pid), Some(cell));
    let replacement = js_object_alloc(0, 0);
    // The collector's forwarding visitor writes this exact existing slot.
    unsafe { *cell = bits(replacement) };
    assert_eq!(shape_prototype_word(stamp), bits(replacement));
    super::write_identity_word(pid, bits(proto));
    let before = shape_prototype_word(stamp);
    prune_dead_shape_prototypes(&|addr| addr == proto as usize);
    assert_eq!(unsafe { *cell }, 0);
    assert_eq!(shape_prototype_word(stamp), 0);
    // Negative control: an answer copied out of the cell before pruning is
    // stale. A live cell read must distinguish the two.
    assert_ne!(
        before,
        shape_prototype_word(stamp),
        "negative control: skipped live cell load"
    );
}

#[test]
fn c3_unrooted_prototype_collection_clears_the_stable_cell() {
    let _lock = crate::gc::global_side_table_test_lock();
    let cell = {
        let _no_move = crate::gc::GcSuppressScope::new();
        let proto = js_object_alloc(0, 0);
        let receiver = js_object_alloc(0, 0);
        object_link_class_default_prototype(receiver as usize, bits(proto));
        let stamp = unsafe { super::super::object_shape_stamp(receiver) };
        let record = super::super::ShapeSlab::agent_record(stamp);
        let cell = unsafe { (*record).proto_cell as usize as *const u64 };
        assert_eq!(unsafe { *cell }, bits(proto));
        cell
    };
    // Native locals are not registered shadow roots. Both carriers are dead;
    // the identity page remains stable after their slab records are pruned.
    crate::gc::js_gc_collect();
    assert_eq!(
        unsafe { *cell },
        0,
        "dead prototype word survived a full collection"
    );
}

#[test]
fn c3_holder_hit_matches_generic_get_across_shape_link_mutations() {
    use crate::object::method_site::read_holder::{prime_read_holder, read_holder_hit};
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "c3_holder_hit_matches_generic_get_across_shape_link_mutations",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let first = js_object_alloc(0, 1);
    let second = js_object_alloc(0, 1);
    let recv = js_object_alloc(0, 1);
    let key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(first, key, 13.0);
    crate::object::js_object_set_field_by_name(second, key, 29.0);
    object_link_class_default_prototype(recv as usize, bits(first));
    let mut slot: crate::object::PicCacheSlot = std::ptr::null_mut();
    unsafe {
        assert_eq!(
            prime_read_holder(recv, key, &mut slot).unwrap().as_number(),
            13.0
        );
        assert_eq!(
            read_holder_hit(recv, &mut slot),
            Some(13.0),
            "fixture must hit"
        );
        for stage in 0..7 {
            match stage {
                0 => crate::object::js_object_set_field_by_name(first, key, 17.0),
                1 => crate::object::js_object_set_field_by_name(recv, key, 19.0),
                2 => {
                    crate::object::js_object_delete_field(recv, key);
                }
                3 => object_set_user_prototype(recv as usize, bits(second)),
                4 => {
                    crate::object::js_object_delete_field(second, key);
                }
                5 => object_set_user_prototype(recv as usize, crate::value::TAG_NULL),
                _ => object_set_user_prototype(recv as usize, bits(first)),
            }
            let generic = crate::object::js_object_get_field_by_name(recv, key).bits();
            if let Some(hit) = read_holder_hit(recv, &mut slot) {
                assert_eq!(hit.to_bits(), generic, "stale hit at stage {stage}");
            }
            let answer =
                crate::object::js_object_get_field_ic(recv as i64, key, 190_731, &mut slot);
            assert_eq!(answer.to_bits(), generic, "Get mismatch at stage {stage}");
        }
    }
}
