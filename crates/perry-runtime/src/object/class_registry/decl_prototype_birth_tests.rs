//! A declared class's prototype is born in its final shape, linked from the
//! class's function object, and found again from either end in O(1).

use super::*;

extern "C" fn entry_a(
    _: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    1.0
}

extern "C" fn entry_b(
    _: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    2.0
}

extern "C" fn method_body(_this: f64) -> f64 {
    0.0
}

fn register(cid: u32, name: &[u8]) {
    {
        let mut guard = crate::object::REGISTERED_CLASS_IDS.write().unwrap();
        guard
            .get_or_insert_with(crate::fast_hash::new_ptr_hash_set)
            .insert(cid);
    }
    unsafe {
        crate::object::js_register_class_name(cid, name.as_ptr(), name.len() as u32);
        crate::object::js_register_class_length(cid, 0);
    }
}

unsafe fn register_method(cid: u32, name: &[u8], entry: Option<usize>) {
    super::js_register_class_method_with_entry(
        cid as i64,
        name.as_ptr(),
        name.len() as i64,
        method_body as *const () as usize as i64,
        0,
        0,
        0,
        entry.unwrap_or(0) as i64,
    );
}

fn infos() -> (usize, usize) {
    (
        crate::fn_info!(entry_a, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE))
            as *const crate::closure::JsFunctionInfo as usize,
        crate::fn_info!(entry_b, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE))
            as *const crate::closure::JsFunctionInfo as usize,
    )
}

fn born_final_builds() -> u32 {
    super::decl_prototype_birth::BORN_FINAL_BUILDS.with(std::cell::Cell::get)
}

fn object_of(value: f64) -> *mut ObjectHeader {
    let v = crate::value::JSValue::from_bits(value.to_bits());
    assert!(v.is_pointer(), "a prototype object");
    v.as_pointer::<ObjectHeader>() as *mut ObjectHeader
}

unsafe fn slot(obj: *const ObjectHeader, i: usize) -> u64 {
    std::ptr::read(
        ((obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64).add(i),
    )
}

#[test]
fn a_declared_prototype_is_born_in_its_final_linked_shape() {
    let cid = 0x6E41;
    register(cid, b"Born");
    let (a, b) = infos();
    unsafe {
        register_method(cid, b"a", Some(a));
        register_method(cid, b"b", Some(b));
    }
    let before = born_final_builds();
    let proto = object_of(class_decl_prototype_value(cid));
    assert_eq!(
        born_final_builds(),
        before + 1,
        "the born-final path built it"
    );

    // Linked from the class, found from the object, built once.
    assert_eq!(class_decl_prototype_object(cid), proto);
    assert_eq!(
        class_id_for_decl_prototype_object(proto as usize),
        Some(cid)
    );
    assert_eq!(object_of(class_decl_prototype_value(cid)), proto);
    assert_eq!(
        born_final_builds(),
        before + 1,
        "a second demand builds nothing"
    );

    unsafe {
        let d = crate::object::shapes::object_shape_descriptor(proto).expect("shaped");
        assert_eq!(d.logical_key_count, 3);
        assert_eq!(d.live_inline_slot_count, 3);
        assert_eq!(d.hole_count, 0);
        let keys = d.keys_view();
        for (pos, name) in [b"constructor".as_slice(), b"a", b"b"].iter().enumerate() {
            assert!(
                crate::string::js_string_key_matches_bytes(keys.get(pos as u32), name),
                "key {pos} in ClassBody order"
            );
            assert_eq!(
                crate::object::key_attrs::keys_entry(
                    d.keys as usize as *const crate::ArrayHeader,
                    pos as u32
                ),
                crate::object::key_attrs::ENTRY_NON_ENUMERABLE,
                "key {pos} is {{ writable, !enumerable, configurable }}"
            );
        }
        // Slot 0 is the class itself; each method slot names its own body.
        assert_eq!(
            slot(proto, 0),
            crate::object::class_constructor_ref_value(cid).to_bits()
        );
        let record = crate::object::shapes::shape_record_by_id(
            crate::object::shapes::object_shape_stamp(proto),
        )
        .expect("record");
        assert_eq!(record.constfn_info(0), None);
        assert_eq!(record.constfn_info(1), Some(a as u64));
        assert_eq!(record.constfn_info(2), Some(b as u64));
        assert_eq!(
            slot(proto, 1),
            crate::object::class_prototype_method_value_for_name(cid, "a").to_bits()
        );
        // Linked to Object.prototype through the shape's identity.
        assert_eq!(
            crate::object::shapes::object_prototype_word(proto),
            crate::object::object_prototype_intrinsic_bits().expect("Object.prototype")
        );
        assert_eq!(
            crate::object::shapes::object_proto_id(proto),
            d.proto_id,
            "the shape names the link the object records"
        );
    }
}

#[test]
fn a_subclass_prototype_links_its_parents() {
    let (pa, pb) = (0x6E42, 0x6E43);
    register(pa, b"Base");
    register(pb, b"Derived");
    let (a, b) = infos();
    unsafe {
        register_method(pa, b"a", Some(a));
        register_method(pb, b"b", Some(b));
        crate::object::js_register_class_parent(pb, pa);
    }
    let before = born_final_builds();
    let derived = object_of(class_decl_prototype_value(pb));
    assert_eq!(
        born_final_builds(),
        before + 2,
        "both born final, parent first"
    );
    let base = class_decl_prototype_object(pa);
    assert!(!base.is_null());
    unsafe {
        assert_eq!(
            crate::object::shapes::object_prototype_word(derived),
            crate::value::js_nanbox_pointer(base as i64).to_bits()
        );
    }
}

#[test]
fn a_prototype_the_registry_does_not_fully_describe_takes_the_general_path() {
    let cid = 0x6E44;
    register(cid, b"Mixed");
    let (a, _) = infos();
    unsafe {
        register_method(cid, b"a", Some(a));
        // No entry: the name trampoline, which no shape lane can name.
        register_method(cid, b"old", None);
    }
    let before = born_final_builds();
    let proto = object_of(class_decl_prototype_value(cid));
    assert_eq!(born_final_builds(), before, "the general path built it");
    assert_eq!(class_decl_prototype_object(cid), proto);
    assert_eq!(
        class_id_for_decl_prototype_object(proto as usize),
        Some(cid)
    );
}

#[test]
fn the_reverse_lookup_answers_only_the_linked_object() {
    let cid = 0x6E45;
    register(cid, b"Rev");
    let (a, _) = infos();
    unsafe { register_method(cid, b"a", Some(a)) };
    let proto = object_of(class_decl_prototype_value(cid));
    // An instance carries the same class id but is not the linked object.
    let instance = crate::object::js_object_alloc(cid, 1);
    assert_eq!(class_id_for_decl_prototype_object(instance as usize), None);
    assert_eq!(class_id_for_decl_prototype_object(0), None);
    assert_eq!(class_id_for_decl_prototype_object(0x10), None);
    // A class function object is not an ordinary object.
    let class = crate::object::class_value::class_value_ptr(cid) as usize;
    assert_eq!(class_id_for_decl_prototype_object(class), None);
    assert_eq!(
        class_id_for_decl_prototype_object(proto as usize),
        Some(cid)
    );
}
