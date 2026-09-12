use super::*;
use crate::regex::perex_match_search::{self as operations, Operation};
use crate::regex::RegExpHeader;
use crate::regex::{perex_api as api, perex_dispatch as dispatch};
use crate::string::StringHeader;
use crate::value::{js_nanbox_pointer, js_nanbox_string, TAG_NULL, TAG_UNDEFINED};

fn text<'s>(scope: &'s RuntimeHandleScope, s: &[u8]) -> RuntimeHandle<'s> {
    scope.root_nanbox_f64(js_nanbox_string(crate::string::js_string_from_bytes(
        s.as_ptr(),
        s.len() as u32,
    ) as i64))
}
fn object<'s>(scope: &'s RuntimeHandleScope) -> RuntimeHandle<'s> {
    scope.root_nanbox_f64(js_nanbox_pointer(
        crate::object::js_object_alloc(0, 4) as i64
    ))
}
fn regex<'s>(scope: &'s RuntimeHandleScope, p: &[u8], f: &[u8]) -> RuntimeHandle<'s> {
    let p = text(scope, p);
    let f = text(scope, f);
    scope.root_nanbox_f64(js_nanbox_pointer(crate::regex::js_regexp_new(
        crate::value::js_get_string_pointer_unified(p.get_nanbox_f64()) as *const StringHeader,
        crate::value::js_get_string_pointer_unified(f.get_nanbox_f64()) as *const StringHeader,
    ) as i64))
}
fn function<'s>(scope: &'s RuntimeHandleScope, fp: *const u8, arity: u32) -> RuntimeHandle<'s> {
    crate::closure::js_register_closure_arity(fp, arity);
    scope.root_nanbox_f64(js_nanbox_pointer(
        crate::closure::js_closure_alloc_singleton(fp) as i64,
    ))
}
fn put(owner: &RuntimeHandle<'_>, name: &[u8], value: &RuntimeHandle<'_>) {
    let key = crate::string::canonical_key(name);
    assert!(crate::proxy::create_data_property(
        owner.get_nanbox_f64(),
        js_nanbox_string(key as i64),
        value.get_nanbox_f64()
    ));
}
fn put_number(owner: &RuntimeHandle<'_>, name: &[u8], value: f64) {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    put(owner, name, &value);
}
fn symbol(owner: &RuntimeHandle<'_>, name: &str, value: &RuntimeHandle<'_>) {
    let key = crate::symbol::well_known_symbol(name);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            owner.get_nanbox_f64(),
            js_nanbox_pointer(key as i64),
            value.get_nanbox_f64(),
        );
    }
}
fn item(array: &RuntimeHandle<'_>, index: u32) -> f64 {
    crate::array::js_array_get_f64(
        crate::value::js_nanbox_get_pointer(array.get_nanbox_f64())
            as *const crate::array::ArrayHeader,
        index,
    )
}
fn length(array: &RuntimeHandle<'_>) -> u32 {
    crate::array::js_array_length(crate::value::js_nanbox_get_pointer(array.get_nanbox_f64())
        as *const crate::array::ArrayHeader)
}
fn bytes(value: f64) -> Vec<u8> {
    let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
    let (data, n) = crate::string::str_bytes_from_jsvalue(value, &mut short).unwrap();
    unsafe { std::slice::from_raw_parts(data, n as usize).to_vec() }
}
extern "C" fn identity(_: *const crate::closure::ClosureHeader, arg: f64) -> f64 {
    arg
}
extern "C" fn throw_string(_: *const crate::closure::ClosureHeader) -> f64 {
    crate::exception::js_throw(901.0)
}
extern "C" fn search_override(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    gc_collect_minor();
    api::finish(dispatch::set_last_index(&receiver, 7.0));
    receiver.get_nanbox_f64()
}
extern "C" fn search_throw(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    api::finish(dispatch::set_last_index(&receiver, 7.0));
    crate::exception::js_throw(902.0)
}
extern "C" fn global_override(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    gc_collect_minor();
    let count = api::finish(dispatch::get(&receiver, b"calls"));
    if count == 0.0 {
        assert_eq!(api::finish(dispatch::get(&receiver, b"lastIndex")), 0.0);
    } else {
        assert_eq!(api::finish(dispatch::get(&receiver, b"lastIndex")), 2.0);
    }
    put_number(&receiver, b"calls", count + 1.0);
    if count >= 2.0 {
        return f64::from_bits(TAG_NULL);
    }
    let capture = text(&scope, if count == 0.0 { b"" } else { b"X" });
    put(&receiver, b"0", &capture);
    receiver.get_nanbox_f64()
}
extern "C" fn collecting_flags(_: *const crate::closure::ClosureHeader) -> f64 {
    gc_collect_minor();
    js_nanbox_string(crate::string::js_string_from_bytes(b"gu".as_ptr(), 2) as i64)
}

