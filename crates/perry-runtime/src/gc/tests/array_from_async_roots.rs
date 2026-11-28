//! `Array.fromAsync` builds its result promise and step closures, then runs
//! the first element read before it returns: an array-like's index getter is
//! user code inside that setup window, and later getters run between steps.
//! A copying minor forced from each getter must leave the returned promise
//! and the state the steps re-read pointing at live cells.

use super::super::*;
use super::support::*;

use crate::gc::RuntimeHandleScope;

thread_local! {
    static GETTER_COPIED_OBJECTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn object_value(obj: *mut crate::object::ObjectHeader) -> f64 {
    f64::from_bits(ptr_bits(obj as usize))
}

fn string_value(text: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    f64::from_bits(string_bits(ptr as usize))
}

fn addr_of(value: f64) -> usize {
    (value.to_bits() & POINTER_MASK) as usize
}

fn moving_minor() {
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    GETTER_COPIED_OBJECTS.with(|c| c.set(c.get() + trace.copying_nursery.copied_objects));
}

extern "C" fn moving_getter_a(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    moving_minor();
    string_value("a")
}

extern "C" fn moving_getter_b(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    moving_minor();
    string_value("b")
}

/// `Object.defineProperty(target, key, { get })`.
unsafe fn define_getter(
    scope: &RuntimeHandleScope,
    target: f64,
    key: &str,
    getter: *mut crate::closure::ClosureHeader,
) {
    let target = scope.root_nanbox_f64(target);
    let getter = scope.root_nanbox_f64(f64::from_bits(ptr_bits(getter as usize)));
    let descriptor = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
    crate::object::js_object_set_field_by_name(
        addr_of(descriptor.get_nanbox_f64()) as *mut crate::object::ObjectHeader,
        get_key,
        getter.get_nanbox_f64(),
    );
    let key = scope.root_nanbox_f64(string_value(key));
    crate::object::js_object_define_property(
        target.get_nanbox_f64(),
        key.get_nanbox_f64(),
        descriptor.get_nanbox_f64(),
    );
}

/// `{ length: 2, get 0() { <moving minor>; return "a" }, get 1() { <moving minor>; return "b" } }`.
unsafe fn moving_array_like(scope: &RuntimeHandleScope) -> f64 {
    let items = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    let length_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    crate::object::js_object_set_field_by_name(
        addr_of(items.get_nanbox_f64()) as *mut crate::object::ObjectHeader,
        length_key,
        2.0,
    );
    let getter_a = crate::closure::js_closure_alloc(crate::fn_info!(moving_getter_a, 0), 0);
    define_getter(scope, items.get_nanbox_f64(), "0", getter_a);
    let getter_b = crate::closure::js_closure_alloc(crate::fn_info!(moving_getter_b, 0), 0);
    define_getter(scope, items.get_nanbox_f64(), "1", getter_b);
    items.get_nanbox_f64()
}

unsafe fn string_text(value: f64) -> String {
    let header = addr_of(value) as *const crate::StringHeader;
    let len = (*header).byte_len as usize;
    let bytes = crate::string::string_data(header);
    String::from_utf8_lossy(std::slice::from_raw_parts(bytes, len)).into_owned()
}

#[test]
fn from_async_result_promise_survives_minors_in_setup_and_between_steps() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    GETTER_COPIED_OBJECTS.with(|c| c.set(0));

    unsafe {
        let scope = RuntimeHandleScope::new();
        let items = scope.root_nanbox_f64(moving_array_like(&scope));
        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);

        let returned =
            crate::promise::js_array_from_async(items.get_nanbox_f64(), undefined, undefined);
        let copied_in_setup = GETTER_COPIED_OBJECTS.with(|c| c.get());
        assert!(
            copied_in_setup > 0,
            "the setup-window getter moved nothing -- the checks below would be vacuous"
        );
        let header = (addr_of(returned) as *const u8).sub(crate::gc::GC_HEADER_SIZE)
            as *const crate::gc::GcHeader;
        assert_eq!(
            (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED,
            0,
            "Array.fromAsync returned the result promise's pre-collection address"
        );
        let returned = scope.root_nanbox_f64(returned);

        crate::promise::js_promise_run_microtasks();
        assert!(
            GETTER_COPIED_OBJECTS.with(|c| c.get()) > copied_in_setup,
            "the between-steps getter moved nothing"
        );

        let promise = addr_of(returned.get_nanbox_f64()) as *const crate::promise::Promise;
        assert_eq!((*promise).state, crate::promise::PromiseState::Fulfilled);
        let result = addr_of((*promise).value) as *const crate::array::ArrayHeader;
        assert_eq!((*result).length, 2);
        assert_eq!(string_text(crate::array::js_array_get_f64(result, 0)), "a");
        assert_eq!(string_text(crate::array::js_array_get_f64(result, 1)), "b");
    }
}
