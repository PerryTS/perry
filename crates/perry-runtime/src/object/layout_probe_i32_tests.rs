use super::*;

unsafe fn atom(name: &str) -> (u64, u64) {
    let hash = crate::object::key_bytes_hash(name.as_ptr(), name.len());
    let key = crate::string::js_string_pool_atom(name.as_ptr(), name.len() as u32, hash, 0);
    (crate::JSValue::string_ptr(key).bits(), hash)
}

extern "C" fn method_body(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    11.0
}

#[test]
fn mixed_i32_layout_keeps_ordinary_probe_entries() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let obj = crate::object::js_object_alloc(0, 4);
        crate::object::mark_object_plain_ordinary(obj);
        let keys = [
            atom("i32_probe_number"),
            atom("i32_probe_integer"),
            atom("i32_probe_method"),
        ];
        let info = Box::leak(Box::new(crate::closure::JsFunctionInfo::of(
            method_body as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
        )));
        let closure = crate::closure::js_closure_alloc(info, 0);
        let function = crate::value::js_nanbox_pointer(closure as i64);
        for ((key, _), value) in keys.iter().zip([9.0, -7.0, function]) {
            crate::object::js_object_set_field_by_name(
                obj,
                (*key & POINTER_MASK) as *const crate::StringHeader,
                value,
            );
        }
        crate::object::field_rep_store::install_i32_lanes(obj, 1 << 1);
        let dir = crate::object::shapes::ordinary_dir_addr();
        let bits = crate::value::js_nanbox_pointer(obj as i64).to_bits();
        let biased = (obj as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;
        let r = record_of(dir, LayoutWord((*obj).parent_class_id));
        assert_ne!((*r).i32_lanes() & (1 << 1), 0);
        assert_eq!((*r).probe, 0);

        // First derivation and warm hits must both answer mixed layouts.
        for _ in 0..2 {
            for ((key, hash), want) in keys.iter().zip([9.0, -7.0, function]) {
                assert_eq!(
                    perry_probe_get(dir, biased, *key, *hash).to_bits(),
                    want.to_bits()
                );
            }
            assert_eq!((*r).probe & PROBE_DECLINE, 0);
        }
        let table = ((*r).probe & !PROBE_FLAGS) as *const KeyIndex;
        assert!(
            !table.is_null(),
            "an I32 layout cannot use raw POSBOUND loads"
        );
        let buckets =
            std::slice::from_raw_parts(KeyIndex::buckets(table), (*table).mask as usize + 1);
        for pos in 0..3 {
            let entry = *buckets
                .iter()
                .find(|&&e| e != u32::MAX && e & POS_BITS == pos)
                .unwrap();
            if pos == 1 {
                assert_eq!(entry & (DIRECT_ENTRY | SLOT_ENTRY | WRITE_ENTRY), 0);
            } else {
                assert_ne!(entry & DIRECT_ENTRY, 0, "ordinary slots keep direct loads");
                assert_ne!(entry & WRITE_ENTRY, 0, "ordinary slots keep direct stores");
            }
        }
        assert_eq!(perry_probe_set(dir, bits, keys[0].0, keys[0].1, 19.0), 1);
        assert_eq!(perry_probe_get(dir, biased, keys[0].0, keys[0].1), 19.0);
        assert_eq!(perry_probe_set(dir, bits, keys[1].0, keys[1].1, -9.0), 0);
        crate::layout_slot::slot_set(obj, 1, (-9.0f64).to_bits());
        assert_eq!(perry_probe_get(dir, biased, keys[1].0, keys[1].1), -9.0);
        assert_eq!(
            perry_probe_own_method(dir, f64::from_bits(bits), keys[2].0, keys[2].1).to_bits(),
            function.to_bits()
        );

        let memo = site_memo::ReadSiteMemo::EMPTY;
        assert!(
            site_memo::prime(dir, obj, keys[1].0, keys[1].1, &memo).is_none(),
            "an encoded slot must not become a raw-load memo"
        );
        // Prototype promotion rewrites lane metadata. Establish that identity
        // before installing the encoded lane so the fixture's slots and record agree.
        let proto = crate::object::js_object_alloc(0, 1);
        crate::object::mark_object_plain_ordinary(proto);
        crate::object::js_object_set_field_by_name(
            proto,
            (keys[1].0 & POINTER_MASK) as *const crate::StringHeader,
            -9.0,
        );
        let inherited =
            crate::object::js_object_create(crate::value::js_nanbox_pointer(proto as i64));
        crate::object::field_rep_store::install_i32_lanes(proto, 1);
        let pr = record_of(dir, LayoutWord((*proto).parent_class_id));
        assert_ne!((*pr).i32_lanes() & 1, 0);
        let child = (inherited.to_bits() & POINTER_MASK) as *const ObjectHeader;
        let child_biased = (child as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;
        for _ in 0..2 {
            let answer = perry_probe_get(dir, child_biased, keys[1].0, keys[1].1);
            // A per-object prototype may decline; its complete lookup still
            // uses the holder's layout accessor, never the encoded word.
            if answer.to_bits() == TAG_HOLE {
                assert_eq!(
                    crate::layout_slot::js_object_rest_get(inherited, f64::from_bits(keys[1].0),),
                    -9.0,
                );
            } else {
                assert_eq!(
                    answer.to_bits(),
                    (-9.0f64).to_bits(),
                    "inherited probe word {:#018x}",
                    answer.to_bits(),
                );
            }
        }
        assert!(site_memo::prime(dir, child, keys[1].0, keys[1].1, &memo).is_none());
    }
}
