//! Split semantics with original inputs, mutable roots and actual GC movement.
use super::*;
use crate::regex::{perex_api as api, perex_dispatch as dispatch, perex_split as split};
use crate::value::{
    js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string, TAG_NULL, TAG_UNDEFINED,
};
fn text<'s>(scope: &'s RuntimeHandleScope, bytes: &[u8]) -> RuntimeHandle<'s> {
    scope.root_nanbox_f64(js_nanbox_string(crate::string::js_string_from_bytes(
        bytes.as_ptr(),
        bytes.len() as u32,
    ) as i64))
}
fn object<'s>(scope: &'s RuntimeHandleScope) -> RuntimeHandle<'s> {
    scope.root_nanbox_f64(js_nanbox_pointer(
        crate::object::js_object_alloc(0, 8) as i64
    ))
}
fn regex<'s>(scope: &'s RuntimeHandleScope, source: &[u8], flags: &[u8]) -> RuntimeHandle<'s> {
    let source = text(scope, source);
    let flags = text(scope, flags);
    scope.root_nanbox_f64(js_nanbox_pointer(crate::regex::js_regexp_construct(
        source.get_nanbox_f64(),
        flags.get_nanbox_f64(),
    ) as i64))
}
fn function<'s>(scope: &'s RuntimeHandleScope, fp: *const u8, arity: u32) -> RuntimeHandle<'s> {
    crate::closure::js_register_closure_arity(fp, arity);
    scope.root_nanbox_f64(js_nanbox_pointer(
        crate::closure::js_closure_alloc_singleton(fp) as i64,
    ))
}
fn get(owner: &RuntimeHandle<'_>, name: &[u8]) -> f64 {
    api::finish(dispatch::get(owner, name))
}
fn put(owner: &RuntimeHandle<'_>, name: &[u8], value: f64) {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let key = crate::string::canonical_key(name);
    assert!(crate::proxy::create_data_property(
        owner.get_nanbox_f64(),
        js_nanbox_string(key as i64),
        value.get_nanbox_f64()
    ));
}
fn symbol(owner: &RuntimeHandle<'_>, name: &str, value: f64) {
    let key = crate::symbol::well_known_symbol(name);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            owner.get_nanbox_f64(),
            js_nanbox_pointer(key as i64),
            value,
        );
    }
}
fn accessor(owner: &RuntimeHandle<'_>, key: f64, getter: f64, setter: f64) {
    let scope = RuntimeHandleScope::new();
    let key = scope.root_nanbox_f64(key);
    let getter = scope.root_nanbox_f64(getter);
    let setter = scope.root_nanbox_f64(setter);
    let descriptor = object(&scope);
    put(&descriptor, b"get", getter.get_nanbox_f64());
    put(&descriptor, b"set", setter.get_nanbox_f64());
    put(
        &descriptor,
        b"configurable",
        f64::from_bits(crate::value::TAG_TRUE),
    );
    assert_eq!(
        crate::proxy::js_reflect_define_property(
            owner.get_nanbox_f64(),
            key.get_nanbox_f64(),
            descriptor.get_nanbox_f64()
        )
        .to_bits(),
        crate::value::TAG_TRUE
    );
}

fn bytes(value: f64) -> Vec<u8> {
    let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
    let (data, n) = crate::string::str_bytes_from_jsvalue(value, &mut short).unwrap();
    unsafe { std::slice::from_raw_parts(data, n as usize).to_vec() }
}
fn captured<'s>(
    scope: &'s RuntimeHandleScope,
    fp: *const u8,
    arity: u32,
    state: &RuntimeHandle<'_>,
) -> RuntimeHandle<'s> {
    crate::closure::js_register_closure_arity(fp, arity);
    let f = scope.root_nanbox_f64(js_nanbox_pointer(
        crate::closure::js_closure_alloc(fp, 1) as i64
    ));
    crate::closure::js_closure_set_capture_f64(
        js_nanbox_get_pointer(f.get_nanbox_f64()) as *mut _,
        0,
        state.get_nanbox_f64(),
    );
    f
}

fn item(array: &RuntimeHandle<'_>, index: u32) -> f64 {
    crate::array::js_array_get_f64(
        js_nanbox_get_pointer(array.get_nanbox_f64()) as *const crate::array::ArrayHeader,
        index,
    )
}
fn check(array: &RuntimeHandle<'_>, expected: &[Option<&[u8]>]) {
    assert_eq!(get(array, b"length"), expected.len() as f64);
    for (i, part) in expected.iter().enumerate() {
        match part {
            Some(part) => assert_eq!(bytes(item(array, i as u32)), *part, "part {i}"),
            None => assert_eq!(item(array, i as u32).to_bits(), TAG_UNDEFINED, "part {i}"),
        }
    }
}
fn run<'s>(
    scope: &'s RuntimeHandleScope,
    input: &RuntimeHandle<'_>,
    separator: &RuntimeHandle<'_>,
    limit: f64,
) -> RuntimeHandle<'s> {
    scope.root_nanbox_f64(crate::regex::js_string_split_js(
        input.get_nanbox_f64(),
        separator.get_nanbox_f64(),
        limit,
    ))
}
fn getter(owner: &RuntimeHandle<'_>, name: &[u8], value: &RuntimeHandle<'_>) {
    accessor(
        owner,
        js_nanbox_string(crate::string::canonical_key(name) as i64),
        value.get_nanbox_f64(),
        f64::from_bits(TAG_UNDEFINED),
    );
}
extern "C" fn throwing(_: *const crate::closure::ClosureHeader) -> f64 {
    crate::exception::js_throw(983.0)
}

