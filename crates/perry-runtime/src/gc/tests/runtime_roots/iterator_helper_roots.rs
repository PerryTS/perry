//! Iterator helper locals must follow nursery relocation inside next/callbacks.
use super::*;
use crate::closure::{ClosureHeader, JsThis};
use crate::object::ObjectHeader;
use crate::value::{JSValue, TAG_UNDEFINED};

thread_local! {
    static COPIED: Cell<usize> = const { Cell::new(0) };
    static ORDER: Cell<u64> = const { Cell::new(0) };
}

fn record(event: u64) {
    ORDER.with(|order| {
        order.set(
            order
                .get()
                .checked_mul(10)
                .and_then(|value| value.checked_add(event))
                .expect("fixture callback order must fit its bounded sequence"),
        );
    });
}

fn move_nursery() {
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.eligible,
        "the callback must run a copying minor"
    );
    COPIED.with(|count| count.set(count.get() + trace.copying_nursery.copied_objects));
}

fn prepare() {
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_class_keys_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::object::canonical_keys::scan_canonical_keys_roots_mut);
    gc_register_mutable_root_scanner(crate::string::scan_intern_table_roots_mut);
    gc_register_mutable_root_scanner(crate::iter_result::scan_iter_result_keys_roots_mut);
    gc_register_mutable_root_scanner(crate::symbol::scan_symbol_side_table_roots_mut);
    assert!(crate::object::iterator_prototype_for_class_id(
        crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID
    )
    .is_some());
    crate::iter_result::reset_shared_keys_for_test();
    COPIED.with(|count| count.set(0));
    ORDER.with(|order| order.set(0));
}

fn boxed(obj: *mut ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(obj as i64)
}

unsafe fn dispatch(value: f64, name: &str, args: &[f64]) -> f64 {
    crate::object::js_native_call_method(
        value,
        name.as_ptr() as *const i8,
        name.len(),
        args.as_ptr(),
        args.len(),
    )
}

unsafe fn result(value: f64) -> (f64, bool) {
    let obj = crate::value::js_nanbox_get_pointer(value) as *const ObjectHeader;
    assert!(!obj.is_null(), "next must return an object");
    let value = crate::object::js_object_get_field(obj, 0);
    let done = crate::object::js_object_get_field(obj, 1);
    (
        f64::from_bits(value.bits()),
        crate::value::js_is_truthy(f64::from_bits(done.bits())) != 0,
    )
}

extern "C" fn moving_next(_c: *const ClosureHeader, this: JsThis) -> f64 {
    let scope = RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this.as_f64());
    move_nursery();
    unsafe {
        let obj = crate::value::js_nanbox_get_pointer(this.get_nanbox_f64()) as *mut ObjectHeader;
        let index = crate::object::js_object_get_field(obj, 1).to_number();
        let length = crate::object::js_object_get_field(obj, 2).to_number();
        if index >= length {
            return crate::iter_result::make_iter_result(JSValue::undefined(), true);
        }
        crate::object::js_object_set_field(obj, 1, JSValue::number(index + 1.0));
        record(index as u64 + 1);
        let payload = crate::object::js_object_get_field(obj, 3);
        crate::iter_result::make_iter_result(
            if payload.is_undefined() {
                JSValue::number(index + 1.0)
            } else {
                payload
            },
            false,
        )
    }
}

unsafe fn source(length: f64, payload: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(payload);
    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc_null_proto(0, 0));
    let next = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(moving_next, 0), 0) as i64,
    ));
    for name in [b"next".as_slice(), b"index", b"length", b"payload"] {
        let key = crate::string::intern_ascii_literal(name);
        let value = match name {
            b"next" => next.get_nanbox_f64(),
            b"index" => 0.0,
            b"length" => length,
            _ => payload.get_nanbox_f64(),
        };
        crate::object::js_object_set_field_by_name(obj.get_raw_mut_ptr(), key, value);
    }
    boxed(obj.get_raw_mut_ptr())
}

extern "C" fn moving_map(c: *const ClosureHeader, _this: JsThis, x: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let c = scope.root_raw_const_ptr(c);
    // Read a capture before collecting: a stale callback address from the
    // source step must not silently fall back to yielding x unchanged.
    let offset = crate::closure::js_closure_get_capture_f64(c.get_raw_const_ptr(), 0);
    record(7);
    move_nursery();
    x + offset
}

