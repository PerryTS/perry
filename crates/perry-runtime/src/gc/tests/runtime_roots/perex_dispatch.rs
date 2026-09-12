//! RegExpExec witnesses through JS callbacks, proxies and actual moving GC.
use super::*;
use crate::object::ObjectHeader;
use crate::regex::perex_dispatch as dispatch;
use crate::regex::perex_memory::MemoryBudget;
use crate::regex::perex_runtime::EngineError;
use crate::regex::{perex_api as api, RegExpHeader};
use crate::string::StringHeader;
use crate::value::{js_nanbox_pointer, js_nanbox_string, TAG_NULL, TAG_UNDEFINED};
use perex::Budget;

thread_local! { static ORDER: Cell<u32> = const { Cell::new(0) }; }

fn text<'s>(scope: &'s RuntimeHandleScope, value: &[u8]) -> RuntimeHandle<'s> {
    scope.root_string_ptr(crate::string::js_string_from_bytes(
        value.as_ptr(),
        value.len() as u32,
    ))
}
fn object<'s>(scope: &'s RuntimeHandleScope) -> RuntimeHandle<'s> {
    scope.root_nanbox_f64(js_nanbox_pointer(
        crate::object::js_object_alloc(0, 4) as i64
    ))
}
fn regex<'s>(scope: &'s RuntimeHandleScope) -> RuntimeHandle<'s> {
    let source = text(scope, b"NEVER");
    let flags = text(scope, b"");
    let re = source.with_const_ptr(|source| {
        flags.with_const_ptr(|flags| crate::regex::js_regexp_new(source, flags))
    });
    scope.root_nanbox_f64(js_nanbox_pointer(re as i64))
}
fn function<'s>(scope: &'s RuntimeHandleScope, fp: *const u8, arity: u32) -> RuntimeHandle<'s> {
    crate::closure::js_register_closure_arity(fp, arity);
    scope.root_nanbox_f64(js_nanbox_pointer(
        crate::closure::js_closure_alloc_singleton(fp) as i64,
    ))
}
fn put(owner: &RuntimeHandle<'_>, name: &[u8], value: &RuntimeHandle<'_>) {
    let key = crate::string::canonical_key(name);
    crate::object::js_object_set_field_by_name(
        crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64()) as *mut ObjectHeader,
        key,
        value.get_nanbox_f64(),
    );
}
fn getter(owner: &RuntimeHandle<'_>, method: &RuntimeHandle<'_>) {
    getter_named(owner, b"exec", method);
}
fn getter_named(owner: &RuntimeHandle<'_>, name: &[u8], method: &RuntimeHandle<'_>) {
    // DefineProperty publishes both an own key and its descriptor. The raw
    // descriptor-table setter alone does not create an object property.
    let scope = RuntimeHandleScope::new();
    let undefined = scope.root_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
    put(owner, name, &undefined);
    crate::object::set_accessor_descriptor(
        crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64()) as usize,
        std::str::from_utf8(name).unwrap().to_string(),
        crate::object::AccessorDescriptor {
            get: method.get_nanbox_f64().to_bits(),
            set: 0,
        },
    );
}
fn test(receiver: &RuntimeHandle<'_>, input: &RuntimeHandle<'_>) -> bool {
    let raw = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as *const RegExpHeader;
    input.with_const_ptr(|input| crate::regex::js_regexp_test(raw, input)) != 0
}

