//! Replacement through the public boxed ABI and actual moving collection.
use super::*;
use crate::regex::{perex_api as api, perex_dispatch as dispatch, perex_replace as replace};
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
extern "C" fn throw_text(_: *const crate::closure::ClosureHeader) -> f64 {
    crate::exception::js_throw(971.0)
}
extern "C" fn hook(_: *const crate::closure::ClosureHeader, input: f64, replacement: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let input = scope.root_nanbox_f64(input);
    let replacement = scope.root_nanbox_f64(replacement);
    gc_collect_minor();
    assert_eq!(replacement.get_nanbox_f64(), 47.0);
    input.get_nanbox_f64()
}
extern "C" fn builtin_callback(
    c: *const crate::closure::ClosureHeader,
    matched: f64,
    first: f64,
    absent: f64,
    position: f64,
    input: f64,
    groups: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let state = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(c, 0));
    let matched = scope.root_nanbox_f64(matched);
    let first = scope.root_nanbox_f64(first);
    let input = scope.root_nanbox_f64(input);
    let groups = scope.root_nanbox_f64(groups);
    gc_collect_minor();
    assert_eq!(
        crate::object::js_implicit_this_get().to_bits(),
        TAG_UNDEFINED
    );
    assert_eq!(
        get(&state, b"lastIndex"),
        if position == 0.0 { 0.0 } else { 77.0 }
    );
    assert_eq!(
        get(&state, b"input").to_bits(),
        input.get_nanbox_f64().to_bits()
    );
    assert_eq!(bytes(first.get_nanbox_f64()), b"a");
    assert_eq!(absent.to_bits(), TAG_UNDEFINED);
    assert_eq!(bytes(get(&groups, b"x")), b"a");
    put(&state, b"calls", get(&state, b"calls") + 1.0);
    api::finish(dispatch::set_last_index(&state, 77.0));
    // Reentrant literal replacement owns its own budget, roots and scratch.
    let needle = text(&scope, b"a");
    let replacement = text(&scope, b"Q");
    assert_eq!(
        bytes(crate::regex::js_string_replace_js(
            matched.get_nanbox_f64(),
            needle.get_nanbox_f64(),
            replacement.get_nanbox_f64()
        )),
        b"Q"
    );
    matched.get_nanbox_f64()
}
extern "C" fn alias_exec(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let state = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    gc_collect_minor();
    let n = get(&state, b"calls");
    put(&state, b"calls", n + 1.0);
    put(&state, b"index", if n == 2.0 { 1.0 } else { n * 2.0 });
    let value = text(&scope, if n == 2.0 { b"Z" } else { b"a" });
    put(&state, b"0", value.get_nanbox_f64());
    if n == 2.0 {
        f64::from_bits(TAG_NULL)
    } else {
        state.get_nanbox_f64()
    }
}
extern "C" fn alias_callback(
    c: *const crate::closure::ClosureHeader,
    matched: f64,
    position: f64,
    input: f64,
    _groups: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let state = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(c, 0));
    let matched = scope.root_nanbox_f64(matched);
    let input = scope.root_nanbox_f64(input);
    gc_collect_minor();
    assert_eq!(
        get(&state, b"calls"),
        3.0,
        "all exec calls must finish first"
    );
    assert_eq!(position, 1.0);
    assert_eq!(
        bytes(matched.get_nanbox_f64()),
        b"Z",
        "retain aliased results, not early snapshots"
    );
    assert_eq!(bytes(input.get_nanbox_f64()), b"abc");
    put(&state, b"replacements", get(&state, b"replacements") + 1.0);
    js_nanbox_string(crate::string::js_string_from_bytes(b"X".as_ptr(), 1) as i64)
}
extern "C" fn named_getter(c: *const crate::closure::ClosureHeader) -> f64 {
    let scope = RuntimeHandleScope::new();
    let state = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(c, 0));
    gc_collect_minor();
    assert_eq!(get(&state, b"calls"), 3.0);
    let n = get(&state, b"replacements") + 1.0;
    put(&state, b"replacements", n);
    n
}
extern "C" fn collecting_throw(
    _: *const crate::closure::ClosureHeader,
    _: f64,
    _: f64,
    _: f64,
) -> f64 {
    gc_collect_minor();
    crate::exception::js_throw(972.0)
}

