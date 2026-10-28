use super::*;

unsafe fn s(text: &str) -> f64 {
    let p = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    boxed(p)
}

unsafe fn text(v: f64) -> String {
    let p = js_get_string_pointer_unified(v) as *const StringHeader;
    crate::string::string_as_str(p).to_string()
}

#[test]
fn pristine_string_entries_run_the_builtin_bodies() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let abcd = s("abcdef");
        assert_eq!(text(js_string_m_slice2(abcd, 1.0, 3.0)), "bc");
        assert_eq!(text(js_string_m_slice1(abcd, -2.0)), "ef");
        assert_eq!(text(js_string_m_slice2(abcd, 1.0, UNDEFINED)), "bcdef");
        assert_eq!(text(js_string_m_substring2(abcd, 3.0, 1.0)), "bc");
        assert_eq!(js_string_m_indexOf1(abcd, s("cd")), 2.0);
        assert_eq!(js_string_m_indexOf2(abcd, s("a"), 1.0), -1.0);
        assert_eq!(js_string_m_lastIndexOf1(abcd, s("f")), 5.0);
        assert_eq!(
            js_string_m_includes1(abcd, s("de")).to_bits(),
            JSValue::bool(true).bits()
        );
        assert_eq!(
            js_string_m_startsWith2(abcd, s("cd"), 2.0).to_bits(),
            JSValue::bool(true).bits()
        );
        assert_eq!(
            js_string_m_endsWith1(abcd, s("ef")).to_bits(),
            JSValue::bool(true).bits()
        );
        assert_eq!(js_string_m_charCodeAt1(abcd, 1.0), 98.0);
        assert!(js_string_m_charCodeAt1(abcd, 9.0).is_nan());
        assert_eq!(text(js_string_m_charAt0(abcd)), "a");
        assert_eq!(text(js_string_m_toUpperCase0(abcd)), "ABCDEF");
        assert_eq!(text(js_string_m_padStart2(s("7"), 3.0, s("0"))), "007");
        assert_eq!(text(js_string_m_padEnd1(s("7"), 3.0)), "7  ");
        assert_eq!(text(js_string_m_repeat1(s("ab"), 2.0)), "abab");
    }
}

#[test]
fn the_fast_path_admits_only_operands_that_need_no_allocation() {
    assert_eq!(number_index(1.5), Some(1));
    assert_eq!(number_index(f64::NAN), Some(0));
    assert_eq!(
        number_index(UNDEFINED),
        None,
        "undefined converts through ToNumber"
    );
    assert_eq!(
        number_index(crate::value::js_nanbox_pointer(0x1000_0000)),
        None
    );
    assert_eq!(
        number_index(f64::from_bits(crate::value::HANDLE_TAG | 7)),
        None,
        "a handle is not a number"
    );
    assert!(heap_string(crate::value::js_nanbox_pointer(0x1000_0000)).is_none());
}

#[test]
fn the_array_admission_reads_the_prototype_word_and_the_cell() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let arr = crate::array::js_array_alloc(4);
        let mut recv = crate::value::js_nanbox_pointer(arr as i64);
        assert!(
            array_admits(&mut recv, b"map", &mut []),
            "a plain array on a pristine realm"
        );
        let proto = crate::object::js_object_alloc(0, 1);
        crate::object::builtin_proto::seal_object(proto, BuiltinProto::Array);
        crate::agent_ptrs::publish_intrinsic_proto(BuiltinProto::Array as usize, proto);
        crate::object::store_object_field_slot(proto, 0, 1.0f64.to_bits());
        assert!(
            !array_admits(&mut recv, b"map", &mut []),
            "a modified Array.prototype"
        );
        assert_ne!(
            crate::array::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED
                .load(std::sync::atomic::Ordering::Relaxed)
                & crate::array::ARRAY_METHODS_MODIFIED,
            0,
            "the inline push/pop admission byte mirrors the Array byte"
        );
        assert_eq!(
            crate::array::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED
                .load(std::sync::atomic::Ordering::Relaxed)
                & crate::array::INDEX_INVALIDATED,
            0,
            "a method write leaves the index fast paths alone"
        );
        assert_ne!(
            crate::object::own_override::PERRY_OWN_NAMED_PROP_INSTALLED
                .load(std::sync::atomic::Ordering::Relaxed),
            0,
            "the own-override gate is armed for the predicate's answer"
        );
        assert_eq!(
            crate::object::own_override::js_receiver_may_own_named_method(
                recv,
                f64::from_bits(
                    crate::JSValue::string_ptr(crate::string::canonical_key(b"map")).bits()
                )
            ),
            1,
            "and the predicate answers for a receiver whose family was modified"
        );
    }
}

#[test]
fn proxy_layout_never_proves_an_array_builtin() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let target = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
        let mut ordinary = crate::value::js_nanbox_pointer(
            target.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64,
        );
        assert!(array_admits(&mut ordinary, b"map", &mut []));
        let handler = crate::object::js_object_alloc(0, 0);
        let mut proxy = crate::proxy_hooks::proxy_new(
            crate::value::js_nanbox_pointer(
                target.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64
            ),
            crate::value::js_nanbox_pointer(handler as i64),
        );
        assert!(!array_fast_admits(proxy));
        assert!(!array_admits(&mut proxy, b"map", &mut []));
        assert!(!crate::object::builtin_proto::receiver_pristine(proxy));
        crate::proxy_hooks::proxy_revoke(proxy);
        assert!(!array_admits(&mut proxy, b"push", &mut []));
        assert!(!crate::object::builtin_proto::receiver_pristine(proxy));
    }
}