#[test]
fn perex_split_builtin_captures_limits_and_original_last_index() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, b"a1b2");
    let re = regex(&scope, br"(\d)(x)?", b"gd");
    let original_index = object(&scope);
    api::finish(dispatch::set_last_index(
        &re,
        original_index.get_nanbox_f64(),
    ));
    let out = run(&scope, &input, &re, -1.0);
    check(
        &out,
        &[
            Some(b"a"),
            Some(b"1"),
            None,
            Some(b"b"),
            Some(b"2"),
            None,
            Some(b""),
        ],
    );
    let limited = run(&scope, &input, &re, 3.0);
    check(&limited, &[Some(b"a"), Some(b"1"), None]);
    assert_eq!(
        get(&re, b"lastIndex").to_bits(),
        original_index.get_nanbox_f64().to_bits()
    );
    let method = scope.root_nanbox_f64(api::finish(dispatch::get_symbol(&re, "split")));
    assert_eq!(get(&method, b"length"), 2.0);
}

#[test]
fn perex_split_empty_matches_advance_exact_utf16_with_unicode_flags() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, "😀x".as_bytes());
    for flags in [b"".as_slice(), b"u", b"v"] {
        let local = RuntimeHandleScope::new();
        let re = regex(&local, b"(?:)", flags);
        let result = run(&local, &input, &re, f64::from_bits(TAG_UNDEFINED));
        if flags.is_empty() {
            check(
                &result,
                &[Some(b"\xed\xa0\xbd"), Some(b"\xed\xb8\x80"), Some(b"x")],
            );
        } else {
            check(&result, &[Some("😀".as_bytes()), Some(b"x")]);
        }
    }
    let empty = text(&scope, b"");
    for (pattern, count) in [(b"a".as_slice(), 1.0), (b"a*", 0.0)] {
        let local = RuntimeHandleScope::new();
        let re = regex(&local, pattern, b"");
        let result = run(&local, &empty, &re, -1.0);
        assert_eq!(get(&result, b"length"), count);
    }
}

#[test]
fn perex_split_literal_half_pairs_nonoverlap_and_uint32_limits() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, "😀😀".as_bytes());
    let low = text(&scope, b"\xed\xb8\x80");
    let result = run(&scope, &input, &low, -1.0);
    check(
        &result,
        &[Some(b"\xed\xa0\xbd"), Some(b"\xed\xa0\xbd"), Some(b"")],
    );
    let input = text(&scope, b"aaaaa");
    let needle = text(&scope, b"aa");
    let result = run(&scope, &input, &needle, -1.0);
    check(&result, &[Some(b""), Some(b""), Some(b"a")]);
    for (limit, count) in [
        (4_294_967_297.0, 1.0),
        (-1.0, 3.0),
        (f64::INFINITY, 0.0),
        (f64::NAN, 0.0),
        (f64::from_bits(TAG_NULL), 0.0),
    ] {
        let local = RuntimeHandleScope::new();
        let result = run(&local, &input, &needle, limit);
        assert_eq!(get(&result, b"length"), count);
    }
    // Adjacent non-ASCII pieces use retained offsets, under one work budget.
    let large = text(&scope, "😀".repeat(2048).as_bytes());
    let empty = text(&scope, b"");
    let result = run(&scope, &large, &empty, -1.0);
    assert_eq!(get(&result, b"length"), 4096.0);
    assert_eq!(bytes(item(&result, 4095)), b"\xed\xb8\x80");
}

extern "C" fn hook(_: *const crate::closure::ClosureHeader, input: f64, limit: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let input = scope.root_nanbox_f64(input);
    let limit = scope.root_nanbox_f64(limit);
    let before = input.get_nanbox_f64().to_bits();
    gc_collect_minor();
    assert_ne!(before, input.get_nanbox_f64().to_bits());
    assert!(crate::proxy::reflect_value_is_object(
        limit.get_nanbox_f64()
    ));
    input.get_nanbox_f64()
}
extern "C" fn primitive_hook(_: *const crate::closure::ClosureHeader) -> f64 {
    assert_eq!(crate::object::js_implicit_this_get(), 23.0);
    gc_collect_minor();
    crate::closure::js_register_closure_arity(hook as *const u8, 2);
    js_nanbox_pointer(crate::closure::js_closure_alloc_singleton(hook as *const u8) as i64)
}