#[test]
fn perex_match_search_builtin_results_and_empty_unicode_advancement() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, "😀".as_bytes());
    let re = regex(&scope, b"(?<half>.)(x)?", b"d");
    let result = scope.root_nanbox_f64(crate::regex::js_string_match_js(
        input.get_nanbox_f64(),
        re.get_nanbox_f64(),
    ));
    assert_eq!(length(&result), 3);
    assert_eq!(bytes(item(&result, 0)), [0xed, 0xa0, 0xbd]);
    assert_eq!(item(&result, 2).to_bits(), TAG_UNDEFINED);
    assert_eq!(
        api::finish(dispatch::get(&result, b"input")).to_bits(),
        input.get_nanbox_f64().to_bits()
    );
    for (flags, expected) in [(b"g".as_slice(), 3), (b"gu", 2)] {
        let iteration = RuntimeHandleScope::new();
        let re = regex(&iteration, b"(?:)", flags);
        assert_eq!(bytes(api::finish(dispatch::get(&re, b"flags"))), flags);
        assert_eq!(
            api::finish(dispatch::get(&re, b"global")).to_bits(),
            crate::value::TAG_TRUE
        );
        let result = iteration.root_nanbox_f64(crate::regex::js_string_match_js(
            input.get_nanbox_f64(),
            re.get_nanbox_f64(),
        ));
        assert_eq!(length(&result), expected);
        for i in 0..expected {
            assert!(bytes(item(&result, i)).is_empty());
        }
        assert_eq!(api::finish(dispatch::get(&re, b"lastIndex")), 0.0);
        assert!(
            crate::regex::test_original_strings_and_program(crate::value::js_nanbox_get_pointer(
                re.get_nanbox_f64()
            ) as *const RegExpHeader)
            .2
        );
    }
    let re = regex(&scope, b"a*", b"g");
    let input = text(&scope, b"a");
    let result = scope.root_nanbox_f64(crate::regex::js_string_match_js(
        input.get_nanbox_f64(),
        re.get_nanbox_f64(),
    ));
    assert_eq!(length(&result), 2);
    assert_eq!(bytes(item(&result, 0)), b"a");
    assert_eq!(bytes(item(&result, 1)), b"");
}

#[test]
fn perex_search_restores_negative_zero_and_original_object_across_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, b"ba");
    let re = regex(&scope, b"a", b"g");
    api::finish(dispatch::set_last_index(&re, -0.0));
    assert_eq!(
        crate::regex::js_string_search_js(input.get_nanbox_f64(), re.get_nanbox_f64()),
        1.0
    );
    assert_eq!(
        api::finish(dispatch::get(&re, b"lastIndex")).to_bits(),
        (-0.0f64).to_bits()
    );
    let method = function(&scope, search_override as *const u8, 1);
    put(&re, b"exec", &method);
    let answer = text(&scope, b"arbitrary index");
    put(&re, b"index", &answer);
    let previous = object(&scope);
    api::finish(dispatch::set_last_index(&re, previous.get_nanbox_f64()));
    let before = previous.get_nanbox_f64().to_bits();
    let result = crate::regex::js_string_search_js(input.get_nanbox_f64(), re.get_nanbox_f64());
    assert_eq!(result.to_bits(), answer.get_nanbox_f64().to_bits());
    assert_ne!(previous.get_nanbox_f64().to_bits(), before);
    assert_eq!(
        api::finish(dispatch::get(&re, b"lastIndex")).to_bits(),
        previous.get_nanbox_f64().to_bits()
    );
    gc_collect_minor();
    assert_eq!(
        api::finish(dispatch::get(&re, b"lastIndex")).to_bits(),
        previous.get_nanbox_f64().to_bits()
    );
}