#[test]
fn perex_replace_templates_named_unset_and_number_fallback() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, b"abc");
    let re = regex(&scope, b"(?<mid>b)(x)?", b"");
    let template = text(
        &scope,
        b"[$$][$&][$`][$'][$1][$2][$01][$10][$00][$<mid>][$<missing>]",
    );
    let result = crate::regex::js_string_replace_js(
        input.get_nanbox_f64(),
        re.get_nanbox_f64(),
        template.get_nanbox_f64(),
    );
    assert_eq!(bytes(result), b"a[$][b][a][c][b][][b][b0][$00][b][]c");
    let method = scope.root_nanbox_f64(api::finish(dispatch::get_symbol(&re, "replace")));
    assert_eq!(get(&method, b"length"), 2.0);
}

#[test]
fn perex_replace_exact_empty_and_half_pair_literal_positions() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, "😀".as_bytes());
    let dash = text(&scope, b"-");
    for (flags, expected) in [
        (b"g".as_slice(), b"-\xed\xa0\xbd-\xed\xb8\x80-".as_slice()),
        (b"gu", "-😀-".as_bytes()),
    ] {
        let local = RuntimeHandleScope::new();
        let re = regex(&local, b"(?:)", flags);
        let output = crate::regex::js_string_replace_all_js(
            input.get_nanbox_f64(),
            re.get_nanbox_f64(),
            dash.get_nanbox_f64(),
        );
        assert_eq!(bytes(output), expected);
        assert_eq!(get(&re, b"lastIndex"), 0.0);
    }
    let low = text(&scope, b"\xed\xb8\x80");
    assert_eq!(
        bytes(crate::regex::js_string_replace_all_js(
            input.get_nanbox_f64(),
            low.get_nanbox_f64(),
            dash.get_nanbox_f64()
        )),
        b"\xed\xa0\xbd-"
    );
    let input = text(&scope, "éééXééX".as_bytes());
    let needle = text(&scope, "ééX".as_bytes());
    assert_eq!(
        bytes(crate::regex::js_string_replace_all_js(
            input.get_nanbox_f64(),
            needle.get_nanbox_f64(),
            dash.get_nanbox_f64()
        )),
        "é--".as_bytes()
    );
}

#[test]
fn perex_replace_callbacks_observe_completed_global_exec_and_original_input() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, b"aba");
    let re = regex(&scope, b"(?<x>a)(z)?", b"g");
    put(&re, b"input", input.get_nanbox_f64());
    put(&re, b"calls", 0.0);
    let callback = captured(&scope, builtin_callback as *const u8, 6, &re);
    let before = input.get_nanbox_f64().to_bits();
    let result = scope.root_nanbox_f64(crate::regex::js_string_replace_all_js(
        input.get_nanbox_f64(),
        re.get_nanbox_f64(),
        callback.get_nanbox_f64(),
    ));
    assert_eq!(bytes(result.get_nanbox_f64()), b"aba");
    assert_eq!(get(&re, b"calls"), 2.0);
    assert_ne!(
        input.get_nanbox_f64().to_bits(),
        before,
        "input must actually move"
    );
}

fn alias_state<'s>(scope: &'s RuntimeHandleScope) -> RuntimeHandle<'s> {
    let state = object(scope);
    let flags = text(scope, b"g");
    put(&state, b"flags", flags.get_nanbox_f64());
    put(&state, b"calls", 0.0);
    put(&state, b"replacements", 0.0);
    put(&state, b"length", 1.0);
    let exec = function(scope, alias_exec as *const u8, 1);
    put(&state, b"exec", exec.get_nanbox_f64());
    state
}
#[test]
fn perex_replace_retains_aliases_and_calls_replacer_for_ignored_overlap() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let state = alias_state(&scope);
    let callback = captured(&scope, alias_callback as *const u8, 4, &state);
    let input = text(&scope, b"abc");
    assert_eq!(
        bytes(api::finish(replace::regexp(
            state.get_nanbox_f64(),
            input.get_nanbox_f64(),
            callback.get_nanbox_f64()
        ))),
        b"aXc"
    );
    assert_eq!(get(&state, b"replacements"), 2.0);
}

#[test]
fn perex_replace_named_getters_run_for_ignored_overlap_and_coerce_values() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let state = alias_state(&scope);
    let groups = object(&scope);
    let method = captured(&scope, named_getter as *const u8, 0, &state);
    let key = text(&scope, b"k");
    accessor(
        &groups,
        key.get_nanbox_f64(),
        method.get_nanbox_f64(),
        f64::from_bits(TAG_UNDEFINED),
    );
    put(&state, b"groups", groups.get_nanbox_f64());
    let input = text(&scope, b"abc");
    let template = text(&scope, b"$<k>");
    assert_eq!(
        bytes(api::finish(replace::regexp(
            state.get_nanbox_f64(),
            input.get_nanbox_f64(),
            template.get_nanbox_f64()
        ))),
        b"a1c"
    );
    assert_eq!(get(&state, b"replacements"), 2.0);
}

