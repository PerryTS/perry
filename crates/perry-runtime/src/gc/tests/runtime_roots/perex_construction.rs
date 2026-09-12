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
    scope.root_raw_mut_ptr(crate::regex::js_regexp_new(
        p.get_raw_const_ptr(),
        f.get_raw_const_ptr(),
    ))
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
        js_nanbox_pointer(re.get_raw_mut_ptr::<RegExpHeader>() as i64),
        f64::from_bits(TAG_UNDEFINED),
    );
    assert_eq!(called, re.get_raw_mut_ptr::<RegExpHeader>());
    let marker = crate::symbol::well_known_symbol("match");
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            js_nanbox_pointer(re.get_raw_mut_ptr::<RegExpHeader>() as i64),
            js_nanbox_pointer(marker as i64),
            f64::from_bits(crate::value::TAG_FALSE),
        );
    }
    let copied = crate::regex::js_regexp_construct_call(
        js_nanbox_pointer(re.get_raw_mut_ptr::<RegExpHeader>() as i64),
        f64::from_bits(TAG_UNDEFINED),
    );
    let copied = scope.root_raw_mut_ptr(copied);
    assert_ne!(
        copied.get_raw_mut_ptr::<RegExpHeader>(),
        re.get_raw_mut_ptr()
    );
    assert_eq!(
        bytes(crate::regex::js_regexp_get_source(
            copied.get_raw_const_ptr()
        )),
        b"a"
    );
    let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
    for (key, value) in [
        (b"source".as_slice(), b"(?<=x)a".as_slice()),
        (b"flags", b"i"),
    ] {
        let key = text(&scope, key);
        let value = text(&scope, value);
        crate::object::js_object_set_field_by_name(
            object.get_raw_mut_ptr(),
            key.get_raw_const_ptr(),
            js_nanbox_string(value.get_raw_const_ptr::<StringHeader>() as i64),
        );
    }
    let marker = crate::symbol::well_known_symbol("match");
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            js_nanbox_pointer(object.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64),
            js_nanbox_pointer(marker as i64),
            f64::from_bits(crate::value::TAG_TRUE),
        );
    }
    let built = crate::regex::js_regexp_construct(
        js_nanbox_pointer(object.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64),
        f64::from_bits(TAG_UNDEFINED),
    );
    let built = scope.root_raw_mut_ptr(built);
    let subject = text(&scope, b"xA");
    assert_eq!(
        crate::regex::js_regexp_test(built.get_raw_const_ptr(), subject.get_raw_const_ptr()),
        1
    );
    assert_eq!(
        bytes(crate::regex::js_regexp_get_source(
            built.get_raw_const_ptr()
        )),
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
    let re = scope.root_raw_mut_ptr(crate::regex::js_regexp_new(
        pattern.get_raw_const_ptr(),
        flags.get_raw_const_ptr(),
    ));
    let inspect = || crate::regex::test_original_strings_and_program(re.get_raw_const_ptr());
    assert_eq!(
        inspect(),
        (pattern.get_raw_const_ptr(), flags.get_raw_const_ptr(), true)
    );
    gc_collect_minor();
    assert_eq!(
        inspect(),
        (pattern.get_raw_const_ptr(), flags.get_raw_const_ptr(), true)
    );
    let source = crate::regex::js_regexp_get_source(re.get_raw_const_ptr());
    assert_eq!(bytes(source), b"\xed\xa0\x80");
    let copy = scope.root_raw_mut_ptr(crate::regex::js_regexp_construct(
        js_nanbox_pointer(re.get_raw_mut_ptr::<RegExpHeader>() as i64),
        f64::from_bits(TAG_UNDEFINED),
    ));
    assert_ne!(
        copy.get_raw_const_ptr::<RegExpHeader>(),
        re.get_raw_const_ptr()
    );
    let (p, f, perex) = crate::regex::test_original_strings_and_program(copy.get_raw_const_ptr());
    assert_eq!(
        (p, f, perex),
        (pattern.get_raw_const_ptr(), flags.get_raw_const_ptr(), true)
    );
    assert_eq!(
        crate::regex::js_regexp_test(copy.get_raw_const_ptr(), pattern.get_raw_const_ptr()),
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
            bytes(crate::regex::js_regexp_to_string(re.get_raw_const_ptr())),
            display
        );
    }
    for (pattern, flags) in [("(", ""), ("a", "gg"), ("a", "uv"), ("a", "q")] {
        let scope = RuntimeHandleScope::new();
        let pattern = text(&scope, pattern.as_bytes());
        let flags = text(&scope, flags.as_bytes());
        let roots = RuntimeHandleScope::active_len_for_tests();
        let live = external_side_live_bytes();
        assert!(
            crate::exception::catch_js_throw(|| crate::regex::js_regexp_new(
                pattern.get_raw_const_ptr(),
                flags.get_raw_const_ptr()
            ))
            .is_err()
        );
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
    assert!(
        crate::exception::catch_js_throw(|| crate::regex::js_regexp_compile_value(
            re.get_raw_mut_ptr(),
            js_nanbox_string(invalid.get_raw_const_ptr::<StringHeader>() as i64),
            f64::from_bits(TAG_UNDEFINED)
        ))
        .is_err()
    );
    assert_eq!(
        bytes(crate::regex::js_regexp_get_source(re.get_raw_const_ptr())),
        b"a"
    );
    let next = text(&scope, b"b");
    crate::object::set_property_attrs(
        re.get_raw_mut_ptr::<RegExpHeader>() as usize,
        "lastIndex".to_string(),
        crate::object::PropertyAttrs::new(false, false, false),
    );
    assert!(
        crate::exception::catch_js_throw(|| crate::regex::js_regexp_compile_value(
            re.get_raw_mut_ptr(),
            js_nanbox_string(next.get_raw_const_ptr::<StringHeader>() as i64),
            f64::from_bits(TAG_UNDEFINED)
        ))
        .is_err()
    );
    assert_eq!(
        bytes(crate::regex::js_regexp_get_source(re.get_raw_const_ptr())),
        b"b"
    );
    // New flags are non-global: the new matcher runs despite the readonly
    // lastIndex which made reinitialization throw after publication.
    assert_eq!(
        crate::regex::js_regexp_test(re.get_raw_const_ptr(), next.get_raw_const_ptr()),
        1
    );
    assert_eq!(
        crate::regex::test_original_strings_and_program(re.get_raw_const_ptr()).2,
        true
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
    let before = pattern.get_raw_const_ptr::<StringHeader>() as usize;
    let flags = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 1));
    let key = text(&scope, b"toString");
    let fp = flags_collect_then_return as *const u8;
    crate::closure::js_register_closure_arity(fp, 0);
    let closure = crate::closure::js_closure_alloc_singleton(fp);
    crate::object::js_object_set_field_by_name(
        flags.get_raw_mut_ptr(),
        key.get_raw_const_ptr(),
        js_nanbox_pointer(closure as i64),
    );
    let re = crate::regex::js_regexp_construct(
        js_nanbox_string(pattern.get_raw_const_ptr::<StringHeader>() as i64),
        js_nanbox_pointer(flags.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64),
    );
    let re = scope.root_raw_mut_ptr(re);
    assert_ne!(pattern.get_raw_const_ptr::<StringHeader>() as usize, before);
    let (p, f, perex) = crate::regex::test_original_strings_and_program(re.get_raw_const_ptr());
    assert_eq!(p, pattern.get_raw_const_ptr());
    assert_eq!(bytes(f), b"g");
    assert!(perex);
    assert_eq!(
        crate::regex::js_regexp_test(re.get_raw_const_ptr(), pattern.get_raw_const_ptr()),
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
    let before = pattern.get_raw_const_ptr::<StringHeader>() as usize;
    let flags = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 1));
    let key = text(&scope, b"toString");
    let fp = flags_collect_then_throw as *const u8;
    crate::closure::js_register_closure_arity(fp, 0);
    let closure = crate::closure::js_closure_alloc_singleton(fp);
    crate::object::js_object_set_field_by_name(
        flags.get_raw_mut_ptr(),
        key.get_raw_const_ptr(),
        js_nanbox_pointer(closure as i64),
    );
    let roots = RuntimeHandleScope::active_len_for_tests();
    let live = external_side_live_bytes();
    assert_eq!(
        crate::exception::catch_js_throw(|| crate::regex::js_regexp_construct(
            js_nanbox_string(pattern.get_raw_const_ptr::<StringHeader>() as i64),
            js_nanbox_pointer(flags.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64)
        ))
        .unwrap_err(),
        812.0
    );
    assert_ne!(pattern.get_raw_const_ptr::<StringHeader>() as usize, before);
    assert_eq!(bytes(pattern.get_raw_const_ptr()), b"pattern");
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(external_side_live_bytes(), live);
}
