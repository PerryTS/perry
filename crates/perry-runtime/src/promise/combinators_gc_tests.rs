//! Promise entry points that hold JS values while user code runs must reread
//! them afterwards. The user code here is a thenable's `then` getter (read by
//! `PromiseResolve`). Each test makes that code run a moving minor once, asserts that the values
//! under test actually moved, and poisons the from-space so a stale address
//! reads as no object instead of passing as the live one.
use super::*;
use crate::closure::ClosureHeader;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use std::cell::Cell;

thread_local! {
    static THEN_GETTER_CALLS: Cell<usize> = const { Cell::new(0) };
    static THEN_THIS: Cell<u64> = const { Cell::new(0) };
}

struct MovingGc {
    _nursery: crate::gc::CopyingNurseryTestGuard,
    _triggers: crate::gc::GcTriggerThresholdTestGuard,
    _evacuate: crate::gc::knob_overrides::ForcedEvacuationTestGuard,
    _poison: crate::arena::ProtectionModeGuard,
}

fn moving_gc() -> MovingGc {
    let guards = MovingGc {
        _nursery: crate::gc::CopyingNurseryTestGuard::new(0),
        _triggers: crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers(),
        _evacuate: crate::gc::knob_overrides::ForcedEvacuationTestGuard::on(),
        _poison: crate::arena::ProtectionModeGuard::set(
            crate::arena::FromSpaceProtection::PoisonOnly,
        ),
    };
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    crate::gc::gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    crate::gc::gc_register_mutable_root_scanner(
        crate::object::descriptor_state::scan_descriptor_roots_mut,
    );
    THEN_GETTER_CALLS.with(|count| count.set(0));
    THEN_THIS.with(|this| this.set(0));
    guards
}

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn boxed<T>(ptr: *const T) -> f64 {
    crate::value::js_nanbox_pointer(ptr as i64)
}

fn object(scope: &RuntimeHandleScope) -> RuntimeHandle<'_> {
    scope.root_nanbox_f64(boxed(crate::object::js_object_alloc(0, 1)))
}

/// `get then()`: collects on its first call and returns capture 0.
extern "C" fn collecting_then_getter(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let closure = scope.root_nanbox_f64(boxed(closure));
    if THEN_GETTER_CALLS.with(|count| count.replace(count.get() + 1)) == 0 {
        crate::gc::gc_collect_minor();
    }
    let closure = crate::value::js_nanbox_get_pointer(closure.get_nanbox_f64()) as *const _;
    crate::closure::js_closure_get_capture_f64(closure, 0)
}

/// A young object whose `then` accessor collects and returns `then_value`.
fn thenable<'s>(scope: &'s RuntimeHandleScope, then_value: f64) -> RuntimeHandle<'s> {
    let then_value = scope.root_nanbox_f64(then_value);
    let target = object(scope);
    let getter = crate::closure::js_closure_alloc(
        crate::fn_info!(collecting_then_getter, 0; with_declared(0)),
        1,
    );
    crate::closure::js_closure_set_capture_f64(getter, 0, then_value.get_nanbox_f64());
    let getter = scope.root_nanbox_f64(boxed(getter));
    let key = b"then";
    let key = crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
        key.as_ptr(),
        key.len() as u32,
    ) as i64);
    crate::object::js_object_define_accessor(
        target.get_nanbox_f64(),
        key,
        getter.get_nanbox_f64(),
        undefined(),
    );
    target
}

/// A young input array `[thenable, plain]`, where reading the thenable's
/// `then` collects (and finds no `then`), plus handles on both elements.
fn combinator_inputs(
    scope: &RuntimeHandleScope,
) -> (RuntimeHandle<'_>, RuntimeHandle<'_>, RuntimeHandle<'_>) {
    let first = thenable(scope, undefined());
    let second = object(scope);
    let inputs = scope.root_nanbox_f64(boxed(crate::array::js_array_alloc(2)));
    for value in [&first, &second] {
        let grown = crate::array::js_array_push_f64(
            crate::value::js_nanbox_get_pointer(inputs.get_nanbox_f64()) as *mut _,
            value.get_nanbox_f64(),
        );
        inputs.set_nanbox_f64(boxed(grown));
    }
    (inputs, first, second)
}

fn run_microtasks() {
    for _ in 0..100 {
        if crate::promise::js_promise_run_microtasks() == 0 {
            break;
        }
    }
}

fn array_values(array: f64) -> Vec<u64> {
    let arr = crate::value::js_nanbox_get_pointer(array) as *const crate::array::ArrayHeader;
    (0..crate::array::js_array_length(arr))
        .map(|i| crate::array::js_array_get_f64(arr, i).to_bits())
        .collect()
}

