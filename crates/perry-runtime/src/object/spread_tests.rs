use super::spread::js_object_spread_fresh_leading;
use super::*;

unsafe fn object(entries: &[(&str, f64)]) -> f64 {
    let obj = js_object_alloc_plain(entries.len() as u32);
    for &(name, value) in entries {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        js_object_set_field_by_name(obj, key, value);
    }
    crate::value::js_nanbox_pointer(obj as i64)
}

unsafe fn receiver(value: f64) -> *mut ObjectHeader {
    crate::value::js_nanbox_get_pointer(value) as *mut ObjectHeader
}

unsafe fn get(value: f64, name: &str) -> f64 {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    f64::from_bits(js_object_get_field_by_name(receiver(value), key).bits())
}

fn empty_prefix() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[test]
fn spread_tail_retains_the_confirmed_image_key_in_its_shape_owner() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _suppress = crate::gc::GcSuppressScope::new();
    #[repr(C, align(8))]
    struct Image {
        gc: crate::gc::GcHeader,
        string: crate::StringHeader,
        bytes: [u8; 8],
        // The atom-hash trailer every program literal image carries.
        hash: u64,
    }
    static IMAGE: Image = Image {
        gc: crate::gc::GcHeader {
            obj_type: crate::gc::GC_TYPE_STRING,
            gc_flags: crate::gc::GC_FLAG_MARKED
                | crate::gc::GC_FLAG_PINNED
                | crate::gc::GC_FLAG_INTERNED,
            _reserved: 0x494d,
            size: crate::string::image_string_gc_size(6) as u32,
        },
        string: crate::StringHeader {
            utf16_len: 6,
            byte_len: 6,
            capacity: 6,
            refcount: 0,
            flags: crate::string::STRING_FLAG_WTF8_VALIDATED,
        },
        bytes: *b"xkey_1\0\0",
        hash: 0,
    };
    // The key must be longer than an inline short string: those are canonical
    // immediates in the shape owner and have no image spelling to retain.
    unsafe {
        let source = object(&[("xkey_1", 3.0), ("y", 5.0), ("z", 7.0)]);
        let result = js_object_spread_fresh_leading(empty_prefix(), source);
        let id = shapes::object_shape_stamp(receiver(result));
        let record = shapes::shape_record_for_word(id).unwrap();
        let key = crate::JSValue::string_ptr(&IMAGE.string as *const _ as *mut _);
        assert_eq!(record.plain_own_identity_slot(key.bits()), None);
        js_object_literal_define(result, f64::from_bits(key.bits()), 9.0);
        assert_eq!(record.plain_own_identity_slot(key.bits()), Some(0));
        assert_eq!(shapes::object_shape_stamp(receiver(result)), id);
        assert_eq!(record.summary(), 0);
        assert_eq!(get(result, "xkey_1"), 9.0);
        assert_eq!(get(result, "y"), 5.0);
        assert_eq!(get(result, "z"), 7.0);
        assert_eq!(get(source, "xkey_1"), 3.0);
    }
}

#[test]
fn spread_birth_reuses_shape_and_keeps_all_values_inline() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _suppress = crate::gc::GcSuppressScope::new();
    unsafe {
        let names: Vec<String> = (0..40).map(|i| format!("spread_{i}")).collect();
        let entries: Vec<(&str, f64)> = names
            .iter()
            .enumerate()
            .map(|(i, s)| (s.as_str(), i as f64))
            .collect();
        let source = object(&entries);
        let a = js_object_spread_fresh_leading(empty_prefix(), source);
        let b = js_object_spread_fresh_leading(empty_prefix(), source);
        assert_ne!(a.to_bits(), b.to_bits());
        assert_eq!(
            shapes::object_shape_stamp(receiver(a)),
            shapes::object_shape_stamp(receiver(b))
        );
        assert_eq!(object_live_slot_count(receiver(b)), 40);
        for &(name, value) in &entries {
            assert_eq!(get(b, name), value);
        }
        assert_eq!(
            shapes::shape_record_for_word(shapes::object_shape_stamp(receiver(b)))
                .unwrap()
                .summary(),
            0
        );
    }
}

