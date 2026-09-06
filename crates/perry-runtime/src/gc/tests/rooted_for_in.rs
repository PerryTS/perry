//! Enumeration retains its output and receiver across Proxy callbacks (#4644).

use super::super::*;
use super::support::*;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};

thread_local! {
    static COPIED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

extern "C" fn moving_own_keys(_closure: *const crate::closure::ClosureHeader, target: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    COPIED.with(|count| count.set(count.get() + trace.copying_nursery.copied_objects));
    crate::object::js_object_get_own_property_names(target.get_nanbox_f64())
}

fn object(scope: &RuntimeHandleScope) -> RuntimeHandle<'_> {
    scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0))
}

fn boxed(handle: RuntimeHandle<'_>) -> f64 {
    handle.with_const_ptr(|ptr: *const crate::object::ObjectHeader| {
        f64::from_bits(ptr_bits(ptr as usize))
    })
}

fn set(handle: RuntimeHandle<'_>, name: &str, value: f64) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    handle.with_mut_ptr(|ptr| crate::object::js_object_set_field_by_name(ptr, key, value));
}

fn run(inherited_proxy: bool, descriptor_trap: bool) {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner_with_source(
        scan_runtime_handle_roots_mut,
        MutableRootScannerSource::RuntimeHandles,
    );
    gc_register_mutable_root_scanner(crate::proxy::scan_proxy_roots_mut);
    COPIED.with(|count| count.set(0));
    let scope = RuntimeHandleScope::new();
    let target = object(&scope);
    crate::object::js_object_set_prototype_of(
        boxed(target),
        f64::from_bits(crate::value::TAG_NULL),
    );
    for index in 0..14 {
        set(target, &format!("property_{index}"), index as f64);
    }
    let handler = object(&scope);
    let (name, function) = if descriptor_trap {
        ("getOwnPropertyDescriptor", moving_descriptor as *const u8)
    } else {
        ("ownKeys", moving_own_keys as *const u8)
    };
    let callback = crate::closure::js_closure_alloc(function, 0);
    set(handler, name, f64::from_bits(ptr_bits(callback as usize)));
    let proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(boxed(target), boxed(handler)));
    let receiver = object(&scope);
    if inherited_proxy {
        // These entries grow the result before reaching the Proxy prototype.
        for index in 0..10 {
            set(receiver, &format!("local_{index}"), index as f64);
        }
        crate::object::js_object_set_prototype_of(boxed(receiver), proxy.get_nanbox_f64());
    }
    let target_before = boxed(target).to_bits();
    let receiver_before = boxed(receiver).to_bits();
    let result = crate::object::js_for_in_keys_value(if inherited_proxy {
        boxed(receiver)
    } else {
        proxy.get_nanbox_f64()
    });
    let result = scope.root_raw_const_ptr(result);
    assert!(
        COPIED.with(|count| count.get()) > 0,
        "the callback must move live objects"
    );
    assert_ne!(
        boxed(target).to_bits(),
        target_before,
        "the Proxy target must relocate"
    );
    assert_ne!(
        boxed(receiver).to_bits(),
        receiver_before,
        "the receiver must relocate"
    );
    let local_count = if inherited_proxy { 10 } else { 0 };
    assert_eq!(
        result.with_const_ptr(|array| crate::array::js_array_length(array)),
        local_count + 14
    );
    for index in 0..local_count + 14 {
        let expected = if index < local_count {
            format!("local_{index}")
        } else {
            format!("property_{}", index - local_count)
        };
        let value = result.with_const_ptr(|ptr| crate::array::js_array_get(ptr, index));
        unsafe {
            assert_string_bytes(
                (value.bits() & POINTER_MASK) as *const crate::StringHeader,
                expected.as_bytes(),
            );
        }
    }
}

#[test]
fn for_in_result_survives_own_keys_collection() {
    run(false, false);
}

#[test]
fn for_in_grown_result_and_receiver_survive_prototype_collection() {
    run(true, false);
}

extern "C" fn moving_descriptor(
    _closure: *const crate::closure::ClosureHeader,
    target: f64,
    key: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let key = scope.root_nanbox_f64(key);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    COPIED.with(|count| count.set(count.get() + trace.copying_nursery.copied_objects));
    crate::object::js_object_get_own_property_descriptor(
        target.get_nanbox_f64(),
        key.get_nanbox_f64(),
    )
}

#[test]
fn descriptor_trap_collection_preserves_for_in_target_and_keys() {
    run(false, true);
}

extern "C" fn moving_value(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    COPIED.with(|count| count.set(count.get() + trace.copying_nursery.copied_objects));
    23.0
}

extern "C" fn descriptor_with_moving_field(
    _closure: *const crate::closure::ClosureHeader,
    _target: f64,
    _key: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let result = object(&scope);
    for name in ["enumerable", "configurable", "writable"] {
        set(result, name, f64::from_bits(crate::value::TAG_TRUE));
    }
    let getter = crate::closure::js_closure_alloc(moving_value as *const u8, 0);
    let descriptor = object(&scope);
    set(descriptor, "get", f64::from_bits(ptr_bits(getter as usize)));
    let key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    crate::object::js_object_define_property(
        boxed(result),
        f64::from_bits(string_bits(key as usize)),
        boxed(descriptor),
    );
    boxed(result)
}

#[test]
fn descriptor_completion_reloads_after_field_getter_collection() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner_with_source(
        scan_runtime_handle_roots_mut,
        MutableRootScannerSource::RuntimeHandles,
    );
    gc_register_mutable_root_scanner(crate::proxy::scan_proxy_roots_mut);
    COPIED.with(|count| count.set(0));
    let scope = RuntimeHandleScope::new();
    let target = object(&scope);
    set(target, "property_name", 7.0);
    let handler = object(&scope);
    let callback = crate::closure::js_closure_alloc(descriptor_with_moving_field as *const u8, 0);
    set(
        handler,
        "getOwnPropertyDescriptor",
        f64::from_bits(ptr_bits(callback as usize)),
    );
    let proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(boxed(target), boxed(handler)));
    let key = crate::string::js_string_from_bytes(b"property_name".as_ptr(), 13);
    let before = boxed(target).to_bits();
    let result = crate::proxy::js_reflect_get_own_property_descriptor(
        proxy.get_nanbox_f64(),
        f64::from_bits(string_bits(key as usize)),
    );
    let result = scope.root_nanbox_f64(result);
    assert!(COPIED.with(|count| count.get()) > 0);
    assert_ne!(
        boxed(target).to_bits(),
        before,
        "the descriptor getter must move live objects"
    );
    for (name, expected) in [
        ("value", 23.0),
        ("writable", f64::from_bits(crate::value::TAG_TRUE)),
        ("enumerable", f64::from_bits(crate::value::TAG_TRUE)),
        ("configurable", f64::from_bits(crate::value::TAG_TRUE)),
    ] {
        let value = unsafe {
            crate::value::js_get_property(
                result.get_nanbox_f64(),
                name.as_ptr() as i64,
                name.len() as i64,
            )
        };
        assert_eq!(
            value.to_bits(),
            expected.to_bits(),
            "descriptor field {name}"
        );
    }
}
