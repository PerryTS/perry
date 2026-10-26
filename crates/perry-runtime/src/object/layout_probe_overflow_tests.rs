//! The existing owned table answers without deriving another key index.
use super::*;

unsafe fn atom(name: &str) -> (u64, u64) {
    let hash = crate::object::key_bytes_hash(name.as_ptr(), name.len());
    let key = crate::string::js_string_pool_atom(name.as_ptr(), name.len() as u32, hash, 0);
    (crate::JSValue::string_ptr(key).bits(), hash)
}

#[test]
fn owned_table_reads_do_not_derive_the_record_index() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            crate::gc::initialize_runtime_fixture();
            let _no_move = crate::gc::GcSuppressScope::new();
            let object = crate::object::js_object_alloc(0, 2);
            crate::object::mark_object_plain_ordinary(object);
            let (fixed, hash) = atom("owned_fixed");
            crate::object::js_object_set_field_by_name(
                object,
                (fixed & POINTER_MASK) as *const crate::StringHeader,
                7.0,
            );
            let table = crate::object::overflow::ensure(object);
            let i = crate::object::overflow::find(table, fixed).unwrap();
            assert!(crate::object::overflow::fixed_slot_of(table, i).is_some());
            crate::object::overflow::set_entry_attrs(
                object,
                table,
                i,
                crate::object::key_attrs::ENTRY_NON_WRITABLE,
            );
            let dir = test_dir();
            let record = record_of(dir, (*object).parent_class_id);
            assert!(!record.is_null());
            assert!((*record).present());
            assert_eq!(
                (*record).object_kind(),
                super::super::ShapeObjectKind::Ordinary
            );
            assert_eq!((*record).probe, 0, "the fresh record has no derived index");
            let biased = (object as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;
            assert_eq!(perry_probe_get(dir, biased, fixed, hash), 7.0);
            assert_eq!(
                (*record).probe,
                0,
                "the existing table must answer before record derivation"
            );
            assert_eq!(perry_probe_get_binding(dir, biased, fixed, hash), 7.0);
            let proto = crate::object::js_object_alloc(0, 1);
            crate::object::mark_object_plain_ordinary(proto);
            let (inherited, ih) = atom("owned_inherited");
            crate::object::js_object_set_field_by_name(
                proto,
                (inherited & POINTER_MASK) as *const crate::StringHeader,
                41.0,
            );
            crate::object::js_object_set_prototype_of(
                crate::value::js_nanbox_pointer(object as i64),
                crate::value::js_nanbox_pointer(proto as i64),
            );
            let record = record_of(dir, (*object).parent_class_id);
            assert!(!record.is_null() && (*record).present());
            assert_eq!((*record).probe, 0);
            assert_eq!(
                perry_probe_get(dir, biased, inherited, ih),
                41.0,
                "actual table absence must continue through the existing proven prototype walk"
            );
            assert_eq!(
                (*record).probe,
                0,
                "inherited data does not derive the receiver index"
            );
            let (dynamic, dh) = atom("owned_dynamic");
            crate::object::overflow::insert_key_bits(
                object,
                dynamic,
                crate::value::TAG_UNDEFINED,
                0,
            );
            assert_eq!(
                perry_probe_get(dir, biased, dynamic, dh).to_bits(),
                crate::value::TAG_UNDEFINED
            );
            assert!(crate::object::overflow::delete_key_bits(object, dynamic));
            assert!(matches!(
                perry_probe_get(dir, biased, dynamic, dh).to_bits(),
                TAG_HOLE | crate::value::TAG_UNDEFINED
            ));
            crate::object::overflow::insert_key_bits(object, dynamic, 17.0f64.to_bits(), 0);
            assert_eq!(perry_probe_get(dir, biased, dynamic, dh), 17.0);
            let table = crate::object::overflow::table(object);
            let i = crate::object::overflow::find(table, dynamic).unwrap();
            crate::object::overflow::set_entry_attrs(
                object,
                table,
                i,
                crate::object::key_attrs::ENTRY_ACCESSOR,
            );
            assert_eq!(
                perry_probe_get(dir, biased, dynamic, dh).to_bits(),
                TAG_HOLE
            );
            let (private, ph) = atom("owned_private");
            crate::object::overflow::insert_key_bits(
                object,
                private,
                29.0f64.to_bits(),
                crate::object::key_attrs::ENTRY_PRIVATE,
            );
            assert!(matches!(
                perry_probe_get(dir, biased, private, ph).to_bits(),
                TAG_HOLE | crate::value::TAG_UNDEFINED
            ));
            let (length, lh) = atom("length");
            crate::object::overflow::insert_key_bits(object, length, 31.0f64.to_bits(), 0);
            assert_eq!(perry_probe_get_length(dir, biased, length, lh), 31.0);
            assert_eq!((*record).probe, 0);
        })
        .unwrap()
        .join()
        .unwrap();
}
