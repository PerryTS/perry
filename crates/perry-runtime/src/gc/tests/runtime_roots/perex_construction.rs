use super::*;
use crate::regex::RegExpHeader;
use crate::string::StringHeader;
use crate::value::{js_nanbox_pointer, js_nanbox_string, TAG_UNDEFINED};

fn text<'s>(scope: &'s RuntimeHandleScope, bytes: &[u8]) -> RuntimeHandle<'s> {
    scope.root_string_ptr(crate::string::js_string_from_bytes(
        bytes.as_ptr(),
        bytes.len() as u32,
    ))
}
fn bytes(ptr: *const StringHeader) -> Vec<u8> {
    unsafe {
        std::slice::from_raw_parts(crate::string::string_data(ptr), (*ptr).byte_len as usize)
            .to_vec()
    }
}
fn construct<'s>(scope: &'s RuntimeHandleScope, pattern: &[u8], flags: &[u8]) -> RuntimeHandle<'s> {
    let p = text(scope, pattern);
    let f = text(scope, flags);
    scope.root_raw_mut_ptr(
        p.with_const_ptr(|p| f.with_const_ptr(|f| crate::regex::js_regexp_new(p, f))),
    )
}
/// `value` NaN-boxed as the pointer its handle currently holds.
fn boxed<T>(handle: &RuntimeHandle<'_>) -> f64 {
    handle.with_const_ptr(|p: *const T| js_nanbox_pointer(p as i64))
}

#[test]
fn perex_constructor_isregexp_and_call_identity_follow_current_values() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    gc_register_mutable_root_scanner(crate::symbol::scan_symbol_side_table_roots_mut);
    let scope = RuntimeHandleScope::new();
    let re = construct(&scope, b"a", b"i");
    let called = crate::regex::js_regexp_construct_call(
        boxed::<RegExpHeader>(&re),
        f64::from_bits(TAG_UNDEFINED),
    );
    re.with_mut_ptr(|re: *mut RegExpHeader| assert_eq!(called, re));
    let marker = crate::symbol::well_known_symbol("match");
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            boxed::<RegExpHeader>(&re),
            js_nanbox_pointer(marker as i64),
            f64::from_bits(crate::value::TAG_FALSE),
        );
    }
    let copied = crate::regex::js_regexp_construct_call(
        boxed::<RegExpHeader>(&re),
        f64::from_bits(TAG_UNDEFINED),
    );
    let copied = scope.root_raw_mut_ptr(copied);
    copied.with_mut_ptr(|copied: *mut RegExpHeader| {
        re.with_mut_ptr(|re: *mut RegExpHeader| assert_ne!(copied, re))
    });
    assert_eq!(
        bytes(copied.with_const_ptr(|p| crate::regex::js_regexp_get_source(p))),
        b"a"
    );
    let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
    for (key, value) in [
        (b"source".as_slice(), b"(?<=x)a".as_slice()),
        (b"flags", b"i"),
    ] {
        let key = text(&scope, key);
        let value = text(&scope, value);
        object.with_mut_ptr(|object| {
            key.with_const_ptr(|key| {
                value.with_const_ptr(|value: *const StringHeader| {
                    crate::object::js_object_set_field_by_name(
                        object,
                        key,
                        js_nanbox_string(value as i64),
                    )
                })
            })
        });
    }
    let marker = crate::symbol::well_known_symbol("match");
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            boxed::<crate::object::ObjectHeader>(&object),
            js_nanbox_pointer(marker as i64),
            f64::from_bits(crate::value::TAG_TRUE),
        );
    }
    let built = crate::regex::js_regexp_construct(
        boxed::<crate::object::ObjectHeader>(&object),
        f64::from_bits(TAG_UNDEFINED),
    );
    let built = scope.root_raw_mut_ptr(built);
    let subject = text(&scope, b"xA");
    assert_eq!(
        built.with_const_ptr(|re| subject.with_const_ptr(|s| crate::regex::js_regexp_test(re, s))),
        1
    );
    assert_eq!(
        bytes(built.with_const_ptr(|p| crate::regex::js_regexp_get_source(p))),
        b"(?<=x)a"
    );
}

