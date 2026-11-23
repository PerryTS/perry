//! #15159: the generic `Array.prototype` mutators over a plain-object
//! receiver keep it rooted across its `length` getter. The getter runs a
//! copying minor, so a mutator that keeps the pre-call receiver writes into
//! retired from-space and the relocated object never sees the mutation.

use super::*;
use crate::closure::ClosureHeader;

extern "C" fn collecting_length(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    gc_collect_minor();
    2.0
}

extern "C" fn accept_length(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _value: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn index(recv: u64, k: usize) -> f64 {
    crate::object::js_object_get_index_polymorphic(recv as i64, k as f64)
}

const HOLE: u64 = crate::value::TAG_UNDEFINED;

#[test]
fn object_mutators_follow_the_receiver_across_a_collecting_length_getter() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();

    // (method, args, returned value or None for "the receiver", final elements)
    let cases: [(&str, &[f64], Option<f64>, &[u64]); 6] = [
        ("pop", &[], Some(20.0), &[10f64.to_bits(), HOLE]),
        ("shift", &[], Some(10.0), &[20f64.to_bits(), HOLE]),
        (
            "push",
            &[30.0],
            Some(3.0),
            &[10f64.to_bits(), 20f64.to_bits(), 30f64.to_bits()],
        ),
        (
            "unshift",
            &[5.0],
            Some(3.0),
            &[5f64.to_bits(), 10f64.to_bits(), 20f64.to_bits()],
        ),
        ("reverse", &[], None, &[20f64.to_bits(), 10f64.to_bits()]),
        ("splice", &[0.0, 1.0], None, &[20f64.to_bits(), HOLE]),
    ];
    for (method, args, expected, elements) in cases {
        let scope = RuntimeHandleScope::new();
        let receiver =
            scope.root_nanbox_u64(ptr_bits(crate::object::js_object_alloc(0, 0) as usize));
        for (k, v) in [10.0, 20.0].into_iter().enumerate() {
            crate::object::js_object_set_index_polymorphic(
                receiver.get_nanbox_u64() as i64,
                k as f64,
                v,
            );
        }
        let getter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(collecting_length, 0),
            0,
        ));
        let setter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(accept_length, 1),
            0,
        ));
        let descriptor = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        for (name, function) in [(b"get", &getter), (b"set", &setter)] {
            let key = crate::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            descriptor.with_mut_ptr(|desc| {
                function.with_const_ptr(|func: *const ClosureHeader| {
                    crate::object::js_object_set_field_by_name(
                        desc,
                        key,
                        f64::from_bits(ptr_bits(func as usize)),
                    );
                });
            });
        }
        let key = crate::js_string_from_bytes(b"length".as_ptr(), 6);
        descriptor.with_const_ptr(|desc: *const u8| {
            crate::object::js_object_define_property(
                receiver.get_nanbox_f64(),
                f64::from_bits(string_bits(key as usize)),
                f64::from_bits(ptr_bits(desc as usize)),
            );
        });
        let before = receiver.get_nanbox_u64();
        let returned = crate::array::array_proto_mutator(
            receiver.get_nanbox_f64(),
            method,
            args.as_ptr(),
            args.len(),
        );
        let current = receiver.get_nanbox_u64();
        assert_ne!(
            current, before,
            "{method}: the getter must relocate the receiver"
        );
        match expected {
            Some(v) => assert_eq!(returned, v, "{method}: returned value"),
            None if method == "splice" => {
                assert_eq!(
                    index(returned.to_bits(), 0),
                    10.0,
                    "splice: removed element"
                )
            }
            None => assert_eq!(
                returned.to_bits(),
                current,
                "{method}: must return the live receiver"
            ),
        }
        for (k, bits) in elements.iter().enumerate() {
            assert_eq!(index(current, k).to_bits(), *bits, "{method}: element {k}");
        }
    }
}
