//! Lazy iterator semantics and ownership through actual moving collection.
use super::*;
use crate::regex::{match_all, perex_api as api, perex_dispatch as dispatch, RegExpHeader};
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
fn getter(owner: &RuntimeHandle<'_>, name: &[u8], method: &RuntimeHandle<'_>) {
    accessor(
        owner,
        js_nanbox_string(crate::string::canonical_key(name) as i64),
        method.get_nanbox_f64(),
        f64::from_bits(TAG_UNDEFINED),
    );
}
fn next(iter: &RuntimeHandle<'_>) -> f64 {
    unsafe {
        match_all::dispatch_regexp_string_iterator_method_builtin(
            js_nanbox_get_pointer(iter.get_nanbox_f64()) as *mut crate::object::ObjectHeader,
            "next",
        )
    }
}
fn item(array: &RuntimeHandle<'_>, index: u32) -> f64 {
    crate::array::js_array_get_f64(
        js_nanbox_get_pointer(array.get_nanbox_f64()) as *const crate::array::ArrayHeader,
        index,
    )
}
fn bytes(value: f64) -> Vec<u8> {
    let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
    let (data, n) = crate::string::str_bytes_from_jsvalue(value, &mut short).unwrap();
    unsafe { std::slice::from_raw_parts(data, n as usize).to_vec() }
}
fn slot(iter: &RuntimeHandle<'_>, n: u32) -> f64 {
    f64::from_bits(
        crate::object::js_object_get_field(
            js_nanbox_get_pointer(iter.get_nanbox_f64()) as *const crate::object::ObjectHeader,
            n,
        )
        .bits(),
    )
}
fn arena_population(mut matches: impl FnMut(*const GcHeader, usize) -> bool) -> usize {
    let mut cursor = crate::arena::ArenaObjectCursor::new(crate::arena::ArenaWalkOrder::Address);
    let mut remaining = usize::MAX;
    let mut count = 0;
    while let Some((header, _)) = cursor.next_budgeted(&mut remaining) {
        if matches(header.cast(), header as usize + GC_HEADER_SIZE) {
            count += 1;
        }
    }
    count
}
fn program_population() -> usize {
    arena_population(|header, _| unsafe { (*header).obj_type == GC_TYPE_REGEX_PROGRAM })
}
fn retained_input_population() -> usize {
    arena_population(|header, p| unsafe {
        (*header).obj_type == GC_TYPE_STRING
            && bytes(js_nanbox_string(p as i64)) == b"a unique input retained only by the iterator"
    })
}
extern "C" fn identity(_: *const crate::closure::ClosureHeader, arg: f64) -> f64 {
    arg
}
extern "C" fn throw_getter(_: *const crate::closure::ClosureHeader) -> f64 {
    crate::exception::js_throw(941.0)
}
extern "C" fn throw_exec(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    gc_collect_minor();
    crate::exception::js_throw(942.0)
}
extern "C" fn no_match(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    f64::from_bits(TAG_NULL)
}
extern "C" fn return_this(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    crate::object::js_implicit_this_get()
}
extern "C" fn factory(_: *const crate::closure::ClosureHeader, receiver: f64, flags: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let flags = scope.root_nanbox_f64(flags);
    gc_collect_minor();
    let matcher = scope.root_nanbox_f64(get(&receiver, b"matcher"));
    put(&matcher, b"seenFlags", flags.get_nanbox_f64());
    matcher.get_nanbox_f64()
}
fn custom<'s>(
    scope: &'s RuntimeHandleScope,
    flags: &[u8],
) -> (RuntimeHandle<'s>, RuntimeHandle<'s>) {
    let receiver = object(scope);
    let matcher = object(scope);
    put(&receiver, b"matcher", matcher.get_nanbox_f64());
    let holder = object(scope);
    let constructor = function(scope, factory as *const u8, 2);
    symbol(&holder, "species", constructor.get_nanbox_f64());
    put(&receiver, b"constructor", holder.get_nanbox_f64());
    let flags = text(scope, flags);
    put(&receiver, b"flags", flags.get_nanbox_f64());
    put(&receiver, b"lastIndex", 0.0);
    (receiver, matcher)
}