#[test]
fn perex_split_symbol_hook_precedes_coercion_and_preserves_arbitrary_result() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = object(&scope);
    let lim = object(&scope);
    let throws = function(&scope, throwing as *const u8, 0);
    put(&input, b"toString", throws.get_nanbox_f64());
    put(&lim, b"valueOf", throws.get_nanbox_f64());
    let sep = object(&scope);
    let method = function(&scope, hook as *const u8, 2);
    symbol(&sep, "split", method.get_nanbox_f64());
    let result = run(&scope, &input, &sep, lim.get_nanbox_f64());
    assert_eq!(
        result.get_nanbox_f64().to_bits(),
        input.get_nanbox_f64().to_bits()
    );
    // Exact Node 26.5.1 skips primitive-prototype hooks. Keep the original
    // throwing receiver: its coercion must now be reached and propagated.
    let proto = scope.root_nanbox_f64(crate::object::builtin_prototype_value("Number"));
    let get_hook = function(&scope, primitive_hook as *const u8, 0);
    let key = js_nanbox_pointer(crate::symbol::well_known_symbol("split") as i64);
    accessor(
        &proto,
        key,
        get_hook.get_nanbox_f64(),
        f64::from_bits(TAG_UNDEFINED),
    );
    let fresh = object(&scope);
    put(&fresh, b"toString", throws.get_nanbox_f64());
    let result = crate::exception::catch_js_throw(|| {
        crate::regex::js_string_split_js(fresh.get_nanbox_f64(), 23.0, lim.get_nanbox_f64())
    });
    assert_eq!(result.unwrap_err(), 983.0);
    let key = scope.root_nanbox_f64(js_nanbox_pointer(
        crate::symbol::well_known_symbol("split") as i64
    ));
    assert_eq!(
        crate::proxy::js_reflect_delete(proto.get_nanbox_f64(), key.get_nanbox_f64()).to_bits(),
        crate::value::TAG_TRUE
    );
}

thread_local! { static ORDER: Cell<u64> = const { Cell::new(0) }; }
fn event(n: u64) {
    ORDER.with(|o| o.set(o.get() * 10 + n));
}

extern "C" fn matrix_third(_: *const crate::closure::ClosureHeader) -> f64 {
    field_event(2, b"value")
}

extern "C" fn matrix_hook(_: *const crate::closure::ClosureHeader, input: f64, _: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let input = scope.root_nanbox_f64(input);
    let receiver = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    assert!(crate::proxy::reflect_value_is_object(
        receiver.get_nanbox_f64()
    ));
    let before = receiver.get_nanbox_f64().to_bits();
    event(4);
    gc_collect_minor();
    assert_ne!(receiver.get_nanbox_f64().to_bits(), before);
    input.get_nanbox_f64()
}

extern "C" fn matrix_hook_getter(_: *const crate::closure::ClosureHeader) -> f64 {
    assert!(crate::proxy::reflect_value_is_object(
        crate::object::js_implicit_this_get()
    ));
    event(3);
    gc_collect_minor();
    crate::closure::js_register_closure_arity(matrix_hook as *const u8, 2);
    js_nanbox_pointer(crate::closure::js_closure_alloc_singleton(matrix_hook as *const u8) as i64)
}

fn matrix_call(method: &str, receiver: f64, argument: f64, third: f64) -> f64 {
    match method {
        "match" => crate::regex::js_string_match_js(receiver, argument),
        "matchAll" => crate::regex::js_string_match_all_js(receiver, argument),
        "search" => crate::regex::js_string_search_js(receiver, argument),
        "replace" => crate::regex::js_string_replace_js(receiver, argument, third),
        "replaceAll" => crate::regex::js_string_replace_all_js(receiver, argument, third),
        "split" => crate::regex::js_string_split_js(receiver, argument, third),
        _ => unreachable!(),
    }
}

