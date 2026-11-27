use super::*;
use crate::gc::RuntimeHandleScope;
use std::cell::Cell;

thread_local! {
    static VALUE_OF_CALLS: Cell<u32> = const { Cell::new(0) };
    static COLLECT: Cell<bool> = const { Cell::new(false) };
    static BIGINT_RESULT: Cell<bool> = const { Cell::new(false) };
}

/// `valueOf`: counts its calls, runs one moving minor when `COLLECT` is set,
/// and returns 3 (a BigInt `3n` when `BIGINT_RESULT` is set).
extern "C" fn counting_value_of(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    VALUE_OF_CALLS.with(|calls| calls.set(calls.get() + 1));
    if COLLECT.with(|collect| collect.replace(false)) {
        crate::gc::gc_collect_minor();
    }
    if BIGINT_RESULT.with(Cell::get) {
        return crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_i64(3) as i64);
    }
    3.0
}

/// A moving minor: objects born now move when one runs, and from-space is
/// poisoned so a stale address does not read as the live object.
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
    guards
}

/// A fresh `{ valueOf }`, rooted in `scope`.
fn value_with_value_of(scope: &RuntimeHandleScope) -> crate::gc::RuntimeHandle {
    let value_of = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(counting_value_of, 0; with_declared(0)), 0)
            as i64,
    ));
    let value = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let key = scope.root_nanbox_f64(crate::value::js_nanbox_string(
        crate::string::js_string_from_bytes(b"valueOf".as_ptr(), 7) as i64,
    ));
    value.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| unsafe {
        crate::object::js_object_set_field_by_name(
            obj,
            crate::value::js_nanbox_get_pointer(key.get_nanbox_f64()) as *const _,
            value_of.get_nanbox_f64(),
        )
    });
    scope.root_nanbox_f64(
        value.with_const_ptr::<crate::object::ObjectHeader, _>(|obj| {
            crate::value::js_nanbox_pointer(obj as i64)
        }),
    )
}

fn string_key(text: &[u8]) -> f64 {
    crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
        text.as_ptr(),
        text.len() as u32,
    ) as i64)
}

#[test]
fn a_non_finite_number_key_store_still_converts_the_value() {
    // TypedArraySetElement runs ToNumber(value) before IsValidIntegerIndex:
    // `ta[NaN] = { valueOf }` calls valueOf once and stores nothing.
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let ta = crate::typedarray::typed_array_alloc(crate::typedarray::KIND_INT16, 2);
    let value = value_with_value_of(&scope);
    for key in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.5] {
        VALUE_OF_CALLS.with(|calls| calls.set(0));
        js_typed_array_index_set_dynamic(ta, key, value.get_nanbox_f64());
        assert_eq!(VALUE_OF_CALLS.with(Cell::get), 1, "key {key}");
    }
    assert_eq!(crate::typedarray::js_typed_array_get(ta, 0), 0.0);
    assert_eq!(crate::typedarray::js_typed_array_get(ta, 1), 0.0);
}

#[test]
fn a_store_returns_its_value_after_the_conversion_moved_it() {
    // The store expression's result is the value, and its ToNumber / ToBigInt
    // (or an object key's ToPropertyKey) can move it: every key arm returns
    // the value at its new address, on a Number view and a BigInt view.
    // `moving_gc` holds the global side-table lock through its guards.
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    for kind in [
        crate::typedarray::KIND_INT16,
        crate::typedarray::KIND_BIGINT64,
    ] {
        BIGINT_RESULT.with(|b| b.set(kind == crate::typedarray::KIND_BIGINT64));
        let ta = crate::typedarray::typed_array_alloc(kind, 2);
        // A key is born right before its store: a collection in an earlier
        // store would leave a string key born ahead of it stale.
        let keys: [fn() -> f64; 6] = [
            || f64::NAN,
            || 0.0,
            || 9.0,
            || string_key(b"1"),
            || string_key(b"1.5"),
            || string_key(b"named"),
        ];
        for (slot, key) in keys.iter().enumerate() {
            let value = value_with_value_of(&scope);
            let key = key();
            let before = value.get_nanbox_f64().to_bits();
            VALUE_OF_CALLS.with(|calls| calls.set(0));
            COLLECT.with(|collect| collect.set(true));
            let returned = js_typed_array_index_set_dynamic(ta, key, value.get_nanbox_f64());
            let calls = VALUE_OF_CALLS.with(Cell::get);
            COLLECT.with(|collect| collect.set(false));
            let named = slot == 5;
            assert_eq!(calls, u32::from(!named), "kind {kind} key {slot}");
            if !named {
                assert_ne!(
                    value.get_nanbox_f64().to_bits(),
                    before,
                    "kind {kind}: the value must move in the collection"
                );
            }
            assert_eq!(
                returned.to_bits(),
                value.get_nanbox_f64().to_bits(),
                "kind {kind}: the store returns the value at its current address"
            );
        }
        let stored = crate::typedarray::js_typed_array_get(ta, 0);
        if kind == crate::typedarray::KIND_INT16 {
            assert_eq!(stored, 3.0);
        }
    }
    BIGINT_RESULT.with(|b| b.set(false));
}