/// Run `combinator` over `[thenable, plain]`; the thenable's `then` getter
/// collects while the combinator is still walking its inputs. Asserts that
/// the inputs moved and that the returned promise settled (with `expected`
/// state); hands back the settled value with the live input addresses.
fn combinator_after_collecting_then_getter(
    combinator: extern "C" fn(*const crate::array::ArrayHeader) -> *mut Promise,
    expected_state: i32,
) -> (f64, u64, u64) {
    let scope = RuntimeHandleScope::new();
    let (inputs, first, second) = combinator_inputs(&scope);
    let before = [
        inputs.get_nanbox_f64().to_bits(),
        first.get_nanbox_f64().to_bits(),
        second.get_nanbox_f64().to_bits(),
    ];

    let result =
        combinator(crate::value::js_nanbox_get_pointer(inputs.get_nanbox_f64()) as *const _);
    let result = scope.root_nanbox_f64(boxed(result));
    run_microtasks();

    assert_eq!(
        THEN_GETTER_CALLS.with(Cell::get),
        1,
        "the then getter must run once"
    );
    let after = [
        inputs.get_nanbox_f64().to_bits(),
        first.get_nanbox_f64().to_bits(),
        second.get_nanbox_f64().to_bits(),
    ];
    for (i, (before, after)) in before.iter().zip(after.iter()).enumerate() {
        assert_ne!(before, after, "input {i} must actually move");
    }
    let promise = crate::value::js_nanbox_get_pointer(result.get_nanbox_f64()) as *mut Promise;
    assert_eq!(
        crate::promise::js_promise_state(promise),
        expected_state,
        "the live result promise must settle"
    );
    let value = if expected_state == 1 {
        crate::promise::js_promise_value(promise)
    } else {
        crate::promise::js_promise_reason(promise)
    };
    (value, after[1], after[2])
}

#[test]
fn promise_all_rereads_inputs_and_state_after_collecting_then_getter() {
    let _gc = moving_gc();
    let (value, first, second) = combinator_after_collecting_then_getter(js_promise_all, 1);
    assert_eq!(array_values(value), vec![first, second]);
}

#[test]
fn promise_race_rereads_inputs_and_result_after_collecting_then_getter() {
    let _gc = moving_gc();
    let (value, first, _) = combinator_after_collecting_then_getter(js_promise_race, 1);
    assert_eq!(value.to_bits(), first, "race settles with the first input");
}

#[test]
fn promise_all_settled_rereads_inputs_and_state_after_collecting_then_getter() {
    let _gc = moving_gc();
    let (value, first, second) = combinator_after_collecting_then_getter(js_promise_all_settled, 1);
    let entries = array_values(value);
    assert_eq!(entries.len(), 2);
    let key = b"value";
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    for (entry, expected) in entries.into_iter().zip([first, second]) {
        let entry = crate::value::js_nanbox_get_pointer(f64::from_bits(entry))
            as *const crate::object::ObjectHeader;
        let value = crate::object::js_object_get_field_by_name_f64(entry, key);
        assert_eq!(value.to_bits(), expected, "each entry holds the live input");
    }
}

#[test]
fn promise_any_rereads_inputs_and_result_after_collecting_then_getter() {
    let _gc = moving_gc();
    let (value, first, _) = combinator_after_collecting_then_getter(js_promise_any, 1);
    assert_eq!(
        value.to_bits(),
        first,
        "any settles with the first fulfilled input"
    );
}

/// `then(resolve, reject)`: records its receiver and resolves with 42.
extern "C" fn recording_then(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    resolve: f64,
    _reject: f64,
) -> f64 {
    THEN_THIS.with(|then_this| then_this.set(this.as_f64().to_bits()));
    let resolve = crate::value::js_nanbox_get_pointer(resolve) as *const ClosureHeader;
    crate::closure::js_closure_call1(resolve, crate::closure::plain_call_receiver(), 42.0);
    undefined()
}

/// `Promise.resolve(thenable)`: the `then` getter collects, then returns a
/// callable `then`. The job must call it on the live thenable and settle the
/// live promise.
#[test]
fn promise_resolve_rereads_promise_and_thenable_after_collecting_then_getter() {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let then =
        crate::closure::js_closure_alloc(crate::fn_info!(recording_then, 2; with_declared(2)), 0);
    let target = thenable(&scope, boxed(then));
    let before = target.get_nanbox_f64().to_bits();

    let promise = crate::promise::js_promise_resolved(target.get_nanbox_f64());
    let promise = scope.root_nanbox_f64(boxed(promise));
    run_microtasks();

    assert_eq!(
        THEN_GETTER_CALLS.with(Cell::get),
        1,
        "the then getter must run once"
    );
    assert_ne!(
        target.get_nanbox_f64().to_bits(),
        before,
        "the thenable must move"
    );
    assert_eq!(
        THEN_THIS.with(Cell::get),
        target.get_nanbox_f64().to_bits(),
        "then must be called on the live thenable"
    );
    let promise = crate::value::js_nanbox_get_pointer(promise.get_nanbox_f64()) as *mut Promise;
    assert_eq!(crate::promise::js_promise_state(promise), 1);
    assert_eq!(crate::promise::js_promise_value(promise), 42.0);
}
