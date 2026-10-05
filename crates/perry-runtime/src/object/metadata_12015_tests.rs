//! Shape-owned key views must preserve prefix bounds, front offsets and
//! dictionary ownership. The fixtures establish each premise explicitly.

use crate::object::*;

fn key(name: &[u8]) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

unsafe fn name(value: JSValue) -> String {
    let mut short = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    String::from_utf8_lossy(crate::string::js_string_key_bytes(value, &mut short).unwrap())
        .into_owned()
}

#[test]
fn metadata_12015_key_view_uses_the_receivers_shape_prefix() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let proof = canonical_keys::SharedLayout::shape_cache_entry();
        let first = canonical_keys::extend_key(
            &proof,
            canonical_keys::CanonicalKeys::EMPTY,
            key(b"meta_prefix_a"),
        );
        let second = canonical_keys::extend_key(&proof, first, key(b"meta_prefix_b"));
        assert_eq!(
            first.as_ptr(),
            second.as_ptr(),
            "premise: one shared backing"
        );
        assert_eq!(crate::array::js_array_length(first.as_ptr()), 2);
        let object = js_object_alloc(0, 4);
        set_object_keys_with_live(object, first.view(), 4);
        let (view, live) = object_keys_and_live_slot_count(object);
        assert_eq!(
            view.count(),
            1,
            "the backing length is another receiver's count"
        );
        assert_eq!(live, 4, "live slots and key count are distinct shape facts");
        assert_eq!(name(view.get(0)), "meta_prefix_a");
        let (serializer_view, serializer_live) = object_keys_and_live_slots(object).unwrap();
        assert_eq!(serializer_view, view);
        assert_eq!(serializer_live, live);
    }
}

#[test]
fn metadata_12015_key_get_observes_front_offset_and_holes() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let mut keys = crate::array::js_array_alloc(4);
        for spelling in [
            b"meta_front_dropped".as_slice(),
            b"meta_front_a",
            b"meta_front_b",
        ] {
            keys = crate::array::js_array_push(keys, JSValue::string_ptr(key(spelling)));
        }
        crate::array::js_array_shift_f64(keys);
        assert_ne!(
            crate::array::array_elements_ptr(keys) as usize,
            keys as usize + std::mem::size_of::<ArrayHeader>(),
            "premise: element zero is past a consumed front"
        );
        let view = ObjectKeys::owned(keys);
        assert_eq!(name(view.get(0)), "meta_front_a");
        assert_eq!(name(view.get(1)), "meta_front_b");
        // A key-list tombstone must never become an enumerable key.
        let (slots, len) = view.dense_slots();
        assert_eq!(len, 2);
        // GC_STORE_AUDIT(POINTER_FREE): TAG_HOLE contains no heap pointer.
        (slots as *mut f64)
            .add(1)
            .write(f64::from_bits(crate::value::TAG_HOLE));
        assert!(view.get(1).is_undefined());
    }
}

#[test]
fn metadata_12015_dictionary_keys_are_owned_by_the_receiver() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let object = js_object_alloc(0, 4);
        for (i, spelling) in [b"meta_dictionary_a".as_slice(), b"meta_dictionary_b"]
            .iter()
            .enumerate()
        {
            js_object_set_field_by_name(object, key(spelling), i as f64);
        }
        assert!(
            dictionary::latch_object_to_dictionary(object),
            "premise: dictionary transition"
        );
        assert_eq!(
            shapes::object_shape_descriptor(object).unwrap().keys,
            0,
            "premise: the shape delegates keys to the receiver"
        );
        let (view, live) = object_keys_and_live_slot_count(object);
        assert_eq!(view.arr(), dictionary::keys_array(object));
        assert_eq!(view.count(), 2);
        assert_eq!(live, object_live_slot_count(object));
        assert_eq!(name(view.get(1)), "meta_dictionary_b");
    }
}