#[test]
fn spread_prefix_overlap_keeps_order_and_overwrites_source_slots() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _suppress = crate::gc::GcSuppressScope::new();
    unsafe {
        let prefix = object(&[("before", 1.0), ("shared", 2.0)]);
        let source = object(&[("shared", 3.0), ("after", 4.0)]);
        for _ in 0..2 {
            let result = js_object_spread_fresh(prefix, source);
            assert_eq!(get(result, "before"), 1.0);
            assert_eq!(get(result, "shared"), 3.0);
            assert_eq!(get(result, "after"), 4.0);
            let keys = object_keys(receiver(result));
            assert_eq!(keys.count(), 3);
            for (slot, name) in ["before", "shared", "after"].iter().enumerate() {
                assert!(crate::string::js_string_key_matches_bytes(
                    keys.get(slot as u32),
                    name.as_bytes()
                ));
            }
        }
        assert_eq!(get(prefix, "shared"), 2.0);
    }
}

#[test]
fn spread_frozen_source_births_default_writable_attributes() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _suppress = crate::gc::GcSuppressScope::new();
    unsafe {
        let source = object(&[("x", 3.0), ("y", 5.0)]);
        js_object_freeze(source);
        let result = js_object_spread_fresh_leading(empty_prefix(), source);
        let record =
            shapes::shape_record_for_word(shapes::object_shape_stamp(receiver(result))).unwrap();
        assert_eq!(record.summary(), 0);
        let key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
        js_object_set_field_by_name(receiver(result), key, 9.0);
        assert_eq!(get(result, "x"), 9.0);
        assert_eq!(get(source, "x"), 3.0);
    }
}

#[test]
fn spread_number_copy_refuses_a_generalized_source_shape() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _suppress = crate::gc::GcSuppressScope::new();
    unsafe {
        let source = object(&[("number_copy_x", 3.0), ("number_copy_y", 5.0)]);
        let first = js_object_spread_fresh_leading(empty_prefix(), source);
        let second = js_object_spread_fresh_leading(empty_prefix(), source);
        let record =
            shapes::shape_record_for_word(shapes::object_shape_stamp(receiver(second))).unwrap();
        assert_eq!(field_rep::slot_rep(record.rep(), 0), field_rep::REP_F64);
        assert_eq!(get(second, "number_copy_x"), 3.0);
        let child = object(&[("value", 17.0)]);
        let key = crate::string::js_string_from_bytes(b"number_copy_x".as_ptr(), 13);
        js_object_set_field_by_name(receiver(source), key, child);
        let third = js_object_spread_fresh_leading(empty_prefix(), source);
        assert_eq!(get(third, "number_copy_x").to_bits(), child.to_bits());
        assert_eq!(get(third, "number_copy_y"), 5.0);
        assert_eq!(get(first, "number_copy_x"), 3.0);
    }
}

#[test]
fn spread_namespace_is_not_rewritten_as_a_heap_key_and_prunes_retired_source() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _suppress = crate::gc::GcSuppressScope::new();
    unsafe {
        let source = object(&[("spread_gc_namespace", 3.0)]);
        js_object_freeze(source);
        let result = js_object_spread_fresh_leading(empty_prefix(), source);
        let id = shapes::object_shape_stamp(receiver(source));
        let kid = id as usize;
        let slot = spread::cache_slot(0, id);
        let entry = with_transition_cache(|t| (*t)[slot]);
        assert_eq!(spread::source_shape(&entry), Some(id));
        assert_eq!(
            entry.target_shape_id,
            shapes::object_shape_stamp(receiver(result))
        );
        let mut addresses = Vec::new();
        let mut mark = |value: f64| addresses.push(value.to_bits() & crate::value::POINTER_MASK);
        let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(&mut mark);
        scan_transition_cache_roots_mut(&mut visitor);
        assert!(
            !addresses.contains(&(kid as u64)),
            "source ShapeId is scalar metadata"
        );
        // A cold cache edge must not keep a source descriptor alive. The
        // complete target list may remain live independently of that source.
        prune_dead_transition_cache_entries(&|_| false);
        assert_ne!(with_transition_cache(|t| (*t)[slot].next_keys), 0);
        assert_ne!(
            object_keys(receiver(source)).arr(),
            object_keys(receiver(result)).arr()
        );
        shapes::test_drop_shape_descriptors(object_keys(receiver(source)).arr() as usize);
        assert!(shapes::shape_record_for_word(entry.target_shape_id).is_some());
        assert!(shapes::shape_is_retired(id));
        prune_dead_transition_cache_entries(&|_| false);
        assert_eq!(with_transition_cache(|t| (*t)[slot].next_keys), 0);
    }
}