extern "C" fn return_this(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    crate::object::js_implicit_this_get()
}
extern "C" fn return_null(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    f64::from_bits(TAG_NULL)
}
extern "C" fn read_answer(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    api::finish(dispatch::get(&receiver, b"answer"))
}
extern "C" fn collect_and_echo(_: *const crate::closure::ClosureHeader, arg: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    let argument = scope.root_nanbox_f64(arg);
    gc_collect_minor();
    put(&receiver, b"seen", &argument);
    receiver.get_nanbox_f64()
}
extern "C" fn collecting_getter(_: *const crate::closure::ClosureHeader) -> f64 {
    gc_collect_minor();
    let fp = collect_and_echo as *const u8;
    crate::closure::js_register_closure_arity(fp, 1);
    js_nanbox_pointer(crate::closure::js_closure_alloc_singleton(fp) as i64)
}
extern "C" fn collect_missing_apply(_: *const crate::closure::ClosureHeader) -> f64 {
    gc_collect_minor();
    f64::from_bits(TAG_UNDEFINED)
}
extern "C" fn echo_apply(
    _: *const crate::closure::ClosureHeader,
    _target: f64,
    receiver: f64,
    args: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let args = scope.root_nanbox_f64(args);
    gc_collect_minor();
    let value = crate::array::js_array_get_f64(
        crate::value::js_nanbox_get_pointer(args.get_nanbox_f64())
            as *const crate::array::ArrayHeader,
        0,
    );
    let value = scope.root_nanbox_f64(value);
    put(&receiver, b"seen", &value);
    receiver.get_nanbox_f64()
}
extern "C" fn throw_exec(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    gc_collect_minor();
    crate::exception::js_throw(843.0)
}
extern "C" fn throw_getter(_: *const crate::closure::ClosureHeader) -> f64 {
    gc_collect_minor();
    crate::exception::js_throw(844.0)
}
extern "C" fn ordered_string(_: *const crate::closure::ClosureHeader) -> f64 {
    ORDER.with(|n| n.set(n.get() * 10 + 1));
    js_nanbox_string(crate::string::js_string_from_bytes(b"x".as_ptr(), 1) as i64)
}
extern "C" fn ordered_exec(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    ORDER.with(|n| n.set(n.get() * 10 + 3));
    js_nanbox_pointer(crate::array::js_array_alloc(0) as i64)
}
extern "C" fn ordered_getter(_: *const crate::closure::ClosureHeader) -> f64 {
    ORDER.with(|n| n.set(n.get() * 10 + 2));
    let fp = ordered_exec as *const u8;
    crate::closure::js_register_closure_arity(fp, 1);
    js_nanbox_pointer(crate::closure::js_closure_alloc_singleton(fp) as i64)
}

#[test]
fn perex_dispatch_public_test_honors_exec_and_noncallable_fallback() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let re = regex(&scope);
    let subject = text(&scope, b"x");
    assert!(crate::object::regex_proto_thunks::is_builtin_regexp_exec(
        api::finish(dispatch::get(&re, b"exec"))
    ));
    let yes = function(&scope, return_this as *const u8, 1);
    let no = function(&scope, return_null as *const u8, 1);
    put(&re, b"exec", &yes);
    assert!(
        test(&re, &subject),
        "override must run despite incompatible builtin pattern"
    );
    put(&re, b"exec", &no);
    assert!(!test(&re, &subject));
    let noncallable = object(&scope);
    put(&re, b"exec", &noncallable);
    assert!(!test(&re, &subject));
    let matched = text(&scope, b"NEVER");
    assert!(
        test(&re, &matched),
        "noncallable exec falls back to the Perex program"
    );
    assert!(
        crate::regex::test_original_strings_and_program(crate::value::js_nanbox_get_pointer(
            re.get_nanbox_f64()
        ) as *const RegExpHeader)
        .2
    );
}

#[test]
fn perex_dispatch_generic_prototype_test_coerces_before_getting_exec() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = object(&scope);
    let argument = object(&scope);
    let stringer = function(&scope, ordered_string as *const u8, 0);
    put(&argument, b"toString", &stringer);
    let get = function(&scope, ordered_getter as *const u8, 0);
    getter(&receiver, &get);
    let method = scope.root_nanbox_f64(crate::collection_iter::builtin_prototype_method(
        "RegExp", "test",
    ));
    assert!(crate::proxy::proxy_wraps_callable(method.get_nanbox_f64()));
    ORDER.with(|n| n.set(0));
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_set(
        receiver.get_nanbox_f64(),
    ));
    let args = [argument.get_nanbox_f64()];
    let result = crate::exception::catch_js_throw(|| unsafe {
        crate::closure::js_native_call_value(method.get_nanbox_f64(), args.as_ptr(), 1)
    });
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    assert_eq!(result.unwrap().to_bits(), crate::value::TAG_TRUE);
    assert_eq!(ORDER.with(Cell::get), 123);
    ORDER.with(|n| n.set(0));
    assert!(matches!(
        dispatch::test_value(1.0, argument.get_nanbox_f64()),
        Err(EngineError::Type(_))
    ));
    assert_eq!(
        ORDER.with(Cell::get),
        0,
        "object requirement precedes ToString"
    );
}

