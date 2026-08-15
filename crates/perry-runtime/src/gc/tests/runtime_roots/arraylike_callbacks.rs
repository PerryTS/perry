//! Moving-GC regression for the generic array-like callback loops (#8082).
//!
//! The forced-moving Next production gate caught `js_arraylike_map` writing a
//! mapped element through a pre-collection pointer into mprotect-poisoned
//! retired from-space: the callback allocated, a copying minor moved the
//! result array, and the loop kept the element pointer it had derived before
//! the call. This test plants that exact collection point — a callback that
//! runs a copying minor on every invocation — and asserts the loop observes
//! the relocated receiver and still assembles the correct result.

use super::super::super::*;
use super::super::support::*;

extern "C" fn collect_then_double(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    _index: f64,
    _recv: f64,
) -> f64 {
    crate::gc::gc_collect_minor();
    value * 2.0
}

#[test]
fn arraylike_map_survives_a_moving_minor_inside_every_callback() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();

    let scope = crate::gc::RuntimeHandleScope::new();
    let mut arr = crate::array::js_array_alloc(0);
    for v in [1.0f64, 2.0, 3.0, 4.0] {
        arr = crate::array::js_array_push_f64(arr, v);
    }
    let recv_h = scope.root_nanbox_f64(f64::from_bits(
        crate::JSValue::pointer(arr as *const u8).bits(),
    ));
    let recv_before = arr as usize;

    let cb = crate::closure::js_closure_alloc_singleton(collect_then_double as *const u8);
    let cb_value = f64::from_bits(crate::JSValue::pointer(cb as *const u8).bits());

    let cycles_before = copying_minor_cycles();
    let mapped = crate::array::js_arraylike_map(
        recv_h.get_nanbox_f64(),
        cb_value,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    let cycles_after = copying_minor_cycles();
    assert!(
        cycles_after >= cycles_before + 4,
        "each of the four callbacks must run a copying minor \
         (before={cycles_before}, after={cycles_after})"
    );
    let recv_after = (recv_h.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    assert_ne!(
        recv_before, recv_after,
        "the collections must actually move the receiver array"
    );

    // The discriminating check: pre-fix, every `ptr::write` of a mapped
    // element went through the PRE-collection element pointer, so the
    // relocated result never received the values.
    let result =
        (mapped.to_bits() & crate::value::POINTER_MASK) as *const crate::array::ArrayHeader;
    assert_eq!(crate::array::js_array_length(result), 4);
    for (index, expected) in [2.0f64, 4.0, 6.0, 8.0].into_iter().enumerate() {
        let got = crate::array::js_array_get_f64(result, index as u32);
        assert_eq!(
            got, expected,
            "mapped element {index} must land in the relocated result array"
        );
    }
}