#[test]
fn perex_constructor_owns_original_wtf8_and_only_a_perex_program() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let pattern = text(&scope, b"\xed\xa0\x80");
    let flags = text(&scope, b"gi");
    let re = scope.root_raw_mut_ptr(
        pattern.with_const_ptr(|p| flags.with_const_ptr(|f| crate::regex::js_regexp_new(p, f))),
    );
    // The header's string edges are the rooted strings themselves, before and
    // after a collection moves all three.
    let owns_originals = |re: &RuntimeHandle<'_>| {
        re.with_const_ptr(|re| {
            let found = crate::regex::test_original_strings_and_program(re);
            pattern.with_const_ptr(|p| flags.with_const_ptr(|f| assert_eq!(found, (p, f, true))))
        })
    };
    owns_originals(&re);
    gc_collect_minor();
    owns_originals(&re);
    let source = re.with_const_ptr(|p| crate::regex::js_regexp_get_source(p));
    assert_eq!(bytes(source), b"\xed\xa0\x80");
    let copy = scope.root_raw_mut_ptr(crate::regex::js_regexp_construct(
        boxed::<RegExpHeader>(&re),
        f64::from_bits(TAG_UNDEFINED),
    ));
    copy.with_const_ptr(|copy: *const RegExpHeader| {
        re.with_const_ptr(|re: *const RegExpHeader| assert_ne!(copy, re))
    });
    owns_originals(&copy);
    assert_eq!(
        copy.with_const_ptr(|re| pattern.with_const_ptr(|s| crate::regex::js_regexp_test(re, s))),
        1
    );
}

#[test]
fn perex_constructor_validates_before_publication_and_preserves_display_units() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    for (pattern, flags, display) in [
        (
            b"a/b\n\r".as_slice(),
            b"ig".as_slice(),
            b"/a\\/b\\n\\r/gi".as_slice(),
        ),
        (b"[/]", b"", b"/[/]/"),
        (b"\\/", b"", b"/\\//"),
        (b"", b"", b"/(?:)/"),
        (
            b"\xf0\x9f\x98\x80\xed\xa0\x80",
            b"",
            b"/\xf0\x9f\x98\x80\xed\xa0\x80/",
        ),
        (b"\xe2\x80\xa8", b"", b"/\\u2028/"),
    ] {
        let scope = RuntimeHandleScope::new();
        let re = construct(&scope, pattern, flags);
        assert_eq!(
            bytes(re.with_const_ptr(|p| crate::regex::js_regexp_to_string(p))),
            display
        );
    }
    for (pattern, flags) in [("(", ""), ("a", "gg"), ("a", "uv"), ("a", "q")] {
        let scope = RuntimeHandleScope::new();
        let pattern = text(&scope, pattern.as_bytes());
        let flags = text(&scope, flags.as_bytes());
        let roots = RuntimeHandleScope::active_len_for_tests();
        let live = external_side_live_bytes();
        assert!(crate::exception::catch_js_throw(|| pattern
            .with_const_ptr(|p| { flags.with_const_ptr(|f| crate::regex::js_regexp_new(p, f)) }))
        .is_err());
        assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
        assert_eq!(external_side_live_bytes(), live);
    }
}

#[test]
fn perex_recompile_publishes_after_success_before_throwing_lastindex_write() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let re = construct(&scope, b"a", b"g");
    let invalid = text(&scope, b"(");
    assert!(crate::exception::catch_js_throw(|| re.with_mut_ptr(|re| {
        invalid.with_const_ptr(|source: *const StringHeader| {
            crate::regex::js_regexp_compile_value(
                re,
                js_nanbox_string(source as i64),
                f64::from_bits(TAG_UNDEFINED),
            )
        })
    }))
    .is_err());
    assert_eq!(
        bytes(re.with_const_ptr(|p| crate::regex::js_regexp_get_source(p))),
        b"a"
    );
    let next = text(&scope, b"b");
    re.with_mut_ptr(|re: *mut RegExpHeader| {
        crate::object::set_property_attrs(
            re as usize,
            "lastIndex".to_string(),
            crate::object::PropertyAttrs::new(false, false, false),
        )
    });
    assert!(crate::exception::catch_js_throw(|| re.with_mut_ptr(|re| {
        next.with_const_ptr(|source: *const StringHeader| {
            crate::regex::js_regexp_compile_value(
                re,
                js_nanbox_string(source as i64),
                f64::from_bits(TAG_UNDEFINED),
            )
        })
    }))
    .is_err());
    assert_eq!(
        bytes(re.with_const_ptr(|p| crate::regex::js_regexp_get_source(p))),
        b"b"
    );
    // New flags are non-global: the new matcher runs despite the readonly
    // lastIndex which made reinitialization throw after publication.
    assert_eq!(
        re.with_const_ptr(|re| next.with_const_ptr(|s| crate::regex::js_regexp_test(re, s))),
        1
    );
    assert!(
        re.with_const_ptr(|p| crate::regex::test_original_strings_and_program(p))
            .2
    );
}