extern "C" fn moving_predicate(_c: *const ClosureHeader, _this: JsThis, _x: f64) -> f64 {
    record(7);
    move_nursery();
    f64::from_bits(crate::value::TAG_TRUE)
}

extern "C" fn moving_false(c: *const ClosureHeader, this: JsThis, x: f64) -> f64 {
    moving_predicate(c, this, x);
    f64::from_bits(crate::value::TAG_FALSE)
}

extern "C" fn moving_for_each(_c: *const ClosureHeader, _this: JsThis, x: f64, index: f64) -> f64 {
    assert_eq!(x, index + 1.0, "forEach index must retain its order");
    record(7);
    move_nursery();
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn moving_reduce(_c: *const ClosureHeader, _this: JsThis, acc: f64, x: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let acc = scope.root_nanbox_f64(acc);
    record(7);
    move_nursery();
    let old = crate::value::js_nanbox_get_pointer(acc.get_nanbox_f64()) as *const ObjectHeader;
    let sum = crate::object::js_object_get_field(old, 0).to_number() + x;
    // The new accumulator is owned only by reduce while the next source
    // step collects; the initial arg root cannot keep this result alive.
    let new = crate::object::js_object_alloc_null_proto(0, 1);
    crate::object::js_object_set_field(new, 0, JSValue::number(sum));
    boxed(new)
}

extern "C" fn moving_flat_map(_c: *const ClosureHeader, _this: JsThis, x: f64) -> f64 {
    record(7);
    move_nursery();
    unsafe {
        let scope = RuntimeHandleScope::new();
        let iterable = scope.root_raw_mut_ptr(crate::object::js_object_alloc_null_proto(0, 0));
        let key = crate::string::intern_ascii_literal(b"payload");
        crate::object::js_object_set_field_by_name(iterable.get_raw_mut_ptr(), key, x * 10.0);
        let factory = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(crate::fn_info!(moving_iterator_factory, 0), 0) as i64,
        ));
        let symbol = crate::symbol::well_known_symbol("iterator");
        crate::symbol::js_object_set_symbol_property(
            boxed(iterable.get_raw_mut_ptr()),
            crate::value::js_nanbox_pointer(symbol as i64),
            factory.get_nanbox_f64(),
        );
        assert_eq!(
            crate::object::js_object_get_field(iterable.get_raw_mut_ptr(), 0).to_number(),
            x * 10.0,
            "named payload must survive the symbol-property install"
        );
        boxed(iterable.get_raw_mut_ptr())
    }
}

extern "C" fn moving_iterator_factory(_c: *const ClosureHeader, this: JsThis) -> f64 {
    let scope = RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this.as_f64());
    record(8);
    move_nursery();
    unsafe {
        let iterable =
            crate::value::js_nanbox_get_pointer(this.get_nanbox_f64()) as *const ObjectHeader;
        source(
            1.0,
            f64::from_bits(crate::object::js_object_get_field(iterable, 0).bits()),
        )
    }
}

unsafe fn map_callback() -> f64 {
    let callback = crate::closure::js_closure_alloc(crate::fn_info!(moving_map, 1), 1);
    crate::closure::js_closure_set_capture_f64(callback, 0, 10.0);
    crate::value::js_nanbox_pointer(callback as i64)
}

fn current_helper() -> *mut ObjectHeader {
    crate::value::js_nanbox_get_pointer(f64::from_bits(js_shadow_slot_get(0))) as *mut ObjectHeader
}

fn assert_relocated(before: usize) {
    assert!(
        COPIED.with(Cell::get) > 0,
        "the tested calls must move live nursery objects"
    );
    assert_ne!(
        current_helper() as usize,
        before,
        "the helper itself must relocate inside the call"
    );
}