#[test]
fn perex_dispatch_getter_and_callback_reacquire_original_input_after_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = object(&scope);
    let input = text(&scope, b"\xed\xa0\x80-original");
    let before = handle_address::<StringHeader>(&input);
    let method = function(&scope, collecting_getter as *const u8, 0);
    getter(&receiver, &method);
    let displaced = object(&scope);
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_set(
        displaced.get_nanbox_f64(),
    ));
    let roots = RuntimeHandleScope::active_len_for_tests();
    let result = api::finish(dispatch::execute(
        &receiver,
        &input,
        true,
        &mut Budget::new(api::WORK),
        &MemoryBudget::new(api::SCRATCH_BYTES),
        &mut crate::regex::perex_runtime::poll,
    ))
    .unwrap()
    .object();
    assert_eq!(result.to_bits(), receiver.get_nanbox_f64().to_bits());
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_ne!(handle_address::<StringHeader>(&input), before);
    assert_eq!(
        api::finish(dispatch::get(&receiver, b"seen")).to_bits(),
        handle_string_value(&input).to_bits()
    );
    assert_eq!(
        crate::object::js_implicit_this_get().to_bits(),
        displaced.get_nanbox_f64().to_bits()
    );
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
}

#[test]
fn perex_dispatch_actual_throws_restore_this_roots_and_accounting() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    gc_register_mutable_root_scanner(crate::proxy::scan_proxy_roots_mut);
    for (mode, expected) in [(0, 843.0), (1, 844.0), (2, 843.0)] {
        let scope = RuntimeHandleScope::new();
        let receiver = object(&scope);
        let input = text(&scope, b"young-input");
        let before = handle_address::<StringHeader>(&input);
        let is_getter = mode == 1;
        let fp = if is_getter {
            throw_getter as *const u8
        } else {
            throw_exec as *const u8
        };
        let method = function(&scope, fp, u32::from(!is_getter));
        if mode == 2 {
            let handler = object(&scope);
            let proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
                method.get_nanbox_f64(),
                handler.get_nanbox_f64(),
            ));
            put(&receiver, b"exec", &proxy);
        } else if is_getter {
            getter(&receiver, &method);
        } else {
            put(&receiver, b"exec", &method);
        }
        let displaced = object(&scope);
        let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_set(
            displaced.get_nanbox_f64(),
        ));
        let roots = RuntimeHandleScope::active_len_for_tests();
        let live = external_side_live_bytes();
        let result =
            crate::exception::catch_js_throw(|| {
                api::finish(input.with_const_ptr(|input| {
                    dispatch::test_string(receiver.get_nanbox_f64(), input)
                }))
            });
        assert_eq!(result.unwrap_err(), expected);
        assert_ne!(handle_address::<StringHeader>(&input), before);
        assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
        assert_eq!(external_side_live_bytes(), live);
        assert_eq!(
            crate::object::js_implicit_this_get().to_bits(),
            displaced.get_nanbox_f64().to_bits()
        );
        crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    }
}