#[test]
fn perex_replace_hooks_preserve_receiver_and_arbitrary_result_before_coercion() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = object(&scope);
    let throwing = function(&scope, throw_text as *const u8, 0);
    put(&input, b"toString", throwing.get_nanbox_f64());
    let search = object(&scope);
    let method = function(&scope, hook as *const u8, 2);
    symbol(&search, "replace", method.get_nanbox_f64());
    let result =
        crate::regex::js_string_replace_js(input.get_nanbox_f64(), search.get_nanbox_f64(), 47.0);
    assert_eq!(result.to_bits(), input.get_nanbox_f64().to_bits());
    symbol(&search, "match", f64::from_bits(crate::value::TAG_TRUE));
    let flags = text(&scope, b"");
    put(&search, b"flags", flags.get_nanbox_f64());
    assert!(matches!(
        replace::string(true, input.get_nanbox_f64(), search.get_nanbox_f64(), 47.0),
        Err(crate::regex::perex_runtime::EngineError::Type(_))
    ));
}

#[test]
fn perex_replace_collecting_throw_cleans_native_arguments_roots_and_this() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, b"abc");
    let search = text(&scope, b"b");
    let callback = function(&scope, collecting_throw as *const u8, 3);
    let previous = object(&scope);
    let displaced = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    // Warm lazy String/Function prototype setup before checking native bytes.
    let empty = text(&scope, b"none");
    let _ = crate::regex::js_string_replace_js(
        input.get_nanbox_f64(),
        empty.get_nanbox_f64(),
        search.get_nanbox_f64(),
    );
    let roots = RuntimeHandleScope::active_len_for_tests();
    let live = external_side_live_bytes();
    let before = previous.get_nanbox_f64().to_bits();
    let error = crate::exception::catch_js_throw(|| {
        crate::regex::js_string_replace_js(
            input.get_nanbox_f64(),
            search.get_nanbox_f64(),
            callback.get_nanbox_f64(),
        )
    })
    .unwrap_err();
    assert_eq!(error, 972.0);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(external_side_live_bytes(), live);
    assert_ne!(previous.get_nanbox_f64().to_bits(), before);
    assert_eq!(
        crate::object::js_implicit_this_get().to_bits(),
        previous.get_nanbox_f64().to_bits()
    );
    crate::object::js_implicit_this_set(displaced.get_nanbox_f64());
}

extern "C" fn primitive_hook_getter(_: *const crate::closure::ClosureHeader) -> f64 {
    assert_eq!(crate::object::js_implicit_this_get(), 23.0);
    gc_collect_minor();
    crate::closure::js_register_closure_arity(hook as *const u8, 2);
    js_nanbox_pointer(crate::closure::js_closure_alloc_singleton(hook as *const u8) as i64)
}

#[test]
fn perex_replace_primitive_search_skips_prototype_hook_and_coerces_receiver() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(crate::object::builtin_prototype_value("Number"));
    assert_eq!(
        api::finish(dispatch::get_symbol(&proto, "replace")).to_bits(),
        TAG_UNDEFINED
    );
    let key = scope.root_nanbox_f64(js_nanbox_pointer(
        crate::symbol::well_known_symbol("replace") as i64,
    ));
    let getter = function(&scope, primitive_hook_getter as *const u8, 0);
    accessor(
        &proto,
        key.get_nanbox_f64(),
        getter.get_nanbox_f64(),
        f64::from_bits(TAG_UNDEFINED),
    );
    let input = object(&scope);
    let throwing = function(&scope, throw_text as *const u8, 0);
    put(&input, b"toString", throwing.get_nanbox_f64());
    // The pinned Node oracle skips the primitive's hook for both operations.
    // Preserve the original receiver and getter; observable coercion must throw.
    for all in [false, true] {
        let result = crate::exception::catch_js_throw(|| {
            if all {
                crate::regex::js_string_replace_all_js(input.get_nanbox_f64(), 23.0, 47.0)
            } else {
                crate::regex::js_string_replace_js(input.get_nanbox_f64(), 23.0, 47.0)
            }
        });
        assert_eq!(result.unwrap_err(), 971.0);
    }
    unsafe {
        assert_eq!(
            crate::symbol::js_object_delete_symbol_property(
                proto.get_nanbox_f64(),
                key.get_nanbox_f64()
            ),
            1
        );
    }
}