#[test]
fn perex_string_methods_ignore_primitive_hooks_and_preserve_boxed_hooks_after_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    // Sixty complete cases mirror the exact Node 26.5.1 hook matrix. Both
    // sides include Number, Boolean, String, BigInt and Symbol arguments.
    for method in [
        "match",
        "matchAll",
        "search",
        "replace",
        "replaceAll",
        "split",
    ] {
        for kind in ["Number", "Boolean", "String", "BigInt", "Symbol"] {
            for boxed in [false, true] {
                let scope = RuntimeHandleScope::new();
                let primitive = match kind {
                    "Number" => scope.root_nanbox_f64(23.0),
                    "Boolean" => scope.root_nanbox_f64(f64::from_bits(crate::value::TAG_TRUE)),
                    "String" => text(&scope, b"23"),
                    "BigInt" => scope.root_nanbox_f64(crate::value::js_nanbox_bigint(
                        crate::bigint::js_bigint_from_i64(23) as i64,
                    )),
                    "Symbol" => {
                        scope.root_nanbox_f64(unsafe { crate::symbol::js_symbol_new_empty() })
                    }
                    _ => unreachable!(),
                };
                let argument = scope.root_nanbox_f64(if boxed {
                    crate::object::js_object_coerce(primitive.get_nanbox_f64())
                } else {
                    primitive.get_nanbox_f64()
                });
                assert_eq!(
                    crate::proxy::reflect_value_is_object(argument.get_nanbox_f64()),
                    boxed
                );
                let proto = scope.root_nanbox_f64(crate::object::builtin_prototype_value(kind));
                let symbol_name = if method == "replaceAll" {
                    "replace"
                } else {
                    method
                };
                let key = scope.root_nanbox_f64(js_nanbox_pointer(
                    crate::symbol::well_known_symbol(symbol_name) as i64,
                ));
                let getter = function(&scope, matrix_hook_getter as *const u8, 0);
                accessor(
                    &proto,
                    key.get_nanbox_f64(),
                    getter.get_nanbox_f64(),
                    f64::from_bits(TAG_UNDEFINED),
                );
                let input = object(&scope);
                let source = text(&scope, b"x23y23true");
                put(&input, b"text", source.get_nanbox_f64());
                let convert = function(&scope, input_text as *const u8, 0);
                put(&input, b"toString", convert.get_nanbox_f64());
                let third = object(&scope);
                let replacement = text(&scope, b"R");
                put(
                    &third,
                    b"value",
                    if method == "split" {
                        2.0
                    } else {
                        replacement.get_nanbox_f64()
                    },
                );
                let convert = function(&scope, matrix_third as *const u8, 0);
                put(
                    &third,
                    if method == "split" {
                        b"valueOf"
                    } else {
                        b"toString"
                    },
                    convert.get_nanbox_f64(),
                );
                ORDER.with(|o| o.set(0));
                let result = crate::exception::catch_js_throw(|| {
                    matrix_call(
                        method,
                        input.get_nanbox_f64(),
                        argument.get_nanbox_f64(),
                        third.get_nanbox_f64(),
                    )
                });
                let result = result
                    .map(|r| scope.root_nanbox_f64(r))
                    .map_err(|e| scope.root_nanbox_f64(e));
                assert_eq!(
                    crate::proxy::js_reflect_delete(proto.get_nanbox_f64(), key.get_nanbox_f64())
                        .to_bits(),
                    crate::value::TAG_TRUE
                );
                if boxed {
                    assert_eq!(
                        result
                            .unwrap_or_else(|_| panic!("boxed hook failed: {kind} {method}"))
                            .get_nanbox_f64()
                            .to_bits(),
                        input.get_nanbox_f64().to_bits(),
                        "boxed {kind} {method}"
                    );
                    ORDER.with(|o| assert_eq!(o.get(), 34, "boxed {kind} {method}"));
                    continue;
                }
                if kind == "Symbol" {
                    let error = match result {
                        Err(error) => error,
                        Ok(_) => panic!("Symbol must fail abstract ToString"),
                    };
                    assert_eq!(bytes(get(&error, b"name")), b"TypeError");
                    ORDER.with(|o| assert_eq!(o.get(), if method == "split" { 12 } else { 1 }));
                    continue;
                }
                let result = result
                    .unwrap_or_else(|_| panic!("primitive operation failed: {kind} {method}"));
                let boolean = kind == "Boolean";
                match method {
                    "match" => {
                        check(&result, &[Some(if boolean { b"true" } else { b"23" })]);
                        assert_eq!(get(&result, b"index"), if boolean { 6.0 } else { 1.0 });
                    }
                    "search" => {
                        assert_eq!(result.get_nanbox_f64(), if boolean { 6.0 } else { 1.0 })
                    }
                    "replace" => assert_eq!(
                        bytes(result.get_nanbox_f64()),
                        if boolean {
                            b"x23y23R".as_slice()
                        } else {
                            b"xRy23true"
                        }
                    ),
                    "replaceAll" => assert_eq!(
                        bytes(result.get_nanbox_f64()),
                        if boolean {
                            b"x23y23R".as_slice()
                        } else {
                            b"xRyRtrue"
                        }
                    ),
                    "split" => check(
                        &result,
                        if boolean {
                            &[Some(b"x23y23"), Some(b"")]
                        } else {
                            &[Some(b"x"), Some(b"y")]
                        },
                    ),
                    "matchAll" => {
                        for index in 0..=if boolean { 1 } else { 2 } {
                            let step = scope.root_nanbox_f64(unsafe {
                                crate::regex::dispatch_regexp_string_iterator_method(
                                    js_nanbox_get_pointer(result.get_nanbox_f64())
                                        as *mut crate::object::ObjectHeader,
                                    "next",
                                )
                            });
                            let done = index == if boolean { 1 } else { 2 };
                            assert_eq!(
                                get(&step, b"done").to_bits(),
                                if done {
                                    crate::value::TAG_TRUE
                                } else {
                                    crate::value::TAG_FALSE
                                }
                            );
                            if !done {
                                let value = scope.root_nanbox_f64(get(&step, b"value"));
                                check(&value, &[Some(if boolean { b"true" } else { b"23" })]);
                                assert_eq!(
                                    get(&value, b"index"),
                                    if boolean { 6.0 } else { (1 + index * 3) as f64 }
                                );
                            }
                        }
                    }
                    _ => unreachable!(),
                }
                ORDER.with(|o| {
                    assert_eq!(
                        o.get(),
                        if matches!(method, "split" | "replace" | "replaceAll") {
                            12
                        } else {
                            1
                        },
                        "{kind} {method}"
                    )
                });
            }
        }
    }
}