#[test]
fn perex_search_throw_order_and_sticky_behavior_follow_public_operations() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, b"ba");
    let re = regex(&scope, b"a", b"y");
    api::finish(dispatch::set_last_index(&re, 1.0));
    assert_eq!(
        crate::regex::js_string_search_js(input.get_nanbox_f64(), re.get_nanbox_f64()),
        -1.0
    );
    assert_eq!(api::finish(dispatch::get(&re, b"lastIndex")), 1.0);
    let result = scope.root_nanbox_f64(crate::regex::js_string_match_js(
        input.get_nanbox_f64(),
        re.get_nanbox_f64(),
    ));
    assert_eq!(api::finish(dispatch::get(&result, b"index")), 1.0);
    let method = function(&scope, search_throw as *const u8, 1);
    put(&re, b"exec", &method);
    api::finish(dispatch::set_last_index(&re, 9.0));
    assert_eq!(
        crate::exception::catch_js_throw(|| crate::regex::js_string_search_js(
            input.get_nanbox_f64(),
            re.get_nanbox_f64()
        ))
        .unwrap_err(),
        902.0
    );
    assert_eq!(
        api::finish(dispatch::get(&re, b"lastIndex")),
        7.0,
        "exec throw does not run restoration steps"
    );
    crate::object::set_property_attrs(
        crate::value::js_nanbox_get_pointer(re.get_nanbox_f64()) as usize,
        "lastIndex".to_string(),
        crate::object::PropertyAttrs::new(false, false, false),
    );
    let error = crate::exception::catch_js_throw(|| {
        crate::regex::js_string_search_js(input.get_nanbox_f64(), re.get_nanbox_f64())
    })
    .unwrap_err();
    assert_ne!(
        error.to_bits(),
        902.0f64.to_bits(),
        "readonly initial reset must throw before exec"
    );
    assert_eq!(api::finish(dispatch::get(&re, b"lastIndex")), 7.0);
}

#[test]
fn perex_generic_global_match_preserves_output_during_collecting_overrides() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = object(&scope);
    let flags = text(&scope, b"gu");
    put(&receiver, b"flags", &flags);
    put_number(&receiver, b"calls", 0.0);
    let method = function(&scope, global_override as *const u8, 1);
    put(&receiver, b"exec", &method);
    let input = text(&scope, "😀".as_bytes());
    let before = input.get_nanbox_f64().to_bits();
    let result = scope.root_nanbox_f64(api::finish(operations::regexp(
        Operation::Match,
        receiver.get_nanbox_f64(),
        input.get_nanbox_f64(),
    )));
    assert_ne!(input.get_nanbox_f64().to_bits(), before);
    assert_eq!(length(&result), 2);
    assert_eq!(bytes(item(&result, 0)), b"");
    assert_eq!(bytes(item(&result, 1)), b"X");
    assert_eq!(api::finish(dispatch::get(&receiver, b"calls")), 3.0);
    assert_eq!(api::finish(dispatch::get(&receiver, b"lastIndex")), 2.0);
}

#[test]
fn perex_string_symbol_hooks_preserve_values_before_receiver_coercion() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = object(&scope);
    let pattern = object(&scope);
    let coercion = function(&scope, throw_string as *const u8, 0);
    put(&receiver, b"toString", &coercion);
    let hook = function(&scope, identity as *const u8, 1);
    for name in ["match", "search"] {
        symbol(&pattern, name, &hook);
        let method = scope.root_nanbox_f64(crate::collection_iter::builtin_prototype_method(
            "String", name,
        ));
        let result = api::finish(dispatch::call_one(&method, &receiver, &pattern));
        assert_eq!(result.to_bits(), receiver.get_nanbox_f64().to_bits());
    }
    let primitive = text(&scope, b"xyz");
    assert_eq!(
        crate::regex::js_string_match_js(primitive.get_nanbox_f64(), pattern.get_nanbox_f64())
            .to_bits(),
        primitive.get_nanbox_f64().to_bits()
    );
    let noncallable = scope.root_nanbox_f64(0.0);
    symbol(&pattern, "match", &noncallable);
    assert!(
        crate::exception::catch_js_throw(|| crate::regex::js_string_match_js(
            primitive.get_nanbox_f64(),
            pattern.get_nanbox_f64()
        ))
        .is_err()
    );
    let source = text(&scope, b"y");
    assert_eq!(
        crate::regex::js_string_search_js(primitive.get_nanbox_f64(), source.get_nanbox_f64()),
        1.0
    );
    let result = scope.root_nanbox_f64(crate::regex::js_string_match_js(
        primitive.get_nanbox_f64(),
        f64::from_bits(TAG_UNDEFINED),
    ));
    assert_eq!(bytes(item(&result, 0)), b"");
}