#[test]
fn drop_state_and_repeated_source_steps_follow_relocation() {
    for count in [2.0, 8.0] {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _age = crate::gc::tenuring::set_survivals_for_test(4);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        prepare();
        unsafe {
            let helper = crate::iterator_helpers::js_iterator_from(source(
                3.0,
                f64::from_bits(TAG_UNDEFINED),
            ));
            let helper = dispatch(helper, "drop", &[count]);
            js_shadow_slot_set(0, helper.to_bits());
            let before = current_helper() as usize;
            let (value, done) = result(dispatch(helper, "next", &[]));
            assert_relocated(before);
            assert_eq!(
                crate::object::js_object_get_field(current_helper(), 3).to_number(),
                0.0
            );
            assert_eq!(done, count > 3.0);
            if !done {
                assert_eq!(value, 3.0);
            }
            assert_eq!(
                ORDER.with(Cell::get),
                123,
                "each source step occurs once, in order"
            );
        }
    }
}

#[test]
fn map_callback_is_refreshed_after_source_relocation() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _age = crate::gc::tenuring::set_survivals_for_test(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    prepare();
    unsafe {
        let helper =
            crate::iterator_helpers::js_iterator_from(source(2.0, f64::from_bits(TAG_UNDEFINED)));
        let helper = dispatch(helper, "map", &[map_callback()]);
        js_shadow_slot_set(0, helper.to_bits());
        let before = current_helper() as usize;
        for expected in [11.0, 12.0] {
            assert_eq!(
                result(dispatch(f64::from_bits(js_shadow_slot_get(0)), "next", &[])),
                (expected, false)
            );
        }
        assert_relocated(before);
        assert_eq!(ORDER.with(Cell::get), 1727, "map follows each source step");
    }
}

#[test]
fn filter_and_find_keep_heap_values_across_predicate_relocation() {
    for method in ["filter", "find"] {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _age = crate::gc::tenuring::set_survivals_for_test(4);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        prepare();
        unsafe {
            let payload = crate::object::js_object_alloc_null_proto(0, 1);
            crate::object::js_object_set_field(payload, 0, JSValue::number(42.0));
            let helper = crate::iterator_helpers::js_iterator_from(source(1.0, boxed(payload)));
            let callback =
                crate::closure::js_closure_alloc(crate::fn_info!(moving_predicate, 1), 0);
            let callback = crate::value::js_nanbox_pointer(callback as i64);
            js_shadow_slot_set(0, helper.to_bits());
            let before = current_helper() as usize;
            let found = if method == "filter" {
                let filtered = dispatch(helper, method, &[callback]);
                js_shadow_slot_set(0, filtered.to_bits());
                let before = current_helper() as usize;
                let found = result(dispatch(filtered, "next", &[])).0;
                assert_relocated(before);
                found
            } else {
                dispatch(helper, method, &[callback])
            };
            assert_relocated(before);
            let found = crate::value::js_nanbox_get_pointer(found) as *const ObjectHeader;
            assert_ne!(found, payload, "the yielded heap value must move");
            assert_eq!(
                crate::object::js_object_get_field(found, 0).to_number(),
                42.0
            );
            assert_eq!(ORDER.with(Cell::get), 17);
        }
    }
}

#[test]
fn terminal_callbacks_and_replacement_accumulator_follow_relocation() {
    for method in ["forEach", "some", "every", "reduce"] {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _age = crate::gc::tenuring::set_survivals_for_test(4);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        prepare();
        unsafe {
            let helper = crate::iterator_helpers::js_iterator_from(source(
                3.0,
                f64::from_bits(TAG_UNDEFINED),
            ));
            let info = match method {
                "forEach" => crate::fn_info!(moving_for_each, 2),
                "some" => crate::fn_info!(moving_false, 1),
                "every" => crate::fn_info!(moving_predicate, 1),
                _ => crate::fn_info!(moving_reduce, 2),
            };
            let callback = crate::closure::js_closure_alloc(info, 0);
            let callback = crate::value::js_nanbox_pointer(callback as i64);
            let initial = crate::object::js_object_alloc_null_proto(0, 1);
            crate::object::js_object_set_field(initial, 0, JSValue::number(10.0));
            js_shadow_slot_set(0, helper.to_bits());
            let before = current_helper() as usize;
            let answer = dispatch(helper, method, &[callback, boxed(initial)]);
            assert_relocated(before);
            match method {
                "forEach" => assert_eq!(answer.to_bits(), TAG_UNDEFINED),
                "some" => assert_eq!(answer.to_bits(), crate::value::TAG_FALSE),
                "every" => assert_eq!(answer.to_bits(), crate::value::TAG_TRUE),
                _ => {
                    let acc = crate::value::js_nanbox_get_pointer(answer) as *const ObjectHeader;
                    assert_ne!(
                        acc, initial,
                        "reduce must return its replacement accumulator"
                    );
                    assert_eq!(crate::object::js_object_get_field(acc, 0).to_number(), 16.0);
                }
            }
            assert_eq!(
                ORDER.with(Cell::get),
                172737,
                "terminal callback runs after each source step"
            );
        }
    }
}