fn field_event(n: u64, name: &[u8]) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    event(n);
    gc_collect_minor();
    get(&receiver, name)
}
extern "C" fn input_text(_: *const crate::closure::ClosureHeader) -> f64 {
    field_event(1, b"text")
}
extern "C" fn constructor_get(_: *const crate::closure::ClosureHeader) -> f64 {
    field_event(2, b"holder")
}
extern "C" fn species_get(_: *const crate::closure::ClosureHeader) -> f64 {
    field_event(3, b"factory")
}
extern "C" fn flags_get(_: *const crate::closure::ClosureHeader) -> f64 {
    field_event(4, b"flagText")
}
extern "C" fn limit_get(_: *const crate::closure::ClosureHeader) -> f64 {
    field_event(6, b"number")
}
extern "C" fn factory(_: *const crate::closure::ClosureHeader, receiver: f64, flags: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let flags = scope.root_nanbox_f64(flags);
    event(5);
    gc_collect_minor();
    let matcher = scope.root_nanbox_f64(get(&receiver, b"matcher"));
    put(&matcher, b"seenFlags", flags.get_nanbox_f64());
    matcher.get_nanbox_f64()
}
extern "C" fn empty_exec(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    event(7);
    gc_collect_minor();
    f64::from_bits(TAG_NULL)
}

#[test]
fn perex_split_species_order_zero_limit_and_empty_input() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = object(&scope);
    let input = object(&scope);
    let holder = object(&scope);
    let matcher = object(&scope);
    let lim = object(&scope);
    let empty = text(&scope, b"");
    let flags = text(&scope, b"v");
    put(&input, b"text", empty.get_nanbox_f64());
    put(&receiver, b"holder", holder.get_nanbox_f64());
    put(&receiver, b"matcher", matcher.get_nanbox_f64());
    put(&receiver, b"flagText", flags.get_nanbox_f64());
    let ctor = function(&scope, factory as *const u8, 2);
    put(&holder, b"factory", ctor.get_nanbox_f64());
    for (owner, name, fp) in [
        (&input, b"toString".as_slice(), input_text as *const u8),
        (&lim, b"valueOf", limit_get as *const u8),
    ] {
        let f = function(&scope, fp, 0);
        put(owner, name, f.get_nanbox_f64());
    }
    for (owner, name, fp) in [
        (
            &receiver,
            b"constructor".as_slice(),
            constructor_get as *const u8,
        ),
        (&receiver, b"flags", flags_get as *const u8),
    ] {
        let f = function(&scope, fp, 0);
        getter(owner, name, &f);
    }
    let f = function(&scope, species_get as *const u8, 0);
    accessor(
        &holder,
        js_nanbox_pointer(crate::symbol::well_known_symbol("species") as i64),
        f.get_nanbox_f64(),
        f64::from_bits(TAG_UNDEFINED),
    );
    let f = function(&scope, empty_exec as *const u8, 1);
    put(&matcher, b"exec", f.get_nanbox_f64());
    let f = function(&scope, throwing as *const u8, 0);
    getter(&receiver, b"lastIndex", &f);
    getter(&matcher, b"lastIndex", &f);
    for (number, expected, count) in [(0.0, 123456, 0.0), (1.0, 1234567, 1.0)] {
        ORDER.with(|o| o.set(0));
        put(&lim, b"number", number);
        let out = scope.root_nanbox_f64(api::finish(split::regexp(
            receiver.get_nanbox_f64(),
            input.get_nanbox_f64(),
            lim.get_nanbox_f64(),
        )));
        assert_eq!(ORDER.with(Cell::get), expected);
        assert_eq!(get(&out, b"length"), count);
        assert_eq!(bytes(get(&matcher, b"seenFlags")), b"vy");
    }
}

extern "C" fn custom_exec(c: *const crate::closure::ClosureHeader, input: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let state = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(c, 0));
    let input = scope.root_nanbox_f64(input);
    let receiver = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    gc_collect_minor();
    assert_eq!(
        input.get_nanbox_f64().to_bits(),
        get(&state, b"input").to_bits()
    );
    let at = get(&receiver, b"lastIndex");
    if at != 1.0 {
        return f64::from_bits(TAG_NULL);
    }
    api::finish(dispatch::set_last_index(&receiver, 2.0));
    // Reenter through literal split while the outer original input is retained.
    let needle = text(&scope, b"b");
    let nested = run(&scope, &input, &needle, -1.0);
    check(&nested, &[Some(b"a"), Some(b"c")]);
    get(&state, b"result")
}
extern "C" fn capture_get(c: *const crate::closure::ClosureHeader) -> f64 {
    let scope = RuntimeHandleScope::new();
    let state = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(c, 0));
    gc_collect_minor();
    get(&state, b"capture")
}

