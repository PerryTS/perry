use super::*;

#[test]
fn weak_class_method_memo_matches_get_across_mutations() {
    if !run_with_fresh_worker_gate("weak_class_method_memo_matches_get_across_mutations") {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        for (name, class) in [
            ("WeakMap", crate::weakref::CLASS_ID_WEAKMAP),
            ("WeakSet", crate::weakref::CLASS_ID_WEAKSET),
            ("WeakRef", crate::weakref::CLASS_ID_WEAKREF),
            (
                "FinalizationRegistry",
                crate::weakref::CLASS_ID_FINALIZATION_REGISTRY,
            ),
        ] {
            let prototype = crate::object::builtin_prototype_value(name);
            assert_eq!(
                crate::object::shapes::identity_prototype_word(
                    crate::object::shapes::PROTO_ID_CLASS | u64::from(class)
                ),
                prototype.to_bits(),
                "the materialized intrinsic must publish its CLASS word"
            );
        }
        let proto = crate::object::builtin_prototype_value("WeakMap");
        let holder = crate::JSValue::from_bits(proto.to_bits()).as_pointer::<ObjectHeader>()
            as *mut ObjectHeader;
        let receiver = crate::weakref::js_weakmap_new();
        assert_eq!(
            crate::object::shapes::shape_proto_id(crate::object::shapes::object_shape_stamp(
                receiver
            )),
            Some(
                crate::object::shapes::PROTO_ID_CLASS | u64::from(crate::weakref::CLASS_ID_WEAKMAP)
            ),
            "the final branded birth must retain its intrinsic CLASS identity"
        );
        let recv = crate::value::js_nanbox_pointer(receiver as i64);
        let key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
        let original = crate::object::js_object_get_field_by_name(holder, key);
        let mut slot: MethodSiteSlot = std::ptr::null_mut();
        prime(&mut slot, recv, b"get", 1);
        let (value, _) = memo_hit(&mut slot, recv.to_bits()).unwrap_or_else(|| {
            panic!("CLASS native site must hit: slot={slot:p}, refusals={:?}, receiver={:?}, original={:x}",
                SITE_REFUSED.iter().map(|n| n.load(Ordering::Relaxed)).collect::<Vec<_>>(),
                super::super::shapes::object_shape_descriptor(receiver), original.bits());
        });
        assert_eq!(value, original.bits());
        let saved = (*slot)
            .entries
            .iter()
            .find(|e| e.word == receiver_word((receiver as *const u64).read()))
            .copied()
            .unwrap();
        assert_ne!(saved.slot & METHOD_SITE_INHERITED, 0);
        let replacement = crate::object::js_object_get_field_by_name(
            holder,
            crate::string::js_string_from_bytes(b"has".as_ptr(), 3),
        );
        crate::object::js_object_set_field_by_name(holder, key, f64::from_bits(replacement.bits()));
        assert_eq!(
            crate::object::js_object_get_field_by_name(receiver, key).bits(),
            replacement.bits()
        );
        assert!(memo_hit(&mut slot, recv.to_bits()).is_none());
        prime(&mut slot, recv, b"get", 1);
        assert_eq!(
            memo_hit(&mut slot, recv.to_bits()).unwrap().0,
            replacement.bits()
        );
        // Negative control: bypassing the holder/body checks retains the
        // saved body although generic Get has changed. The fixture must
        // distinguish this bypass from the validated answer.
        let replacement_ptr = crate::JSValue::from_bits(replacement.bits())
            .as_pointer::<crate::closure::ClosureHeader>();
        assert_ne!(original.bits(), replacement.bits());
        assert_ne!(saved.info, (*replacement_ptr).info as u64);
        let object_key =
            crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64);
        crate::weakref::js_weakmap_set(recv, object_key, 7.0);
        let this = crate::closure::JsThis::from_f64(recv);
        let generic =
            crate::closure::call_value(f64::from_bits(replacement.bits()), this, &[object_key]);
        let unchecked: unsafe extern "C" fn(
            *const crate::closure::ClosureHeader,
            crate::closure::JsThis,
            f64,
        ) -> f64 = std::mem::transmute(saved.code as usize);
        let bypass = unchecked(replacement_ptr, this, object_key);
        assert_eq!(generic.to_bits(), crate::value::TAG_TRUE);
        assert_eq!(bypass, 7.0);
        assert_ne!(
            bypass.to_bits(),
            generic.to_bits(),
            "negative control: bypassed validation must fail the Get/body differential"
        );
        crate::object::js_object_set_field_by_name(holder, key, f64::from_bits(original.bits()));
        assert!(memo_hit(&mut slot, recv.to_bits()).is_none());
        prime(&mut slot, recv, b"get", 1);
        assert_eq!(
            memo_hit(&mut slot, recv.to_bits()).unwrap().0,
            original.bits()
        );
    }
}