#[test]
fn perex_match_all_clones_index_and_preserves_complete_results_across_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, "x😀".as_bytes());
    let re = regex(&scope, b"(?<half>.)(x)?", b"gd");
    api::finish(dispatch::set_last_index(&re, 1.75));
    let iter = scope.root_nanbox_f64(crate::regex::js_string_match_all_js(
        input.get_nanbox_f64(),
        re.get_nanbox_f64(),
    ));
    assert_ne!(slot(&iter, 0).to_bits(), re.get_nanbox_f64().to_bits());
    assert_eq!(get(&re, b"lastIndex"), 1.75);
    let first = scope.root_nanbox_f64(next(&iter));
    let found = scope.root_nanbox_f64(get(&first, b"value"));
    assert_eq!(get(&first, b"done").to_bits(), crate::value::TAG_FALSE);
    assert_eq!(get(&found, b"index"), 1.0);
    assert_eq!(bytes(item(&found, 0)), [0xed, 0xa0, 0xbd]);
    assert_eq!(item(&found, 2).to_bits(), TAG_UNDEFINED);
    let before = input.get_nanbox_f64().to_bits();
    gc_collect_minor();
    assert_ne!(before, input.get_nanbox_f64().to_bits());
    assert_eq!(
        get(&found, b"input").to_bits(),
        input.get_nanbox_f64().to_bits()
    );
    let groups = scope.root_nanbox_f64(get(&found, b"groups"));
    assert_eq!(
        crate::object::js_object_get_prototype_of(groups.get_nanbox_f64()).to_bits(),
        TAG_NULL
    );
    assert_eq!(bytes(get(&groups, b"half")), [0xed, 0xa0, 0xbd]);
    let indices = scope.root_nanbox_f64(get(&found, b"indices"));
    let pair = scope.root_nanbox_f64(item(&indices, 1));
    assert_eq!((item(&pair, 0), item(&pair, 1)), (1.0, 2.0));
    let second = scope.root_nanbox_f64(next(&iter));
    assert_ne!(
        first.get_nanbox_f64().to_bits(),
        second.get_nanbox_f64().to_bits()
    );
    let second_value = scope.root_nanbox_f64(get(&second, b"value"));
    assert_eq!(bytes(item(&second_value, 0)), [0xed, 0xb8, 0x80]);
    assert_eq!(bytes(item(&found, 0)), [0xed, 0xa0, 0xbd]);
    let done = scope.root_nanbox_f64(next(&iter));
    assert_eq!(get(&done, b"done").to_bits(), crate::value::TAG_TRUE);
    assert_eq!(get(&re, b"lastIndex"), 1.75);
}

#[test]
fn perex_match_all_empty_advancement_and_sticky_termination() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    for (flags, expected) in [
        (b"g".as_slice(), vec![0.0, 1.0, 2.0]),
        (b"gu", vec![0.0, 2.0]),
    ] {
        let scope = RuntimeHandleScope::new();
        let input = text(&scope, "😀".as_bytes());
        let re = regex(&scope, b"(?:)", flags);
        let iter = scope.root_nanbox_f64(crate::regex::js_string_match_all_js(
            input.get_nanbox_f64(),
            re.get_nanbox_f64(),
        ));
        for index in expected {
            let step = scope.root_nanbox_f64(next(&iter));
            assert_eq!(get(&step, b"done").to_bits(), crate::value::TAG_FALSE);
            let found = scope.root_nanbox_f64(get(&step, b"value"));
            assert_eq!(get(&found, b"index"), index);
        }
        for _ in 0..2 {
            let step = scope.root_nanbox_f64(next(&iter));
            assert_eq!(get(&step, b"done").to_bits(), crate::value::TAG_TRUE);
            assert_eq!(get(&step, b"value").to_bits(), TAG_UNDEFINED);
        }
    }
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, b"a_a");
    let re = regex(&scope, b"a", b"gy");
    let iter = scope.root_nanbox_f64(crate::regex::js_string_match_all_js(
        input.get_nanbox_f64(),
        re.get_nanbox_f64(),
    ));
    let first = scope.root_nanbox_f64(next(&iter));
    assert_eq!(get(&first, b"done").to_bits(), crate::value::TAG_FALSE);
    let done = scope.root_nanbox_f64(next(&iter));
    assert_eq!(get(&done, b"done").to_bits(), crate::value::TAG_TRUE);
}