#[test]
fn perex_dispatch_validates_override_results_and_keeps_one_work_allowance() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = object(&scope);
    let input = text(&scope, b"x");
    let method = function(&scope, read_answer as *const u8, 1);
    put(&receiver, b"exec", &method);
    for value in [
        1.0,
        f64::from_bits(TAG_UNDEFINED),
        f64::from_bits(crate::value::TAG_TRUE),
        handle_string_value(&input),
    ] {
        let value = scope.root_nanbox_f64(value);
        put(&receiver, b"answer", &value);
        assert!(matches!(
            input.with_const_ptr(|input| dispatch::test_string(receiver.get_nanbox_f64(), input)),
            Err(EngineError::Type(_))
        ));
    }
    let null = scope.root_nanbox_f64(f64::from_bits(TAG_NULL));
    put(&receiver, b"answer", &null);
    assert!(!api::finish(input.with_const_ptr(|input| {
        dispatch::test_string(receiver.get_nanbox_f64(), input)
    })));
    put(&receiver, b"answer", &method); // functions are valid objects too
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let mut budget = Budget::new(3);
    for expected in [2, 1, 0] {
        assert!(api::finish(dispatch::execute(
            &receiver,
            &input,
            false,
            &mut budget,
            &memory,
            &mut crate::regex::perex_runtime::poll
        ))
        .is_some());
        assert_eq!(budget.remaining(), expected);
    }
    assert!(matches!(
        dispatch::execute(
            &receiver,
            &input,
            false,
            &mut budget,
            &memory,
            &mut crate::regex::perex_runtime::poll
        ),
        Err(EngineError::Execution(
            perex::executor::ExecError::WorkLimit
        ))
    ));
}

#[test]
fn perex_dispatch_callable_proxy_keeps_receiver_and_noncallable_proxy_falls_back() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    gc_register_mutable_root_scanner(crate::proxy::scan_proxy_roots_mut);
    let scope = RuntimeHandleScope::new();
    let receiver = object(&scope);
    let input = text(&scope, b"original-proxy-input");
    let before = handle_address::<StringHeader>(&input);
    let function = function(&scope, collect_and_echo as *const u8, 1);
    let handler = object(&scope);
    let proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
        function.get_nanbox_f64(),
        handler.get_nanbox_f64(),
    ));
    put(&receiver, b"exec", &proxy);
    assert!(api::finish(input.with_const_ptr(|input| {
        dispatch::test_string(receiver.get_nanbox_f64(), input)
    })));
    assert_ne!(handle_address::<StringHeader>(&input), before);
    assert_eq!(
        api::finish(dispatch::get(&receiver, b"seen")).to_bits(),
        handle_string_value(&input).to_bits()
    );
    let re = regex(&scope);
    let target = object(&scope);
    let noncallable = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
        target.get_nanbox_f64(),
        handler.get_nanbox_f64(),
    ));
    put(&re, b"exec", &noncallable);
    let input = text(&scope, b"NEVER");
    assert!(test(&re, &input));
    crate::proxy::js_proxy_revoke(proxy.get_nanbox_f64());
    assert!(
        crate::exception::catch_js_throw(|| api::finish(
            input.with_const_ptr(|input| dispatch::test_string(receiver.get_nanbox_f64(), input))
        ))
        .is_err(),
        "revoked callable proxies must throw, not fall back"
    );
}

#[test]
fn perex_dispatch_proxy_apply_getter_and_nested_trap_survive_movement() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    gc_register_mutable_root_scanner(crate::proxy::scan_proxy_roots_mut);
    for nested_trap in [false, true] {
        let scope = RuntimeHandleScope::new();
        let receiver = object(&scope);
        let input = text(&scope, b"original-apply-input");
        let before = handle_address::<StringHeader>(&input);
        let target = function(&scope, collect_and_echo as *const u8, 1);
        let handler = object(&scope);
        if nested_trap {
            let trap = function(&scope, echo_apply as *const u8, 3);
            let inner_handler = object(&scope);
            let inner = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
                trap.get_nanbox_f64(),
                inner_handler.get_nanbox_f64(),
            ));
            put(&handler, b"apply", &inner);
        } else {
            let get = function(&scope, collect_missing_apply as *const u8, 0);
            getter_named(&handler, b"apply", &get);
        }
        let proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
            target.get_nanbox_f64(),
            handler.get_nanbox_f64(),
        ));
        put(&receiver, b"exec", &proxy);
        assert!(api::finish(input.with_const_ptr(|input| {
            dispatch::test_string(receiver.get_nanbox_f64(), input)
        })));
        assert_ne!(handle_address::<StringHeader>(&input), before);
        assert_eq!(
            api::finish(dispatch::get(&receiver, b"seen")).to_bits(),
            handle_string_value(&input).to_bits()
        );
    }
}