#[test]
fn flat_map_inner_state_follows_callback_and_inner_step_relocation() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _age = crate::gc::tenuring::set_survivals_for_test(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    prepare();
    unsafe {
        let helper =
            crate::iterator_helpers::js_iterator_from(source(2.0, f64::from_bits(TAG_UNDEFINED)));
        let callback = crate::closure::js_closure_alloc(crate::fn_info!(moving_flat_map, 1), 0);
        let helper = dispatch(
            helper,
            "flatMap",
            &[crate::value::js_nanbox_pointer(callback as i64)],
        );
        js_shadow_slot_set(0, helper.to_bits());
        let before = current_helper() as usize;
        for expected in [10.0, 20.0] {
            assert_eq!(
                result(dispatch(f64::from_bits(js_shadow_slot_get(0)), "next", &[])),
                (expected, false)
            );
            assert!(JSValue::from_bits(
                crate::object::js_object_get_field(current_helper(), 3).bits()
            )
            .is_pointer());
        }
        assert!(result(dispatch(f64::from_bits(js_shadow_slot_get(0)), "next", &[])).1);
        assert_relocated(before);
        assert!(
            crate::object::js_object_get_field(current_helper(), 3).is_undefined(),
            "exhausted inner must be cleared on the live helper"
        );
        assert_eq!(
            ORDER.with(Cell::get),
            17812781,
            "outer step, callback, iterator factory, inner step, then next outer"
        );
    }
}

#[test]
fn to_array_keeps_output_and_helper_across_source_relocation_and_growth() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _age = crate::gc::tenuring::set_survivals_for_test(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    prepare();
    unsafe {
        let helper =
            crate::iterator_helpers::js_iterator_from(source(12.0, f64::from_bits(TAG_UNDEFINED)));
        js_shadow_slot_set(0, helper.to_bits());
        let before = current_helper() as usize;
        let array = dispatch(helper, "toArray", &[]);
        assert_relocated(before);
        let array = crate::value::js_nanbox_get_pointer(array) as *const crate::array::ArrayHeader;
        assert_eq!(
            crate::array::js_array_length(array),
            12,
            "must grow past initial capacity eight"
        );
        for index in 0..12 {
            assert_eq!(
                crate::array::js_array_get_f64(array, index),
                index as f64 + 1.0
            );
        }
    }
}

struct AllocPointRelocationGuard(Option<crate::gc::roots::ConservativeStackScanMode>);
impl AllocPointRelocationGuard {
    fn new() -> Self {
        Self(crate::gc::roots::set_conservative_stack_scan_override(
            Some(crate::gc::roots::ConservativeStackScanMode::Disabled),
        ))
    }
}
impl Drop for AllocPointRelocationGuard {
    fn drop(&mut self) {
        crate::gc::roots::set_conservative_stack_scan_override(self.0);
    }
}

#[test]
fn helper_birth_refreshes_source_and_callback_during_allocation() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _age = crate::gc::tenuring::set_survivals_for_test(4);
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _relocation = AllocPointRelocationGuard::new();
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    prepare();
    unsafe {
        let helper =
            crate::iterator_helpers::js_iterator_from(source(1.0, f64::from_bits(TAG_UNDEFINED)));
        let callback = map_callback();
        js_shadow_slot_set(0, helper.to_bits());
        js_shadow_slot_set(1, callback.to_bits());
        force_next_general_arena_alloc_slow();
        triggers.make_arena_trigger_due();
        // Enter this dispatcher directly so its first allocation is the
        // helper birth, rather than any work in the outer method tower.
        let mapped = crate::iterator_helpers::dispatch_iterator_helper_method(
            current_helper(),
            "map",
            [callback].as_ptr(),
            1,
        );
        assert_ne!(
            js_shadow_slot_get(0),
            helper.to_bits(),
            "source must move in helper allocation"
        );
        assert_ne!(
            js_shadow_slot_get(1),
            callback.to_bits(),
            "callback must move in helper allocation"
        );
        let mapped = crate::value::js_nanbox_get_pointer(mapped) as *const ObjectHeader;
        assert_eq!(
            crate::object::js_object_get_field(mapped, 0).bits(),
            js_shadow_slot_get(0)
        );
        assert_eq!(
            crate::object::js_object_get_field(mapped, 2).bits(),
            js_shadow_slot_get(1)
        );
    }
}