#[test]
fn perex_match_all_iterator_is_the_only_input_owner_and_releases_on_completion() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let (value, input_before, program_before) = {
        let temporary = RuntimeHandleScope::new();
        let input = text(&temporary, b"a unique input retained only by the iterator");
        let re = regex(&temporary, b"NEVER", b"g");
        let iter = temporary.root_nanbox_f64(crate::regex::js_string_match_all_js(
            input.get_nanbox_f64(),
            re.get_nanbox_f64(),
        ));
        let matcher = js_nanbox_get_pointer(slot(&iter, 0)) as *const RegExpHeader;
        (
            iter.get_nanbox_f64(),
            input.get_nanbox_f64().to_bits(),
            unsafe { (*matcher).perex_program as usize },
        )
    };
    let iter = scope.root_nanbox_f64(value);
    gc_collect_minor();
    assert_ne!(slot(&iter, 1).to_bits(), input_before);
    let matcher = js_nanbox_get_pointer(slot(&iter, 0)) as *const RegExpHeader;
    let program = unsafe { (*matcher).perex_program as usize };
    assert_ne!(program, program_before);
    assert!(build_valid_pointer_set().contains(&program));
    let programs_before = program_population();
    assert_eq!(retained_input_population(), 1);
    let input = crate::value::js_get_string_pointer_unified(slot(&iter, 1)) as usize;
    let step = scope.root_nanbox_f64(next(&iter));
    assert_eq!(get(&step, b"done").to_bits(), crate::value::TAG_TRUE);
    assert_eq!(slot(&iter, 0).to_bits(), TAG_UNDEFINED);
    assert_eq!(slot(&iter, 1).to_bits(), TAG_UNDEFINED);
    gc_collect_minor();
    let live = build_valid_pointer_set();
    assert!(
        !live.contains(&input),
        "completed iterator must release its only input edge"
    );
    assert!(
        !live.contains(&program),
        "completed iterator must release its matcher/program graph"
    );
    // Old addresses alone cannot distinguish collection from relocation.
    // Count the surviving graph as well, independent of its current address.
    assert_eq!(program_population() + 1, programs_before);
    assert_eq!(retained_input_population(), 0);
    let step = scope.root_nanbox_f64(next(&iter));
    assert_eq!(get(&step, b"done").to_bits(), crate::value::TAG_TRUE);
}

#[test]
fn perex_match_all_is_lazy_observes_exec_changes_and_retries_after_throw() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let (receiver, matcher) = custom(&scope, b"gu");
    let method = function(&scope, throw_exec as *const u8, 1);
    put(&matcher, b"exec", method.get_nanbox_f64());
    let empty = text(&scope, b"");
    put(&matcher, b"0", empty.get_nanbox_f64());
    let input = text(&scope, "😀".as_bytes());
    let before = input.get_nanbox_f64().to_bits();
    let iter = scope.root_nanbox_f64(api::finish(match_all::regexp(
        receiver.get_nanbox_f64(),
        input.get_nanbox_f64(),
    )));
    assert_ne!(
        before,
        input.get_nanbox_f64().to_bits(),
        "species callback must actually collect"
    );
    assert_eq!(bytes(get(&matcher, b"seenFlags")), b"gu");
    let roots = RuntimeHandleScope::active_len_for_tests();
    let live = external_side_live_bytes();
    assert_eq!(
        crate::exception::catch_js_throw(|| next(&iter)).unwrap_err(),
        942.0
    );
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(external_side_live_bytes(), live);
    let method = function(&scope, return_this as *const u8, 1);
    put(&matcher, b"exec", method.get_nanbox_f64());
    let step = scope.root_nanbox_f64(next(&iter));
    assert_eq!(
        get(&step, b"value").to_bits(),
        matcher.get_nanbox_f64().to_bits()
    );
    assert_eq!(get(&matcher, b"lastIndex"), 2.0);
    let method = function(&scope, no_match as *const u8, 1);
    put(&matcher, b"exec", method.get_nanbox_f64());
    let step = scope.root_nanbox_f64(next(&iter));
    assert_eq!(get(&step, b"done").to_bits(), crate::value::TAG_TRUE);
}