#[test]
fn perex_split_custom_exec_capture_values_reentrancy_and_limit_short_circuit() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let re = object(&scope);
    let matcher = object(&scope);
    let holder = object(&scope);
    let state = object(&scope);
    let result = object(&scope);
    let capture = object(&scope);
    let input = text(&scope, b"abc");
    let flags = text(&scope, b"y");
    put(&re, b"matcher", matcher.get_nanbox_f64());
    put(&re, b"constructor", holder.get_nanbox_f64());
    put(&re, b"flags", flags.get_nanbox_f64());
    let ctor = function(&scope, factory as *const u8, 2);
    symbol(&holder, "species", ctor.get_nanbox_f64());
    put(&state, b"input", input.get_nanbox_f64());
    put(&state, b"result", result.get_nanbox_f64());
    put(&state, b"capture", capture.get_nanbox_f64());
    let exec = captured(&scope, custom_exec as *const u8, 1, &state);
    put(&matcher, b"exec", exec.get_nanbox_f64());
    put(&result, b"length", 3.0);
    let getter_fn = captured(&scope, capture_get as *const u8, 0, &state);
    getter(&result, b"1", &getter_fn);
    let throws = function(&scope, throwing as *const u8, 0);
    getter(&result, b"0", &throws);
    getter(&result, b"index", &throws);
    put(&capture, b"toString", throws.get_nanbox_f64());
    let before = input.get_nanbox_f64().to_bits();
    let out = scope.root_nanbox_f64(api::finish(split::regexp(
        re.get_nanbox_f64(),
        input.get_nanbox_f64(),
        -1.0,
    )));
    assert_ne!(before, input.get_nanbox_f64().to_bits());
    assert_eq!(get(&out, b"length"), 4.0);
    assert_eq!(bytes(item(&out, 0)), b"a");
    assert_eq!(item(&out, 1).to_bits(), capture.get_nanbox_f64().to_bits());
    assert_eq!(item(&out, 2).to_bits(), TAG_UNDEFINED);
    assert_eq!(bytes(item(&out, 3)), b"c");
    getter(&result, b"length", &throws);
    let out = scope.root_nanbox_f64(api::finish(split::regexp(
        re.get_nanbox_f64(),
        input.get_nanbox_f64(),
        1.0,
    )));
    check(&out, &[Some(b"a")]);
}

extern "C" fn throwing_hook(_: *const crate::closure::ClosureHeader, _: f64, _: f64) -> f64 {
    gc_collect_minor();
    crate::exception::js_throw(984.0)
}
#[test]
fn perex_split_collecting_throw_releases_native_arguments_and_restores_this() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, b"abc");
    let sep = object(&scope);
    let method = function(&scope, throwing_hook as *const u8, 2);
    symbol(&sep, "split", method.get_nanbox_f64());
    let previous = object(&scope);
    let displaced = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    let roots = RuntimeHandleScope::active_len_for_tests();
    let live = external_side_live_bytes();
    let before = previous.get_nanbox_f64().to_bits();
    let error = crate::exception::catch_js_throw(|| {
        crate::regex::js_string_split_js(input.get_nanbox_f64(), sep.get_nanbox_f64(), -1.0)
    })
    .unwrap_err();
    assert_eq!(error, 984.0);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(external_side_live_bytes(), live);
    assert_ne!(previous.get_nanbox_f64().to_bits(), before);
    assert_eq!(
        crate::object::js_implicit_this_get().to_bits(),
        previous.get_nanbox_f64().to_bits()
    );
    crate::object::js_implicit_this_set(displaced.get_nanbox_f64());
}

#[test]
fn perex_split_raw_payloads_preserve_bytes_and_use_nonempty_byte_delimiters() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, b"a\x80b\x80c");
    // This stored delimiter has zero cached UTF-16 units, but one byte.
    let needle = text(&scope, b"\x80");
    let result = run(&scope, &input, &needle, -1.0);
    check(&result, &[Some(b"a"), Some(b"b"), Some(b"c")]);
    let limited = run(&scope, &input, &needle, 2.0);
    check(&limited, &[Some(b"a"), Some(b"b")]);

    let mut source = vec![0x80];
    source.extend(std::iter::repeat_n(b'a', 4096));
    source.push(b'b');
    let mut delimiter = vec![b'a'; 2048];
    delimiter.push(b'b');
    let input = text(&scope, &source);
    let needle = text(&scope, &delimiter);
    let live = external_side_live_bytes();
    let result = run(&scope, &input, &needle, -1.0);
    check(&result, &[Some(&source[..2049]), Some(b"")]);
    assert_eq!(
        external_side_live_bytes(),
        live,
        "KMP scratch must be released"
    );
}

