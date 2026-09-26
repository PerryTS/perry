//! Accessor pairs live in the key's slot (`accessor_pair.rs`).

use super::*;

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

unsafe fn closure_bits() -> u64 {
    extern "C" fn noop(_c: *const crate::closure::ClosureHeader) -> f64 {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
    let c = crate::closure::js_closure_alloc(noop as *const u8, 0);
    crate::value::js_nanbox_pointer(c as i64).to_bits()
}

#[test]
fn a_pair_round_trips_both_forms() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let get = closure_bits();
        let acc = Accessor {
            get,
            set: 0,
            raw_get: 0x5555_1234_5678,
            raw_set: 0,
        };
        let pair = pair_new(acc);
        let value = crate::value::js_nanbox_pointer(pair as i64).to_bits();
        assert_eq!(pair_of_value(value), Some(acc));
        assert_eq!(
            pair_of_value(crate::value::TAG_UNDEFINED),
            None,
            "a slot without a pair is not one"
        );
    }
}

/// An ordinary object's accessor lives in its key's slot, not in the
/// owner-keyed table, and reads back through the descriptor API. Sabotage:
/// storing the pair in the table instead leaves the slot `undefined` and the
/// first assertion fails.
#[test]
fn an_ordinary_objects_accessor_lives_in_its_slot() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(obj, key("plain"), 1.0);
        let get = closure_bits();
        crate::object::set_accessor_descriptor(
            obj as usize,
            "acc".to_string(),
            crate::object::AccessorDescriptor { get, set: 0 },
        );
        let keys = crate::object::object_keys(obj);
        let pos = crate::object::keys_find_slot_by_bytes(keys.arr(), keys.count(), b"acc")
            .expect("the accessor claimed its key");
        assert_eq!(
            slot_accessor(obj, pos).get,
            get,
            "the pair is in the key's slot"
        );
        assert!(
            !crate::state::state()
                .descriptors
                .accessor_descriptors
                .borrow()
                .contains_key(&(obj as usize, "acc".to_string())),
            "an ordinary object's accessor never reaches the owner table"
        );
        assert_eq!(
            crate::object::get_accessor_descriptor(obj as usize, "acc").map(|a| a.get),
            Some(get)
        );
        assert!(
            crate::object::key_attrs::object_slot_data(obj, pos).is_undefined(),
            "a slot walker sees no data value for an accessor key"
        );
        crate::object::clear_accessor_descriptor(obj as usize, "acc");
        assert!(crate::object::get_accessor_descriptor(obj as usize, "acc").is_none());
        assert!(
            crate::object::js_object_get_field(obj, pos).is_undefined(),
            "clearing the accessor clears its slot"
        );
    }
}