#[test]
fn perex_match_all_string_hooks_precede_coercion_but_follow_global_validation() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = object(&scope);
    let pattern = object(&scope);
    let throw = function(&scope, throw_getter as *const u8, 0);
    put(&receiver, b"toString", throw.get_nanbox_f64());
    let hook = function(&scope, identity as *const u8, 1);
    symbol(&pattern, "matchAll", hook.get_nanbox_f64());
    let method = scope.root_nanbox_f64(crate::collection_iter::builtin_prototype_method(
        "String", "matchAll",
    ));
    assert_eq!(
        api::finish(dispatch::call_one(&method, &receiver, &pattern)).to_bits(),
        receiver.get_nanbox_f64().to_bits()
    );
    let re = regex(&scope, b"a", b"");
    symbol(&re, "matchAll", hook.get_nanbox_f64());
    assert!(
        crate::exception::catch_js_throw(|| crate::regex::js_string_match_all_js(
            receiver.get_nanbox_f64(),
            re.get_nanbox_f64()
        ))
        .is_err()
    );
    symbol(&re, "match", f64::from_bits(crate::value::TAG_FALSE));
    assert_eq!(
        crate::regex::js_string_match_all_js(receiver.get_nanbox_f64(), re.get_nanbox_f64())
            .to_bits(),
        receiver.get_nanbox_f64().to_bits()
    );
    let input = text(&scope, b"aba");
    let pattern = text(&scope, b"a");
    let iter = scope.root_nanbox_f64(crate::regex::js_string_match_all_js(
        input.get_nanbox_f64(),
        pattern.get_nanbox_f64(),
    ));
    for index in [0.0, 2.0] {
        let step = scope.root_nanbox_f64(next(&iter));
        let found = scope.root_nanbox_f64(get(&step, b"value"));
        assert_eq!(get(&found, b"index"), index);
    }
}

#[test]
fn perex_match_all_nonglobal_does_not_read_zero_and_builtin_next_has_its_own_brand() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let (receiver, matcher) = custom(&scope, b"");
    let method = function(&scope, return_this as *const u8, 1);
    put(&matcher, b"exec", method.get_nanbox_f64());
    let throw = function(&scope, throw_getter as *const u8, 0);
    getter(&matcher, b"0", &throw);
    let input = text(&scope, b"original");
    let iter = scope.root_nanbox_f64(api::finish(match_all::regexp(
        receiver.get_nanbox_f64(),
        input.get_nanbox_f64(),
    )));
    let builtin = scope.root_nanbox_f64(get(&iter, b"next"));
    let override_next = function(&scope, identity as *const u8, 1);
    put(&iter, b"next", override_next.get_nanbox_f64());
    let argument = scope.root_nanbox_f64(57.0);
    assert_eq!(
        api::finish(dispatch::call_one(&override_next, &iter, &argument)),
        57.0
    );
    let step = scope.root_nanbox_f64(api::finish(dispatch::call_one(&builtin, &iter, &argument)));
    assert_eq!(
        get(&step, b"value").to_bits(),
        matcher.get_nanbox_f64().to_bits()
    );
    let done = scope.root_nanbox_f64(next(&iter));
    assert_eq!(get(&done, b"done").to_bits(), crate::value::TAG_TRUE);
    assert_eq!(get(&iter, b"return").to_bits(), TAG_UNDEFINED);
    assert!(
        crate::exception::catch_js_throw(|| api::finish(dispatch::call_one(
            &builtin, &matcher, &argument
        )))
        .is_err()
    );
}