#[test]
fn perex_match_reads_overridden_flags_and_global_getters() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let re = regex(&scope, b"(?:)", b"g");
    let getter = function(&scope, collecting_flags as *const u8, 0);
    let descriptor = object(&scope);
    put(&descriptor, b"get", &getter);
    put_number(
        &descriptor,
        b"configurable",
        f64::from_bits(crate::value::TAG_TRUE),
    );
    let key = crate::string::canonical_key(b"flags");
    assert_eq!(
        crate::proxy::js_reflect_define_property(
            re.get_nanbox_f64(),
            js_nanbox_string(key as i64),
            descriptor.get_nanbox_f64()
        )
        .to_bits(),
        crate::value::TAG_TRUE
    );
    let input = text(&scope, "😀".as_bytes());
    let before = input.get_nanbox_f64().to_bits();
    let result = scope.root_nanbox_f64(crate::regex::js_string_match_js(
        input.get_nanbox_f64(),
        re.get_nanbox_f64(),
    ));
    assert_ne!(input.get_nanbox_f64().to_bits(), before);
    assert_eq!(length(&result), 2);
    let re = regex(&scope, b"(?:)", b"g");
    put_number(&re, b"global", f64::from_bits(crate::value::TAG_FALSE));
    let result = scope.root_nanbox_f64(crate::regex::js_string_match_js(
        input.get_nanbox_f64(),
        re.get_nanbox_f64(),
    ));
    assert_eq!(length(&result), 1);
    assert_eq!(api::finish(dispatch::get(&result, b"index")), 0.0);
}

#[test]
fn perex_regexp_prototype_ignores_constructor_and_honors_explicit_parent() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let re = regex(&scope, b"a", b"");
    let original = scope.root_nanbox_f64(crate::object::js_object_get_prototype_of(
        re.get_nanbox_f64(),
    ));
    let getter = function(&scope, throw_string as *const u8, 0);
    let descriptor = object(&scope);
    put(&descriptor, b"get", &getter);
    let key = crate::string::canonical_key(b"constructor");
    assert_eq!(
        crate::proxy::js_reflect_define_property(
            re.get_nanbox_f64(),
            js_nanbox_string(key as i64),
            descriptor.get_nanbox_f64(),
        )
        .to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        crate::exception::catch_js_throw(|| api::finish(dispatch::get(&re, b"constructor")))
            .unwrap_err(),
        901.0,
        "the constructor getter must be installed and observable"
    );
    assert_eq!(
        crate::object::js_object_get_prototype_of(re.get_nanbox_f64()).to_bits(),
        original.get_nanbox_f64().to_bits()
    );
    let parent = object(&scope);
    let hook = function(&scope, identity as *const u8, 1);
    symbol(&parent, "match", &hook);
    assert_eq!(
        crate::proxy::js_reflect_set_prototype_of(re.get_nanbox_f64(), parent.get_nanbox_f64())
            .to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        crate::object::js_object_get_prototype_of(re.get_nanbox_f64()).to_bits(),
        parent.get_nanbox_f64().to_bits()
    );
    let input = text(&scope, b"original subject");
    assert_eq!(
        crate::regex::js_string_match_js(input.get_nanbox_f64(), re.get_nanbox_f64()).to_bits(),
        input.get_nanbox_f64().to_bits()
    );
    assert_eq!(
        api::finish(dispatch::get(&re, b"source")).to_bits(),
        TAG_UNDEFINED
    );
    assert_eq!(
        crate::proxy::js_reflect_set_prototype_of(re.get_nanbox_f64(), f64::from_bits(TAG_NULL))
            .to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        crate::object::js_object_get_prototype_of(re.get_nanbox_f64()).to_bits(),
        TAG_NULL
    );
}