#[test]
fn raw_iterator_adapters_refresh_arguments_after_wrapper_birth_relocation() {
    for builtin in [false, true] {
        let _guard = CopyingNurseryTestGuard::new(2);
        let _age = crate::gc::tenuring::set_survivals_for_test(4);
        let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
        let _relocation = AllocPointRelocationGuard::new();
        let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        prepare();
        unsafe {
            let iterator = if builtin {
                let array = crate::array::js_array_alloc(1);
                let array = crate::array::js_array_push_f64(array, 1.0);
                crate::array::array_values_iter(crate::value::js_nanbox_pointer(array as i64))
            } else {
                // Ordinary sources traverse allocating GetIterator probes
                // before wrapper birth, so both stages must preserve roots.
                source(1.0, f64::from_bits(TAG_UNDEFINED))
            };
            let callback = map_callback();
            js_shadow_slot_set(0, iterator.to_bits());
            js_shadow_slot_set(1, callback.to_bits());
            force_next_general_arena_alloc_slow();
            triggers.make_arena_trigger_due();
            let args = [callback];
            let mapped = if builtin {
                crate::iterator_helpers::maybe_dispatch_helper_on_builtin_iterator(
                    current_helper(),
                    "map",
                    args.as_ptr(),
                    args.len(),
                )
            } else {
                crate::iterator_helpers::maybe_dispatch_helper_on_iterator(
                    current_helper(),
                    "map",
                    args.as_ptr(),
                    args.len(),
                    false,
                )
            }
            .expect("iterator adapter must dispatch map");
            assert_ne!(
                js_shadow_slot_get(0),
                iterator.to_bits(),
                "raw iterator must move inside its wrapper birth"
            );
            assert_ne!(
                js_shadow_slot_get(1),
                callback.to_bits(),
                "argument must move inside the wrapper birth"
            );
            let mapped = crate::value::js_nanbox_get_pointer(mapped) as *const ObjectHeader;
            assert_eq!(
                crate::object::js_object_get_field(mapped, 2).bits(),
                js_shadow_slot_get(1)
            );
            let wrapper = crate::object::js_object_get_field(mapped, 0);
            let wrapper = crate::value::js_nanbox_get_pointer(f64::from_bits(wrapper.bits()))
                as *const ObjectHeader;
            assert_eq!(
                crate::object::js_object_get_field(wrapper, 0).bits(),
                js_shadow_slot_get(0)
            );
        }
    }
}

extern "C" fn moving_iterator_getter(c: *const ClosureHeader, this: JsThis) -> f64 {
    let scope = RuntimeHandleScope::new();
    let c = scope.root_raw_const_ptr(c);
    let this = scope.root_nanbox_f64(this.as_f64());
    record(6);
    move_nursery();
    assert_eq!(this.get_nanbox_f64().to_bits(), js_shadow_slot_get(0));
    crate::closure::js_closure_get_capture_f64(c.get_raw_const_ptr(), 0)
}

extern "C" fn moving_get_iterator_factory(_c: *const ClosureHeader, this: JsThis) -> f64 {
    let scope = RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this.as_f64());
    record(8);
    move_nursery();
    assert_eq!(
        this.get_nanbox_f64().to_bits(),
        js_shadow_slot_get(0),
        "factory must receive the original iterable after getter relocation"
    );
    unsafe {
        let iterable =
            crate::value::js_nanbox_get_pointer(this.get_nanbox_f64()) as *const ObjectHeader;
        source(
            1.0,
            f64::from_bits(crate::object::js_object_get_field(iterable, 0).bits()),
        )
    }
}

