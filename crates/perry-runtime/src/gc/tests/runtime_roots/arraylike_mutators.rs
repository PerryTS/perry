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
        define_accessor(
            &receiver,
            b"length",
            crate::closure::js_closure_alloc(crate::fn_info!(collecting_length, 0), 0),
        );
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

/// Defines a configurable accessor `name` on `receiver` with `getter` and an
/// accepting setter.
fn define_accessor(
    receiver: &crate::gc::RuntimeHandle<'_>,
    name: &[u8],
    getter: *mut ClosureHeader,
) {
    let scope = RuntimeHandleScope::new();
    let getter = scope.root_raw_mut_ptr(getter);
    let setter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(accept_length, 1),
        0,
    ));
    let descriptor = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    for (field, value) in [
        (&b"get"[..], Some(&getter)),
        (b"set", Some(&setter)),
        (b"configurable", None),
    ] {
        let key = crate::js_string_from_bytes(field.as_ptr(), field.len() as u32);
        let bits = match value {
            Some(function) => {
                function.with_const_ptr(|func: *const ClosureHeader| ptr_bits(func as usize))
            }
            None => crate::value::TAG_TRUE,
        };
        descriptor.with_mut_ptr(|desc| {
            crate::object::js_object_set_field_by_name(desc, key, f64::from_bits(bits));
        });
    }
    let key = crate::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    descriptor.with_const_ptr(|desc: *const u8| {
        crate::object::js_object_define_property(
            receiver.get_nanbox_f64(),
            f64::from_bits(string_bits(key as usize)),
            f64::from_bits(ptr_bits(desc as usize)),
        );
    });
}

extern "C" fn collecting_index(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    gc_collect_minor();
    7.0
}

/// A saved element, an inserted argument and the receiver all move under the
/// mutator: each must land where the live copies are.
#[test]
fn object_mutators_follow_object_values_across_a_collection() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();

    // shift: element 0 is an object; reading element 1 collects.
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_u64(ptr_bits(crate::object::js_object_alloc(0, 0) as usize));
    let element = scope.root_nanbox_u64(ptr_bits(crate::object::js_object_alloc(0, 0) as usize));
    crate::object::js_object_set_index_polymorphic(
        receiver.get_nanbox_u64() as i64,
        0.0,
        element.get_nanbox_f64(),
    );
    define_accessor(
        &receiver,
        b"1",
        crate::closure::js_closure_alloc(crate::fn_info!(collecting_index, 0), 0),
    );
    let length = crate::js_string_from_bytes(b"length".as_ptr(), 6);
    crate::object::js_object_set_field_by_name(
        (receiver.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut _,
        length,
        2.0,
    );
    let before = (receiver.get_nanbox_u64(), element.get_nanbox_u64());
    let returned =
        crate::array::array_proto_mutator(receiver.get_nanbox_f64(), "shift", std::ptr::null(), 0);
    assert_ne!(
        receiver.get_nanbox_u64(),
        before.0,
        "shift: the getter must relocate the receiver"
    );
    assert_ne!(
        element.get_nanbox_u64(),
        before.1,
        "shift: the getter must relocate the element"
    );
    assert_eq!(
        returned.to_bits(),
        element.get_nanbox_u64(),
        "shift: must return the live element"
    );
    assert_eq!(index(receiver.get_nanbox_u64(), 0), 7.0, "shift: element 0");

    // unshift: the inserted argument is an object; the length getter collects.
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_u64(ptr_bits(crate::object::js_object_alloc(0, 0) as usize));
    let argument = scope.root_nanbox_u64(ptr_bits(crate::object::js_object_alloc(0, 0) as usize));
    define_accessor(
        &receiver,
        b"length",
        crate::closure::js_closure_alloc(crate::fn_info!(collecting_length, 0), 0),
    );
    let before = argument.get_nanbox_u64();
    let args = [argument.get_nanbox_f64()];
    crate::array::array_proto_mutator(receiver.get_nanbox_f64(), "unshift", args.as_ptr(), 1);
    assert_ne!(
        argument.get_nanbox_u64(),
        before,
        "unshift: the getter must relocate the argument"
    );
    assert_eq!(
        index(receiver.get_nanbox_u64(), 0).to_bits(),
        argument.get_nanbox_u64(),
        "unshift: must insert the live argument"
    );
}
