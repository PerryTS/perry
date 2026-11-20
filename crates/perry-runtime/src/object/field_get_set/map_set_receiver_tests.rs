//! `size` on a Map/Set cell is the ordinary [[Get]]: the storage count only
//! while the receiver has no own `size` and inherits the intrinsic, unwritten
//! `%Map.prototype%` / `%Set.prototype%`.

use super::map_set_receiver::map_set_instance_property;

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn size_of(cell: usize, is_map: bool) -> f64 {
    unsafe {
        let v = map_set_instance_property(cell as *const crate::ObjectHeader, key("size"), is_map);
        f64::from_bits(v.bits())
    }
}

/// A prototype whose own `size` is a data property.
fn proto_with_size(value: f64) -> f64 {
    let proto = crate::object::js_object_alloc(0, 0);
    crate::object::js_object_set_field_by_name(proto, key("size"), value);
    crate::value::js_nanbox_pointer(proto as i64)
}

fn meta_of(cell: usize) -> *mut crate::object::ObjectMeta {
    unsafe {
        *super::super::cell_meta::cell_meta_slot(cell).expect("a Map/Set cell has a meta edge")
    }
}

#[test]
fn plain_collections_answer_their_count() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, 1.0, 10.0);
    crate::map::js_map_set(map, 2.0, 20.0);
    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, 3.0);
    assert!(meta_of(map as usize).is_null() && meta_of(set as usize).is_null());
    assert_eq!(size_of(map as usize, true), 2.0);
    assert_eq!(size_of(set as usize, false), 1.0);
}

#[test]
fn a_relinked_collection_answers_its_chain_not_its_count() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, 1.0, 10.0);
    crate::object::js_object_set_prototype_of(
        crate::value::js_nanbox_pointer(map as i64),
        proto_with_size(7.0),
    );
    // The writer leaves its trace where the emitted `.size` arm looks.
    assert!(!meta_of(map as usize).is_null());
    assert_eq!(size_of(map as usize, true), 7.0);

    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, 1.0);
    crate::object::js_object_set_prototype_of(
        crate::value::js_nanbox_pointer(set as i64),
        proto_with_size(9.0),
    );
    assert!(!meta_of(set as usize).is_null());
    assert_eq!(size_of(set as usize, false), 9.0);
}

#[test]
fn an_own_expando_creates_the_meta_record() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, 1.0, 10.0);
    let receiver = crate::value::js_nanbox_pointer(map as i64);
    assert!(unsafe {
        crate::object::exotic_expando::exotic_set_property(
            map as usize,
            crate::object::exotic_expando::ExoticKind::Map,
            "tag",
            5.0,
            receiver,
        )
    });
    assert!(!meta_of(map as usize).is_null());
    assert_eq!(size_of(map as usize, true), 1.0);
}
