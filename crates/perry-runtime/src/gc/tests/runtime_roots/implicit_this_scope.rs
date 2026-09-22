//! Moving-GC regression for the runtime's implicit-`this` guards (#10490).
//!
//! A runtime entry point that binds `IMPLICIT_THIS` for a callback displaces
//! the CALLER's receiver and writes it back when the callback returns. The
//! callback is user code, so a copying minor inside it can move that receiver;
//! a displaced value kept in a plain Rust field is not a root and is restored
//! as a retired from-space address — the caller's next `this.x` reads
//! `undefined`. `js_native_call_method`'s prototype-override early path and
//! the dense/array-like `Array.prototype` callback engines each carried such a
//! guard. Every test here plants a callback that runs a copying minor, then
//! asserts the restored cell is the caller's RELOCATED receiver — and that the
//! receiver actually moved, so a green run cannot be vacuous.

use super::super::super::*;
use super::super::support::*;

extern "C" fn collect_arity0(_closure: *const crate::closure::ClosureHeader) -> f64 {
    crate::gc::gc_collect_minor();
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn collect_arity3(
    _closure: *const crate::closure::ClosureHeader,
    _value: f64,
    _index: f64,
    _recv: f64,
) -> f64 {
    crate::gc::gc_collect_minor();
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn boxed(ptr: *const u8) -> f64 {
    f64::from_bits(crate::JSValue::pointer(ptr).bits())
}

/// Install a young object as the caller's `this`, run `call`, and assert the
/// cell afterwards holds that object's post-collection address.
fn assert_caller_this_survives(what: &str, call: impl FnOnce()) {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();

    let scope = crate::gc::RuntimeHandleScope::new();
    let caller = scope.root_nanbox_f64(boxed(crate::object::js_object_alloc(0, 0) as *const u8));
    let caller_before = caller.get_nanbox_u64();
    let outer = crate::object::js_implicit_this_set(caller.get_nanbox_f64());

    let cycles_before = copying_minor_cycles();
    call();
    assert!(
        copying_minor_cycles() > cycles_before,
        "{what}: the callback must run a copying minor"
    );
    assert_ne!(
        caller.get_nanbox_u64(),
        caller_before,
        "{what}: the collection must actually move the caller's receiver"
    );
    let restored = crate::object::js_implicit_this_set(outer);
    assert_eq!(
        restored.to_bits(),
        caller.get_nanbox_u64(),
        "{what}: the displaced `this` must be restored at its relocated address, \
         not the pre-collection one ({caller_before:#x})"
    );
}

#[test]
fn implicit_this_scope_restores_a_relocated_receiver() {
    assert_caller_this_survives("ImplicitThisScope", || {
        let scope = crate::gc::RuntimeHandleScope::new();
        let _bound = crate::object::ImplicitThisScope::bind(
            &scope,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        crate::gc::gc_collect_minor();
    });
}

#[test]
fn prototype_override_method_call_restores_a_relocated_caller_this() {
    assert_caller_this_survives("js_native_call_method prototype override", || {
        crate::closure::js_register_closure_arity(collect_arity0 as *const u8, 0);
        let scope = crate::gc::RuntimeHandleScope::new();
        let method = scope
            .root_nanbox_f64(boxed(
                crate::closure::js_closure_alloc(collect_arity0 as *const u8, 0) as *const u8,
            ));
        let proto = scope.root_nanbox_f64(boxed(crate::object::js_object_alloc(0, 1) as *const u8));
        let key = crate::string::js_string_from_bytes(b"run".as_ptr(), 3);
        crate::object::js_object_set_field_by_name(
            crate::value::js_nanbox_get_pointer(proto.get_nanbox_f64()) as *mut crate::ObjectHeader,
            key,
            method.get_nanbox_f64(),
        );
        let receiver =
            scope.root_nanbox_f64(boxed(crate::object::js_object_alloc(0, 0) as *const u8));
        crate::object::object_ops::js_object_set_prototype_of(
            receiver.get_nanbox_f64(),
            proto.get_nanbox_f64(),
        );
        assert!(
            crate::object::prototype_chain::object_has_individual_class_prototype(
                crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as usize
            ),
            "the receiver must take the #9247 prototype-override early path"
        );
        unsafe {
            crate::object::js_native_call_method(
                receiver.get_nanbox_f64(),
                b"run".as_ptr().cast(),
                3,
                std::ptr::null(),
                0,
            );
        }
    });
}

#[test]
fn dense_array_for_each_restores_a_relocated_caller_this() {
    assert_caller_this_survives("js_array_forEach", || {
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr = crate::array::js_array_push_f64(crate::array::js_array_alloc(0), 1.0);
        let arr = scope.root_raw_const_ptr(arr);
        let cb = crate::closure::js_closure_alloc_singleton(collect_arity3 as *const u8);
        // `js_array_forEach` roots the receiver itself, so a scoped argument is
        // the right shape here (#7341).
        arr.with_const_ptr(|ptr| crate::array::js_array_forEach(ptr, cb));
    });
}

#[test]
fn arraylike_for_each_restores_a_relocated_caller_this() {
    assert_caller_this_survives("js_arraylike_forEach", || {
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr = crate::array::js_array_push_f64(crate::array::js_array_alloc(0), 1.0);
        let arr = scope.root_nanbox_f64(boxed(arr as *const u8));
        let cb = crate::closure::js_closure_alloc_singleton(collect_arity3 as *const u8);
        crate::array::js_arraylike_forEach(
            arr.get_nanbox_f64(),
            boxed(cb as *const u8),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
    });
}