#[test]
fn perex_match_all_proxy_species_and_dynamic_regexp_constructors_keep_original_source() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    gc_register_mutable_root_scanner(crate::proxy::scan_proxy_roots_mut);
    let scope = RuntimeHandleScope::new();
    let re = regex(&scope, b"a/b", b"g");
    let constructor = scope.root_nanbox_f64(get(&re, b"constructor"));
    let flags = text(&scope, b"g");
    let fresh = scope.root_nanbox_f64(crate::object::construct_two_rooted(
        constructor.get_nanbox_f64(),
        re.get_nanbox_f64(),
        flags.get_nanbox_f64(),
    ));
    assert_ne!(
        fresh.get_nanbox_f64().to_bits(),
        re.get_nanbox_f64().to_bits()
    );
    assert_eq!(bytes(get(&fresh, b"source")), b"a\\/b");
    let undefined = scope.root_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
    assert_eq!(
        api::finish(dispatch::call_one(&constructor, &undefined, &re)).to_bits(),
        re.get_nanbox_f64().to_bits()
    );
    let (receiver, matcher) = custom(&scope, b"g");
    let holder = scope.root_nanbox_f64(get(&receiver, b"constructor"));
    let factory = scope.root_nanbox_f64(api::finish(dispatch::get_symbol(&holder, "species")));
    let handler = object(&scope);
    let collecting = function(&scope, collecting_missing_construct as *const u8, 0);
    getter(&handler, b"construct", &collecting);
    let proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
        factory.get_nanbox_f64(),
        handler.get_nanbox_f64(),
    ));
    assert!(crate::proxy::is_constructor_function(
        proxy.get_nanbox_f64()
    ));
    symbol(&holder, "species", proxy.get_nanbox_f64());
    let method = function(&scope, no_match as *const u8, 1);
    put(&matcher, b"exec", method.get_nanbox_f64());
    let input = text(&scope, b"input through proxy species");
    let iter = scope.root_nanbox_f64(api::finish(match_all::regexp(
        receiver.get_nanbox_f64(),
        input.get_nanbox_f64(),
    )));
    assert_eq!(slot(&iter, 0).to_bits(), matcher.get_nanbox_f64().to_bits());
    let step = scope.root_nanbox_f64(next(&iter));
    assert_eq!(get(&step, b"done").to_bits(), crate::value::TAG_TRUE);
}

extern "C" fn collecting_missing_construct(_: *const crate::closure::ClosureHeader) -> f64 {
    gc_collect_minor();
    f64::from_bits(TAG_UNDEFINED)
}

thread_local! { static ORDER: Cell<u64> = const { Cell::new(0) }; }
fn ordered(event: u64) {
    ORDER.with(|n| n.set(n.get() * 10 + event));
}
fn ordered_field(event: u64, name: &[u8]) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    ordered(event);
    gc_collect_minor();
    get(&receiver, name)
}
extern "C" fn ordered_input(_: *const crate::closure::ClosureHeader) -> f64 {
    ordered_field(1, b"text")
}
extern "C" fn ordered_constructor(_: *const crate::closure::ClosureHeader) -> f64 {
    ordered_field(2, b"holder")
}
extern "C" fn ordered_species(_: *const crate::closure::ClosureHeader) -> f64 {
    ordered_field(3, b"factory")
}
extern "C" fn ordered_flags(_: *const crate::closure::ClosureHeader) -> f64 {
    ordered_field(4, b"flagText")
}
extern "C" fn ordered_factory(
    closure: *const crate::closure::ClosureHeader,
    receiver: f64,
    flags: f64,
) -> f64 {
    ordered(5);
    factory(closure, receiver, flags)
}
extern "C" fn ordered_index(_: *const crate::closure::ClosureHeader) -> f64 {
    ordered(6);
    gc_collect_minor();
    1.75
}
extern "C" fn ordered_set_index(_: *const crate::closure::ClosureHeader, index: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    ordered(7);
    gc_collect_minor();
    put(&receiver, b"seenIndex", index);
    f64::from_bits(TAG_UNDEFINED)
}

