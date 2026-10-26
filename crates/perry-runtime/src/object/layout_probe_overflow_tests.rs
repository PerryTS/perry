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

#[test]
fn owned_fixed_entry_reader_keeps_inline_and_spill_semantics() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            crate::gc::initialize_runtime_fixture();
            let _no_move = crate::gc::GcSuppressScope::new();
            for capacity in [2, 8, 24] {
                let object = crate::object::js_object_alloc(0, capacity);
                crate::object::mark_object_plain_ordinary(object);
                let mut keys = Vec::new();
                for n in 0..32 {
                    let (key, hash) = atom(&format!("owned_bound_{capacity}_{n}"));
                    crate::object::js_object_set_field_by_name(
                        object,
                        (key & POINTER_MASK) as *const crate::StringHeader,
                        n as f64 + 0.25,
                    );
                    keys.push((key, hash));
                }
                let table = crate::object::overflow::ensure(object);
                let dir = test_dir();
                let raw = record_of(dir, (*object).parent_class_id);
                assert!(!raw.is_null() && (*raw).present());
                assert_eq!(
                    (*raw).object_kind(),
                    super::super::ShapeObjectKind::Ordinary
                );
                assert_eq!((*raw).probe, 0);
                let record = super::super::ShapeRecordRef(std::ptr::NonNull::new_unchecked(raw));
                let live = record.live_inline_slot_count();
                let biased =
                    (object as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;
                let mut inline = 0;
                let mut spilled = 0;
                for &(key, hash) in &keys {
                    let entry = crate::object::overflow::find_hashed(table, key, hash).unwrap();
                    let fixed = crate::object::overflow::fixed_slot_of(table, entry).unwrap();
                    if fixed < live {
                        inline += 1;
                    } else {
                        spilled += 1;
                    }
                    let expected = crate::object::overflow::get_from(table, object, entry);
                    assert_eq!(
                        crate::object::overflow::get_found_from_record(
                            table, object, entry, record
                        ),
                        expected
                    );
                    assert_eq!(
                        Some(perry_probe_get(dir, biased, key, hash).to_bits()),
                        expected
                    );
                    crate::object::overflow::set_entry_attrs(
                        object,
                        table,
                        entry,
                        crate::object::key_attrs::ENTRY_NON_WRITABLE,
                    );
                    assert_eq!(
                        Some(perry_probe_get_binding(dir, biased, key, hash).to_bits()),
                        expected
                    );
                }
                assert!(inline > 0 && spilled > 0, "both storage arms must execute");
                for &(key, hash) in &[keys[0], keys[31]] {
                    let entry = crate::object::overflow::find_hashed(table, key, hash).unwrap();
                    let fixed = crate::object::overflow::fixed_slot_of(table, entry).unwrap();
                    for bits in [
                        crate::value::TAG_UNDEFINED,
                        TAG_HOLE,
                        0x7ff8_0000_0000_0000,
                        (-0.0f64).to_bits(),
                    ] {
                        if fixed < live {
                            let fields = (object as *mut u8)
                                .add(std::mem::size_of::<ObjectHeader>())
                                as *mut u64;
                            // GC_STORE_AUDIT(POINTER_FREE): the closed list above
                            // contains only undefined, hole, canonical NaN and -0.
                            fields.add(fixed as usize).write(bits);
                        } else {
                            crate::object::spill::spill_set(object as usize, fixed as usize, bits);
                        }
                        let expected = crate::object::overflow::get_from(table, object, entry);
                        assert_eq!(
                            crate::object::overflow::get_found_from_record(
                                table, object, entry, record
                            ),
                            expected
                        );
                        assert_eq!(
                            perry_probe_get(dir, biased, key, hash).to_bits(),
                            expected.unwrap_or(TAG_HOLE)
                        );
                    }
                }
                let (key, hash) = keys[0];
                let entry = crate::object::overflow::find_hashed(table, key, hash).unwrap();
                let fixed = crate::object::overflow::fixed_slot_of(table, entry).unwrap();
                assert!(fixed < live);
                let fields =
                    (object as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
                // GC_STORE_AUDIT(POINTER_FREE): a null-pointer sentinel has no
                // child edge; this fixture exercises the reader's default.
                fields.add(fixed as usize).write(0x7ffd_0000_0000_0000);
                assert_eq!(
                    perry_probe_get(dir, biased, key, hash).to_bits(),
                    crate::value::TAG_UNDEFINED
                );
                assert_eq!(
                    (*raw).probe,
                    0,
                    "the current record must not acquire a derived index"
                );
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