#[test]
fn perex_split_raw_copy_reacquires_moved_storage_and_cleans_up_partial_output() {
    use crate::regex::perex_literal_bytes::copy_bytes;
    use crate::regex::perex_runtime::EngineError;
    use perex::{executor::ExecError, Budget};
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let mut source = vec![b'a'; 4096];
    source[0] = 0x80;
    source[15..18].copy_from_slice(b"\xed\xa0\x80");
    source[4095] = 0xf0;
    let input = scope.root_string_ptr(crate::string::js_string_from_bytes(
        source.as_ptr(),
        source.len() as u32,
    ));
    let before = input.get_raw_const_ptr::<crate::StringHeader>() as usize;
    let roots = RuntimeHandleScope::active_len_for_tests();
    let live = external_side_live_bytes();
    let mut polls = 0;
    {
        let local = RuntimeHandleScope::new();
        let result = copy_bytes(
            &input,
            0,
            source.len(),
            &mut Budget::new(1_000_000),
            17,
            &mut || {
                polls += 1;
                gc_collect_minor();
                Ok(())
            },
        )
        .unwrap();
        let result = local.root_string_ptr(result);
        assert_eq!(
            bytes(js_nanbox_string(
                result.get_raw_const_ptr::<crate::StringHeader>() as i64
            )),
            source
        );
        result.with_const_ptr::<crate::StringHeader, _>(|p| unsafe {
            assert_eq!((*p).utf16_len, 4094);
            assert_ne!(
                (*p).flags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES,
                0
            );
        });
    }
    assert!(polls > 20);
    assert_ne!(
        input.get_raw_const_ptr::<crate::StringHeader>() as usize,
        before
    );
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(external_side_live_bytes(), live);
    // 242 polls finish measurement; this cancellation occurs while a rooted,
    // partly initialized final result exists in the second pass.
    let mut polls = 0;
    let result = copy_bytes(
        &input,
        0,
        source.len(),
        &mut Budget::new(1_000_000),
        17,
        &mut || {
            polls += 1;
            gc_collect_minor();
            if polls == 245 {
                Err(EngineError::Cancelled)
            } else {
                Ok(())
            }
        },
    );
    assert!(matches!(result, Err(EngineError::Cancelled)));
    assert_eq!(polls, 245);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(external_side_live_bytes(), live);
    let result = copy_bytes(
        &input,
        0,
        source.len(),
        &mut Budget::new(source.len() + 17),
        17,
        &mut || Ok(()),
    );
    assert!(matches!(
        result,
        Err(EngineError::Execution(ExecError::WorkLimit))
    ));
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(external_side_live_bytes(), live);
}

extern "C" fn primitive_result(c: *const crate::closure::ClosureHeader) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(c, 0));
    gc_collect_minor();
    value.get_nanbox_f64()
}
extern "C" fn primitive_number_hint(c: *const crate::closure::ClosureHeader, hint: f64) -> f64 {
    assert_eq!(bytes(hint), b"number");
    primitive_result(c)
}
extern "C" fn primitive_string_hint(c: *const crate::closure::ClosureHeader, hint: f64) -> f64 {
    assert_eq!(bytes(hint), b"string");
    primitive_result(c)
}

#[test]
fn perex_abstract_string_conversion_rejects_symbols_after_collecting_object_hooks() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let symbol_value = scope.root_nanbox_f64(unsafe { crate::symbol::js_symbol_new_empty() });
    let string_value = text(&scope, b"23");
    let previous = object(&scope);
    let displaced = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    for value in [&symbol_value, &string_value] {
        for mode in 0..4 {
            let local = RuntimeHandleScope::new();
            let input = if mode == 0 {
                local.root_nanbox_f64(value.get_nanbox_f64())
            } else {
                let input = object(&local);
                let method = captured(
                    &local,
                    if mode == 1 {
                        primitive_string_hint as *const u8
                    } else {
                        primitive_result as *const u8
                    },
                    if mode == 1 { 1 } else { 0 },
                    value,
                );
                if mode == 1 {
                    symbol(&input, "toPrimitive", method.get_nanbox_f64());
                } else if mode == 2 {
                    put(&input, b"toString", method.get_nanbox_f64());
                } else {
                    let other_object = object(&local);
                    let nonprimitive =
                        captured(&local, primitive_result as *const u8, 0, &other_object);
                    put(&input, b"toString", nonprimitive.get_nanbox_f64());
                    put(&input, b"valueOf", method.get_nanbox_f64());
                }
                input
            };
            let result = dispatch::to_string(&input);
            if unsafe { crate::symbol::js_is_symbol(value.get_nanbox_f64()) } != 0 {
                assert!(
                    matches!(
                        result,
                        Err(crate::regex::perex_runtime::EngineError::Type(_))
                    ),
                    "mode {mode}"
                );
            } else {
                let result = local.root_string_ptr(result.unwrap());
                assert_eq!(
                    bytes(js_nanbox_string(
                        result.get_raw_const_ptr::<crate::StringHeader>() as i64
                    )),
                    b"23"
                );
            }
            assert_eq!(
                crate::object::js_implicit_this_get().to_bits(),
                previous.get_nanbox_f64().to_bits()
            );
        }
    }
    crate::object::js_implicit_this_set(displaced.get_nanbox_f64());
}