extern "C" fn moving_missing_iterator(c: *const ClosureHeader, this: JsThis) -> f64 {
    let scope = RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this.as_f64());
    let event = crate::closure::js_closure_get_capture_f64(c, 0) as u64;
    record(event);
    move_nursery();
    assert_eq!(this.get_nanbox_f64().to_bits(), js_shadow_slot_get(0));
    f64::from_bits(TAG_UNDEFINED)
}

#[test]
fn get_iterator_refreshes_receiver_after_moving_symbol_getter() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _age = crate::gc::tenuring::set_survivals_for_test(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    prepare();
    unsafe {
        let scope = RuntimeHandleScope::new();
        let iterable = scope.root_raw_mut_ptr(crate::object::js_object_alloc_null_proto(0, 0));
        let key = crate::string::intern_ascii_literal(b"payload");
        crate::object::js_object_set_field_by_name(iterable.get_raw_mut_ptr(), key, 73.0);
        let factory = scope.root_raw_const_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(moving_get_iterator_factory, 0),
            0,
        ));
        let getter = scope.root_raw_const_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(moving_iterator_getter, 0),
            1,
        ));
        crate::closure::js_closure_set_capture_f64(
            getter.get_raw_const_ptr::<ClosureHeader>().cast_mut(),
            0,
            crate::value::js_nanbox_pointer(factory.get_raw_const_ptr::<ClosureHeader>() as i64),
        );
        let symbol = crate::symbol::well_known_symbol("iterator");
        crate::symbol::set_symbol_accessor_property(
            boxed(iterable.get_raw_mut_ptr()),
            crate::value::js_nanbox_pointer(symbol as i64),
            crate::value::js_nanbox_pointer(getter.get_raw_const_ptr::<ClosureHeader>() as i64)
                .to_bits(),
            TAG_UNDEFINED,
        );
        let before = boxed(iterable.get_raw_mut_ptr());
        js_shadow_slot_set(0, before.to_bits());
        let iterator = crate::symbol::js_get_iterator(before);
        assert_relocated(crate::value::js_nanbox_get_pointer(before) as usize);
        assert_eq!(
            ORDER.with(Cell::get),
            68,
            "getter precedes exactly one factory call"
        );
        let iterator = crate::value::js_nanbox_get_pointer(iterator) as *const ObjectHeader;
        assert_eq!(
            crate::object::js_object_get_field(iterator, 3).to_number(),
            73.0
        );
    }
}

#[test]
fn get_iterator_fallback_follows_both_moving_symbol_lookups() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _age = crate::gc::tenuring::set_survivals_for_test(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    prepare();
    unsafe {
        let scope = RuntimeHandleScope::new();
        let iterator = scope.root_nanbox_f64(source(1.0, f64::from_bits(TAG_UNDEFINED)));
        for (name, event) in [("iterator", 1.0), ("asyncIterator", 2.0)] {
            let getter = scope.root_raw_const_ptr(crate::closure::js_closure_alloc(
                crate::fn_info!(moving_missing_iterator, 0),
                1,
            ));
            crate::closure::js_closure_set_capture_f64(
                getter.get_raw_const_ptr::<ClosureHeader>().cast_mut(),
                0,
                event,
            );
            let symbol = crate::symbol::well_known_symbol(name);
            crate::symbol::set_symbol_accessor_property(
                iterator.get_nanbox_f64(),
                crate::value::js_nanbox_pointer(symbol as i64),
                crate::value::js_nanbox_pointer(getter.get_raw_const_ptr::<ClosureHeader>() as i64)
                    .to_bits(),
                TAG_UNDEFINED,
            );
        }
        let before = iterator.get_nanbox_f64();
        js_shadow_slot_set(0, before.to_bits());
        let result = crate::symbol::js_get_iterator(before);
        assert_relocated(crate::value::js_nanbox_get_pointer(before) as usize);
        assert_eq!(
            ORDER.with(Cell::get),
            12,
            "sync lookup precedes async lookup, once each"
        );
        assert_eq!(
            result.to_bits(),
            js_shadow_slot_get(0),
            "fallback must return the current source"
        );
    }
}