#[test]
fn perex_match_all_species_and_lastindex_order_survives_collecting_callbacks() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let (receiver, matcher) = custom(&scope, b"g");
    let holder = object(&scope);
    put(&receiver, b"holder", holder.get_nanbox_f64());
    let factory = function(&scope, ordered_factory as *const u8, 2);
    put(&holder, b"factory", factory.get_nanbox_f64());
    let flags = text(&scope, b"gu");
    put(&receiver, b"flagText", flags.get_nanbox_f64());
    for (name, fp) in [
        (b"constructor".as_slice(), ordered_constructor as *const u8),
        (b"flags", ordered_flags as *const u8),
        (b"lastIndex", ordered_index as *const u8),
    ] {
        let method = function(&scope, fp, 0);
        getter(&receiver, name, &method);
    }
    let method = function(&scope, ordered_species as *const u8, 0);
    let key = js_nanbox_pointer(crate::symbol::well_known_symbol("species") as i64);
    accessor(
        &holder,
        key,
        method.get_nanbox_f64(),
        f64::from_bits(TAG_UNDEFINED),
    );
    let setter = function(&scope, ordered_set_index as *const u8, 1);
    accessor(
        &matcher,
        js_nanbox_string(crate::string::canonical_key(b"lastIndex") as i64),
        f64::from_bits(TAG_UNDEFINED),
        setter.get_nanbox_f64(),
    );
    let input = object(&scope);
    let text = text(&scope, "😀".as_bytes());
    put(&input, b"text", text.get_nanbox_f64());
    let coercer = function(&scope, ordered_input as *const u8, 0);
    put(&input, b"toString", coercer.get_nanbox_f64());
    let method = function(&scope, throw_exec as *const u8, 1);
    put(&matcher, b"exec", method.get_nanbox_f64());
    ORDER.with(|n| n.set(0));
    let before = text.get_nanbox_f64().to_bits();
    let iter = scope.root_nanbox_f64(api::finish(match_all::regexp(
        receiver.get_nanbox_f64(),
        input.get_nanbox_f64(),
    )));
    assert_eq!(ORDER.with(Cell::get), 1234567);
    assert_ne!(before, text.get_nanbox_f64().to_bits());
    assert_eq!(slot(&iter, 1).to_bits(), text.get_nanbox_f64().to_bits());
    assert_eq!(get(&matcher, b"seenIndex"), 1.0);
    assert_eq!(bytes(get(&matcher, b"seenFlags")), b"gu");
}

extern "C" fn arrow_species(_: *const crate::closure::ClosureHeader, value: f64) -> f64 {
    value
}

#[test]
fn perex_match_all_species_rejects_proxy_of_arrow_before_flags_and_keeps_revoked_brand() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    gc_register_mutable_root_scanner(crate::proxy::scan_proxy_roots_mut);
    let scope = RuntimeHandleScope::new();
    let (receiver, _) = custom(&scope, b"g");
    let holder = scope.root_nanbox_f64(get(&receiver, b"constructor"));
    let constructor = scope.root_nanbox_f64(api::finish(dispatch::get_symbol(&holder, "species")));
    let arrow = function(&scope, arrow_species as *const u8, 1);
    crate::closure::js_register_closure_arrow_function(arrow_species as *const u8);
    let handler = object(&scope);
    let arrow_proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
        arrow.get_nanbox_f64(),
        handler.get_nanbox_f64(),
    ));
    assert!(!crate::proxy::is_constructor_function(
        arrow_proxy.get_nanbox_f64()
    ));
    symbol(&holder, "species", arrow_proxy.get_nanbox_f64());
    let throw = function(&scope, throw_getter as *const u8, 0);
    getter(&receiver, b"flags", &throw);
    let input = text(&scope, b"input");
    let error = crate::exception::catch_js_throw(|| {
        api::finish(match_all::regexp(
            receiver.get_nanbox_f64(),
            input.get_nanbox_f64(),
        ))
    })
    .unwrap_err();
    assert_ne!(
        error.to_bits(),
        941.0f64.to_bits(),
        "nonconstructable species throws before Get(flags)"
    );
    let proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
        constructor.get_nanbox_f64(),
        handler.get_nanbox_f64(),
    ));
    crate::proxy::js_proxy_revoke(proxy.get_nanbox_f64());
    assert!(crate::proxy::is_constructor_function(
        proxy.get_nanbox_f64()
    ));
    symbol(&holder, "species", proxy.get_nanbox_f64());
    assert_eq!(
        crate::exception::catch_js_throw(|| api::finish(match_all::regexp(
            receiver.get_nanbox_f64(),
            input.get_nanbox_f64()
        )))
        .unwrap_err(),
        941.0,
        "revocation does not remove [[Construct]]; flags getter is reached"
    );
}