#[test]
fn perex_construction_uses_strict_string_conversion_for_pattern_and_flags() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let symbol_value = scope.root_nanbox_f64(unsafe { crate::symbol::js_symbol_new_empty() });
    for converted in [false, true] {
        let local = RuntimeHandleScope::new();
        let value = if converted {
            let value = object(&local);
            let method = captured(&local, primitive_string_hint as *const u8, 1, &symbol_value);
            symbol(&value, "toPrimitive", method.get_nanbox_f64());
            value
        } else {
            local.root_nanbox_f64(symbol_value.get_nanbox_f64())
        };
        let source = text(&local, b"a");
        for bad_flags in [false, true] {
            for operation in 0..3 {
                let re = regex(&local, b"a", b"g");
                let ptr = || {
                    js_nanbox_get_pointer(re.get_nanbox_f64()) as *mut crate::regex::RegExpHeader
                };
                crate::regex::js_regexp_set_last_index(ptr(), 9.0);
                let error = crate::exception::catch_js_throw(|| {
                    let pattern = if bad_flags {
                        source.get_nanbox_f64()
                    } else {
                        value.get_nanbox_f64()
                    };
                    let flags = if bad_flags {
                        value.get_nanbox_f64()
                    } else {
                        f64::from_bits(TAG_UNDEFINED)
                    };
                    match operation {
                        0 => {
                            crate::regex::js_regexp_construct(pattern, flags);
                        }
                        1 => {
                            crate::regex::js_regexp_construct_call(pattern, flags);
                        }
                        _ => {
                            crate::regex::js_regexp_compile_value(ptr(), pattern, flags);
                        }
                    }
                })
                .expect_err("Symbol conversion must throw before publishing a program");
                let error = local.root_nanbox_f64(error);
                assert_eq!(bytes(get(&error, b"name")), b"TypeError");
                assert_eq!(bytes(get(&re, b"source")), b"a");
                assert_eq!(bytes(get(&re, b"flags")), b"g");
                assert_eq!(get(&re, b"lastIndex"), 9.0);
            }
        }
    }
}
extern "C" fn separator_text(_: *const crate::closure::ClosureHeader) -> f64 {
    field_event(8, b"text")
}

#[test]
fn perex_split_fallback_coercion_order_including_zero_limit() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = object(&scope);
    let sep = object(&scope);
    let lim = object(&scope);
    let abc = text(&scope, b"abc");
    let b = text(&scope, b"b");
    put(&input, b"text", abc.get_nanbox_f64());
    put(&sep, b"text", b.get_nanbox_f64());
    for (owner, name, fp) in [
        (&input, b"toString".as_slice(), input_text as *const u8),
        (&sep, b"toString", separator_text as *const u8),
        (&lim, b"valueOf", limit_get as *const u8),
    ] {
        let f = function(&scope, fp, 0);
        put(owner, name, f.get_nanbox_f64());
    }
    for limit in [0.0, 1.0] {
        put(&lim, b"number", limit);
        ORDER.with(|o| o.set(0));
        let out = run(&scope, &input, &sep, lim.get_nanbox_f64());
        assert_eq!(ORDER.with(Cell::get), 168);
        assert_eq!(get(&out, b"length"), limit);
    }
    // A throwing separator conversion is still observed with limit zero.
    let throws = function(&scope, throwing as *const u8, 0);
    put(&sep, b"toString", throws.get_nanbox_f64());
    let err = crate::exception::catch_js_throw(|| {
        crate::regex::js_string_split_js(input.get_nanbox_f64(), sep.get_nanbox_f64(), 0.0)
    })
    .unwrap_err();
    assert_eq!(err, 983.0);
}

#[test]
fn perex_numeric_arguments_reject_bigint_after_observable_primitive_conversion() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let big = scope.root_nanbox_f64(crate::value::js_nanbox_bigint(
        crate::bigint::js_bigint_from_i64(1) as i64,
    ));
    let ordinary = object(&scope);
    let f = captured(&scope, primitive_result as *const u8, 0, &big);
    put(&ordinary, b"valueOf", f.get_nanbox_f64());
    let exotic = object(&scope);
    let f = captured(&scope, primitive_number_hint as *const u8, 1, &big);
    symbol(&exotic, "toPrimitive", f.get_nanbox_f64());
    let input = text(&scope, b"abc");
    let separator = object(&scope);
    let throws = function(&scope, throwing as *const u8, 0);
    put(&separator, b"toString", throws.get_nanbox_f64());
    let re = regex(&scope, b"a", b"");
    for value in [&big, &ordinary, &exotic] {
        assert!(matches!(
            split::string(
                input.get_nanbox_f64(),
                separator.get_nanbox_f64(),
                value.get_nanbox_f64()
            ),
            Err(crate::regex::perex_runtime::EngineError::Type(_))
        ));
        assert!(matches!(
            dispatch::to_length(value),
            Err(crate::regex::perex_runtime::EngineError::Type(_))
        ));
        api::finish(dispatch::set_last_index(&re, value.get_nanbox_f64()));
        // Builtin exec must coerce lastIndex even without g/y.
        assert!(matches!(
            dispatch::test_string(
                re.get_nanbox_f64(),
                crate::value::js_get_string_pointer_unified(input.get_nanbox_f64())
                    as *const crate::StringHeader
            ),
            Err(crate::regex::perex_runtime::EngineError::Abrupt(_))
        ));
    }
    // ToLength clamps to the exact ECMAScript maximum, independently of usize.
    let huge = scope.root_nanbox_f64(f64::INFINITY);
    assert_eq!(
        api::finish(dispatch::to_length(&huge)),
        9_007_199_254_740_991.0
    );
    // An overridden array valueOf precedes inherited toString/join.
    let array = scope.root_nanbox_f64(js_nanbox_pointer(crate::array::js_array_alloc(0) as i64));
    let f = captured(&scope, primitive_result as *const u8, 0, &big);
    put(&array, b"valueOf", f.get_nanbox_f64());
    assert!(matches!(
        dispatch::to_number(&array),
        Err(crate::regex::perex_runtime::EngineError::Type(_))
    ));
}