extern "C" fn flags_collect_then_throw(_closure: *const crate::closure::ClosureHeader) -> f64 {
    gc_collect_minor();
    crate::exception::js_throw(812.0)
}

extern "C" fn flags_collect_then_return(_closure: *const crate::closure::ClosureHeader) -> f64 {
    gc_collect_minor();
    js_nanbox_string(crate::string::js_string_from_bytes(b"g".as_ptr(), 1) as i64)
}

#[test]
fn perex_constructor_coercion_reacquires_original_pattern_before_successful_compile() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let pattern = text(&scope, b"\xed\xa0\x80");
    let before = pattern.with_const_ptr(|p: *const StringHeader| p as usize);
    let flags = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 1));
    let key = text(&scope, b"toString");
    let fp = flags_collect_then_return as *const u8;
    crate::closure::js_register_closure_arity(fp, 0);
    let closure = crate::closure::js_closure_alloc_singleton(fp);
    flags.with_mut_ptr(|flags| {
        key.with_const_ptr(|key| {
            crate::object::js_object_set_field_by_name(
                flags,
                key,
                js_nanbox_pointer(closure as i64),
            )
        })
    });
    let re = crate::regex::js_regexp_construct(
        pattern.with_const_ptr(|p: *const StringHeader| js_nanbox_string(p as i64)),
        boxed::<crate::object::ObjectHeader>(&flags),
    );
    let re = scope.root_raw_mut_ptr(re);
    pattern.with_const_ptr(|p: *const StringHeader| assert_ne!(p as usize, before));
    let (p, f, perex) = re.with_const_ptr(|p| crate::regex::test_original_strings_and_program(p));
    pattern.with_const_ptr(|pattern| assert_eq!(p, pattern));
    assert_eq!(bytes(f), b"g");
    assert!(perex);
    assert_eq!(
        re.with_const_ptr(|re| pattern.with_const_ptr(|s| crate::regex::js_regexp_test(re, s))),
        1
    );
}

#[test]
fn perex_constructor_flags_coercion_can_collect_and_throw_without_source_copy() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let pattern = text(&scope, b"pattern");
    let before = pattern.with_const_ptr(|p: *const StringHeader| p as usize);
    let flags = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 1));
    let key = text(&scope, b"toString");
    let fp = flags_collect_then_throw as *const u8;
    crate::closure::js_register_closure_arity(fp, 0);
    let closure = crate::closure::js_closure_alloc_singleton(fp);
    flags.with_mut_ptr(|flags| {
        key.with_const_ptr(|key| {
            crate::object::js_object_set_field_by_name(
                flags,
                key,
                js_nanbox_pointer(closure as i64),
            )
        })
    });
    let roots = RuntimeHandleScope::active_len_for_tests();
    let live = external_side_live_bytes();
    assert_eq!(
        crate::exception::catch_js_throw(|| crate::regex::js_regexp_construct(
            pattern.with_const_ptr(|p: *const StringHeader| js_nanbox_string(p as i64)),
            boxed::<crate::object::ObjectHeader>(&flags),
        ))
        .unwrap_err(),
        812.0
    );
    pattern.with_const_ptr(|p: *const StringHeader| {
        assert_ne!(p as usize, before);
        assert_eq!(bytes(p), b"pattern");
    });
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(external_side_live_bytes(), live);
}
